//! Friendship chain storage.
//!
//! Stores FriendshipChains as a single vault entry at `vault_key("chains", friendship_id)` — a flat 32-byte address, never a path. This is chain *state* (the ratchet machinery), not conversation *content*; content lives in the rārangi conversation DB.
//!
//! All encryption, addressing, and atomicity is handled by FlatStorage.

use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::{VsfSection, VsfType};

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
fn chains_schema() -> SectionSchema {
    SectionSchema::new("friendship_chains")
        .field("version", TypeConstraint::AnyUnsigned)
        .field("friendship_id", TypeConstraint::AnyHash)
        .field("participant", TypeConstraint::AnyHash) // One per participant (handle_hash as hb)
        .field("chain", TypeConstraint::Wrapped(b'C')) // vC: CLUTCH chain (512×32) per participant
        // Hash chain state (v2)
        .field("last_sent_hash", TypeConstraint::AnyHash) // hp type: last msg_hp we sent
        .field("last_received_hash", TypeConstraint::AnyHash) // One per participant (hp or empty hb)
        // Pending messages (v2) - each message has 6 fields
        .field("pending_eagle_time", TypeConstraint::Any)
        .field("pending_plaintext", TypeConstraint::Wrapped(b'P')) // vP: RAW plaintext bytes (the flattened VSF payload — full of binary hp/pad, NOT valid UTF-8; storing as x lossily mangled it to U+FFFD and desynced the chain)
        .field("pending_plaintext_hash", TypeConstraint::AnyHash) // hp
        .field("pending_prev_msg_hp", TypeConstraint::AnyHash) // hp
        .field("pending_msg_hp", TypeConstraint::AnyHash) // hp
        .field("pending_ciphertext", TypeConstraint::Wrapped(b'X')) // vX: ciphertext bytes
        // Bidirectional entropy state (v3)
        .field("last_received_weave", TypeConstraint::AnyHash) // hp: derived weave hash (32 bytes)
        .field("last_sent_weave", TypeConstraint::AnyHash) // hp: what we sent (what they received)
        .field("last_incorporated_hp", TypeConstraint::AnyHash) // hp: which of theirs we mixed in
        // Last plaintexts (v4) - needed for salt derivation after restart
        .field("last_plaintext", TypeConstraint::Wrapped(b'P')) // vP: RAW plaintext bytes (salt + braid weave ingredient — must round-trip byte-identical; see pending_plaintext)
        // Last received times (v5) - for duplicate detection after restart
        .field("last_received_time", TypeConstraint::Any) // i64 oscillations, one per participant
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

    // Build VSF section
    let schema = chains_schema();
    let mut builder = schema
        .build()
        .set("version", 5u8) // v5: includes last_received_times for duplicate detection after restart
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set(
            "friendship_id",
            VsfType::hb(friendship_id.as_bytes().to_vec()),
        )
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    // Add each participant's handle_hash and their chain (vC with 512×32 tensor data)
    for participant in chains.participants() {
        builder = builder
            .append_multi("participant", vec![VsfType::hb(participant.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;

        // Get this participant's chain as 512×32 tensor bytes
        let chain = chains
            .chain(participant)
            .ok_or_else(|| StorageError::Parse("Missing chain".to_string()))?;
        let chain_bytes = chain.to_bytes();

        // Store as vC (CLUTCH chain) - internally it's a 512×32 u8 tensor
        builder = builder
            .append_multi("chain", vec![VsfType::v(b'C', chain_bytes)])
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
        builder = builder
            .append_multi(
                "pending_eagle_time",
                vec![VsfType::e(vsf::types::EtType::e6(pending.eagle_time))],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?
            // RAW bytes via vP — NEVER from_utf8_lossy (the plaintext is binary; lossy conversion
            // mangles non-UTF-8 bytes to U+FFFD and desyncs the chain on reload).
            .append_multi(
                "pending_plaintext",
                vec![VsfType::v(b'P', pending.plaintext.clone())],
            )
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
        // RAW bytes via vP — see pending_plaintext above; lossy UTF-8 would desync salt + braid weave.
        builder = builder
            .append_multi("last_plaintext", vec![VsfType::v(b'P', plaintext.clone())])
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

    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    storage.write_addr(&chains_key(friendship_id), &vsf_bytes)
}

/// Load FriendshipChains from disk
pub fn load_friendship_chains(
    friendship_id: &FriendshipId,
    storage: &FlatStorage,
) -> Result<FriendshipChains, StorageError> {
    use crate::types::friendship::PendingMessage;

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

    // Parse VSF
    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| StorageError::Parse(format!("VSF parse: {}", e)))?;

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

    // Extract chain bytes (vC per participant, 512×32 = 16KB each)
    let mut chain_bytes = Vec::new();
    for field in section.get_fields("chain") {
        if let Some(VsfType::v(b'C', data)) = field.values.first() {
            chain_bytes.extend(data);
        }
    }
    if chain_bytes.is_empty() {
        return Err(StorageError::Parse("Missing chain data".to_string()));
    }

    // === Hash chain state (v2) ===

    // last_sent_hash - optional (None if not present or never sent)
    let last_sent_hash: Option<[u8; 32]> = section
        .get_field("last_sent_hash")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

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
            VsfType::v(b'P', data) => Some(data.clone()), // raw bytes (current)
            VsfType::x(s) => Some(s.as_bytes().to_vec()), // legacy lossy-UTF-8 form
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
            // Not persisted (runtime-only braid-strand snapshot). A pending message reloaded after
            // restart weaves no strands; in practice pending messages are short-lived (cleared on ACK) so
            // this edge only matters if the app restarts mid-flight with an unacked message AND its braid
            // strands were non-empty — a known minor gap, not the steady-state desync this fix addresses.
            woven_strands: Vec::new(),
            // Reliability state is runtime-only. A pending message reloaded after restart is eligible
            // to resend immediately (attempts reset to 1, deadline = its eagle_time so it's already due).
            attempts: 1,
            next_retry_osc: eagle_times[i],
        })
        .collect();

    // === Bidirectional entropy state (v3) ===

    // last_received_weave - derived weave hash for mixing (32 bytes)
    let last_received_weave: Option<[u8; 32]> = section
        .get_field("last_received_weave")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // last_sent_weave - what we sent (what they received)
    let last_sent_weave: Option<[u8; 32]> = section
        .get_field("last_sent_weave")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // last_incorporated_hp - which of their messages we mixed in
    let last_incorporated_hp: Option<[u8; 32]> = section
        .get_field("last_incorporated_hp")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // === Last plaintexts (v4) - one per participant ===
    let last_plaintexts: Vec<Vec<u8>> = section
        .get_fields("last_plaintext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::v(b'P', data) => Some(data.clone()), // raw bytes (current)
            VsfType::x(s) => Some(s.as_bytes().to_vec()), // legacy lossy-UTF-8 form
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

    // Reconstruct chains with full v5 state
    FriendshipChains::from_storage_v5(
        *friendship_id,
        participants,
        &chain_bytes,
        last_sent_hash,
        last_received_hashes,
        pending_messages,
        last_received_weave,
        last_sent_weave,
        last_incorporated_hp,
        last_plaintexts,
        last_received_times,
    )
    .ok_or_else(|| StorageError::Parse("Failed to reconstruct chains".to_string()))
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
                crate::log(&format!(
                    "Failed to load friendship {}: {}",
                    hex::encode(&friendship_id.as_bytes()[..8]),
                    e
                ));
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

    #[test]
    fn test_friendship_storage_roundtrip() {
        // Create test chains
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        let test_seed = [0xAA; 32];
        let device_secret = [0xBB; 32];

        let storage = FlatStorage::new(crate::storage::APP, test_seed, device_secret).unwrap();

        // Save
        save_friendship_chains(&chains, &storage).unwrap();

        // Load
        let loaded = load_friendship_chains(chains.id(), &storage).unwrap();

        // Verify
        assert_eq!(loaded.id().as_bytes(), chains.id().as_bytes());
        assert_eq!(loaded.participants(), chains.participants());
        assert_eq!(
            loaded.current_key(&alice).unwrap(),
            chains.current_key(&alice).unwrap()
        );
        assert_eq!(
            loaded.current_key(&bob).unwrap(),
            chains.current_key(&bob).unwrap()
        );
    }
}
