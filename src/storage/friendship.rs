//! Friendship chain storage.
//!
//! Stores FriendshipChains as a single vault entry at `vault_key("chains", friendship_id)` — a flat 32-byte address, never a path. This is chain *state* (the ratchet machinery), not conversation *content*; content lives in the rārangi conversation DB.
//!
//! All encryption, addressing, and atomicity is handled by FlatStorage.

use vsf::VsfType;

use crate::storage::{FlatStorage, StorageError};
use crate::types::{FriendshipChains, FriendshipId};

/// The chains document's section names, shared by the writer and the reader — the two must never drift.
/// One `friendship_chains` section holds the friendship's scalars; every lane, pending message and KEM bundle is its OWN section beside it (the 2026-09-25 flag day — index-aligned columns are gone), so a record's fields travel together.
const CHAINS_SECTION: &str = "friendship_chains";
const LANE_SECTION: &str = "lane";
const PENDING_SECTION: &str = "pending";
const KEM_SECTION: &str = "kem";

/// The chains codec's version: 10 = one section per record. The strict decoder refuses anything else; the v8/v9 column shape is read only by the disk migration (legacy_columns.rs).
const CHAINS_VERSION: u8 = 10;

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
    // GROWTH CONFESSION #2 (field 2026-08-28): the settings confession stayed SILENT while the 920ee297 monster grew 6.4→7.9MB with 6.8s puts — so the settings blob is cleared and this one is the next suspect with means (persists on every ACK — the observed put PAIRS) and motive (gap_buffer grows on every "buffering (ahead of us)" frame, and wedge-era lanes buffer forever). Name the component that holds the bytes.
    if vsf_bytes.len() > 512 * 1024 {
        crate::logf!(
            "CHAINS: blob for {} is {} bytes — {} pending, {} gap-buffered, {} lane(s), {} plaintext(s)",
            hex::encode(&friendship_id.as_bytes()[..4]),
            vsf_bytes.len(),
            chains.pending_messages.len(),
            chains.gap_buffer_count(),
            chains.lane_heads().len(),
            chains.last_plaintexts().len()
        );
    }
    storage.write_addr(&chains_key(&friendship_id), &vsf_bytes)
}

/// Encode FriendshipChains to their canonical VSF bytes — the SAME encoding save_friendship_chains persists, reused verbatim by the fleet chain-replication push (the bytes are sealed under the fleet key and shipped to siblings, whose decoder is chains_from_vsf_bytes).
/// A COMPLETE VSF FILE (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY"): the adopt path on the far side parses these bytes back into live RATCHET STATE, and the header's BLAKE3 provenance hash is what makes the payload self-consistent.
pub fn chains_to_vsf_bytes(chains: &FriendshipChains) -> Result<Vec<u8>, StorageError> {
    let e6 = |v: i64| VsfType::e(vsf::types::EtType::e6(v));
    let uint = |v: u64| VsfType::u(v as usize, false); // WHY/PROOF: every photon target is 64-bit, so usize holds a u64 whole
    let f = |name: &str, v: VsfType| (name.to_string(), v);
    let mut main: Vec<(String, VsfType)> = vec![f("version", uint(CHAINS_VERSION as u64)), f("friendship_id", VsfType::hb(chains.id().as_bytes().to_vec()))];
    // Identity participants — a LIST of one kind (conversation membership), not a column zipped against another.
    for participant in chains.participants() {
        main.push(f("participant", VsfType::hb(participant.to_vec())));
    }
    if let Some(l) = chains.our_label() {
        main.push(f("our_label", VsfType::hb(l.to_vec())));
    }
    if let Some(h) = chains.last_sent_hash() {
        main.push(f("last_sent_hash", VsfType::hp(h.to_vec())));
    }
    if let Some(w) = chains.last_received_weave() {
        main.push(f("last_received_weave", VsfType::hp(w.to_vec())));
    }
    if let Some(w) = chains.last_sent_weave() {
        main.push(f("last_sent_weave", VsfType::hp(w.to_vec())));
    }
    if let Some(hp) = chains.last_incorporated_hp() {
        main.push(f("last_incorporated_hp", VsfType::hp(hp.to_vec())));
    }
    if let Some(k) = chains.history_key() {
        main.push(f("history_key", VsfType::hb(k.to_vec())));
    }
    if let Some(root) = chains.lane_root() {
        main.push(f("lane_root", VsfType::hb(root.to_vec())));
    }
    // The replication ordering key, and the era stamp that decides which lane_root supersedes across a re-key.
    main.push(f("mutated_osc", e6(chains.mutated_osc)));
    main.push(f("genesis_osc", e6(chains.genesis_osc)));
    main.push(f("era_index", uint(chains.era_index)));
    main.push(f("era_lineage", VsfType::hb(chains.era_lineage.to_vec())));
    main.push(f("rows_since_ratchet", uint(chains.rows_since_ratchet as u64)));
    if let Some(r) = chains.retired_era() {
        main.push(f("retired_index", uint(r.era_index)));
        main.push(f("retired_root", VsfType::hb(r.lane_root.to_vec())));
        main.push(f("retired_grace", uint(r.grace_left as u64)));
        if let Some(hk) = r.history_key {
            main.push(f("retired_history_key", VsfType::hb(hk.to_vec())));
        }
    }
    if let Some(pe) = chains.pending_era() {
        main.push(f("pending_index", uint(pe.era_index)));
        main.push(f("pending_root", VsfType::hb(pe.lane_root.to_vec())));
        if let Some(hk) = pe.history_key {
            main.push(f("pending_history_key", VsfType::hb(hk.to_vec())));
        }
        if let Some(o) = pe.resp_osc {
            main.push(f("pending_resp_osc", e6(o)));
        }
    }
    // GROUP state (docs/molecules.md): the flag marks a group's chain state; id = group id, token = group token.
    if chains.molecule {
        main.push(f("group", uint(1)));
        if !chains.molecule_roster().is_empty() {
            main.push(f("molecule_roster", VsfType::hR(chains.molecule_roster().to_vec())));
        }
    }
    let mut doc = vsf::VsfBuilder::new().creation_time_oscillations(vsf::eagle_time_oscillations()).provenance_only().add_section(CHAINS_SECTION, main);

    // One section per LANE. The in-memory per-lane vectors are kept length-equal by install_lanes, so `.get(i)` finds each lane's own entry; an absent receipt stays absent (no zero sentinel).
    for (i, (label, position)) in chains.lane_summary().into_iter().enumerate() {
        let chain = chains.chain(&label).ok_or_else(|| StorageError::Parse("Missing lane chain".to_string()))?;
        let era = chains.lane_era(&label).or_else(|| chains.era_tag().map(u64::from)).unwrap_or(0);
        let mut lane = vec![f("label", VsfType::hb(label.to_vec())), f("position", uint(position)), f("chain", VsfType::v(b'C', chain.to_bytes())), f("era", uint(era))];
        if let Some(Some(h)) = chains.last_received_hashes().get(i) {
            lane.push(f("last_received_hash", VsfType::hp(h.to_vec())));
        }
        // x-text only (the salt source) — valid UTF-8, lossless as x.
        if let Some(pt) = chains.last_plaintexts().get(i) {
            lane.push(f("last_plaintext", VsfType::x(String::from_utf8_lossy(pt).into_owned())));
        }
        if let Some(Some(t)) = chains.last_received_times().get(i) {
            lane.push(f("last_received_time", e6(*t)));
        }
        doc = doc.add_section(LANE_SECTION, lane);
    }

    // One section per PENDING message. pending.plaintext is the message x-text only (the salt/weave ingredient), valid UTF-8, lossless as x.
    for p in chains.pending_messages() {
        let mut rec = vec![
            f("eagle_time", e6(p.eagle_time)),
            f("plaintext", VsfType::x(String::from_utf8_lossy(&p.plaintext).into_owned())),
            f("plaintext_hash", VsfType::hp(p.plaintext_hash.to_vec())),
            f("prev_msg_hp", VsfType::hp(p.prev_msg_hp.to_vec())),
            f("msg_hp", VsfType::hp(p.msg_hp.to_vec())),
            f("ciphertext", VsfType::v(b'X', p.ciphertext.clone())),
            // Attempts SURVIVE restarts: exhaustion is cumulative evidence about the LANE (the anchor-wedge detector's arming gate).
            f("attempts", uint(p.attempts as u64)),
        ];
        if chains.molecule {
            rec.push(f("targets", VsfType::hR(p.targets.iter().flatten().copied().collect())));
            rec.push(f("acked", VsfType::hR(p.acked_by.iter().flatten().copied().collect())));
        }
        doc = doc.add_section(PENDING_SECTION, rec);
    }

    // One section per KEM bundle OUR device published (§3): the wrap carrying a new era's secret targets the bundle in our member record, possibly minted while we slept — so the secrets persist here, the same custody class as the lane links.
    if chains.molecule {
        for k in chains.molecule_kems() {
            doc = doc.add_section(
                KEM_SECTION,
                vec![
                    f("published_era", uint(k.published_era)),
                    f("bundle_id", VsfType::hb(k.bundle_id.to_vec())),
                    f("set", uint(k.kem_set as u64)),
                    f("mlkem_sk", VsfType::v(b'K', k.mlkem_sk.clone())),
                    f("x_sk", VsfType::hb(k.x_sk.to_vec())),
                    f("hqc_sk", VsfType::v(b'K', k.hqc_sk.clone())),
                ],
            );
        }
    }
    doc.build().map_err(StorageError::Parse)
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

    let mut chains = match chains_from_vsf_bytes(&vsf_bytes) {
        Ok(c) => c,
        // DISK-ONLY MIGRATION (legacy_columns.rs): this device's own v8/v9 blob, read once and rewritten in the record shape — refusing it would re-clutch every friendship on upgrade.
        Err(strict) => match crate::storage::legacy_columns::chains_from_v9_bytes(&vsf_bytes) {
            Ok(mut old) => {
                crate::storage::legacy_columns::migrate_embedded_roster(&mut old);
                save_friendship_chains(&old, storage)?;
                crate::logf!("MIGRATION: parallel-column chains for {} rewritten as record sections", hex::encode(&friendship_id.as_bytes()[..4]));
                old
            }
            Err(_) => return Err(strict),
        },
    };
    // LOAD-TIME SELF-HEAL for graveyard blobs minted before rotation learned to sweep (the 8.1MB / 490-lane Emma specimen, 2026-08-28): receipt-less non-active lanes are losslessly re-derivable from the root, so a poisoned blob trims here and the next persist shrinks it for good. Healthy blobs pay one cheap scan.
    let pruned = chains.prune_retired_lanes(8);
    if pruned > 0 {
        crate::logf!(
            "LANE: load-time GC pruned {} retired lane(s) from {} ({} bytes on disk before the trim)",
            pruned,
            hex::encode(&friendship_id.as_bytes()[..4]),
            vsf_bytes.len()
        );
    }
    Ok(chains)
}

/// Decode FriendshipChains from their canonical VSF bytes — the inverse of chains_to_vsf_bytes, shared by the vault loader and the fleet chain-replication adopt path.
/// STRICT verified read, no fallback: this is the decoder that parses ratchet state arriving from another device, so it never accepts a headerless blob or the retired column shape. A record missing a required field fails the whole blob loudly — a verified document cannot be torn, so a gap is a writer bug, never something to paper over.
pub fn chains_from_vsf_bytes(vsf_bytes: &[u8]) -> Result<FriendshipChains, StorageError> {
    use crate::crypto::chain::{Chain, CHAIN_SIZE};
    use crate::storage::record::Rec;
    use crate::types::friendship::PendingMessage;

    let sections = crate::storage::record::verified_sections(vsf_bytes, None).map_err(|e| StorageError::Parse(format!("chains failed verified read: {e}")))?;
    let main = sections.iter().find(|s| s.name == CHAINS_SECTION).map(Rec).ok_or_else(|| StorageError::Parse("chains: no friendship_chains section".to_string()))?;
    if main.uint("version") != Some(CHAINS_VERSION as u64) {
        return Err(StorageError::Parse(format!("chains codec v{:?} refused (this build reads v{})", main.uint("version"), CHAINS_VERSION)));
    }
    let of = |name: &'static str| sections.iter().filter(move |s| s.name == name).map(Rec);
    let torn = |what: &str| StorageError::Parse(format!("chains: a {what} record is missing a field"));

    // The id rides IN the bytes, so the decoder is self-contained — required by the replication path, where the bytes arrive off the wire with no vault address.
    let fid_bytes = main.h32("friendship_id").ok_or_else(|| torn("friendship_chains"))?;
    let friendship_id = FriendshipId::from_bytes(fid_bytes);
    let participants: Vec<[u8; 32]> = main
        .all("participant")
        .into_iter()
        .filter_map(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        })
        .collect();
    if participants.is_empty() {
        return Err(StorageError::Parse("No participants found".to_string()));
    }

    let (mut labels, mut positions, mut lane_chains, mut lane_eras) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut last_plaintexts, mut last_received_hashes, mut last_received_times) = (Vec::new(), Vec::new(), Vec::new());
    for l in of(LANE_SECTION) {
        let (Some(label), Some(position), Some(chain), Some(era)) = (l.h32("label"), l.uint("position"), l.wrapped("chain", b'C'), l.uint("era")) else {
            return Err(torn("lane"));
        };
        if chain.len() != CHAIN_SIZE {
            return Err(StorageError::Parse(format!("lane chain is {} bytes, not {}", chain.len(), CHAIN_SIZE)));
        }
        labels.push(label);
        positions.push(position);
        lane_chains.push(Chain::from_full_bytes(&chain).ok_or_else(|| StorageError::Parse("lane chain malformed".to_string()))?);
        lane_eras.push(era);
        last_plaintexts.push(l.text("last_plaintext").map(String::into_bytes).unwrap_or_default());
        last_received_hashes.push(l.hp32("last_received_hash"));
        last_received_times.push(l.osc("last_received_time"));
    }

    let split32 = |b: Vec<u8>| -> Vec<[u8; 32]> { b.chunks_exact(32).filter_map(|c| <[u8; 32]>::try_from(c).ok()).collect() };
    let mut pending_messages: Vec<PendingMessage> = Vec::new();
    for p in of(PENDING_SECTION) {
        let (Some(eagle_time), Some(plaintext), Some(plaintext_hash), Some(prev_msg_hp), Some(msg_hp), Some(ciphertext), Some(attempts)) =
            (p.osc("eagle_time"), p.text("plaintext"), p.hp32("plaintext_hash"), p.hp32("prev_msg_hp"), p.hp32("msg_hp"), p.wrapped("ciphertext", b'X'), p.uint("attempts"))
        else {
            return Err(torn("pending"));
        };
        pending_messages.push(PendingMessage {
            eagle_time,
            plaintext: plaintext.into_bytes(),
            plaintext_hash,
            prev_msg_hp,
            msg_hp,
            ciphertext,
            // Not persisted (runtime-only braid-strand snapshot): pending messages are short-lived (cleared on ACK), so a reload mid-flight weaves no strands — a known minor gap.
            woven_strands: Vec::new(),
            // WHY/PROOF: a stored count — a u8 on write, so anything wider is a foreign blob and reads as exhausted; 0 would claim a sent message was never tried, and every pending has been (floor 1).
            attempts: u8::try_from(attempts).unwrap_or(u8::MAX).max(1),
            next_retry_osc: eagle_time,
            targets: p.bytes("targets").map(split32).unwrap_or_default(),
            acked_by: p.bytes("acked").map(split32).unwrap_or_default(),
        });
    }
    // A chains blob with no lanes has nothing its pendings could retransmit on — undelivered rows re-send thru the held-messages path instead.
    let has_lanes = !labels.is_empty();
    if !has_lanes {
        pending_messages.clear();
    }

    let mut chains = FriendshipChains::from_storage_v5(
        friendship_id,
        participants,
        &[],
        main.hp32("last_sent_hash"),
        Vec::new(),
        pending_messages,
        main.hp32("last_received_weave"),
        main.hp32("last_sent_weave"),
        main.hp32("last_incorporated_hp"),
        Vec::new(),
        Vec::new(),
    )
    .ok_or_else(|| StorageError::Parse("Failed to reconstruct chains".to_string()))?;
    chains.set_history_key(main.h32("history_key"));
    let lane_root = main.h32("lane_root");
    chains.set_lane_root(lane_root);
    chains.genesis_osc = main.osc("genesis_osc").ok_or_else(|| torn("friendship_chains"))?;
    chains.era_index = main.uint("era_index").ok_or_else(|| torn("friendship_chains"))?;
    chains.era_lineage = main
        .h32("era_lineage")
        .or_else(|| lane_root.as_ref().map(crate::crypto::clutch::era_lineage))
        .ok_or_else(|| torn("friendship_chains"))?;
    // WHY/PROOF: a u32 on write — a wider stored value is a foreign blob, and saturating reads it as "due to ratchet now" rather than wrapping to a small count.
    chains.rows_since_ratchet = main.uint("rows_since_ratchet").map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
    if let (Some(idx), Some(root)) = (main.uint("retired_index"), main.h32("retired_root")) {
        chains.set_retired_era(crate::types::friendship::RetiredEra {
            era_index: idx,
            lane_root: root,
            history_key: main.h32("retired_history_key"),
            tag: crate::crypto::clutch::era_tag(&root),
            // WHY/PROOF: a u32 on write — as above, a wider value saturates to the longest grace instead of wrapping to none.
            grace_left: main.uint("retired_grace").map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX)),
        });
    }
    if let (Some(idx), Some(root)) = (main.uint("pending_index"), main.h32("pending_root")) {
        chains.install_pending(crate::types::friendship::PendingEra {
            era_index: idx,
            lane_root: root,
            history_key: main.h32("pending_history_key"),
            tag: crate::crypto::clutch::era_tag(&root),
            resp_osc: main.osc("pending_resp_osc"),
        });
    }
    if has_lanes {
        chains.install_lanes(labels, positions, lane_chains, last_plaintexts, last_received_hashes, last_received_times, main.h32("our_label"), lane_eras);
    }
    chains.mutated_osc = main.osc("mutated_osc").ok_or_else(|| torn("friendship_chains"))?;

    if main.uint("group").is_some_and(|v| v != 0) {
        chains.molecule = true;
        // The token a friendship derives from its participants is WRONG for a group (membership moves; the token must not): recompute the group token from the id, which IS the group id.
        chains.conversation_token = crate::types::molecule::MoleculeId(fid_bytes).token();
        let mut kems: Vec<crate::crypto::era::EraDecapKeys> = Vec::new();
        for k in of(KEM_SECTION) {
            let (Some(published_era), Some(bundle_id), Some(set), Some(mlkem_sk), Some(x_sk), Some(hqc_sk)) =
                (k.uint("published_era"), k.h32("bundle_id"), k.uint("set"), k.wrapped("mlkem_sk", b'K'), k.h32("x_sk"), k.wrapped("hqc_sk", b'K'))
            else {
                return Err(torn("kem"));
            };
            let kem_set = u8::try_from(set).map_err(|_| StorageError::Parse(format!("kem set {set} does not fit its byte")))?;
            kems.push(crate::crypto::era::EraDecapKeys { published_era, bundle_id, kem_set, mlkem_sk, x_sk, hqc_sk });
        }
        chains.set_molecule_kems(kems);
        if let Some(b) = main.bytes("molecule_roster") {
            chains.set_molecule_roster(b);
        }
    }
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
    /// FIELD 2026-09-09: every eagle-time and era numeral the writer stamps as `e6` must read back — `genesis_osc` came back 0 on every load, so two fresh eras of one friendship compared genesis 0 vs 0 and each side refused the other forever (the phone kept a dead era with Emma while the desktop rang).
    #[test]
    fn era_and_stamp_numerals_survive_the_round_trip() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = crate::types::friendship::FriendshipChains::from_clutch(&[a, b], &eggs);
        chains.genesis_osc = 2_561_124_741_904_843_776;
        chains.mutated_osc = 2_561_124_741_904_843_777;
        chains.era_index = 3;
        chains.rows_since_ratchet = 7;
        let old_root = *chains.lane_root().unwrap();
        chains.install_pending(crate::types::friendship::PendingEra { era_index: 4, lane_root: [9u8; 32], history_key: Some([8u8; 32]), tag: crate::crypto::clutch::era_tag(&[9u8; 32]), resp_osc: Some(4242) });
        chains.cut_over_to_pending().unwrap();
        assert_eq!(chains.era_index, 4);
        let bytes = super::chains_to_vsf_bytes(&chains).unwrap();
        let back = super::chains_from_vsf_bytes(&bytes).unwrap();
        assert_eq!(back.genesis_osc, chains.genesis_osc, "genesis_osc");
        assert_eq!(back.mutated_osc, chains.mutated_osc, "mutated_osc");
        assert_eq!(back.era_index, 4, "era_index");
        assert_eq!(back.era_lineage, chains.era_lineage);
        assert_eq!(back.rows_since_ratchet, 0, "cutover reset it");
        let r = back.retired_era().expect("the retired era loads");
        assert_eq!((r.era_index, r.lane_root, r.tag), (3, old_root, crate::crypto::clutch::era_tag(&old_root)));
        assert_eq!(r.grace_left, crate::types::friendship::RETIRED_ERA_GRACE_ROWS);
    }
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

    /// The main section's bytes alone, cut out of a document by its TOC entry — a headerless blob, the shape the strict decoders must refuse.
    fn bare_main_section(doc: &[u8]) -> Vec<u8> {
        let (header, _) = vsf::verification::read_verified(doc, None).expect("our own document verifies");
        let f = header.fields.iter().find(|f| f.name == CHAINS_SECTION).expect("main section in the TOC");
        doc[f.offset_bytes..f.offset_bytes + f.size_bytes].to_vec()
    }

    /// The SHARED decoder — the one the fleet chain-replication adopt path uses — must be strict. A headerless blob arriving from another device must never be parsed into live ratchet state; that is what the document wrapper exists to prevent.
    #[test]
    fn shared_decoder_rejects_a_headerless_blob() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        let doc = chains_to_vsf_bytes(&chains).expect("encode");
        let bare = bare_main_section(&doc);

        assert!(
            chains_from_vsf_bytes(&bare).is_err(),
            "the network-facing decoder must refuse a blob with no provenance hash"
        );
    }

    /// The pre-document migration is DELETED (expired v56, removed at v0.56.1 with zero field evidence of remaining legacy blobs) — the vault load is strict now, same as the network decoder. A bare-section blob fails loudly instead of being laundered into live ratchet state.
    #[test]
    fn pre_document_vault_blob_now_fails_strict() {
        // A distinct seed from the other tests in this module so the two vaults can't collide.
        crate::storage::isolate_test_storage();
        let storage =
            FlatStorage::new(crate::storage::APP, [0xC1; 32], [0xC2; 32]).expect("storage");

        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let fid = chains.id();

        // Plant a PRE-DOCUMENT blob at the chains address, exactly as a v51-era build left it.
        let doc = chains_to_vsf_bytes(&chains).expect("encode");
        let bare = bare_main_section(&doc);
        storage
            .write_addr(&chains_key(&fid), &bare)
            .expect("plant legacy blob");

        assert!(
            load_friendship_chains(&fid, &storage).is_err(),
            "post-migration, a headerless vault blob must fail the strict read — no silent fallback"
        );
    }

    /// The v9 GROUP blob round-trips: flag, group token (recomputed from the id, never participant-derived), root custody, a minted lane, and the persisted KEM decapsulation bundle — which must still OPEN A WRAP after the trip, because surviving a restart between publish and mint is the whole reason it persists (docs/molecules.md §3).
    #[test]
    fn group_chains_round_trip_with_kem_custody() {
        use crate::crypto::era::{era_decapsulate_molecule, era_encapsulate, era_keygen, KEM_SET_DEFAULT};
        use crate::types::molecule::MoleculeId;
        let gid = MoleculeId::from_nonce(&[0x5Au8; 32]);
        let root = [0x11u8; 32];
        let hk = [0x22u8; 32];
        let lineage = crate::crypto::clutch::era_lineage(&root);
        let members = [[1u8; 32], [2u8; 32], [3u8; 32]];
        let mut chains = FriendshipChains::from_molecule_root(gid, &members, root, hk, 0, lineage);
        let ours = chains.mint_our_lane().expect("lane mints off the delivered root");
        let et = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        chains.advance(&ours, &et, &[7u8; 16], &[]);
        let eph = era_keygen(1, 0, KEM_SET_DEFAULT);
        chains.push_molecule_kem(eph.export_decaps(0));

        let bytes = chains_to_vsf_bytes(&chains).expect("encode");
        let back = chains_from_vsf_bytes(&bytes).expect("decode");
        assert!(back.molecule, "the flag survives");
        assert_eq!(back.conversation_token, gid.token(), "group token, recomputed from the id");
        assert_eq!(back.id().as_bytes(), &gid.0);
        assert_eq!(back.lane_root(), chains.lane_root());
        assert_eq!(back.history_key(), chains.history_key());
        assert_eq!(back.era_lineage, lineage);
        assert_eq!(back.current_key(&ours), chains.current_key(&ours));
        assert_eq!(back.molecule_kems().len(), 1);
        let k = &back.molecule_kems()[0];
        assert_eq!((k.published_era, k.kem_set), (0, KEM_SET_DEFAULT));
        let (resp, f) = era_encapsulate(&eph.init_wire, KEM_SET_DEFAULT).expect("wrap");
        assert_eq!(era_decapsulate_molecule(k, &resp), Some(f), "the reloaded bundle opens the wrap");
        assert_eq!(k.bundle_id, eph.init_wire.bundle_id(), "the bundle id rides with the keys");
        // The roster rides the blob (step 6) and survives the round trip AND the replication subset; a per-member ACK ledger persists on a group pending.
        chains.set_molecule_roster(vec![0xAB; 40]);
        chains.prepare_send(b"hi".to_vec(), b"hi".to_vec(), 77, vec![]).unwrap();
        chains.set_pending_targets(77, vec![[2u8; 32], [3u8; 32]]);
        assert_eq!(chains.process_group_ack(77, [3u8; 32]), Vec::<i64>::new());
        let bytes = chains_to_vsf_bytes(&chains).expect("encode");
        let back = chains_from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(back.molecule_roster(), &[0xAB; 40][..]);
        assert_eq!(back.pending_progress(77), Some((1, 2)));
        assert_eq!(back.pending_unacked(77), Some(vec![[2u8; 32]]));
        let subset = chains.replication_subset(&[ours]);
        assert_eq!(subset.molecule_roster(), &[0xAB; 40][..], "replication carries membership with the keys");
    }

    /// A FRIENDSHIP blob carries no group state: no group flag, no KEM custody sections — and every lane is its own record section.
    #[test]
    fn a_friendship_blob_writes_no_group_fields() {
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[[1u8; 32], [2u8; 32]], &eggs);
        let bytes = chains_to_vsf_bytes(&chains).expect("encode");
        let sections = crate::storage::record::verified_sections(&bytes, None).expect("verified");
        let main = sections.iter().find(|s| s.name == CHAINS_SECTION).map(crate::storage::record::Rec).expect("main");
        assert_eq!(main.uint("version"), Some(CHAINS_VERSION as u64));
        assert!(main.uint("group").is_none(), "no group flag on a friendship");
        assert!(!sections.iter().any(|s| s.name == KEM_SECTION), "no KEM custody records either");
        assert_eq!(sections.iter().filter(|s| s.name == LANE_SECTION).count(), chains.lane_summary().len(), "one section per lane");
        let back = chains_from_vsf_bytes(&bytes).expect("decode");
        assert!(!back.molecule);
        assert!(back.molecule_kems().is_empty());
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
