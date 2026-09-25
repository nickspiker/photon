//! Group roster storage (docs/molecules.md §5).
//!
//! The roster — genesis plus the merged sovereign record sets — persists as one vault entry at `vault_key("molecule", molecule_id)`, beside the group's chains blob at `vault_key("chains", molecule_id)`.
//! The SAME canonical bytes serve the vault and, later, the wire: roster records travel as control rows and re-serve pages, and a whole-roster snapshot rides the pairwise invite — so the codec is a complete VSF file with a provenance header, exactly like the chains blob.
//! Signatures persist verbatim: a loaded record is still verifiable against the folded device set, so storage is custody, never trust.

use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::VsfType;

use crate::storage::{FlatStorage, StorageError};
use crate::types::molecule::{BundleRecord, GenesisRecord, MoleculeId, LeaveRecord, MemberRecord, Roster, TitleRecord, VouchRecord};

/// The section name, shared by the builder and the TOC lookup — the two must never drift.
const ROSTER_SECTION: &str = "molecule_roster";

fn roster_schema() -> SectionSchema {
    SectionSchema::new(ROSTER_SECTION)
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

/// Vault address for a group's roster — beside its chains blob, same scope bytes.
fn roster_key(molecule_id: &MoleculeId) -> [u8; 32] {
    crate::storage::vault_key("molecule", &molecule_id.0)
}

/// Encode a roster to its canonical VSF bytes — vault entry, invite snapshot, and (later) re-serve payload alike.
pub fn roster_to_vsf_bytes(molecule_id: &MoleculeId, roster: &Roster) -> Result<Vec<u8>, StorageError> {
    let e6 = |v: i64| VsfType::e(vsf::types::EtType::e6(v));
    let mut builder = roster_schema()
        .build()
        .set("version", 2u8)
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("molecule_id", VsfType::hb(molecule_id.0.to_vec()))
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    if let Some(g) = roster.genesis.as_ref() {
        builder = builder
            .set("gen_founder", VsfType::hb(g.founder.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("gen_osc", e6(g.genesis_osc))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("gen_from_genesis", VsfType::u(g.history_from_genesis as usize, false))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("gen_title", VsfType::x(g.title.clone()))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("gen_sig", VsfType::hb(g.signature.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("gen_signer", VsfType::hb(g.signer_device.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    // Deterministic row order (sorted by party) so both sides of a replication compare equal bytes for equal rosters.
    let mut members: Vec<&MemberRecord> = roster.members.values().collect();
    members.sort_unstable_by_key(|m| m.party);
    for m in members {
        builder = builder
            .append_multi("m_party", vec![VsfType::hb(m.party.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_proof", vec![VsfType::hb(m.handle_proof.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_name", vec![VsfType::x(m.name.clone())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_avatar", vec![VsfType::hb(m.avatar_pin.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_osc", vec![e6(m.signed_osc)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_sponsor", vec![VsfType::hb(m.sponsor.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_sig", vec![VsfType::hb(m.signature.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("m_signer", vec![VsfType::hb(m.signer_device.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let mut leaves: Vec<&LeaveRecord> = roster.leaves.values().collect();
    leaves.sort_unstable_by_key(|l| l.party);
    for l in leaves {
        builder = builder
            .append_multi("l_party", vec![VsfType::hb(l.party.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("l_osc", vec![e6(l.signed_osc)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("l_sig", vec![VsfType::hb(l.signature.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("l_signer", vec![VsfType::hb(l.signer_device.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(t) = roster.title.as_ref() {
        builder = builder
            .set("t_party", VsfType::hb(t.party.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("t_title", VsfType::x(t.title.clone()))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("t_osc", e6(t.signed_osc))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("t_sig", VsfType::hb(t.signature.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .set("t_signer", VsfType::hb(t.signer_device.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let mut bundles: Vec<&BundleRecord> = roster.bundles.values().collect();
    bundles.sort_unstable_by_key(|b| (b.party, b.device));
    for b in bundles {
        builder = builder
            .append_multi("b_party", vec![VsfType::hb(b.party.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_device", vec![VsfType::hb(b.device.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_era", vec![VsfType::u(b.published_era as usize, false)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_set", vec![VsfType::u(b.kem_set as usize, false)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_mlkem", vec![VsfType::hR(b.mlkem_pk.clone())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_x", vec![VsfType::hR(b.x_pk.clone())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_hqc", vec![VsfType::hR(b.hqc_pk.clone())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_osc", vec![e6(b.signed_osc)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("b_sig", vec![VsfType::hb(b.signature.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let mut vouches: Vec<&VouchRecord> = roster.vouches.values().flatten().collect();
    vouches.sort_unstable_by_key(|v| (v.voucher, v.subject, v.signed_osc));
    for v in vouches {
        builder = builder
            .append_multi("v_voucher", vec![VsfType::hb(v.voucher.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("v_subject", vec![VsfType::hb(v.subject.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("v_osc", vec![e6(v.signed_osc)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("v_withdrawn", vec![VsfType::u(v.withdrawn as usize, false)])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("v_sig", vec![VsfType::hb(v.signature.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("v_signer", vec![VsfType::hb(v.signer_device.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let section_bytes = builder.encode().map_err(|e| StorageError::Parse(e.to_string()))?;
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(ROSTER_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))
}

/// Decode a roster from its canonical bytes — STRICT verified read, shared by the vault loader and every wire arrival (invite snapshot, replication), so a headerless blob never becomes membership state.
pub fn roster_from_vsf_bytes(vsf_bytes: &[u8]) -> Result<(MoleculeId, Roster), StorageError> {
    let section = vsf::schema::SectionBuilder::parse_document(roster_schema(), vsf_bytes, None)
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

/// The group-index section name, shared by the builder and the TOC lookup — the two must never drift.
const MOLECULE_LIST_SECTION: &str = "molecule_list";

/// Schema for the group index: one `group` field per group id. The vault is flat and content-addressed — nothing enumerates — so membership is discoverable at boot ONLY thru this list, exactly as contacts are thru theirs. Vault-internal (FlatStorage encrypts it); never travels.
fn molecule_list_schema() -> SectionSchema {
    SectionSchema::new(MOLECULE_LIST_SECTION).field("group", TypeConstraint::AnyHash)
}

/// Save the group index at `vault_key("molecules", vault_seed)` — a complete VSF document (provenance header + section), the same shape as the roster, so the load side is a verified read.
pub fn save_molecule_list(ids: &[MoleculeId], storage: &FlatStorage) -> Result<(), StorageError> {
    let mut builder = molecule_list_schema().build();
    for id in ids {
        builder = builder
            .append_multi("group", vec![VsfType::hb(id.0.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let section_bytes = builder.encode().map_err(|e| StorageError::Parse(e.to_string()))?;
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(MOLECULE_LIST_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    storage.write_addr(&crate::storage::vault_key("molecules", &storage.vault_seed()), &vsf_bytes)
}

/// Load the group index. Empty = this device belongs to no groups.
pub fn load_molecule_list(storage: &FlatStorage) -> Result<Vec<MoleculeId>, StorageError> {
    let Some(vsf_bytes) = storage.read_addr(&crate::storage::vault_key("molecules", &storage.vault_seed()))? else {
        return Ok(Vec::new());
    };
    let section = vsf::schema::SectionBuilder::parse_document(molecule_list_schema(), &vsf_bytes, None)
        .map_err(|e| StorageError::Parse(format!("group list failed verified read: {e}")))?;
    Ok(section
        .get_fields("group")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok().map(MoleculeId),
            _ => None,
        })
        .collect())
}

/// Add one group to the index iff absent. Returns whether the list changed (caller persists rosters/chains beside it).
pub fn index_molecule(molecule_id: &MoleculeId, storage: &FlatStorage) -> Result<bool, StorageError> {
    let mut ids = load_molecule_list(storage)?;
    if ids.contains(molecule_id) {
        return Ok(false);
    }
    ids.push(*molecule_id);
    save_molecule_list(&ids, storage)?;
    Ok(true)
}

/// Save a group's roster to the vault.
pub fn save_roster(molecule_id: &MoleculeId, roster: &Roster, storage: &FlatStorage) -> Result<(), StorageError> {
    let bytes = roster_to_vsf_bytes(molecule_id, roster)?;
    storage.write_addr(&roster_key(molecule_id), &bytes)
}

/// Load a group's roster from the vault. None = no roster stored (not a member of this group on this device).
pub fn load_roster(molecule_id: &MoleculeId, storage: &FlatStorage) -> Result<Option<Roster>, StorageError> {
    let Some(bytes) = storage.read_addr(&roster_key(molecule_id))? else {
        return Ok(None);
    };
    let (loaded_id, roster) = roster_from_vsf_bytes(&bytes)?;
    if loaded_id != *molecule_id {
        return Err(StorageError::Parse("roster id mismatch at its own address".to_string()));
    }
    Ok(Some(roster))
}

/// Load every group this device belongs to — index → (roster, chains, materialized conversation). The one boot enumeration for groups, shared by the attest worker and the resume loader (the friendship analogue is contacts → load_all_friendships). A group whose roster is missing is skipped loudly; a chains-blob failure still yields the conversation (rows render, sending waits for the root to re-arrive via a refreshed invite).
pub fn load_all_molecules(storage: &FlatStorage) -> Vec<(MoleculeId, Roster, Option<crate::types::FriendshipChains>, crate::types::Conversation)> {
    let ids = match load_molecule_list(storage) {
        Ok(v) => v,
        Err(e) => {
            crate::logf!("MOLECULE: index load failed: {}", e);
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for id in ids {
        let roster = match load_roster(&id, storage) {
            Ok(Some(r)) => r,
            Ok(None) => {
                crate::logf!("MOLECULE: {} indexed but roster missing — skipped", hex::encode(&id.0[..4]));
                continue;
            }
            Err(e) => {
                crate::logf!("MOLECULE: roster load failed for {}: {}", hex::encode(&id.0[..4]), e);
                continue;
            }
        };
        let fid = crate::types::FriendshipId::from_bytes(id.0);
        let chains = match crate::storage::friendship::load_friendship_chains(&fid, storage) {
            Ok(c) => Some(c),
            Err(e) => {
                crate::logf!("MOLECULE: chains load failed for {}: {}", hex::encode(&id.0[..4]), e);
                None
            }
        };
        let mut conv = crate::types::Conversation::new_molecule(id, roster.standing());
        crate::storage::contacts::load_conversation_state(&mut conv, &id.0, storage);
        if let Err(e) = crate::storage::contacts::load_messages(&mut conv, storage) {
            crate::logf!("MOLECULE: message load failed for {}: {}", hex::encode(&id.0[..4]), e);
        }
        out.push((id, roster, chains, conv));
    }
    out
}

/// GROUP-LOCAL state (D12): what this DEVICE holds about a group that must never replicate — the mute, and the phase this device is in. Lives beside the roster at `vault_key("molecule-local", gid)`; the roster replicates and this does not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoleculeLocal {
    pub muted: bool,
    pub phase: MoleculePhase,
    /// Offers THIS device sent: (invitee party, the offer row's eagle time in that friendship conversation) — what labels the sponsor-side row "waiting" / "joined" and what a roster edge refreshes.
    pub offered: Vec<([u8; 32], i64)>,
}

/// The phase this device is in for a group (docs/molecules.md §10.1). Offered/Expired live on the parked offer, not here; None = not held.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MoleculePhase {
    /// Join sent, waiting on the sponsor's wrap.
    Joining,
    /// The normal state (alone is standing with nobody else — derived from the roster, not stored).
    #[default]
    Standing,
    /// A current-era frame we cannot open while no wrap for our device has arrived.
    CatchingUp,
    /// We posted our leave and zeroized the root; read-only.
    Left,
}

impl MoleculePhase {
    fn to_u8(self) -> u8 {
        match self {
            MoleculePhase::Joining => 1,
            MoleculePhase::Standing => 2,
            MoleculePhase::CatchingUp => 3,
            MoleculePhase::Left => 4,
        }
    }
    fn from_u64(v: u64) -> MoleculePhase {
        match v {
            1 => MoleculePhase::Joining,
            3 => MoleculePhase::CatchingUp,
            4 => MoleculePhase::Left,
            _ => MoleculePhase::Standing,
        }
    }
}

const MOLECULE_LOCAL_SECTION: &str = "molecule_local";

fn molecule_local_schema() -> SectionSchema {
    SectionSchema::new(MOLECULE_LOCAL_SECTION)
        .field("muted", TypeConstraint::AnyUnsigned)
        .field("phase", TypeConstraint::AnyUnsigned)
        .field("offered_party", TypeConstraint::AnyHash)
        .field("offered_osc", TypeConstraint::Any)
}

fn molecule_local_key(molecule_id: &MoleculeId) -> [u8; 32] {
    crate::storage::vault_key("molecule-local", &molecule_id.0)
}

pub fn save_molecule_local(molecule_id: &MoleculeId, local: &MoleculeLocal, storage: &FlatStorage) -> Result<(), StorageError> {
    let mut builder = molecule_local_schema()
        .build()
        .set("muted", VsfType::u(local.muted as usize, false))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("phase", VsfType::u(local.phase.to_u8() as usize, false))
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    for (party, osc) in &local.offered {
        builder = builder
            .append_multi("offered_party", vec![VsfType::hb(party.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?
            .append_multi("offered_osc", vec![VsfType::e(vsf::types::EtType::e6(*osc))])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let section_bytes = builder.encode().map_err(|e| StorageError::Parse(e.to_string()))?;
    let bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(MOLECULE_LOCAL_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    storage.write_addr(&molecule_local_key(molecule_id), &bytes)
}

/// Load; absent = default (not muted, standing).
pub fn load_molecule_local(molecule_id: &MoleculeId, storage: &FlatStorage) -> Result<MoleculeLocal, StorageError> {
    let Some(bytes) = storage.read_addr(&molecule_local_key(molecule_id))? else {
        return Ok(MoleculeLocal::default());
    };
    let section = vsf::schema::SectionBuilder::parse_document(molecule_local_schema(), &bytes, None)
        .map_err(|e| StorageError::Parse(format!("group local failed verified read: {e}")))?;
    let u = |name: &str| section.get_fields(name).first().and_then(|f| f.values.first()).and_then(|v| v.as_u64()).unwrap_or(0);
    let parties: Vec<[u8; 32]> = section
        .get_fields("offered_party")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        })
        .collect();
    let oscs: Vec<i64> = section
        .get_fields("offered_osc")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(o)) => Some(*o),
            other => other.as_i64(),
        })
        .collect();
    let offered = parties.into_iter().zip(oscs).collect();
    Ok(MoleculeLocal { muted: u("muted") != 0, phase: MoleculePhase::from_u64(u("phase")), offered })
}

/// A PARKED OFFER (§10.1 Offered): a friend's offer we have not answered — the roster snapshot they sent and who sent it. No secret rides here (D10). Persisted so a relaunch between offer and Join keeps the row alive; deleted on Join or expiry.
#[derive(Clone, Debug)]
pub struct BondOffer {
    pub molecule_id: MoleculeId,
    /// The sponsor's party id (a contact's handle_hash).
    pub sponsor: crate::types::PartyId,
    /// The eagle time of the offer row in the friendship conversation — the card the Join pill lives on.
    pub row_osc: i64,
    pub snapshot: Roster,
    /// Join was tapped: the offer stays parked (it names the group the row is about) but the pill reads "joined" and no second Join goes out.
    pub accepted: bool,
}

const BOND_OFFER_SECTION: &str = "bond_offer";

fn bond_offer_schema() -> SectionSchema {
    SectionSchema::new(BOND_OFFER_SECTION)
        .field("sponsor", TypeConstraint::AnyHash)
        .field("row_osc", TypeConstraint::Any)
        .field("accepted", TypeConstraint::AnyUnsigned)
        .field("snapshot", TypeConstraint::Any) // hR: the roster-codec blob, verbatim
}

fn bond_offer_key(molecule_id: &MoleculeId) -> [u8; 32] {
    crate::storage::vault_key("bond-offer", &molecule_id.0)
}

pub fn save_bond_offer(offer: &BondOffer, storage: &FlatStorage) -> Result<(), StorageError> {
    let snapshot = roster_to_vsf_bytes(&offer.molecule_id, &offer.snapshot)?;
    let section_bytes = bond_offer_schema()
        .build()
        .set("sponsor", VsfType::hb(offer.sponsor.to_vec()))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("row_osc", VsfType::e(vsf::types::EtType::e6(offer.row_osc)))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("accepted", VsfType::u(offer.accepted as usize, false))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("snapshot", VsfType::hR(snapshot))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    let bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(BOND_OFFER_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    storage.write_addr(&bond_offer_key(&offer.molecule_id), &bytes)
}

pub fn load_bond_offer(molecule_id: &MoleculeId, storage: &FlatStorage) -> Result<Option<BondOffer>, StorageError> {
    let Some(bytes) = storage.read_addr(&bond_offer_key(molecule_id))? else {
        return Ok(None);
    };
    let section = vsf::schema::SectionBuilder::parse_document(bond_offer_schema(), &bytes, None)
        .map_err(|e| StorageError::Parse(format!("group offer failed verified read: {e}")))?;
    let sponsor = section.get_value::<[u8; 32]>("sponsor").map_err(|e| StorageError::Parse(format!("sponsor: {e}")))?;
    let row_osc = section
        .get_fields("row_osc")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::e(vsf::types::EtType::e6(o)) => Some(*o),
            other => other.as_i64(),
        })
        .unwrap_or(0);
    let blob = section
        .get_fields("snapshot")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hR(b) => Some(b.clone()),
            _ => None,
        })
        .ok_or_else(|| StorageError::Parse("offer without a snapshot".to_string()))?;
    let accepted = section.get_fields("accepted").first().and_then(|f| f.values.first()).and_then(|v| v.as_u64()).unwrap_or(0) != 0;
    let (gid, snapshot) = roster_from_vsf_bytes(&blob)?;
    if gid != *molecule_id {
        return Err(StorageError::Parse("offer snapshot names another group".to_string()));
    }
    Ok(Some(BondOffer { molecule_id: gid, sponsor, row_osc, snapshot, accepted }))
}

pub fn delete_bond_offer(molecule_id: &MoleculeId, storage: &FlatStorage) -> Result<(), StorageError> {
    storage.delete_addr(&bond_offer_key(molecule_id))
}

/// The parked-offer index at `vault_key("bond-offers", vault_seed)` — offers, like groups, are discoverable at boot only thru a list.
pub fn save_offer_list(ids: &[MoleculeId], storage: &FlatStorage) -> Result<(), StorageError> {
    let mut builder = molecule_list_schema().build();
    for id in ids {
        builder = builder.append_multi("group", vec![VsfType::hb(id.0.to_vec())]).map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let section_bytes = builder.encode().map_err(|e| StorageError::Parse(e.to_string()))?;
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(MOLECULE_LIST_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    storage.write_addr(&crate::storage::vault_key("bond-offers", &storage.vault_seed()), &vsf_bytes)
}

pub fn load_offer_list(storage: &FlatStorage) -> Result<Vec<MoleculeId>, StorageError> {
    let Some(vsf_bytes) = storage.read_addr(&crate::storage::vault_key("bond-offers", &storage.vault_seed()))? else {
        return Ok(Vec::new());
    };
    let section = vsf::schema::SectionBuilder::parse_document(molecule_list_schema(), &vsf_bytes, None)
        .map_err(|e| StorageError::Parse(format!("offer list failed verified read: {e}")))?;
    Ok(section
        .get_fields("group")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok().map(MoleculeId),
            _ => None,
        })
        .collect())
}

/// Park an offer: write it and index it. Replaces an older offer for the same group (a refreshed snapshot).
pub fn park_bond_offer(offer: &BondOffer, storage: &FlatStorage) -> Result<(), StorageError> {
    save_bond_offer(offer, storage)?;
    let mut ids = load_offer_list(storage)?;
    if !ids.contains(&offer.molecule_id) {
        ids.push(offer.molecule_id);
        save_offer_list(&ids, storage)?;
    }
    Ok(())
}

/// Unpark: delete the offer and drop it from the index.
pub fn unpark_bond_offer(molecule_id: &MoleculeId, storage: &FlatStorage) -> Result<(), StorageError> {
    delete_bond_offer(molecule_id, storage)?;
    let ids: Vec<MoleculeId> = load_offer_list(storage)?.into_iter().filter(|g| g != molecule_id).collect();
    save_offer_list(&ids, storage)
}

/// Every parked offer this device holds.
pub fn load_all_offers(storage: &FlatStorage) -> Vec<BondOffer> {
    let ids = match load_offer_list(storage) {
        Ok(v) => v,
        Err(e) => {
            crate::logf!("MOLECULE: offer index load failed: {}", e);
            return Vec::new();
        }
    };
    ids.iter()
        .filter_map(|id| match load_bond_offer(id, storage) {
            Ok(Some(o)) => Some(o),
            Ok(None) => None,
            Err(e) => {
                crate::logf!("MOLECULE: offer {} failed to load: {}", hex::encode(&id.0[..4]), e);
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::molecule::{device_pubkey, found_atom, sign_record, verify_record};

    /// The full birth → persist → reload arc: a founder's roster round-trips with signatures still verifiable — storage is custody, never trust.
    #[test]
    fn roster_round_trips_and_signatures_survive() {
        let seed = [0xA5u8; 32];
        let birth = found_atom([0x01; 32], [0x02; 32], "Nick", [0x03; 32], "purple turtles", false, &seed);
        let mut roster = Roster::default();
        assert!(roster.merge_genesis(birth.genesis.clone()));
        assert!(roster.merge_member(birth.founder_member.clone()));
        assert!(roster.merge_vouch(birth.founder_vouch.clone()));

        let bytes = roster_to_vsf_bytes(&birth.molecule_id, &roster).expect("encode");
        assert!(bytes.starts_with(b"R\xc3\x85<"), "a complete VSF file, not a bare section");
        let (gid, back) = roster_from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(gid, birth.molecule_id);
        let g = back.genesis.as_ref().expect("genesis rides");
        assert_eq!(g, &birth.genesis);
        let m = back.members.get(&birth.founder_member.party).expect("founder stands");
        assert_eq!(m, &birth.founder_member);
        assert!(back.is_standing(&birth.founder_member.party));
        // The reloaded records still verify against the signing device — custody preserved trust material verbatim.
        assert!(verify_record(&g.signing_bytes(), &g.signature, &g.signer_device));
        assert!(verify_record(&m.signing_bytes(), &m.signature, &m.signer_device));
        assert_eq!(g.signer_device, device_pubkey(&seed));
    }

    /// Load-time records flow thru the MERGE, so a stored older record never clobbers a newer one already held — and leaves round-trip as standing changes, not deletions.
    #[test]
    fn roster_storage_obeys_merge_law_and_leaves_ride() {
        let seed = [0x77u8; 32];
        let birth = found_atom([0x0A; 32], [0x0B; 32], "founder", [0; 32], "t", true, &seed);
        let mut roster = Roster::default();
        roster.merge_genesis(birth.genesis.clone());
        roster.merge_member(birth.founder_member.clone());
        roster.merge_vouch(birth.founder_vouch.clone());
        // A second member joins (vouched by the founder), then leaves at a later stamp.
        let mut m2 = birth.founder_member.clone();
        m2.party = [0x0C; 32];
        m2.name = "cousin".into();
        m2.signed_osc = 100;
        m2.signer_device = device_pubkey(&seed);
        m2.signature = sign_record(&m2.signing_bytes(), &seed);
        roster.merge_member(m2.clone());
        roster.merge_vouch(crate::types::molecule::vouch_record(birth.founder_member.party, m2.party, false, &seed));
        let mut l2 = crate::types::molecule::LeaveRecord {
            party: m2.party,
            signed_osc: 200,
            signature: [0; 64],
            signer_device: device_pubkey(&seed),
        };
        l2.signature = sign_record(&l2.signing_bytes(), &seed);
        roster.merge_leave(l2.clone());

        let bytes = roster_to_vsf_bytes(&birth.molecule_id, &roster).expect("encode");
        let (_, back) = roster_from_vsf_bytes(&bytes).expect("decode");
        assert!(!back.is_standing(&m2.party), "left member is not standing");
        assert!(back.members.contains_key(&m2.party), "ostracism never erasure: the record stays as testimony");
        assert_eq!(back.leaves.get(&m2.party), Some(&l2));
        assert_eq!(back.standing(), vec![birth.founder_member.party]);

        // Vault arc thru FlatStorage.
        crate::storage::isolate_test_storage();
        let storage = FlatStorage::new(crate::storage::APP, [0xD1; 32], [0xD2; 32]).expect("storage");
        assert!(load_roster(&birth.molecule_id, &storage).expect("load").is_none(), "absent = not a member here");
        save_roster(&birth.molecule_id, &roster, &storage).expect("save");
        let loaded = load_roster(&birth.molecule_id, &storage).expect("load").expect("present");
        assert_eq!(loaded.standing(), roster.standing());
        assert_eq!(loaded.genesis, roster.genesis);
    }

    /// v2 round-trip: title, bundles and vouches survive with signatures verifiable; the group-local and parked-offer codecs round-trip beside it; a mute never enters the roster bytes.
    #[test]
    fn roster_v2_title_bundles_vouches_and_local_state_round_trip() {
        let seed = [0x31u8; 32];
        let birth = found_atom([0x0A; 32], [0x0B; 32], "founder", [0; 32], "t", false, &seed);
        let mut roster = Roster::default();
        roster.merge_genesis(birth.genesis.clone());
        roster.merge_member(birth.founder_member.clone());
        roster.merge_vouch(birth.founder_vouch.clone());
        let eph = crate::crypto::era::era_keygen(0, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let bundle = crate::types::molecule::bundle_record(birth.founder_member.party, 0, &eph, &seed);
        roster.merge_bundle(bundle.clone());
        let title = crate::types::molecule::title_record(birth.founder_member.party, "taco", &seed);
        roster.merge_title(title.clone());
        let bytes = roster_to_vsf_bytes(&birth.molecule_id, &roster).expect("encode");
        let (_, back) = roster_from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(back.title(), "taco");
        assert_eq!(back.title.as_ref(), Some(&title));
        let b = back.newest_bundle(&device_pubkey(&seed)).expect("bundle rides");
        assert_eq!(b, &bundle);
        assert!(verify_record(&b.signing_bytes(), &b.signature, &b.device));
        let v = &back.vouches.get(&(birth.founder_member.party, birth.founder_member.party)).expect("self-vouch rides")[0];
        assert!(verify_record(&v.signing_bytes(), &v.signature, &v.signer_device));
        assert!(back.is_standing(&birth.founder_member.party));
        assert_eq!(back.standing_bundles().len(), 1);

        crate::storage::isolate_test_storage();
        let storage = FlatStorage::new(crate::storage::APP, [0xD3; 32], [0xD4; 32]).expect("storage");
        // Local state: default when absent; mute + phase persist; and the mute is NOT in the roster bytes.
        assert_eq!(load_molecule_local(&birth.molecule_id, &storage).expect("absent"), MoleculeLocal::default());
        let local = MoleculeLocal { muted: true, phase: MoleculePhase::Joining, offered: vec![([0x0C; 32], 4242)] };
        save_molecule_local(&birth.molecule_id, &local, &storage).expect("save local");
        assert_eq!(load_molecule_local(&birth.molecule_id, &storage).expect("load"), local);
        assert!(!bytes.windows(5).any(|w| w == b"muted"), "a mute never replicates");
        // Parked offer: park, enumerate, unpark.
        let offer = BondOffer { molecule_id: birth.molecule_id, sponsor: [0x0A; 32], row_osc: 777, snapshot: roster.clone(), accepted: false };
        park_bond_offer(&offer, &storage).expect("park");
        let all = load_all_offers(&storage);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].sponsor, offer.sponsor);
        assert_eq!(all[0].row_osc, 777);
        assert_eq!(all[0].snapshot.standing(), roster.standing());
        park_bond_offer(&offer, &storage).expect("re-park replaces");
        assert_eq!(load_offer_list(&storage).expect("list").len(), 1);
        unpark_bond_offer(&birth.molecule_id, &storage).expect("unpark");
        assert!(load_all_offers(&storage).is_empty());
        assert!(load_bond_offer(&birth.molecule_id, &storage).expect("gone").is_none());
    }

    /// The index is the ONLY enumeration the flat vault offers: boot discovers membership thru it, and index_molecule is idempotent.
    #[test]
    fn group_index_enumerates_membership() {
        crate::storage::isolate_test_storage();
        let storage = FlatStorage::new(crate::storage::APP, [0xE1; 32], [0xE2; 32]).expect("storage");
        assert!(load_molecule_list(&storage).expect("empty vault").is_empty());
        let a = crate::types::molecule::MoleculeId::from_nonce(&[1; 32]);
        let b = crate::types::molecule::MoleculeId::from_nonce(&[2; 32]);
        assert!(index_molecule(&a, &storage).expect("index a"));
        assert!(index_molecule(&b, &storage).expect("index b"));
        assert!(!index_molecule(&a, &storage).expect("idempotent"), "a second index of the same group is a no-op");
        assert_eq!(load_molecule_list(&storage).expect("load"), vec![a, b]);
    }
}
