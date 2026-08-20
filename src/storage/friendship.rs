//! Friendship chain storage.
//!
//! Stores FriendshipChains as a single vault entry at `vault_key("chains", friendship_id)` — a flat 32-byte address, never a path. This is chain *state* (the ratchet machinery), not conversation *content*; content lives in the rārangi conversation DB.
//!
//! All encryption, addressing, and atomicity is handled by FlatStorage.

use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::VsfType;

use crate::storage::{FlatStorage, StorageError};
use crate::types::{FriendshipChains, FriendshipId};

/// Schema for friendship_chains section
///
/// Photon-specific VSF wrapped types (uppercase = application-specific):
/// - vC = CLUTCH chain (512×32 = 16KB key chain per participant)
/// - vX = Ciphertext (encrypted message bytes)
///
/// Standard VSF types:
/// - x = UTF-8 text (Huffman compressed Unicode) for message plaintexts
/// The section name, shared by the document builder in `chains_to_vsf_bytes` and the TOC lookup in `chains_from_vsf_bytes` — `parse_document` matches this against the header, so the two must never drift.
const CHAINS_SECTION: &str = "friendship_chains";

fn chains_schema() -> SectionSchema {
    SectionSchema::new(CHAINS_SECTION)
        .field("version", TypeConstraint::AnyUnsigned)
        .field("friendship_id", TypeConstraint::AnyHash)
        .field("participant", TypeConstraint::AnyHash) // One per participant (handle_hash as hb)
        .field("chain", TypeConstraint::Wrapped(b'C')) // vC: CLUTCH chain (512×32) per participant
        // Hash chain state (v2)
        .field("last_sent_hash", TypeConstraint::AnyHash) // hp type: last msg_hp we sent
        .field("last_received_hash", TypeConstraint::AnyHash) // One per participant (hp or empty hb)
        // Pending messages (v2) - each message has 6 fields
        .field("pending_eagle_time", TypeConstraint::Any)
        .field("pending_plaintext", TypeConstraint::Utf8Text) // x: the message x-text (salt/weave ingredient) — text-only, so valid UTF-8
        .field("pending_plaintext_hash", TypeConstraint::AnyHash) // hp
        .field("pending_prev_msg_hp", TypeConstraint::AnyHash) // hp
        .field("pending_msg_hp", TypeConstraint::AnyHash) // hp
        .field("pending_ciphertext", TypeConstraint::Wrapped(b'X')) // vX: ciphertext bytes
        .field("pending_attempts", TypeConstraint::AnyUnsigned) // send attempts SURVIVE restarts: exhaustion is cumulative evidence about the LANE (the anchor-wedge detector's arming gate), and short sessions resetting it to zero meant a dead lane could never be diagnosed (round-7 field, 2026-08-17)
        // Bidirectional entropy state (v3)
        .field("last_received_weave", TypeConstraint::AnyHash) // hp: derived weave hash (32 bytes)
        .field("last_sent_weave", TypeConstraint::AnyHash) // hp: what we sent (what they received)
        .field("last_incorporated_hp", TypeConstraint::AnyHash) // hp: which of theirs we mixed in
        // Last plaintexts (v4) - needed for salt derivation after restart
        .field("last_plaintext", TypeConstraint::Utf8Text) // x: the message x-text (salt source), one per participant — text-only, valid UTF-8
        // Last received times (v5) - for duplicate detection after restart
        .field("last_received_time", TypeConstraint::Any) // i64 oscillations, one per participant
        // Friend-history bulk key (v6) — spaghettify-derived at ceremony birth, seals history-recovery pages outside the ratchet. Optional: absent = pre-feature chains (recovery unavailable until re-key).
        .field("history_key", TypeConstraint::AnyHash)
        // Mutation stamp (v7) — fleet chain-replication ordering key (adopt iff newer). Optional: absent = pre-feature file, treated as 0.
        .field("mutated_osc", TypeConstraint::Any)
        // Lane root (v8, docs/lanes.md) — the secret every per-device lane derives from. The lanes themselves will ride ADDITIVE fields under this same version (absent = no lanes materialized yet), so v8 is the LAST flag-day this schema takes for the lane work.
        .field("lane_root", TypeConstraint::AnyHash)
        .field("genesis_osc", TypeConstraint::Any)
        // Lanes (v8, additive): one label+position per lane; the chain / last_plaintext / last_received_hash / last_received_time multis are INDEX-ALIGNED with lane_label (they carried per-participant state before the flag-day retired it — same tags, new meaning, and a legacy blob's copies are simply ignored because it has no lane_label rows).
        .field("lane_label", TypeConstraint::AnyHash)
        .field("lane_position", TypeConstraint::Any)
        .field("our_label", TypeConstraint::AnyHash)
}

/// Vault address for a friendship's chain state — `vault_key("chains", friendship_id)`. The conversation id is the scope (already `blake3` of the sorted participant seeds, so 1/2/N participants all resolve here); "chains" names the entry.
fn chains_key(friendship_id: &FriendshipId) -> [u8; 32] {
    crate::storage::vault_key("chains", friendship_id.as_bytes())
}

/// Save FriendshipChains to disk
pub fn save_friendship_chains(
    chains: &FriendshipChains,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    let friendship_id = chains.id();
    let vsf_bytes = chains_to_vsf_bytes(chains)?;
    storage.write_addr(&chains_key(&friendship_id), &vsf_bytes)
}

/// Encode FriendshipChains to their canonical VSF bytes — the SAME encoding save_friendship_chains persists, reused verbatim by the fleet chain-replication push (the bytes are sealed under the fleet key and shipped to siblings, whose decoder is chains_from_vsf_bytes).
pub fn chains_to_vsf_bytes(chains: &FriendshipChains) -> Result<Vec<u8>, StorageError> {
    let friendship_id = chains.id();

    // Build VSF section
    let schema = chains_schema();
    let mut builder = schema
        .build()
        .set("version", 8u8) // v8: lanes flag-day — lane_root joins; v≤7 blobs are REJECTED at read (re-clutch re-mints everything)
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set(
            "friendship_id",
            VsfType::hb(friendship_id.as_bytes().to_vec()),
        )
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    // Identity participants — conversation membership, no chain zipped to them since the lanes flag-day.
    for participant in chains.participants() {
        builder = builder
            .append_multi("participant", vec![VsfType::hb(participant.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // Lanes: label + position + chain per lane; the per-lane vec fields further down (last_received_hash / last_plaintext / last_received_time) are index-aligned with these rows.
    for (label, position) in chains.lane_summary() {
        let chain = chains
            .chain(&label)
            .ok_or_else(|| StorageError::Parse("Missing lane chain".to_string()))?;
        builder = builder
            .append_multi("lane_label", vec![VsfType::hb(label.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi(
                "lane_position",
                vec![VsfType::e(vsf::types::EtType::e6(position as i64))],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("chain", vec![VsfType::v(b'C', chain.to_bytes())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(l) = chains.our_label() {
        builder = builder
            .set("our_label", VsfType::hb(l.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === Hash chain state (v2) ===

    // last_sent_hash - use hp (hash provenance) for immutable content ID
    if let Some(hash) = chains.last_sent_hash() {
        builder = builder
            .set("last_sent_hash", VsfType::hp(hash.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // last_received_hashes - one per participant (None serialized as empty hb)
    for hash_opt in chains.last_received_hashes() {
        let vsf_val = match hash_opt {
            Some(hash) => VsfType::hp(hash.to_vec()),
            None => VsfType::hb(Vec::new()), // Empty = no messages received yet (expect anchor)
        };
        builder = builder
            .append_multi("last_received_hash", vec![vsf_val])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === Pending messages (v2) ===
    for pending in chains.pending_messages() {
        // pending.plaintext is the message x-text only (the salt/weave ingredient), NOT the full flattened payload — so it's valid UTF-8 and stores losslessly as x. (It used to be the whole binary payload incl. the random pad, which forced a lossy conversion that mangled non-UTF-8 bytes to U+FFFD and desynced the chain.)
        let plaintext_str = String::from_utf8_lossy(&pending.plaintext).into_owned();
        builder = builder
            .append_multi(
                "pending_eagle_time",
                vec![VsfType::e(vsf::types::EtType::e6(pending.eagle_time))],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("pending_plaintext", vec![VsfType::x(plaintext_str)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_plaintext_hash",
                vec![VsfType::hp(pending.plaintext_hash.to_vec())],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_prev_msg_hp",
                vec![VsfType::hp(pending.prev_msg_hp.to_vec())],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("pending_msg_hp", vec![VsfType::hp(pending.msg_hp.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_ciphertext",
                vec![VsfType::v(b'X', pending.ciphertext.clone())],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_attempts",
                vec![VsfType::u(pending.attempts as usize, false)],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === Bidirectional entropy state (v3) ===

    // last_received_weave - derived weave hash for mixing (32 bytes)
    if let Some(weave) = chains.last_received_weave() {
        builder = builder
            .set("last_received_weave", VsfType::hp(weave.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // last_sent_weave - what we sent (what they received) for their chain advancement
    if let Some(weave) = chains.last_sent_weave() {
        builder = builder
            .set("last_sent_weave", VsfType::hp(weave.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // last_incorporated_hp - which of their messages we mixed in
    if let Some(hp) = chains.last_incorporated_hp() {
        builder = builder
            .set("last_incorporated_hp", VsfType::hp(hp.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === Last plaintexts (v4) - one per participant ===
    for plaintext in chains.last_plaintexts() {
        // x-text only (salt source) — valid UTF-8, lossless as x. See pending_plaintext above.
        let plaintext_str = String::from_utf8_lossy(plaintext).into_owned();
        builder = builder
            .append_multi("last_plaintext", vec![VsfType::x(plaintext_str)])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === Last received times (v5) - one per participant, for duplicate detection ===
    for time_opt in chains.last_received_times() {
        let time_val = time_opt.unwrap_or(0); // 0 means no messages received yet
        builder = builder
            .append_multi(
                "last_received_time",
                vec![VsfType::e(vsf::types::EtType::e6(time_val))],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === History key (v6) — optional; absent = pre-feature chains ===
    if let Some(key) = chains.history_key() {
        builder = builder
            .set("history_key", VsfType::hb(key.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // === Lane root (v8, docs/lanes.md) ===
    if let Some(root) = chains.lane_root() {
        builder = builder
            .set("lane_root", VsfType::hb(root.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    // Mutation stamp (v7) — the replication ordering key.
    builder = builder
        .set(
            "mutated_osc",
            VsfType::e(vsf::types::EtType::e6(chains.mutated_osc)),
        )
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    // Era stamp — decides which lane_root supersedes across a re-key (merge_lanes_from).
    builder = builder
        .set(
            "genesis_osc",
            VsfType::e(vsf::types::EtType::e6(chains.genesis_osc)),
        )
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    // A COMPLETE VSF FILE, not a bare section (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY"). These bytes are not disk-only: `chains_to_vsf_bytes` also feeds fleet chain replication (photon_app.rs `push_chains_to_siblings`), sealed under the fleet key and pushed to every sibling, and the adopt path on the far side parses them back into live RATCHET STATE — chain keys, last plaintexts, mutation stamps. A bare section gave that path nothing to verify: the AEAD proves only "someone in the fleet wrote this", which the signed outer frame already proved. The header's BLAKE3 provenance hash is what makes the payload self-consistent.
    let section_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(CHAINS_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))
}

/// Load FriendshipChains from disk
pub fn load_friendship_chains(
    friendship_id: &FriendshipId,
    storage: &FlatStorage,
) -> Result<FriendshipChains, StorageError> {
    let vsf_bytes = storage
        .read_addr(&chains_key(friendship_id))?
        .ok_or_else(|| {
            StorageError::Parse(format!(
                "No chains found for friendship {}",
                hex::encode(&friendship_id.as_bytes()[..8])
            ))
        })?;

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, "friendship/chains");

    chains_from_vsf_bytes(&vsf_bytes)
}

// MIGRATION COMPLETE (v52→v56, deleted 2026-08-18 at v0.56.1): the pre-document chains fallback is gone — zero "MIGRATION: rewrote" lines across the fleet's recent submitted logs, exactly the evidence bar the block set for itself.

/// Decode FriendshipChains from their canonical VSF bytes — the inverse of chains_to_vsf_bytes, shared by the vault loader and the fleet chain-replication adopt path.
pub fn chains_from_vsf_bytes(vsf_bytes: &[u8]) -> Result<FriendshipChains, StorageError> {
    use crate::types::friendship::PendingMessage;

    // STRICT verified read, no fallback. `parse_document` runs `read_verified` (header decode + provenance self-consistency) before a single field is trusted.
    // This decoder is shared by the DISK loader and the fleet chain-replication ADOPT path, so it is the one that parses ratchet state arriving from another device — it must never accept a headerless blob. Pre-document VAULT files are handled by `migrate_pre_document_chains` at load time instead, which is disk-only and rewrites them on sight.
    let section = vsf::schema::SectionBuilder::parse_document(chains_schema(), vsf_bytes, None)
        .map_err(|e| StorageError::Parse(format!("chains failed verified read: {e}")))?;

    // Extract participants (handle hashes as hb)
    let mut participants: Vec<[u8; 32]> = Vec::new();
    for field in section.get_fields("participant") {
        if let Some(VsfType::hb(b)) = field.values.first() {
            if b.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                participants.push(arr);
            }
        }
    }

    if participants.is_empty() {
        return Err(StorageError::Parse("No participants found".to_string()));
    }

    // Lane labels (v8 additive) — their PRESENCE decides everything below: with labels, the chain / last_* multis are per-lane; without, this is a pre-lane blob whose per-participant copies are dead (the flag-day) and only its scalars survive.
    let mut lane_labels: Vec<[u8; 32]> = Vec::new();
    for field in section.get_fields("lane_label") {
        if let Some(VsfType::hb(b)) = field.values.first() {
            if let Ok(arr) = <[u8; 32]>::try_from(b.as_slice()) {
                lane_labels.push(arr);
            }
        }
    }
    let lane_positions: Vec<u64> = section
        .get_fields("lane_position")
        .iter()
        .filter_map(|f| f.values.first())
        .map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => (*osc).max(0) as u64,
            _ => 0,
        })
        .collect();
    let our_label: Option<[u8; 32]> = section.get_value::<[u8; 32]>("our_label").ok();

    // Chain bytes — per LANE when labels exist, ignored otherwise.
    let mut chain_bytes = Vec::new();
    for field in section.get_fields("chain") {
        if let Some(VsfType::v(b'C', data)) = field.values.first() {
            chain_bytes.extend(data);
        }
    }

    // === Hash chain state (v2) ===

    // last_sent_hash - optional (None if not present or never sent)
    let last_sent_hash: Option<[u8; 32]> = section.get_value::<[u8; 32]>("last_sent_hash").ok();

    // last_received_hashes - one per participant (empty hb = None/anchor expected)
    let mut last_received_hashes: Vec<Option<[u8; 32]>> = Vec::new();
    for field in section.get_fields("last_received_hash") {
        if let Some(v) = field.values.first() {
            let hash_opt = match v {
                VsfType::hp(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    Some(arr)
                }
                VsfType::hb(bytes) if bytes.is_empty() => None,
                _ => None,
            };
            last_received_hashes.push(hash_opt);
        }
    }

    // === Pending messages (v2) ===
    let eagle_times: Vec<i64> = section
        .get_fields("pending_eagle_time")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => Some(*osc),
            _ => None,
        })
        .collect();

    let plaintexts: Vec<Vec<u8>> = section
        .get_fields("pending_plaintext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::x(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        })
        .collect();

    let plaintext_hashes: Vec<[u8; 32]> = section
        .get_fields("pending_plaintext_hash")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
        .collect();

    let prev_msg_hps: Vec<[u8; 32]> = section
        .get_fields("pending_prev_msg_hp")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
        .collect();

    let msg_hps: Vec<[u8; 32]> = section
        .get_fields("pending_msg_hp")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
        .collect();

    let ciphertexts: Vec<Vec<u8>> = section
        .get_fields("pending_ciphertext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::v(b'X', data) => Some(data.clone()),
            _ => None,
        })
        .collect();

    // Persisted attempt counts — width-agnostic read (never variant-match a parsed integer). Absent (pre-field vaults) = the old restart behavior via the .get() fallback below; NOT folded into pending_count, or an old vault would zero the whole pending set.
    let attempts_persisted: Vec<u8> = section
        .get_fields("pending_attempts")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
        .collect();

    // Reconstruct pending messages (all arrays must have same length)
    let pending_count = eagle_times
        .len()
        .min(plaintexts.len())
        .min(plaintext_hashes.len())
        .min(prev_msg_hps.len())
        .min(msg_hps.len())
        .min(ciphertexts.len());

    let pending_messages: Vec<PendingMessage> = (0..pending_count)
        .map(|i| PendingMessage {
            eagle_time: eagle_times[i],
            plaintext: plaintexts[i].clone(),
            plaintext_hash: plaintext_hashes[i],
            prev_msg_hp: prev_msg_hps[i],
            msg_hp: msg_hps[i],
            ciphertext: ciphertexts[i].clone(),
            // Not persisted (runtime-only braid-strand snapshot). A pending message reloaded after restart weaves no strands; in practice pending messages are short-lived (cleared on ACK) so this edge only matters if the app restarts mid-flight with an unacked message AND its braid strands were non-empty — a known minor gap, not the steady-state desync this fix addresses.
            woven_strands: Vec::new(),
            // Attempts SURVIVE the restart (floor 1): exhaustion is cumulative lane evidence — resetting it every launch meant the anchor-wedge detector could never arm inside a short session and a dead lane stayed undiagnosed forever. The deadline is still immediate: a reloaded pending resends right away (or, if already exhausted, sits as the standing evidence the next sync record reads).
            attempts: attempts_persisted.get(i).copied().unwrap_or(1).max(1),
            next_retry_osc: eagle_times[i],
        })
        .collect();

    // === Bidirectional entropy state (v3) ===

    // last_received_weave - derived weave hash for mixing (32 bytes)
    let last_received_weave: Option<[u8; 32]> =
        section.get_value::<[u8; 32]>("last_received_weave").ok();

    // last_sent_weave - what we sent (what they received)
    let last_sent_weave: Option<[u8; 32]> = section.get_value::<[u8; 32]>("last_sent_weave").ok();

    // last_incorporated_hp - which of their messages we mixed in
    let last_incorporated_hp: Option<[u8; 32]> =
        section.get_value::<[u8; 32]>("last_incorporated_hp").ok();

    // === Last plaintexts (v4) - one per participant ===
    let last_plaintexts: Vec<Vec<u8>> = section
        .get_fields("last_plaintext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::x(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        })
        .collect();

    // === Last received times (v5) - one per participant ===
    let last_received_times: Vec<Option<i64>> = section
        .get_fields("last_received_time")
        .iter()
        .filter_map(|f| f.values.first())
        .map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) if *osc == 0 => None,
            VsfType::e(vsf::types::EtType::e6(osc)) => Some(*osc),
            _ => None,
        })
        .collect();

    // === Lanes flag-day gate (v8, docs/lanes.md): a pre-lanes blob is REJECTED, not adapted. Everything in it re-mints at the re-clutch the caller's chainless sweep triggers, and the same gate drops a stale sibling's replicated v7 bytes on the adopt path.
    let version: u8 = section.get_value::<u8>("version").unwrap_or(0);
    if version < 8 {
        return Err(StorageError::Parse(format!(
            "pre-lanes chains (v{version}) — flag-day: re-clutch re-mints"
        )));
    }

    // === History key (v6) — optional; absent (pre-v6 file) leaves None ===
    let history_key: Option<[u8; 32]> = section.get_value::<[u8; 32]>("history_key").ok();

    // === Mutation stamp (v7) — optional; absent (pre-v7 file) = 0, so any stamped replica beats it ===
    let mutated_osc: i64 = section.get_value::<i64>("mutated_osc").unwrap_or(0);
    let genesis_osc: i64 = section.get_value::<i64>("genesis_osc").unwrap_or(0);

    // The id rides IN the bytes (the encoder always writes it), so the decoder is self-contained — required by the replication path, where the bytes arrive off the wire with no vault address.
    let fid_bytes: [u8; 32] = section
        .get_value::<[u8; 32]>("friendship_id")
        .map_err(|e| StorageError::Parse(format!("friendship_id: {}", e)))?;
    let friendship_id = crate::types::friendship::FriendshipId::from_bytes(fid_bytes);

    // Reconstruct chains with full v5 state, then install the optional v6 key
    // A pre-lane blob's pendings were built for the retired per-participant wire — carrying them forward would retransmit frames nobody can decrypt. Undelivered rows re-send thru the held-messages path instead.
    let has_lanes = !lane_labels.is_empty();
    let pending_messages = if has_lanes {
        pending_messages
    } else {
        Vec::new()
    };

    let mut chains = FriendshipChains::from_storage_v5(
        friendship_id,
        participants,
        &[],
        last_sent_hash,
        Vec::new(),
        pending_messages,
        last_received_weave,
        last_sent_weave,
        last_incorporated_hp,
        Vec::new(),
        Vec::new(),
    )
    .ok_or_else(|| StorageError::Parse("Failed to reconstruct chains".to_string()))?;
    chains.set_history_key(history_key);
    chains.set_lane_root(section.get_value::<[u8; 32]>("lane_root").ok());
    chains.genesis_osc = genesis_osc;
    if has_lanes {
        use crate::crypto::chain::{Chain, CHAIN_SIZE};
        if chain_bytes.len() != lane_labels.len() * CHAIN_SIZE {
            return Err(StorageError::Parse(format!(
                "lane chain bytes mismatch: {} lanes, {} bytes",
                lane_labels.len(),
                chain_bytes.len()
            )));
        }
        let mut lane_chains = Vec::with_capacity(lane_labels.len());
        for i in 0..lane_labels.len() {
            let start = i * CHAIN_SIZE;
            let chain = Chain::from_full_bytes(&chain_bytes[start..start + CHAIN_SIZE])
                .ok_or_else(|| StorageError::Parse("lane chain malformed".to_string()))?;
            lane_chains.push(chain);
        }
        // Positions default to 0 when the field is short (never expected; harmless — a checkpoint merge treats it as furthest-behind).
        let mut positions = lane_positions;
        positions.resize(lane_labels.len(), 0);
        let n = lane_labels.len();
        let mut lrh = last_received_hashes;
        lrh.resize(n, None);
        let mut lpt = last_plaintexts;
        lpt.resize(n, Vec::new());
        let mut lrt = last_received_times;
        lrt.resize(n, None);
        chains.install_lanes(
            lane_labels,
            positions,
            lane_chains,
            lpt,
            lrh,
            lrt,
            our_label,
        );
    }
    chains.mutated_osc = mutated_osc;
    Ok(chains)
}

/// Load all friendships for the given friendship IDs
pub fn load_all_friendships(
    friendship_ids: &[FriendshipId],
    storage: &FlatStorage,
) -> Vec<(FriendshipId, FriendshipChains)> {
    let mut result = Vec::new();

    for friendship_id in friendship_ids {
        match load_friendship_chains(friendship_id, storage) {
            Ok(chains) => {
                result.push((*friendship_id, chains));
            }
            Err(e) => {
                crate::logf!(
                    "Failed to load friendship {}: {}",
                    hex::encode(&friendship_id.as_bytes()[..8]),
                    e
                );
            }
        }
    }

    result
}

/// Delete friendship chains from disk (used on re-key)
pub fn delete_friendship_chains(
    friendship_id: &FriendshipId,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    storage.delete_addr(&chains_key(friendship_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical chain bytes must be a COMPLETE VSF FILE (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY"). These same bytes are sealed under the fleet key and pushed to siblings, whose adopt path parses them back into live ratchet state — so the payload needs its own provenance anchor, not just the AEAD (which proves only "someone in the fleet wrote this").
    #[test]
    fn chain_bytes_are_a_complete_vsf_document() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        let bytes = chains_to_vsf_bytes(&chains).expect("encode");
        assert!(
            bytes.starts_with(b"R\xc3\x85<"),
            "must carry the R\u{c5}< magic, got {:?}",
            &bytes[..bytes.len().min(8)]
        );
        let (header, _) =
            vsf::verification::read_verified(&bytes, None).expect("read_verified must accept it");
        assert!(
            matches!(header.provenance_hash, vsf::VsfType::hp(ref h) if h.len() == 32),
            "chains without a 32-byte hp have nothing to verify"
        );

        let back = chains_from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(back.participants(), chains.participants());
    }

    /// Pending attempts SURVIVE the round trip: exhaustion is cumulative lane evidence (the anchor-wedge arming gate), and a restart resetting it meant a dead lane could never be diagnosed inside short sessions. Real encode→decode, so the width-agnostic read is exercised too.
    #[test]
    fn pending_attempts_survive_the_round_trip() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        chains
            .prepare_send(b"hold".to_vec(), b"hold".to_vec(), 1_000_000, vec![])
            .expect("send");
        // Drive some attempts onto the pending, then round-trip.
        for k in 1..5 {
            let _ = chains.collect_due_retransmits(1_000_000 + k * 60 * vsf::OSCILLATIONS_PER_SECOND as i64);
        }
        let attempts_before = chains.pending_messages().first().expect("pending").attempts;
        assert!(attempts_before > 1, "retransmits must have counted");
        let bytes = chains_to_vsf_bytes(&chains).expect("encode");
        let back = chains_from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(
            back.pending_messages().first().expect("pending").attempts,
            attempts_before,
            "a restart must not amnesty a dying lane"
        );
    }

    /// The SHARED decoder — the one the fleet chain-replication adopt path uses — must be strict. A headerless blob arriving from another device must never be parsed into live ratchet state; that is what the document wrapper exists to prevent.
    #[test]
    fn shared_decoder_rejects_a_headerless_blob() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        let doc = chains_to_vsf_bytes(&chains).expect("encode");
        let section = vsf::schema::SectionBuilder::parse_document(chains_schema(), &doc, None)
            .expect("parse our own document");
        let bare = section.encode().expect("bare section");

        assert!(
            chains_from_vsf_bytes(&bare).is_err(),
            "the network-facing decoder must refuse a blob with no provenance hash"
        );
    }

    /// The pre-document migration is DELETED (expired v56, removed at v0.56.1 with zero field evidence of remaining legacy blobs) — the vault load is strict now, same as the network decoder. A bare-section blob fails loudly instead of being laundered into live ratchet state.
    #[test]
    fn pre_document_vault_blob_now_fails_strict() {
        // A distinct seed from the other tests in this module so the two vaults can't collide.
        let storage =
            FlatStorage::new(crate::storage::APP, [0xC1; 32], [0xC2; 32]).expect("storage");

        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let fid = chains.id();

        // Plant a PRE-DOCUMENT blob at the chains address, exactly as a v51-era build left it.
        let doc = chains_to_vsf_bytes(&chains).expect("encode");
        let section = vsf::schema::SectionBuilder::parse_document(chains_schema(), &doc, None)
            .expect("parse");
        let bare = section.encode().expect("bare section");
        storage
            .write_addr(&chains_key(&fid), &bare)
            .expect("plant legacy blob");

        assert!(
            load_friendship_chains(&fid, &storage).is_err(),
            "post-migration, a headerless vault blob must fail the strict read — no silent fallback"
        );
    }

    #[test]
    fn test_friendship_storage_roundtrip() {
        // Create test chains
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        // Populate real lane state: our minted lane (advanced once) plus a received lane.
        let ours = chains.mint_our_lane().unwrap();
        let theirs = [0x77u8; 32];
        chains.ensure_lane(&theirs).unwrap();
        let et = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        chains.advance(&ours, &et, &[9u8; 16], &[]);

        let test_seed = [0xAA; 32];
        let device_secret = [0xBB; 32];

        crate::storage::isolate_test_storage();
        let storage = FlatStorage::new(crate::storage::APP, test_seed, device_secret).unwrap();

        // Save
        save_friendship_chains(&chains, &storage).unwrap();

        // Load
        let loaded = load_friendship_chains(chains.id(), &storage).unwrap();

        // Verify: identity layer, both lanes byte-for-byte, positions, and OUR label all survive.
        assert_eq!(loaded.id().as_bytes(), chains.id().as_bytes());
        assert_eq!(loaded.participants(), chains.participants());
        assert_eq!(
            loaded.current_key(&ours).unwrap(),
            chains.current_key(&ours).unwrap()
        );
        assert_eq!(
            loaded.current_key(&theirs).unwrap(),
            chains.current_key(&theirs).unwrap()
        );
        assert_eq!(loaded.lane_position(&ours), Some(1));
        assert_eq!(loaded.lane_position(&theirs), Some(0));
        assert_eq!(loaded.our_label(), chains.our_label());
        // v6: the history key derived at ceremony birth must survive the round-trip.
        assert!(chains.history_key().is_some());
        assert_eq!(loaded.history_key(), chains.history_key());
        // v8: the lane root rides the same round-trip — the lanes a device mints later all grow from it.
        assert!(chains.lane_root().is_some());
        assert_eq!(loaded.lane_root(), chains.lane_root());
    }

    /// The lane root has the same both-sides birth property as the history key, and the two must be UNRELATED values (distinct domains over the same input).
    #[test]
    fn lane_root_deterministic_and_domain_separated() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let a_side = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let b_side = FriendshipChains::from_clutch(&[bob, alice], &eggs);
        assert_eq!(a_side.lane_root(), b_side.lane_root());
        assert!(a_side.lane_root().is_some());
        assert_ne!(a_side.lane_root(), a_side.history_key());

        let other_eggs: Vec<[u8; 32]> = (0..8).map(|i| [(i + 100) as u8; 32]).collect();
        let rekeyed = FriendshipChains::from_clutch(&[alice, bob], &other_eggs);
        assert_ne!(a_side.lane_root(), rekeyed.lane_root());
    }

    #[test]
    fn history_key_deterministic_both_sides() {
        // The both-sides property: identical participants + eggs (what CLUTCH guarantees at completion) → identical history keys; different eggs → different keys.
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let a_side = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let b_side = FriendshipChains::from_clutch(&[bob, alice], &eggs); // reversed order — sorted internally
        assert_eq!(a_side.history_key(), b_side.history_key());
        assert!(a_side.history_key().is_some());

        let other_eggs: Vec<[u8; 32]> = (0..8).map(|i| [(i + 100) as u8; 32]).collect();
        let rekeyed = FriendshipChains::from_clutch(&[alice, bob], &other_eggs);
        assert_ne!(a_side.history_key(), rekeyed.history_key());

        // And it must differ from the conversation token (domain separation actually separates).
        assert_ne!(a_side.history_key().unwrap(), &a_side.conversation_token);
    }
}
