//! DISK-ONLY MIGRATION (2026-09-25): the parallel-column codecs that the one-section-per-record flag day retired — roster v2 and chains v8/v9 — verbatim, read ONLY from this device's own vault and rewritten in the new shape on sight.
//! Never reachable from a wire arrival: replication, invite snapshots and roster posts go thru the strict new decoders, which refuse these shapes.
//! Why a reader survives at all: refusing the old vault shape would drop every friendship's ratchet (a fleet-wide re-clutch storm, undelivered rows stranded) and every group's roster on the first launch of the new build.
//! DELETE this module once submitted logs show no "MIGRATION: parallel-column" line for a release cycle — the same evidence bar the pre-document chains migration met (deleted 2026-08-18).

use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::VsfType;

use crate::storage::StorageError;
use crate::types::molecule::{BundleRecord, GenesisRecord, LeaveRecord, MemberRecord, MoleculeId, Roster, TitleRecord, VouchRecord};
use crate::types::FriendshipChains;

const ROSTER_SECTION_V2: &str = "molecule_roster";
const CHAINS_SECTION_V9: &str = "friendship_chains";

fn roster_schema_v2() -> SectionSchema {
    SectionSchema::new(ROSTER_SECTION_V2)
        .field("version", TypeConstraint::AnyUnsigned)
        .field("molecule_id", TypeConstraint::AnyHash)
        // Genesis (optional until the record arrives — an invitee's roster snapshot always carries it)
        .field("gen_founder", TypeConstraint::AnyHash)
        .field("gen_osc", TypeConstraint::Any)
        .field("gen_from_genesis", TypeConstraint::AnyUnsigned) // D5: 1 = history served from genesis, 0 = from join (a join mints an era)
        .field("gen_title", TypeConstraint::Utf8Text)
        .field("gen_sig", TypeConstraint::AnyHash) // hb 64: Ed25519 over GenesisRecord::signing_bytes
        .field("gen_signer", TypeConstraint::AnyHash)
        // Member rows, index-aligned multis (§4 Member) — subject-signed, newest-wins per party in the merge
        .field("m_party", TypeConstraint::AnyHash)
        .field("m_proof", TypeConstraint::AnyHash)
        .field("m_name", TypeConstraint::Utf8Text)
        .field("m_avatar", TypeConstraint::AnyHash)
        .field("m_osc", TypeConstraint::Any)
        .field("m_sponsor", TypeConstraint::AnyHash)
        .field("m_sig", TypeConstraint::AnyHash) // hb 64
        .field("m_signer", TypeConstraint::AnyHash)
        // Leave rows (§4 Leave) — testimony, never deleted
        .field("l_party", TypeConstraint::AnyHash)
        .field("l_osc", TypeConstraint::Any)
        .field("l_sig", TypeConstraint::AnyHash) // hb 64
        .field("l_signer", TypeConstraint::AnyHash)
        // v2 (2026-09-15): the shared title (single), per-device KEM bundles and vouches (index-aligned multis). A v1 blob simply lacks them.
        .field("t_party", TypeConstraint::AnyHash)
        .field("t_title", TypeConstraint::Utf8Text)
        .field("t_osc", TypeConstraint::Any)
        .field("t_sig", TypeConstraint::AnyHash) // hb 64
        .field("t_signer", TypeConstraint::AnyHash)
        .field("b_party", TypeConstraint::AnyHash)
        .field("b_device", TypeConstraint::AnyHash)
        .field("b_era", TypeConstraint::AnyUnsigned)
        .field("b_set", TypeConstraint::AnyUnsigned)
        .field("b_mlkem", TypeConstraint::Any) // hR public key
        .field("b_x", TypeConstraint::Any) // hR public key
        .field("b_hqc", TypeConstraint::Any) // hR public key
        .field("b_osc", TypeConstraint::Any)
        .field("b_sig", TypeConstraint::AnyHash) // hb 64
        .field("v_voucher", TypeConstraint::AnyHash)
        .field("v_subject", TypeConstraint::AnyHash)
        .field("v_osc", TypeConstraint::Any)
        .field("v_withdrawn", TypeConstraint::AnyUnsigned)
        .field("v_sig", TypeConstraint::AnyHash) // hb 64
        .field("v_signer", TypeConstraint::AnyHash)
}

/// The v2 roster decoder, verbatim — DISK ONLY (load_roster), never a wire arrival.
pub(super) fn roster_from_v2_bytes(vsf_bytes: &[u8]) -> Result<(MoleculeId, Roster), StorageError> {
    let section = vsf::schema::SectionBuilder::parse_document(roster_schema_v2(), vsf_bytes, None)
        .map_err(|e| StorageError::Parse(format!("roster failed verified read: {e}")))?;

    let gid_bytes: [u8; 32] = section
        .get_value::<[u8; 32]>("molecule_id")
        .map_err(|e| StorageError::Parse(format!("molecule_id: {e}")))?;
    let molecule_id = MoleculeId(gid_bytes);

    // Width-agnostic numeral reads (the writer stamps e6; never variant-match a parsed integer).
    let e6_of = |v: &VsfType| -> Option<i64> {
        match v {
            VsfType::e(vsf::types::EtType::e6(o)) => Some(*o),
            other => other.as_i64(),
        }
    };
    let hb32 = |v: &VsfType| -> Option<[u8; 32]> {
        match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    };
    let hb64 = |v: &VsfType| -> Option<[u8; 64]> {
        match v {
            VsfType::hb(b) => <[u8; 64]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    };
    let col32 = |name: &str| -> Vec<[u8; 32]> {
        section.get_fields(name).iter().filter_map(|f| f.values.first()).filter_map(hb32).collect()
    };
    let col64 = |name: &str| -> Vec<[u8; 64]> {
        section.get_fields(name).iter().filter_map(|f| f.values.first()).filter_map(hb64).collect()
    };
    let col_osc = |name: &str| -> Vec<i64> {
        section.get_fields(name).iter().filter_map(|f| f.values.first()).filter_map(e6_of).collect()
    };
    let col_text = |name: &str| -> Vec<String> {
        section
            .get_fields(name)
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    };
    let col_bytes = |name: &str| -> Vec<Vec<u8>> {
        section
            .get_fields(name)
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| match v {
                VsfType::hR(b) => Some(b.clone()),
                _ => None,
            })
            .collect()
    };
    let col_u = |name: &str| -> Vec<u64> {
        section.get_fields(name).iter().filter_map(|f| f.values.first()).filter_map(|v| v.as_u64()).collect()
    };

    let mut roster = Roster::default();

    if let Ok(founder) = section.get_value::<[u8; 32]>("gen_founder") {
        let sig = section
            .get_fields("gen_sig")
            .first()
            .and_then(|f| f.values.first())
            .and_then(hb64)
            .ok_or_else(|| StorageError::Parse("genesis without a signature".to_string()))?;
        roster.genesis = Some(GenesisRecord {
            molecule_id,
            founder,
            genesis_osc: section.get_fields("gen_osc").first().and_then(|f| f.values.first()).and_then(e6_of).unwrap_or(0),
            history_from_genesis: section
                .get_fields("gen_from_genesis")
                .first()
                .and_then(|f| f.values.first())
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                != 0,
            title: section.get_fields("gen_title").first().and_then(|f| f.values.first()).and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            }).unwrap_or_default(),
            signature: sig,
            signer_device: section.get_value::<[u8; 32]>("gen_signer").unwrap_or([0u8; 32]),
        });
    }

    // Member rows — a short or torn multi truncates to the complete rows; merge (not insert) so the loaded set obeys the same newest-wins law as the wire.
    let (mp, mh, mn, ma, mo, ms, msig, msd) = (
        col32("m_party"),
        col32("m_proof"),
        col_text("m_name"),
        col32("m_avatar"),
        col_osc("m_osc"),
        col32("m_sponsor"),
        col64("m_sig"),
        col32("m_signer"),
    );
    // WHY/PROOF: parallel columns decoded from a stored or received record — a malformed or truncated one can carry columns of different lengths, and the zip takes their common prefix instead of indexing past the shortest.
    let n = mp
        .len()
        .min(mh.len())
        .min(mn.len())
        .min(ma.len())
        .min(mo.len())
        .min(ms.len())
        .min(msig.len())
        .min(msd.len());
    for i in 0..n {
        roster.merge_member(MemberRecord {
            party: mp[i],
            handle_proof: mh[i],
            name: mn[i].clone(),
            avatar_pin: ma[i],
            signed_osc: mo[i],
            sponsor: ms[i],
            signature: msig[i],
            signer_device: msd[i],
        });
    }

    // Vouches BEFORE leaves: merge_vouch needs genesis (present above); standing is derived, so order among the rest is immaterial.
    let (vv, vs, vo, vw, vsig, vsd) = (col32("v_voucher"), col32("v_subject"), col_osc("v_osc"), col_u("v_withdrawn"), col64("v_sig"), col32("v_signer"));
    // WHY/PROOF: parallel columns decoded from a stored or received record — a malformed or truncated one can carry columns of different lengths, and the zip takes their common prefix instead of indexing past the shortest.
    let n = vv.len().min(vs.len()).min(vo.len()).min(vw.len()).min(vsig.len()).min(vsd.len());
    for i in 0..n {
        roster.merge_vouch(VouchRecord { voucher: vv[i], subject: vs[i], signed_osc: vo[i], withdrawn: vw[i] != 0, signature: vsig[i], signer_device: vsd[i] });
    }

    let (bp, bd, be, bs, bm, bx, bh, bo, bsig) = (
        col32("b_party"),
        col32("b_device"),
        col_u("b_era"),
        col_u("b_set"),
        col_bytes("b_mlkem"),
        col_bytes("b_x"),
        col_bytes("b_hqc"),
        col_osc("b_osc"),
        col64("b_sig"),
    );
    // WHY/PROOF: parallel columns decoded from a stored or received record — a malformed or truncated one can carry columns of different lengths, and the zip takes their common prefix instead of indexing past the shortest.
    let n = bp.len().min(bd.len()).min(be.len()).min(bs.len()).min(bm.len()).min(bx.len()).min(bh.len()).min(bo.len()).min(bsig.len());
    for i in 0..n {
        roster.merge_bundle(BundleRecord {
            party: bp[i],
            device: bd[i],
            published_era: be[i],
            kem_set: bs[i] as u8,
            mlkem_pk: bm[i].clone(),
            x_pk: bx[i].clone(),
            hqc_pk: bh[i].clone(),
            signed_osc: bo[i],
            signature: bsig[i],
        });
    }

    if let Ok(party) = section.get_value::<[u8; 32]>("t_party") {
        if let (Some(title), Some(osc), Some(sig), Ok(signer)) = (
            col_text("t_title").into_iter().next(),
            section.get_fields("t_osc").first().and_then(|f| f.values.first()).and_then(e6_of),
            section.get_fields("t_sig").first().and_then(|f| f.values.first()).and_then(hb64),
            section.get_value::<[u8; 32]>("t_signer"),
        ) {
            roster.merge_title(TitleRecord { party, title, signed_osc: osc, signature: sig, signer_device: signer });
        }
    }

    let (lp, lo, lsig, lsd) = (col32("l_party"), col_osc("l_osc"), col64("l_sig"), col32("l_signer"));
    // WHY/PROOF: parallel columns decoded from a stored or received record — a malformed or truncated one can carry columns of different lengths, and the zip takes their common prefix instead of indexing past the shortest.
    let n = lp.len().min(lo.len()).min(lsig.len()).min(lsd.len());
    for i in 0..n {
        roster.merge_leave(LeaveRecord {
            party: lp[i],
            signed_osc: lo[i],
            signature: lsig[i],
            signer_device: lsd[i],
        });
    }

    Ok((molecule_id, roster))
}

fn chains_schema_v9() -> SectionSchema {
    SectionSchema::new(CHAINS_SECTION_V9)
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
        // Era state (2026-09-08, additive): a blob without these is era 0 of the lineage derived from its own root, every lane in that era.
        .field("era_index", TypeConstraint::Any)
        .field("era_lineage", TypeConstraint::AnyHash)
        .field("lane_era", TypeConstraint::Any) // one per lane, INDEX-ALIGNED with lane_label: the tag of the root the lane derives from (was the era index before 2026-09-08; the loader maps those)
        .field("rows_since_ratchet", TypeConstraint::Any)
        .field("retired_index", TypeConstraint::Any)
        .field("retired_root", TypeConstraint::AnyHash)
        .field("retired_history_key", TypeConstraint::AnyHash)
        .field("retired_grace", TypeConstraint::Any)
        .field("pending_index", TypeConstraint::Any)
        .field("pending_root", TypeConstraint::AnyHash)
        .field("pending_history_key", TypeConstraint::AnyHash)
        .field("pending_resp_osc", TypeConstraint::Any)
        // GROUP state (v9, docs/molecules.md, additive: a friendship blob never writes these, so its bytes stay v8-identical and an old sibling adopts it unchanged). The flag marks the blob as a GROUP's chain state: id = group id, token = group token, participant set mutable behind the id.
        .field("group", TypeConstraint::AnyUnsigned)
        // OUR device's published KEM decapsulation bundles (§3), index-aligned rows: the wrap that carries a new era's fresh secret targets the bundle we published in our member record, possibly minted while we slept — so the secrets persist here, the same custody class as the lane links beside them.
        .field("pending_targets", TypeConstraint::Any) // hR: N×32 party ids this pending has to reach (group blobs only)
        .field("pending_acked", TypeConstraint::Any) // hR: N×32 party ids whose ACK arrived (group blobs only)
        .field("molecule_roster", TypeConstraint::Any) // hR: the roster-codec bytes (group blobs only) — membership rides replication with the keys
        .field("kem_published_era", TypeConstraint::Any)
        .field("kem_bundle_id", TypeConstraint::AnyHash) // hb 32: the public bundle's fingerprint (what a wrap names)
        .field("kem_set", TypeConstraint::AnyUnsigned)
        .field("kem_mlkem_sk", TypeConstraint::Wrapped(b'K')) // vK: ML-KEM-1024 decapsulation key (empty when the set excludes it)
        .field("kem_x_sk", TypeConstraint::AnyHash) // hb 32: X25519 secret scalar
        .field("kem_hqc_sk", TypeConstraint::Wrapped(b'K')) // vK: HQC-256 decapsulation key (empty when the set excludes it)
}

/// The v8/v9 chains decoder, verbatim — DISK ONLY (load_friendship_chains), never the replication adopt path.
pub(super) fn chains_from_v9_bytes(vsf_bytes: &[u8]) -> Result<FriendshipChains, StorageError> {
    use crate::types::friendship::PendingMessage;

    // STRICT verified read, no fallback. `parse_document` runs `read_verified` (header decode + provenance self-consistency) before a single field is trusted.
    // This decoder is shared by the DISK loader and the fleet chain-replication ADOPT path, so it is the one that parses ratchet state arriving from another device — it must never accept a headerless blob. Pre-document VAULT files are handled by `migrate_pre_document_chains` at load time instead, which is disk-only and rewrites them on sight.
    let section = vsf::schema::SectionBuilder::parse_document(chains_schema_v9(), vsf_bytes, None)
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
            VsfType::e(vsf::types::EtType::e6(osc)) => (*osc).max(0) as u64, // WHY/PROOF: a stamp read back from disk — i64 on the page; the u64 cast would wrap a negative
            _ => 0,
        })
        .collect();
    let our_label: Option<[u8; 32]> = section.get_value::<[u8; 32]>("our_label").ok();
    let lane_eras: Vec<u64> = section
        .get_fields("lane_era")
        .iter()
        .filter_map(|f| f.values.first())
        .map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => (*osc).max(0) as u64, // WHY/PROOF: as above
            _ => 0,
        })
        .collect();

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
    // WHY/PROOF: parallel columns decoded from a stored or received record — a malformed or truncated one can carry columns of different lengths, and the zip takes their common prefix instead of indexing past the shortest.
    let pending_count = eagle_times
        .len()
        .min(plaintexts.len())
        .min(plaintext_hashes.len())
        .min(prev_msg_hps.len())
        .min(msg_hps.len())
        .min(ciphertexts.len());

    let split32 = |name: &str| -> Vec<Vec<[u8; 32]>> {
        section
            .get_fields(name)
            .iter()
            .filter_map(|f| f.values.first())
            .map(|v| match v {
                VsfType::hR(b) => b.chunks_exact(32).map(|c| <[u8; 32]>::try_from(c).unwrap()).collect(),
                _ => Vec::new(),
            })
            .collect()
    };
    let (pending_targets, pending_acked) = (split32("pending_targets"), split32("pending_acked"));
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
            attempts: attempts_persisted.get(i).copied().unwrap_or(1).max(1), // WHY/PROOF: read back from disk — a 0 would claim a sent message was never tried, and the floor-1 rule above says every pending has
            next_retry_osc: eagle_times[i],
            targets: pending_targets.get(i).cloned().unwrap_or_default(), // WHY/PROOF: optional persisted columns — absent on chains saved before they existed
            acked_by: pending_acked.get(i).cloned().unwrap_or_default(), // (optional persisted column — see above)
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

    // The writer stamps every eagle-time and era numeral as `e6`; `get_value::<i64>` does not read that type back (it returned 0 for every one of them — genesis_osc lost on every load, so two fresh eras compared 0 vs 0 and refused each other forever, field 2026-09-09). Read the typed value explicitly, widening any plain integer a future writer might use.
    let e6_i64 = |name: &str| -> Result<i64, ()> {
        section
            .get_fields(name)
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::e(vsf::types::EtType::e6(o)) => Some(*o),
                other => other.as_i64(),
            })
            .ok_or(())
    };
    // === Mutation stamp (v7) — optional; absent (pre-v7 file) = 0, so any stamped replica beats it ===
    let mutated_osc: i64 = e6_i64("mutated_osc").unwrap_or(0);
    let genesis_osc: i64 = e6_i64("genesis_osc").unwrap_or(0);

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
    let lane_root = section.get_value::<[u8; 32]>("lane_root").ok();
    chains.set_lane_root(lane_root);
    chains.genesis_osc = genesis_osc;
    // Era state: absent = a pre-era blob — era 0 of the lineage its own root names, every lane in it.
    chains.era_index = e6_i64("era_index").map(|v| v.max(0) as u64).unwrap_or(0); // WHY/PROOF: stored i64 counters (era index, rows, grace) decode through a u64/u32 cast that would wrap a negative
    chains.era_lineage = section
        .get_value::<[u8; 32]>("era_lineage")
        .ok()
        .or_else(|| lane_root.as_ref().map(crate::crypto::clutch::era_lineage))
        .unwrap_or([0u8; 32]);
    chains.rows_since_ratchet = e6_i64("rows_since_ratchet").map(|v| v.max(0) as u32).unwrap_or(0); // (stored counter — see above)
    if let (Ok(idx), Ok(root)) = (e6_i64("retired_index"), section.get_value::<[u8; 32]>("retired_root")) {
        chains.set_retired_era(crate::types::friendship::RetiredEra {
            era_index: idx.max(0) as u64, // (stored counter — see above)
            lane_root: root,
            history_key: section.get_value::<[u8; 32]>("retired_history_key").ok(),
            tag: crate::crypto::clutch::era_tag(&root),
            grace_left: e6_i64("retired_grace").map(|v| v.max(0) as u32).unwrap_or(0), // (stored counter — see above)
        });
    }
    if let (Ok(idx), Ok(root)) = (e6_i64("pending_index"), section.get_value::<[u8; 32]>("pending_root")) {
        chains.install_pending(crate::types::friendship::PendingEra {
            era_index: idx.max(0) as u64, // (stored counter — see above)
            lane_root: root,
            history_key: section.get_value::<[u8; 32]>("pending_history_key").ok(),
            tag: crate::crypto::clutch::era_tag(&root),
            resp_osc: e6_i64("pending_resp_osc").ok(),
        });
    }
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
            lane_eras,
        );
    }
    chains.mutated_osc = mutated_osc;
    // === GROUP state (v9, docs/molecules.md) — absent on every friendship blob ===
    // Width-agnostic flag read (never variant-match a parsed integer).
    let is_molecule = section
        .get_fields("group")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        != 0;
    if is_molecule {
        chains.molecule = true;
        // The token a friendship derives from its participants is WRONG for a group (membership moves; the token must not): recompute the group token from the id, which IS the group id.
        chains.conversation_token = crate::types::molecule::MoleculeId(fid_bytes).token();
        // KEM bundle rows, index-aligned; a short or torn multi truncates to the complete rows.
        let eras: Vec<u64> = section
            .get_fields("kem_published_era")
            .iter()
            .filter_map(|f| f.values.first())
            .map(|v| match v {
                VsfType::e(vsf::types::EtType::e6(o)) => (*o).max(0) as u64, // WHY/PROOF: a stamp read back from disk, as above
                other => other.as_u64().unwrap_or(0),
            })
            .collect();
        let sets: Vec<u8> = section
            .get_fields("kem_set")
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect();
        let mlkems: Vec<Vec<u8>> = section
            .get_fields("kem_mlkem_sk")
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| match v {
                VsfType::v(b'K', d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        let xs: Vec<[u8; 32]> = section
            .get_fields("kem_x_sk")
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| match v {
                VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
                _ => None,
            })
            .collect();
        let hqcs: Vec<Vec<u8>> = section
            .get_fields("kem_hqc_sk")
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| match v {
                VsfType::v(b'K', d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        let bids: Vec<[u8; 32]> = section
            .get_fields("kem_bundle_id")
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| match v {
                VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
                _ => None,
            })
            .collect();
        // WHY/PROOF: parallel columns decoded from a stored or received record — a malformed or truncated one can carry columns of different lengths, and the zip takes their common prefix instead of indexing past the shortest.
        let n = eras.len().min(sets.len()).min(mlkems.len()).min(xs.len()).min(hqcs.len()).min(bids.len());
        let kems: Vec<crate::crypto::era::EraDecapKeys> = (0..n)
            .map(|i| crate::crypto::era::EraDecapKeys {
                published_era: eras[i],
                bundle_id: bids[i],
                kem_set: sets[i],
                mlkem_sk: mlkems[i].clone(),
                x_sk: xs[i],
                hqc_sk: hqcs[i].clone(),
            })
            .collect();
        chains.set_molecule_kems(kems);
        if let Some(VsfType::hR(b)) = section.get_fields("molecule_roster").first().and_then(|f| f.values.first()) {
            chains.set_molecule_roster(b.clone());
        }
    }
    Ok(chains)
}


/// A migrated GROUP chains blob still carries its roster in the v2 column shape — re-encode it, or every later read of the embedded roster (replication, boot) would refuse it.
pub(super) fn migrate_embedded_roster(chains: &mut FriendshipChains) {
    if !chains.molecule || chains.molecule_roster().is_empty() {
        return;
    }
    let Ok((gid, roster)) = roster_from_v2_bytes(chains.molecule_roster()) else {
        return;
    };
    match crate::storage::molecule::roster_to_vsf_bytes(&gid, &roster) {
        Ok(b) => chains.set_molecule_roster(b),
        Err(e) => crate::logf!("MIGRATION: parallel-column roster inside chains for {} would not re-encode: {}", hex::encode(&gid.0[..4]), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A v8 column-shaped chains blob in this device's vault loads thru the migration, keeps its ratchet (same lane key), and is REWRITTEN in the record shape — while the strict wire decoder refuses the same bytes.
    #[test]
    fn a_v8_column_blob_on_disk_migrates_and_the_wire_refuses_it() {
        crate::storage::isolate_test_storage();
        let storage = crate::storage::FlatStorage::new(crate::storage::APP, [0xD1; 32], [0xD2; 32]).expect("storage");
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[[1u8; 32], [2u8; 32]], &eggs);
        chains.mint_our_lane().expect("a lane mints off the clutch root");
        assert!(!chains.lane_summary().is_empty());
        let fid = chains.id();
        let e6 = |v: i64| VsfType::e(vsf::types::EtType::e6(v));
        let mut b = chains_schema_v9()
            .build()
            .set("version", 8u8)
            .unwrap()
            .set("friendship_id", VsfType::hb(fid.as_bytes().to_vec()))
            .unwrap()
            .set("mutated_osc", e6(5))
            .unwrap()
            .set("genesis_osc", e6(6))
            .unwrap()
            .set("era_index", e6(0))
            .unwrap()
            .set("era_lineage", VsfType::hb(chains.era_lineage.to_vec()))
            .unwrap();
        if let Some(root) = chains.lane_root() {
            b = b.set("lane_root", VsfType::hb(root.to_vec())).unwrap();
        }
        for p in chains.participants() {
            b = b.append_multi("participant", vec![VsfType::hb(p.to_vec())]).unwrap();
        }
        for (label, position) in chains.lane_summary() {
            b = b
                .append_multi("lane_label", vec![VsfType::hb(label.to_vec())])
                .unwrap()
                .append_multi("lane_position", vec![e6(position as i64)])
                .unwrap()
                .append_multi("chain", vec![VsfType::v(b'C', chains.chain(&label).unwrap().to_bytes())])
                .unwrap()
                .append_multi("lane_era", vec![e6(chains.lane_era(&label).unwrap_or(0) as i64)])
                .unwrap()
                .append_multi("last_plaintext", vec![VsfType::x(String::new())])
                .unwrap()
                .append_multi("last_received_hash", vec![VsfType::hb(Vec::new())])
                .unwrap()
                .append_multi("last_received_time", vec![e6(0)])
                .unwrap();
        }
        let old = vsf::VsfBuilder::new().creation_time_oscillations(1).provenance_only().add_unboxed(CHAINS_SECTION_V9, b.encode().unwrap()).build().unwrap();
        assert!(crate::storage::friendship::chains_from_vsf_bytes(&old).is_err(), "the wire decoder refuses the column shape");
        let addr = crate::storage::vault_key("chains", fid.as_bytes());
        storage.write_addr(&addr, &old).unwrap();

        let loaded = crate::storage::friendship::load_friendship_chains(&fid, &storage).expect("the disk migration reads it");
        assert!(!loaded.lane_summary().is_empty());
        for (label, _) in chains.lane_summary() {
            assert_eq!(loaded.current_key(&label), chains.current_key(&label), "the ratchet survives the migration");
        }
        assert_eq!(loaded.mutated_osc, 5);
        let rewritten = storage.read_addr(&addr).unwrap().unwrap();
        assert!(crate::storage::friendship::chains_from_vsf_bytes(&rewritten).is_ok(), "rewritten in the record shape on sight");
    }
}
