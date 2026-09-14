//! Group roster storage (docs/groups.md §5).
//!
//! The roster — genesis plus the merged sovereign record sets — persists as one vault entry at `vault_key("group", group_id)`, beside the group's chains blob at `vault_key("chains", group_id)`.
//! The SAME canonical bytes serve the vault and, later, the wire: roster records travel as control rows and re-serve pages, and a whole-roster snapshot rides the pairwise invite — so the codec is a complete VSF file with a provenance header, exactly like the chains blob.
//! Signatures persist verbatim: a loaded record is still verifiable against the folded device set, so storage is custody, never trust.

use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::VsfType;

use crate::storage::{FlatStorage, StorageError};
use crate::types::group::{GenesisRecord, GroupId, LeaveRecord, MemberRecord, Roster};

/// The section name, shared by the builder and the TOC lookup — the two must never drift.
const ROSTER_SECTION: &str = "group_roster";

fn roster_schema() -> SectionSchema {
    SectionSchema::new(ROSTER_SECTION)
        .field("version", TypeConstraint::AnyUnsigned)
        .field("group_id", TypeConstraint::AnyHash)
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
}

/// Vault address for a group's roster — beside its chains blob, same scope bytes.
fn roster_key(group_id: &GroupId) -> [u8; 32] {
    crate::storage::vault_key("group", &group_id.0)
}

/// Encode a roster to its canonical VSF bytes — vault entry, invite snapshot, and (later) re-serve payload alike.
pub fn roster_to_vsf_bytes(group_id: &GroupId, roster: &Roster) -> Result<Vec<u8>, StorageError> {
    let e6 = |v: i64| VsfType::e(vsf::types::EtType::e6(v));
    let mut builder = roster_schema()
        .build()
        .set("version", 1u8)
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("group_id", VsfType::hb(group_id.0.to_vec()))
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
    let section_bytes = builder.encode().map_err(|e| StorageError::Parse(e.to_string()))?;
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(ROSTER_SECTION, section_bytes)
        .build()
        .map_err(|e| StorageError::Parse(e.to_string()))
}

/// Decode a roster from its canonical bytes — STRICT verified read, shared by the vault loader and every wire arrival (invite snapshot, replication), so a headerless blob never becomes membership state.
pub fn roster_from_vsf_bytes(vsf_bytes: &[u8]) -> Result<(GroupId, Roster), StorageError> {
    let section = vsf::schema::SectionBuilder::parse_document(roster_schema(), vsf_bytes, None)
        .map_err(|e| StorageError::Parse(format!("roster failed verified read: {e}")))?;

    let gid_bytes: [u8; 32] = section
        .get_value::<[u8; 32]>("group_id")
        .map_err(|e| StorageError::Parse(format!("group_id: {e}")))?;
    let group_id = GroupId(gid_bytes);

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

    let mut roster = Roster::default();

    if let Ok(founder) = section.get_value::<[u8; 32]>("gen_founder") {
        let sig = section
            .get_fields("gen_sig")
            .first()
            .and_then(|f| f.values.first())
            .and_then(hb64)
            .ok_or_else(|| StorageError::Parse("genesis without a signature".to_string()))?;
        roster.genesis = Some(GenesisRecord {
            group_id,
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

    let (lp, lo, lsig, lsd) = (col32("l_party"), col_osc("l_osc"), col64("l_sig"), col32("l_signer"));
    let n = lp.len().min(lo.len()).min(lsig.len()).min(lsd.len());
    for i in 0..n {
        roster.merge_leave(LeaveRecord {
            party: lp[i],
            signed_osc: lo[i],
            signature: lsig[i],
            signer_device: lsd[i],
        });
    }

    Ok((group_id, roster))
}

/// Schema for the group index: one `group` field per group id. The vault is flat and content-addressed — nothing enumerates — so membership is discoverable at boot ONLY thru this list, exactly as contacts are thru theirs. Vault-internal (FlatStorage encrypts it); never travels.
fn group_list_schema() -> SectionSchema {
    SectionSchema::new("group_list").field("group", TypeConstraint::AnyHash)
}

/// Save the group index at `vault_key("groups", vault_seed)`.
pub fn save_group_list(ids: &[GroupId], storage: &FlatStorage) -> Result<(), StorageError> {
    let mut builder = group_list_schema().build();
    for id in ids {
        builder = builder
            .append_multi("group", vec![VsfType::hb(id.0.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let vsf_bytes = builder.encode().map_err(|e| StorageError::Parse(e.to_string()))?;
    storage.write_addr(&crate::storage::vault_key("groups", &storage.vault_seed()), &vsf_bytes)
}

/// Load the group index. Empty = this device belongs to no groups.
pub fn load_group_list(storage: &FlatStorage) -> Result<Vec<GroupId>, StorageError> {
    let Some(vsf_bytes) = storage.read_addr(&crate::storage::vault_key("groups", &storage.vault_seed()))? else {
        return Ok(Vec::new());
    };
    let section = vsf::schema::SectionBuilder::parse(group_list_schema(), &vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("group list parse: {e}")))?;
    Ok(section
        .get_fields("group")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok().map(GroupId),
            _ => None,
        })
        .collect())
}

/// Add one group to the index iff absent. Returns whether the list changed (caller persists rosters/chains beside it).
pub fn index_group(group_id: &GroupId, storage: &FlatStorage) -> Result<bool, StorageError> {
    let mut ids = load_group_list(storage)?;
    if ids.contains(group_id) {
        return Ok(false);
    }
    ids.push(*group_id);
    save_group_list(&ids, storage)?;
    Ok(true)
}

/// Save a group's roster to the vault.
pub fn save_roster(group_id: &GroupId, roster: &Roster, storage: &FlatStorage) -> Result<(), StorageError> {
    let bytes = roster_to_vsf_bytes(group_id, roster)?;
    storage.write_addr(&roster_key(group_id), &bytes)
}

/// Load a group's roster from the vault. None = no roster stored (not a member of this group on this device).
pub fn load_roster(group_id: &GroupId, storage: &FlatStorage) -> Result<Option<Roster>, StorageError> {
    let Some(bytes) = storage.read_addr(&roster_key(group_id))? else {
        return Ok(None);
    };
    let (loaded_id, roster) = roster_from_vsf_bytes(&bytes)?;
    if loaded_id != *group_id {
        return Err(StorageError::Parse("roster id mismatch at its own address".to_string()));
    }
    Ok(Some(roster))
}

/// Load every group this device belongs to — index → (roster, chains, materialized conversation). The one boot enumeration for groups, shared by the attest worker and the resume loader (the friendship analogue is contacts → load_all_friendships). A group whose roster is missing is skipped loudly; a chains-blob failure still yields the conversation (rows render, sending waits for the root to re-arrive via a refreshed invite).
pub fn load_all_groups(storage: &FlatStorage) -> Vec<(GroupId, Roster, Option<crate::types::FriendshipChains>, crate::types::Conversation)> {
    let ids = match load_group_list(storage) {
        Ok(v) => v,
        Err(e) => {
            crate::logf!("GROUP: index load failed: {}", e);
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for id in ids {
        let roster = match load_roster(&id, storage) {
            Ok(Some(r)) => r,
            Ok(None) => {
                crate::logf!("GROUP: {} indexed but roster missing — skipped", hex::encode(&id.0[..4]));
                continue;
            }
            Err(e) => {
                crate::logf!("GROUP: roster load failed for {}: {}", hex::encode(&id.0[..4]), e);
                continue;
            }
        };
        let fid = crate::types::FriendshipId::from_bytes(id.0);
        let chains = match crate::storage::friendship::load_friendship_chains(&fid, storage) {
            Ok(c) => Some(c),
            Err(e) => {
                crate::logf!("GROUP: chains load failed for {}: {}", hex::encode(&id.0[..4]), e);
                None
            }
        };
        let mut conv = crate::types::Conversation::new_group(id, roster.standing());
        crate::storage::contacts::load_conversation_state(&mut conv, &id.0, storage);
        if let Err(e) = crate::storage::contacts::load_messages(&mut conv, storage) {
            crate::logf!("GROUP: message load failed for {}: {}", hex::encode(&id.0[..4]), e);
        }
        out.push((id, roster, chains, conv));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::group::{device_pubkey, found_group, sign_record, verify_record};

    /// The full birth → persist → reload arc: a founder's roster round-trips with signatures still verifiable — storage is custody, never trust.
    #[test]
    fn roster_round_trips_and_signatures_survive() {
        let seed = [0xA5u8; 32];
        let birth = found_group([0x01; 32], [0x02; 32], "Nick", [0x03; 32], "purple turtles", false, &seed);
        let mut roster = Roster::default();
        assert!(roster.merge_genesis(birth.genesis.clone()));
        assert!(roster.merge_member(birth.founder_member.clone()));

        let bytes = roster_to_vsf_bytes(&birth.group_id, &roster).expect("encode");
        assert!(bytes.starts_with(b"R\xc3\x85<"), "a complete VSF file, not a bare section");
        let (gid, back) = roster_from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(gid, birth.group_id);
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
        let birth = found_group([0x0A; 32], [0x0B; 32], "founder", [0; 32], "t", true, &seed);
        let mut roster = Roster::default();
        roster.merge_genesis(birth.genesis.clone());
        roster.merge_member(birth.founder_member.clone());
        // A second member joins, then leaves at a later stamp.
        let mut m2 = birth.founder_member.clone();
        m2.party = [0x0C; 32];
        m2.name = "cousin".into();
        m2.signed_osc = 100;
        m2.signer_device = device_pubkey(&seed);
        m2.signature = sign_record(&m2.signing_bytes(), &seed);
        roster.merge_member(m2.clone());
        let mut l2 = crate::types::group::LeaveRecord {
            party: m2.party,
            signed_osc: 200,
            signature: [0; 64],
            signer_device: device_pubkey(&seed),
        };
        l2.signature = sign_record(&l2.signing_bytes(), &seed);
        roster.merge_leave(l2.clone());

        let bytes = roster_to_vsf_bytes(&birth.group_id, &roster).expect("encode");
        let (_, back) = roster_from_vsf_bytes(&bytes).expect("decode");
        assert!(!back.is_standing(&m2.party), "left member is not standing");
        assert!(back.members.contains_key(&m2.party), "ostracism never erasure: the record stays as testimony");
        assert_eq!(back.leaves.get(&m2.party), Some(&l2));
        assert_eq!(back.standing(), vec![birth.founder_member.party]);

        // Vault arc thru FlatStorage.
        crate::storage::isolate_test_storage();
        let storage = FlatStorage::new(crate::storage::APP, [0xD1; 32], [0xD2; 32]).expect("storage");
        assert!(load_roster(&birth.group_id, &storage).expect("load").is_none(), "absent = not a member here");
        save_roster(&birth.group_id, &roster, &storage).expect("save");
        let loaded = load_roster(&birth.group_id, &storage).expect("load").expect("present");
        assert_eq!(loaded.standing(), roster.standing());
        assert_eq!(loaded.genesis, roster.genesis);
    }

    /// The index is the ONLY enumeration the flat vault offers: boot discovers membership thru it, and index_group is idempotent.
    #[test]
    fn group_index_enumerates_membership() {
        crate::storage::isolate_test_storage();
        let storage = FlatStorage::new(crate::storage::APP, [0xE1; 32], [0xE2; 32]).expect("storage");
        assert!(load_group_list(&storage).expect("empty vault").is_empty());
        let a = crate::types::group::GroupId::from_nonce(&[1; 32]);
        let b = crate::types::group::GroupId::from_nonce(&[2; 32]);
        assert!(index_group(&a, &storage).expect("index a"));
        assert!(index_group(&b, &storage).expect("index b"));
        assert!(!index_group(&a, &storage).expect("idempotent"), "a second index of the same group is a no-op");
        assert_eq!(load_group_list(&storage).expect("load"), vec![a, b]);
    }
}
