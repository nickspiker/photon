//! BONDS (docs/conversations.md §0, §7): the secure channel each pair of friends holds, never a listed conversation. A bond carries control traffic only — invitations, joins, era deliveries — while conversations of any size carry rows. An existing friendship's bond is SPLIT from the era its two-member conversation holds, by the same one-way derivation on both sides, so no wire exchange is needed.

use super::*;

impl PhotonApp {
    /// The registry id of the bond we hold with contact `ci`, when we hold one: a friend's derived bond, or a fleet sibling's own chains (the bond between our own devices). `None` for nobody-but-us and for a friend whose bond is not split yet.
    pub(super) fn bond_fid_of(&self, ci: usize) -> Option<crate::types::FriendshipId> {
        let c = self.contacts.get(ci)?;
        if c.is_sibling {
            return c.friendship_id;
        }
        let us = self.our_party_id(c)?;
        let fid = crate::types::FriendshipId::from_bytes(c.bond_id(&us)?.0);
        self.friendship_chains.iter().any(|(id, _)| *id == fid).then_some(fid)
    }

    /// SPLIT every missing bond (docs/conversations.md §7): a friend whose two-member conversation holds a woven era and who has no bond yet gets one, derived one-way from that era (`bond_split_of`); the conversation records the split era, which it never hands to anyone else.
    /// Both sides derive the same bond from the era they share; a pair that split from different eras (one side cut over while the other upgraded) finds the bond's frames do not open, and the bond re-keys like any broken bond.
    /// Pure computation plus async persists — no vault call on the UI thread. Runs when the chains registry or the contact list changes size (the edge a load, an add or a ceremony raises).
    pub(super) fn ensure_bonds(&mut self) {
        let key = (self.friendship_chains.len(), self.contacts.len());
        if self.bonds_checked == key {
            return;
        }
        self.bonds_checked = key;
        let Some(us) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            return;
        };
        let mut splits: Vec<(crate::types::FriendshipId, crate::types::friendship::BondId, crate::types::FriendshipChains, [u8; 32])> = Vec::new();
        for c in &self.contacts {
            let (Some(b), Some(fid)) = (c.bond_id(&us), c.friendship_id) else {
                continue;
            };
            let bfid = crate::types::FriendshipId::from_bytes(b.0);
            if self.friendship_chains.iter().any(|(id, _)| *id == bfid) {
                continue;
            }
            let Some(conv) = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, ch)| ch) else {
                continue;
            };
            if !conv.lane_capable() {
                continue; // no woven era yet: the ceremony's completion is the edge that splits it
            }
            if let Some(bond) = conv.bond_split_of(b) {
                splits.push((fid, b, bond, c.handle_hash));
            }
        }
        for (fid, b, bond, them) in splits {
            let bfid = crate::types::FriendshipId::from_bytes(b.0);
            let era = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid).map(|(_, conv)| {
                let e = conv.era_index;
                conv.bond_split_era.get_or_insert(e);
                e
            });
            self.friendship_chains.push((bfid, bond));
            if !self.conversations.iter().any(|v| v.id() == bfid) {
                // A freshly split bond has nothing on disk: its hidden conversation starts hydrated and empty.
                let mut bc = crate::types::Conversation::new_bond(b, [us, them]);
                bc.hydrated = true;
                self.conversations.push(bc);
            }
            self.persist_chains_of(&fid);
            self.persist_chains_of(&bfid);
            crate::logf!("BOND: {} split from its conversation {} at era {}", hex::encode(&b.0[..4]), hex::encode(&fid.as_bytes()[..4]), era.map_or_else(|| "?".to_string(), |e| e.to_string()));
        }
        self.bonds_checked = (self.friendship_chains.len(), self.contacts.len());
    }
}
