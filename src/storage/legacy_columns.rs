//! DISK-ONLY MIGRATION (2026-09-25): the parallel-column group ROSTER (v2) that the one-section-per-record flag day retired, verbatim, read ONLY from this device's own vault and rewritten in the new shape on sight.
//! Never reachable from a wire arrival: invite snapshots, replication and roster posts go thru the strict decoder, which refuses this shape.
//! Why this one reader survives (Nick 2026-09-25: "mass re-clutch is okay, we just cannot have lost messages and certainly not friendships"): a refused CHAINS blob only re-clutches — the contact stays, the conversation id is the participants', and undelivered rows re-send — so chains v8/v9 get no reader. A refused ROSTER drops the group from the list and strands its rows, so the roster keeps one.
//! DELETE this module once submitted logs show no "MIGRATION: parallel-column" line for a release cycle — the same evidence bar the pre-document chains migration met (deleted 2026-08-18).

use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::VsfType;

use crate::storage::StorageError;
use crate::types::molecule::{BundleRecord, GenesisRecord, LeaveRecord, MemberRecord, MoleculeId, Roster, TitleRecord, VouchRecord};

const ROSTER_SECTION_V2: &str = "molecule_roster";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::molecule::found_atom;

    /// A v2 column roster in this device's vault loads thru the migration and is REWRITTEN in the record shape — while the strict wire decoder refuses the same bytes.
    #[test]
    fn a_v2_column_roster_on_disk_migrates_and_the_wire_refuses_it() {
        crate::storage::isolate_test_storage();
        let storage = crate::storage::FlatStorage::new(crate::storage::APP, [0xB7; 32], [0xB8; 32]).expect("storage");
        let birth = found_atom([0x01; 32], [0x02; 32], "Nick", [0x03; 32], "purple turtles", false, &[0xA5; 32]);
        let (g, m) = (&birth.genesis, &birth.founder_member);
        let e6 = |v: i64| VsfType::e(vsf::types::EtType::e6(v));
        let hb = |b: &[u8]| VsfType::hb(b.to_vec());
        let b = roster_schema_v2()
            .build()
            .set("version", 2u8)
            .unwrap()
            .set("molecule_id", hb(&birth.molecule_id.0))
            .unwrap()
            .set("gen_founder", hb(&g.founder))
            .unwrap()
            .set("gen_osc", e6(g.genesis_osc))
            .unwrap()
            .set("gen_from_genesis", VsfType::u(g.history_from_genesis as usize, false))
            .unwrap()
            .set("gen_title", VsfType::x(g.title.clone()))
            .unwrap()
            .set("gen_sig", hb(&g.signature))
            .unwrap()
            .set("gen_signer", hb(&g.signer_device))
            .unwrap()
            .append_multi("m_party", vec![hb(&m.party)])
            .unwrap()
            .append_multi("m_proof", vec![hb(&m.handle_proof)])
            .unwrap()
            .append_multi("m_name", vec![VsfType::x(m.name.clone())])
            .unwrap()
            .append_multi("m_avatar", vec![hb(&m.avatar_pin)])
            .unwrap()
            .append_multi("m_osc", vec![e6(m.signed_osc)])
            .unwrap()
            .append_multi("m_sponsor", vec![hb(&m.sponsor)])
            .unwrap()
            .append_multi("m_sig", vec![hb(&m.signature)])
            .unwrap()
            .append_multi("m_signer", vec![hb(&m.signer_device)])
            .unwrap();
        let old = vsf::VsfBuilder::new().creation_time_oscillations(1).provenance_only().add_unboxed(ROSTER_SECTION_V2, b.encode().unwrap()).build().unwrap();
        assert!(crate::storage::molecule::roster_from_vsf_bytes(&old).is_err(), "the wire decoder refuses the column shape");
        let addr = crate::storage::vault_key("molecule", &birth.molecule_id.0);
        storage.write_addr(&addr, &old).unwrap();

        let loaded = crate::storage::molecule::load_roster(&birth.molecule_id, &storage).expect("the disk migration reads it").expect("present");
        assert_eq!(loaded.genesis.as_ref(), Some(g));
        assert_eq!(loaded.members.get(&m.party), Some(m));
        let rewritten = storage.read_addr(&addr).unwrap().unwrap();
        assert!(crate::storage::molecule::roster_from_vsf_bytes(&rewritten).is_ok(), "rewritten in the record shape on sight");
    }
}
