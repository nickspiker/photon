//! Group flows (docs/groups.md): founding, the pairwise invite's adoption, and roster record merges.
//! The receive half rides `commit_braid_rx`'s control-row ladder (a `GroupSignal` row defers here after the chains borrow ends, the era-signal discipline); the send half posts `GROUP_PREFIX` rows with the roster blob on the package's typed `gpl` field.
//! Consent doctrine (§4): adopting an invite creates the group LOCALLY only — the member record that makes us standing posts on our first send in the group (composing IS consent); nothing here auto-joins anyone.

use super::PhotonApp;
use crate::types::group::{GroupId, GroupSignal, Roster};

impl PhotonApp {
    /// A `GroupSignal` control row landed (deferred from commit_braid_rx): `ci` = the sender's contact, `cp` = the conversation the row landed in, `blob` = the package's roster payload.
    pub(super) fn on_group_signal(&mut self, ci: usize, cp: usize, sig: GroupSignal, blob: Option<Vec<u8>>, _ts: i64) {
        match sig {
            GroupSignal::Invite { era_index, group_root, group_history_key, era_lineage } => {
                self.adopt_group_invite(ci, era_index, group_root, group_history_key, era_lineage, blob);
            }
            GroupSignal::Records => self.merge_group_records(ci, cp, blob),
        }
    }

    /// Adopt a sponsor's invite (§4 Invite): the roster snapshot names who is in it, the era-pinned secrets open the current era. Creating the local state is NOT joining — our member record posts on our first send. A re-invite for a group we already hold merges its snapshot records (the sponsor-refresh path; era supersession on a newer-era invite lands with step 3).
    fn adopt_group_invite(&mut self, ci: usize, era_index: u64, group_root: [u8; 32], group_history_key: [u8; 32], era_lineage: [u8; 32], blob: Option<Vec<u8>>) {
        let sponsor = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let Some(blob) = blob else {
            crate::logf!("GROUP: invite from {} without a roster snapshot — refused", sponsor);
            return;
        };
        let (gid, snapshot) = match crate::storage::group::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("GROUP: invite snapshot from {} failed verified read: {} — refused", sponsor, e);
                return;
            }
        };
        // The birth certificate must be present and self-consistent before anything persists. Device-fold verification of every member deepens with GroupPeer in step 3; the genesis signature is the v1 bar.
        let title = {
            let Some(g) = snapshot.genesis.as_ref() else {
                crate::logf!("GROUP: invite snapshot for {} carries no genesis — refused", hex::encode(&gid.0[..4]));
                return;
            };
            if g.group_id != gid || !crate::types::group::verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) {
                crate::logf!("GROUP: invite genesis for {} does not verify — refused", hex::encode(&gid.0[..4]));
                return;
            }
            g.title.clone()
        };
        if let Some(pos) = self.group_rosters.iter().position(|(id, _)| *id == gid) {
            // Already held: the refresh path — merge the snapshot's records; secrets only matter across an era boundary (step 3).
            let mut changed = self.merge_snapshot_into(pos, snapshot);
            changed |= self.sync_group_conversation(&gid);
            if changed {
                self.persist_group(&gid);
            }
            crate::logf!("GROUP: re-invite for {} from {} merged", hex::encode(&gid.0[..4]), sponsor);
            return;
        }
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let chains = crate::types::FriendshipChains::from_group_root(gid, &snapshot.standing(), group_root, group_history_key, era_index, era_lineage);
        if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
            self.friendship_chains.push((fid, chains));
        }
        if !self.conversations.iter().any(|v| v.id() == fid) {
            self.conversations.push(crate::types::Conversation::new_group(gid, snapshot.standing()));
        }
        let members = snapshot.members.len();
        self.group_rosters.push((gid, snapshot));
        self.persist_group(&gid);
        crate::logf!("GROUP: adopted \"{}\" ({}) from {} — {} member(s), era {} — first send posts our member record", title, hex::encode(&gid.0[..4]), sponsor, members, era_index);
    }

    /// Records posted inside a group (§4): merge every record from the blob under the roster's one law, then follow the standing set into the conversation. Invalid signatures are skipped loudly; merge decides precedence.
    fn merge_group_records(&mut self, ci: usize, cp: usize, blob: Option<Vec<u8>>) {
        let sender = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let Some(gid) = self.conversations.get(cp).filter(|c| c.is_group()).map(|c| GroupId(*c.id().as_bytes())) else {
            crate::logf!("GROUP: records row from {} landed outside a group conversation — ignored", sender);
            return;
        };
        let Some(blob) = blob else {
            crate::logf!("GROUP: records row from {} without a payload — ignored", sender);
            return;
        };
        let (blob_gid, snapshot) = match crate::storage::group::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("GROUP: records payload from {} failed verified read: {} — ignored", sender, e);
                return;
            }
        };
        if blob_gid != gid {
            crate::logf!("GROUP: records payload names {} but landed in {} — ignored", hex::encode(&blob_gid.0[..4]), hex::encode(&gid.0[..4]));
            return;
        }
        let Some(pos) = self.group_rosters.iter().position(|(id, _)| *id == gid) else {
            crate::logf!("GROUP: records for unheld group {} — ignored", hex::encode(&gid.0[..4]));
            return;
        };
        let mut changed = self.merge_snapshot_into(pos, snapshot);
        changed |= self.sync_group_conversation(&gid);
        if changed {
            self.persist_group(&gid);
            crate::logf!("GROUP: {} roster advanced by records from {}", hex::encode(&gid.0[..4]), sender);
        }
    }

    /// Merge a snapshot's records into a held roster — signature-checked per record (the subject's own claimed device; the fold check deepens with GroupPeer in step 3), precedence entirely the roster merge's newest-wins law.
    fn merge_snapshot_into(&mut self, pos: usize, snapshot: Roster) -> bool {
        let roster = &mut self.group_rosters[pos].1;
        let mut changed = false;
        if let Some(g) = snapshot.genesis {
            if crate::types::group::verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) {
                changed |= roster.merge_genesis(g);
            }
        }
        for (_, m) in snapshot.members {
            if crate::types::group::verify_record(&m.signing_bytes(), &m.signature, &m.signer_device) {
                changed |= roster.merge_member(m);
            } else {
                crate::logf!("GROUP: member record for {} fails its signature — skipped", crate::fp(&m.party));
            }
        }
        for (_, l) in snapshot.leaves {
            if crate::types::group::verify_record(&l.signing_bytes(), &l.signature, &l.signer_device) {
                changed |= roster.merge_leave(l);
            } else {
                crate::logf!("GROUP: leave record for {} fails its signature — skipped", crate::fp(&l.party));
            }
        }
        changed
    }

    /// Follow the roster's standing set into the group conversation (§4: the set follows the roster; the id never moves). Returns whether the set changed.
    fn sync_group_conversation(&mut self, gid: &GroupId) -> bool {
        let Some(standing) = self.group_rosters.iter().find(|(id, _)| id == gid).map(|(_, r)| r.standing()) else {
            return false;
        };
        self.conversations
            .iter_mut()
            .find(|c| c.id().as_bytes() == &gid.0)
            .map(|c| c.set_participants(standing))
            .unwrap_or(false)
    }

    /// Persist a group's index entry + roster. Chains persist thru the standard chains path on their own mutation edges.
    fn persist_group(&mut self, gid: &GroupId) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if let Err(e) = crate::storage::group::index_group(gid, storage) {
            crate::logf!("GROUP: index write failed for {}: {}", hex::encode(&gid.0[..4]), e);
        }
        if let Some((_, roster)) = self.group_rosters.iter().find(|(id, _)| id == gid) {
            if let Err(e) = crate::storage::group::save_roster(gid, roster, storage) {
                crate::logf!("GROUP: roster write failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
        }
    }
}
