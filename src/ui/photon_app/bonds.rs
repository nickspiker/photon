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

    /// The hidden conversation of our bond with contact `ci` — where the bond's control rows (an offer we sent, a decline) live for re-ACK durability.
    pub(super) fn bond_conv_mut(&mut self, ci: usize) -> Option<&mut crate::types::Conversation> {
        let bfid = self.bond_fid_of(ci)?;
        self.conversations.iter_mut().find(|v| v.id() == bfid)
    }

    /// Land one of OUR control rows in the bond's hidden conversation, persist it, and send it on the bond: the offer / decline / undo pattern.
    pub(super) fn send_bond_row(&mut self, ci: usize, control: crate::types::RowControl, molecule: Option<&crate::network::message_package::MoleculeWire>, ts: i64) -> bool {
        if let Some(conv) = self.bond_conv_mut(ci) {
            conv.insert_message_sorted(crate::types::ChatMessage::control(control.clone(), true, ts));
        }
        if let Some(bfid) = self.bond_fid_of(ci) {
            self.persist_conversation_async(bfid);
        }
        self.bond_transmit_with(ci, ts, None, molecule, &control)
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
            self.persist_chains_async(&fid);
            self.persist_chains_async(&bfid);
            crate::logf!("BOND: {} split from its conversation {} at era {}", hex::encode(&b.0[..4]), hex::encode(&fid.as_bytes()[..4]), era.map_or_else(|| "?".to_string(), |e| e.to_string()));
        }
        self.bonds_checked = (self.friendship_chains.len(), self.contacts.len());
    }
}

impl PhotonApp {
    /// A friend's BOND just completed its ceremony (docs/conversations.md §7): make sure its hidden conversation exists, and found the pair's own conversation over it when they have none yet.
    pub(super) fn bond_completed(&mut self, ci: usize) {
        let Some((b, them, fresh_friend)) = self.contacts.get(ci).and_then(|c| self.our_party_id(c).and_then(|us| c.bond_id(&us)).map(|b| (b, c.handle_hash, c.friendship_id.is_none()))) else {
            return;
        };
        let bfid = crate::types::FriendshipId::from_bytes(b.0);
        if !self.conversations.iter().any(|v| v.id() == bfid) {
            if let Some(us) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) {
                let mut bc = crate::types::Conversation::new_bond(b, [us, them]);
                // A brand-new friend's bond has nothing on disk; an existing friend's re-keyed bond was loaded at boot, and a miss here hydrates thru the persist rescue.
                bc.hydrated = fresh_friend;
                self.conversations.push(bc);
            }
        }
        self.found_pair_conversation(ci);
        // A disagreement the old bond could not carry gets its heal again over this one.
        self.reheal_after_bond(ci);
    }

    /// FOUND the pair's two-member conversation over a fresh bond (docs/conversations.md §7): the LOWER party (by party id) mints its era 0 and delivers it whole on the bond; the other side installs it when it lands (`on_era_delivery`). A friend who already holds a conversation with us keeps it.
    pub(super) fn found_pair_conversation(&mut self, ci: usize) {
        let Some(c) = self.contacts.get(ci) else {
            return;
        };
        if c.is_sibling || c.friendship_id.is_some() {
            return;
        }
        let Some(us) = self.our_party_id(c) else {
            return;
        };
        let them = c.handle_hash;
        if us == them {
            return;
        }
        let fid = crate::types::FriendshipId::derive(&[us, them]);
        if self.friendship_chains.iter().any(|(id, _)| *id == fid) {
            self.adopt_pair_conversation(ci, fid);
            return;
        }
        if us > them {
            crate::logf!("BOND: {} — waiting for their side to found our conversation", crate::fp(&c.handle_proof));
            return;
        }
        let (root, hk, nonce): ([u8; 32], [u8; 32], [u8; 32]) = (rand::random(), rand::random(), rand::random());
        let lineage = crate::crypto::clutch::era_lineage(&root);
        let chains = pair_conversation_chains(fid, us, them, root, hk, 0, lineage);
        self.friendship_chains.push((fid, chains));
        self.adopt_pair_conversation(ci, fid);
        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(&root);
        secret.extend_from_slice(&hk);
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: fid.0, era: 0, era_lineage: lineage, nonce, secret }) };
        let sent = self.bond_transmit_with(ci, crate::network::time_base::stamp_osc(), None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        crate::logf!("BOND: founded our conversation {} with {} — era 0 {} on the bond", hex::encode(&fid.as_bytes()[..4]), crate::fp(&self.contacts[ci].handle_proof), if sent { "delivered" } else { "NOT delivered yet (the bond's retransmit carries it)" });
    }

    /// Point a friend at the pair conversation whose keys we now hold, and persist both.
    fn adopt_pair_conversation(&mut self, ci: usize, fid: crate::types::FriendshipId) {
        if let Some(c) = self.contacts.get_mut(ci) {
            c.friendship_id = Some(fid);
        }
        self.persist_chains_async(&fid);
        if let (Some(storage), Some(c)) = (self.storage.clone(), self.contacts.get(ci).cloned()) {
            queue_job(&self.seal_job_tx, move || {
                if let Err(e) = crate::storage::contacts::save_contact(&c, &storage) {
                    crate::logf!("BOND: contact persist failed: {}", e);
                }
            });
        }
        self.update_sync_records();
        self.scene_dirty = true;
    }

    /// An ERA DELIVERED over the bond with contact `ci` (docs/conversations.md §0, §7). The founding of our two-member conversation (era 0, whole) installs it; a fresh era for a conversation we hold is derived from the era we have and installed as pending, cutting over on the first frame tagged with it — exactly a mint's wrap, carried by the bond instead of a KEM bundle.
    pub(super) fn on_era_delivery(&mut self, ci: usize, wire: Option<crate::network::message_package::MoleculeWire>) {
        let sender = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let Some(d) = wire.as_ref().and_then(|w| w.deliver.as_ref()) else {
            crate::logf!("BOND: delivery from {} without its typed fields — ignored", sender);
            return;
        };
        let fid = crate::types::FriendshipId::from_bytes(d.conversation);
        let Some((us, them)) = self.contacts.get(ci).and_then(|c| self.our_party_id(c).map(|us| (us, c.handle_hash))) else {
            return;
        };
        let held = self.friendship_chains.iter().any(|(id, _)| *id == fid);
        if !held {
            // A FOUNDING: only our own two-member conversation can arrive whole from nowhere — any other conversation is joined thru its own path.
            if fid != crate::types::FriendshipId::derive(&[us, them]) || d.secret.len() != 64 || d.era != 0 {
                crate::logf!("BOND: delivery from {} names conversation {} we do not hold and is no founding of ours — ignored", sender, hex::encode(&fid.as_bytes()[..4]));
                return;
            }
            let (root, hk) = (<[u8; 32]>::try_from(&d.secret[..32]).unwrap(), <[u8; 32]>::try_from(&d.secret[32..]).unwrap()); // WHY/PROOF: the length is 64, checked just above
            let chains = pair_conversation_chains(fid, us, them, root, hk, 0, d.era_lineage);
            self.friendship_chains.push((fid, chains));
            self.adopt_pair_conversation(ci, fid);
            crate::logf!("BOND: our conversation {} with {} founded by them — era 0 installed", hex::encode(&fid.as_bytes()[..4]), sender);
            return;
        }
        // Only a MEMBER delivers an era: our pair's own conversation from its other party, a roster conversation from a party standing in it. Holding a bond with us grants nothing over conversations that friend is not in.
        let pair = fid == crate::types::FriendshipId::derive(&[us, them]);
        let member = self.molecule_rosters.iter().find(|(g, _)| g.0 == fid.0).is_some_and(|(_, r)| r.is_standing(&them));
        if !pair && !member {
            crate::logf!("BOND: {} delivered an era for {}, a conversation they do not stand in — refused", sender, hex::encode(&fid.as_bytes()[..4]));
            return;
        }
        // A mint of a conversation we hold: the next era, derived from the one we have.
        let Some((era, root, hk, token)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, *c.lane_root()?, *c.history_key()?, c.conversation_token))) else {
            return;
        };
        // A WHOLE era (root ‖ history key) repairs divergence or a stranded member: any index past ours. A FRESH one (a mint) derives the very next index from the era we hold.
        let whole = d.secret.len() == 64 && d.era > era;
        if !whole && (d.era != era + 1 || d.secret.len() != 32) {
            crate::logf!("BOND: delivery for era {} of {} while we hold {} — not the next era; ignored", d.era, hex::encode(&fid.as_bytes()[..4]), era);
            return;
        }
        // TWO MINTS OF ONE INDEX converge on the LOWER party's: ours stands against a higher minter's, a lower minter's replaces ours.
        if self.friendship_chains.iter().any(|(id, c)| *id == fid && c.pending_era().is_some()) {
            let ours = self.pending_minter.get(&fid).copied();
            if ours.is_some_and(|m| m < them) {
                crate::logf!("BOND: {} minted era {} of {} too — ours is the lower party's and stands", sender, d.era, hex::encode(&fid.as_bytes()[..4]));
                return;
            }
        }
        self.pending_minter.insert(fid, them);
        if whole {
            let (root, hk) = (<[u8; 32]>::try_from(&d.secret[..32]).unwrap(), <[u8; 32]>::try_from(&d.secret[32..]).unwrap()); // WHY/PROOF: the length is 64, checked above
            if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                chains.install_pending(crate::types::friendship::PendingEra { era_index: d.era, lane_root: root, history_key: Some(hk), tag: crate::crypto::clutch::era_tag(&root), resp_osc: None });
            }
            self.persist_chains_async(&fid);
            crate::logf!("BOND: era {} of {} delivered WHOLE by {} — installed as pending, cutover on the first frame tagged with it", d.era, hex::encode(&fid.as_bytes()[..4]), sender);
            return;
        }
        let fresh: [u8; 32] = <[u8; 32]>::try_from(d.secret.as_slice()).unwrap(); // WHY/PROOF: the length is 32, checked just above
        let transcript = crate::crypto::era::derive_era_transcript(&token, d.era, &d.nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
        let (next_root, next_hk) = crate::crypto::era::derive_era_keys(fid.as_bytes(), d.era, &root, Some(&hk), &fresh, &transcript);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: d.era, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
        }
        self.persist_chains_async(&fid);
        crate::logf!("BOND: era {} of {} delivered by {} — installed as pending, cutover on the first frame tagged with it", d.era, hex::encode(&fid.as_bytes()[..4]), sender);
    }
}

/// The keys of a pair's two-member conversation, from an era root and history key: the pair's id and its pair-derived token (the one every pair conversation has always used), lanes born on demand.
fn pair_conversation_chains(fid: crate::types::FriendshipId, us: [u8; 32], them: [u8; 32], root: [u8; 32], hk: [u8; 32], era: u64, lineage: [u8; 32]) -> crate::types::FriendshipChains {
    let mut chains = crate::types::FriendshipChains::from_molecule_root(crate::types::molecule::MoleculeId(fid.0), &[us, them], root, hk, era, lineage);
    chains.molecule = false;
    chains.conversation_token = crate::crypto::clutch::derive_conversation_token(&[us, them]);
    chains
}

impl PhotonApp {
    /// The chains id our CLUTCH / era machinery runs on for this contact (docs/conversations.md §7): a friend's bond, a sibling's own chains. Pure — whether the chains are held is the caller's lookup.
    pub(super) fn bond_chains_id(&self, contact: &crate::types::Contact) -> Option<crate::types::FriendshipId> {
        if contact.is_sibling {
            return contact.friendship_id;
        }
        let us = self.our_party_id(contact)?;
        contact.bond_id(&us).map(|b| crate::types::FriendshipId::from_bytes(b.0))
    }
}

impl PhotonApp {
    /// MINT the next era of a two-member conversation and deliver it over the bond (docs/conversations.md §0): a fresh secret, the next era derived from the one held, installed as pending, delivered to the other party; the bond's ACK of that delivery is this side's cutover edge (`on_bond_ack`), the first frame tagged with the new era is theirs. A pending era already in flight is left to finish.
    pub(super) fn mint_pair_era(&mut self, conv: crate::types::FriendshipId, why: &str) -> bool {
        let Some(ci) = self.pair_contact_of(conv) else {
            return false;
        };
        let Some(us) = self.our_party_id(&self.contacts[ci]) else {
            return false;
        };
        let Some((era, root, hk, token, lineage, pending)) = self.friendship_chains.iter().find(|(id, _)| *id == conv).and_then(|(_, c)| Some((c.era_index, *c.lane_root()?, *c.history_key()?, c.conversation_token, c.era_lineage, c.pending_era().is_some()))) else {
            return false;
        };
        if pending {
            crate::logf!("ERA: {} already holds a pending era — the cutover edge finishes it ({} waits)", hex::encode(&conv.as_bytes()[..4]), why);
            return false;
        }
        if self.bond_fid_of(ci).is_none() {
            crate::logf!("ERA: no bond yet with the other party of {} — its next era waits for the bond ({})", hex::encode(&conv.as_bytes()[..4]), why);
            return false;
        }
        let next = era + 1;
        let (nonce, mut fresh): ([u8; 32], [u8; 32]) = (rand::random(), rand::random());
        let transcript = crate::crypto::era::derive_era_transcript(&token, next, &nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
        let (next_root, next_hk) = crate::crypto::era::derive_era_keys(conv.as_bytes(), next, &root, Some(&hk), &fresh, &transcript);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: next, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
        }
        self.pending_minter.insert(conv, us);
        self.persist_chains_async(&conv);
        let ts = crate::network::time_base::stamp_osc();
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: conv.0, era: next, era_lineage: lineage, nonce, secret: fresh.to_vec() }) };
        use zeroize::Zeroize;
        fresh.zeroize();
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        if let Some(bfid) = self.bond_fid_of(ci) {
            self.deliveries_in_flight.push((bfid, ts, conv));
        }
        crate::logf!("ERA: minted era#{} of {} ({}) — {} over the bond; pending until the bond ACKs it", next, hex::encode(&conv.as_bytes()[..4]), why, if sent { "delivered" } else { "queued" });
        true
    }

    /// The MINTER's cutover edge for an era delivered over bonds (docs/conversations.md §0): the bond's ACK of a delivery proves a member holds the new era, so the conversation's pending era becomes current here and its undelivered rows re-serve on the fresh lane.
    pub(super) fn on_bond_ack(&mut self, bond: crate::types::FriendshipId, acked: i64) {
        let Some(pos) = self.deliveries_in_flight.iter().position(|(b, t, _)| *b == bond && *t == acked) else {
            return;
        };
        let (_, _, conv) = self.deliveries_in_flight.remove(pos);
        // The first ACK cuts over; the deliveries still in flight for the same era need nothing more of this side.
        self.deliveries_in_flight.retain(|(_, _, c)| *c != conv);
        self.pending_minter.remove(&conv);
        let cut = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv).and_then(|(_, chains)| chains.cut_over_to_pending().map(|r| (r, chains.era_index)));
        if let Some(((old_tag, new_tag, retired), era)) = cut {
            crate::logf!("ERA: {} cut over {:08x} → {:08x} (era#{}) on the bond's ACK of its delivery — {} pending(s) re-serve on the fresh lane", hex::encode(&conv.as_bytes()[..4]), old_tag, new_tag, era, retired);
            if let Some(ci) = self.pair_contact_of(conv) {
                self.era_cutover_flush(ci, &conv);
            } else {
                self.persist_chains_async(&conv);
            }
        }
    }
}

impl PhotonApp {
    /// RE-DELIVER a two-member conversation's era WHOLE over the bond (docs/conversations.md §7): the repair for keys that diverged (garbage past verify) or a member stranded on an era nobody else holds. A fresh root and history key at the next index, installed as pending and delivered whole, so the other side needs nothing it may have lost; two sides repairing at once converge on the lower party's.
    pub(super) fn redeliver_pair_era(&mut self, conv: crate::types::FriendshipId, why: &str) -> bool {
        let Some(ci) = self.pair_contact_of(conv) else {
            return false;
        };
        let Some(us) = self.our_party_id(&self.contacts[ci]) else {
            return false;
        };
        if self.bond_fid_of(ci).is_none() {
            crate::logf!("ERA: {} needs its era re-delivered ({}) but holds no bond yet — the bond's ceremony comes first", hex::encode(&conv.as_bytes()[..4]), why);
            return false;
        }
        let Some((era, lineage)) = self.friendship_chains.iter().find(|(id, _)| *id == conv).map(|(_, c)| (c.era_index, c.era_lineage)) else {
            return false;
        };
        // Past BOTH sides: a whole era must land beyond the index the other party holds too, or a peer ahead of us refuses it as not new.
        let peer_index = self.peer_era_seen.iter().filter(|((f, _), _)| *f == conv).map(|(_, (idx, _))| *idx).max().unwrap_or(0);
        let next = era.max(peer_index) + 1;
        let (root, hk): ([u8; 32], [u8; 32]) = (rand::random(), rand::random());
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: next, lane_root: root, history_key: Some(hk), tag: crate::crypto::clutch::era_tag(&root), resp_osc: None });
        }
        self.pending_minter.insert(conv, us);
        self.persist_chains_async(&conv);
        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(&root);
        secret.extend_from_slice(&hk);
        let ts = crate::network::time_base::stamp_osc();
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: conv.0, era: next, era_lineage: lineage, nonce: rand::random(), secret }) };
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        if let Some(bfid) = self.bond_fid_of(ci) {
            self.deliveries_in_flight.push((bfid, ts, conv));
        }
        crate::logf!("ERA: {} — era#{} re-delivered WHOLE over the bond ({}) — {}", hex::encode(&conv.as_bytes()[..4]), next, why, if sent { "sent" } else { "queued" });
        true
    }
}

impl PhotonApp {
    /// Do these chains know the era `tag` — current, pending, or retired? An era they know is a straggler or a cutover in flight; one they do not is a real disagreement.
    fn chains_know_era(&self, fid: crate::types::FriendshipId, tag: u32) -> bool {
        self.friendship_chains.iter().find(|(id, _)| *id == fid).is_some_and(|(_, c)| c.era_tag() == Some(tag) || c.pending_era().is_some_and(|p| p.tag == tag) || c.retired_era().is_some_and(|r| r.tag == tag))
    }

    /// THE ERA-MISMATCH HEAL (docs/conversations.md §7; Nick 2026-10-25, the Emma stall: one side waited on an answer that never came, the other waited on a repair the first never ran). A peer advertising an era our chains do not know at all is healed by the LOWER party, with nothing to wait for on either side:
    /// a BOND re-runs a fresh CLUTCH; a two-member CONVERSATION has its era re-delivered whole over the bond. The higher party just takes what arrives. Each disagreement acts once (per era seen, and per bond era for a conversation, so a re-keyed bond tries again). Returns whether this was a heal case (the caller's ordinary repair handles the rest).
    pub(super) fn heal_era_mismatch(&mut self, fid: crate::types::FriendshipId, peer_tag: u32) -> bool {
        if self.chains_know_era(fid, peer_tag) {
            return false;
        }
        let Some(us) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            return false;
        };
        if let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && self.bond_chains_id(c) == Some(fid)) {
            let them = self.contacts[ci].handle_hash;
            let fp = crate::fp(&self.contacts[ci].handle_proof);
            if us > them {
                crate::logf!("ERA: {}'s bond advertises era#{:08x}, one ours never held — the lower party re-keys it; waiting", fp, peer_tag);
                return true;
            }
            if self.era_heals.insert((fid, peer_tag, 0)) {
                crate::logf!("ERA: {}'s bond advertises era#{:08x}, one ours never held — we are the lower party: a fresh CLUTCH re-keys the bond", fp, peer_tag);
                self.rekey_without_chains(ci);
            }
            return true;
        }
        if let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && c.friendship_id == Some(fid)) {
            let them = self.contacts[ci].handle_hash;
            let fp = crate::fp(&self.contacts[ci].handle_proof);
            if us > them {
                crate::logf!("ERA: our conversation with {} advertises era#{:08x}, one we never held — the lower party re-delivers it; waiting", fp, peer_tag);
                return true;
            }
            let bond_tag = self.bond_fid_of(ci).and_then(|b| self.friendship_chains.iter().find(|(id, _)| *id == b)).and_then(|(_, c)| c.era_tag()).unwrap_or(0);
            if self.friendship_chains.iter().any(|(id, c)| *id == fid && c.pending_era().is_some()) || self.deliveries_in_flight.iter().any(|(_, _, c)| *c == fid) {
                return true; // a re-delivery is already on its way
            }
            if self.era_heals.insert((fid, peer_tag, bond_tag)) {
                crate::logf!("ERA: our conversation with {} advertises era#{:08x}, one we never held — we are the lower party: its era goes whole over the bond", fp, peer_tag);
                self.redeliver_pair_era(fid, "era mismatch");
            }
            return true;
        }
        false
    }

    /// A bond just (re-)completed: every conversation disagreement already observed gets its heal again over the fresh bond — observations fire on a change only, so nothing else would re-raise them.
    pub(super) fn reheal_after_bond(&mut self, ci: usize) {
        let Some(conv) = self.contacts.get(ci).and_then(|c| c.friendship_id) else {
            return;
        };
        let seen: Vec<u32> = self.peer_era_seen.iter().filter(|((f, dev), _)| *f == conv && self.contacts[ci].knows_device(dev)).map(|(_, (_, tag))| *tag).collect();
        for tag in seen {
            self.heal_era_mismatch(conv, tag);
        }
    }
}
