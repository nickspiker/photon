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
        let Some(us) = self.own_party_id() else {
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
    /// A friend's BOND just completed its ceremony (docs/conversations.md §7): make sure its hidden conversation exists, and heal over it whatever the old bond could not carry. No conversation is founded here (D8).
    pub(super) fn bond_completed(&mut self, ci: usize) {
        let Some((b, them, fresh_friend)) = self.contacts.get(ci).and_then(|c| self.our_party_id(c).and_then(|us| c.bond_id(&us)).map(|b| (b, c.handle_hash, c.friendship_id.is_none()))) else {
            return;
        };
        let bfid = crate::types::FriendshipId::from_bytes(b.0);
        if !self.conversations.iter().any(|v| v.id() == bfid) {
            if let Some(us) = self.own_party_id() {
                let mut bc = crate::types::Conversation::new_bond(b, [us, them]);
                // A brand-new friend's bond has nothing on disk; an existing friend's re-keyed bond was loaded at boot, and a miss here hydrates thru the persist rescue.
                bc.hydrated = fresh_friend;
                self.conversations.push(bc);
            }
        }
        // Adding a friend creates the bond only (docs/conversations.md D8): a conversation is founded when someone taps Message — now, if a Message tap was waiting on this bond.
        if self.found_when_bonded.remove(&them) {
            self.found_pair_conversation(ci);
        }
        // A disagreement the old bond could not carry gets its heal again over this one.
        self.reheal_after_bond(ci);
    }

    /// FOUND the pair's two-member conversation (docs/conversations.md §7, D8 — Message on a person): whoever taps mints its era 0 and delivers it whole on the bond; the other side installs it when it lands (`on_era_delivery`), and two simultaneous foundings converge thru the era-mismatch heal. A friend who already holds a conversation with us keeps it.
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
        // The founding travels on the bond: tapped before the bond completed, it is remembered and runs on the completion edge.
        if self.bond_fid_of(ci).is_none() {
            crate::logf!("BOND: {} — Message waits for the secure channel; our conversation is founded the moment it completes", crate::fp(&self.contacts[ci].handle_proof));
            self.found_when_bonded.insert(them);
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
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: fid.0, era: 0, era_lineage: lineage, nonce, secret, prior_tag: None }), answer: None };
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
        // An ANSWER to an era we delivered rides the same Deliver row kind, with no delivery set (2026-11-13).
        if let Some(a) = wire.as_ref().and_then(|w| w.answer.clone()) {
            self.on_era_answer(ci, a);
            return;
        }
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
        let Some((era, cur_tag, root, hk, token, pending_tag, retired_tag)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_tag()?, *c.lane_root()?, *c.history_key()?, c.conversation_token, c.pending_era().map(|p| p.tag), c.retired_era().map(|r| r.tag)))) else {
            return;
        };
        let whole = d.secret.len() == 64;
        if !whole && d.secret.len() != 32 {
            crate::logf!("BOND: delivery from {} for {} carries a {}-byte secret, neither a whole era nor a mint — refused", sender, hex::encode(&fid.as_bytes()[..4]), d.secret.len());
            self.send_era_answer(ci, fid, d.nonce, crate::network::message_package::EraVerdict::Refuse);
            return;
        }
        self.note_era_index(fid, d.era);
        // The delivered era's keys, where we can know them: a WHOLE era carries its root ‖ history key; a FRESH mint derives the very next index from the era it names, which must be ours.
        let fresh_derivable = !whole && d.era == era + 1 && d.prior_tag.map_or(true, |p| p == cur_tag);
        let delivered: Option<([u8; 32], [u8; 32])> = if whole {
            Some((<[u8; 32]>::try_from(&d.secret[..32]).unwrap(), <[u8; 32]>::try_from(&d.secret[32..]).unwrap())) // WHY/PROOF: the length is 64, checked above
        } else if fresh_derivable {
            let fresh: [u8; 32] = <[u8; 32]>::try_from(d.secret.as_slice()).unwrap(); // WHY/PROOF: the length is 32, checked above
            let transcript = crate::crypto::era::derive_era_transcript(&token, d.era, &d.nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
            Some(crate::crypto::era::derive_era_keys(fid.as_bytes(), d.era, &root, Some(&hk), &fresh, &transcript))
        } else {
            None
        };
        let delivered_tag = delivered.as_ref().map(|(r, _)| crate::crypto::clutch::era_tag(r));
        let facts = DeliveryFacts {
            our_era: era,
            our_tag: cur_tag,
            pending_tag,
            // TWO MINTS OF ONE INDEX converge on the LOWER party's: a pending a lower party than this sender minted stands against it.
            pending_stands: pending_tag.is_some() && self.pending_minter.get(&fid).is_some_and(|m| *m < them),
            retired_tag,
            delivered_era: d.era,
            delivered_tag,
            whole,
            prior_tag: d.prior_tag,
            deliverer_lower: them < us,
        };
        let conv4 = hex::encode(&fid.as_bytes()[..4]);
        match era_delivery_verdict(&facts) {
            DeliveryVerdict::Held => {
                // The same era at a higher index: the sender relabelled its copy past every index it saw (`deliver_own_era_whole`), and ours follows so one era carries one index on both sides.
                if delivered_tag == Some(cur_tag) && d.era > era {
                    if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                        chains.era_index = d.era;
                        chains.mutated_osc = vsf::eagle_time_oscillations();
                    }
                    self.persist_chains_async(&fid);
                }
                crate::logf!("BOND: era {} of {} from {} is one we already hold — accepted again", d.era, conv4, sender);
                self.send_era_answer(ci, fid, d.nonce, crate::network::message_package::EraVerdict::Accept);
            }
            DeliveryVerdict::Install => {
                let (next_root, next_hk) = delivered.expect("an installable delivery always has its keys: a whole era carries them, a fresh next step derives them from ours"); // WHY/PROOF: era_delivery_verdict answers Install only for a whole era or a derivable fresh next step, the two cases `delivered` is Some
                self.pending_minter.insert(fid, them);
                if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                    chains.install_pending(crate::types::friendship::PendingEra { era_index: d.era, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
                }
                self.persist_chains_async(&fid);
                crate::logf!("BOND: era {} of {} delivered {}by {} — installed as pending and accepted; cutover on the first frame tagged with it", d.era, conv4, if whole { "WHOLE " } else { "" }, sender);
                self.send_era_answer(ci, fid, d.nonce, crate::network::message_package::EraVerdict::Accept);
            }
            DeliveryVerdict::Refuse { heal } => {
                // NEVER SILENT (2026-11-13, Brittany's era split): the desktop logged "not the next era; ignored" for her era 13 while it held its own 13, ACKed the row, and her phone cut over to an era the desktop never held. The refusal is typed and names the era we hold.
                let why = if facts.pending_stands {
                    "our pending era is the lower party's and stands"
                } else if d.era == era && delivered_tag.is_some() {
                    "another era at the index we hold"
                } else if !whole && d.prior_tag.is_some_and(|p| p != cur_tag) {
                    "a mint from an era we do not hold"
                } else if d.era <= era {
                    "an index at or below ours"
                } else {
                    "not the next step from our era"
                };
                crate::logf!("BOND: delivery of era {} of {} from {} refused ({}) — we hold era {} {:08x}{}", d.era, conv4, sender, why, era, cur_tag, if heal { "; we are the lower party, so ours goes to them whole" } else { "" });
                self.send_era_answer(ci, fid, d.nonce, crate::network::message_package::EraVerdict::Refuse);
                // One side heals, the same one on both: the lower party delivers its own era whole past every index seen. Once per refused delivery (its nonce) as we stand.
                if heal && pair {
                    let mut n4 = [0u8; 4];
                    n4.copy_from_slice(&d.nonce[..4]);
                    if self.era_heals.insert((fid, u32::from_le_bytes(n4), cur_tag)) {
                        self.deliver_own_era_whole(fid, "a refused delivery");
                    }
                }
            }
        }
    }
}

/// What a recipient does with a delivered era (2026-11-13). Pure, so both sides of a pair settle a conflict the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeliveryVerdict {
    /// Install the delivered era as pending and accept it.
    Install,
    /// We already hold it, current or pending: accept again, nothing to install (a retransmit, or our answer was lost).
    Held,
    /// Refuse it. `heal`: we are the lower party, so ours is the era both sides converge on and we deliver it.
    Refuse { heal: bool },
}

/// The facts a delivery verdict reads.
#[derive(Clone, Copy, Debug)]
pub(super) struct DeliveryFacts {
    pub our_era: u64,
    pub our_tag: u32,
    pub pending_tag: Option<u32>,
    /// Our pending era was minted by a party lower than this sender, so it stands against the delivery.
    pub pending_stands: bool,
    pub retired_tag: Option<u32>,
    pub delivered_era: u64,
    /// The delivered era's tag when it can be known: a whole era's root, or a fresh mint derived from the era it names.
    pub delivered_tag: Option<u32>,
    pub whole: bool,
    /// The era a fresh mint derives from; None from a build before the field (assumed ours, as before).
    pub prior_tag: Option<u32>,
    /// The sender is the lower party of the two.
    pub deliverer_lower: bool,
}

/// THE DELIVERY VERDICT (2026-11-13, Brittany's era split). A delivery at the index we already hold, under another tag, used to be "not the next era; ignored" while its bond row was ACKed, and the sender cut over to an era we never held.
/// Now: an era we hold is accepted again; the next step from ours (a whole era past our index, or a mint from our own era at our index + 1) is installed unless a lower party's pending stands; another era at our very index is settled by the lower party, whose era wins on both sides; anything else is refused, and the lower party heals.
pub(super) fn era_delivery_verdict(f: &DeliveryFacts) -> DeliveryVerdict {
    let we_lower = !f.deliverer_lower;
    if let Some(t) = f.delivered_tag {
        if t == f.our_tag || f.pending_tag == Some(t) {
            return DeliveryVerdict::Held;
        }
        if f.retired_tag == Some(t) {
            return DeliveryVerdict::Refuse { heal: we_lower };
        }
    }
    let next_step = if f.whole { f.delivered_era > f.our_era } else { f.delivered_era == f.our_era + 1 && f.prior_tag.map_or(true, |p| p == f.our_tag) };
    if next_step {
        if f.pending_tag.is_some() && f.pending_stands {
            return DeliveryVerdict::Refuse { heal: false };
        }
        return DeliveryVerdict::Install;
    }
    // ONE INDEX, TWO ERAS: the lower party's era wins on both sides, so the higher party installs it and the lower party keeps its own.
    if f.whole && f.delivered_era == f.our_era && f.deliverer_lower {
        return DeliveryVerdict::Install;
    }
    DeliveryVerdict::Refuse { heal: we_lower }
}

/// An era we offered as our pending and await the answer for (`PhotonApp::era_offers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EraOffer {
    pub conv: crate::types::FriendshipId,
    /// The delivery's nonce: the answer echoes it.
    pub nonce: [u8; 32],
    /// The pending era's tag the offer installs.
    pub tag: u32,
}

/// What the MINTER hears about an era it offered (2026-11-13). The peer's first frame tagged with the era is the other cutover edge; the receive path takes it (status.rs, the pending slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MinterEvent {
    /// The transport ACK of the Deliver row: a refusing peer sends it too.
    BondAck,
    /// The peer's typed answer for the offer.
    Answer { accepted: bool },
}

/// Does this event cut the minter over to the era it offered (`offered`, while `pending` is what it holds as pending now)? Only an accept of that very era does, never the bond row's ordinary ACK. Pure, table-tested.
pub(super) fn minter_cuts_over(event: MinterEvent, offered: u32, pending: Option<u32>) -> bool {
    pending == Some(offered) && event == MinterEvent::Answer { accepted: true }
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
    /// MINT the next era of a two-member conversation and deliver it over the bond (docs/conversations.md §0): a fresh secret, the next era derived from the one held, installed as pending, delivered to the other party.
    /// The other party's ACCEPT is this side's cutover edge (`on_era_answer`), and its first frame tagged with the new era (the receive path); the bond row's ordinary ACK never is (2026-11-13: a refusing peer ACKs too).
    /// An offer still awaiting its answer is left to finish; a pending era nobody ever answered (a restart, or a peer that predates answers) is replaced.
    pub(super) fn mint_pair_era(&mut self, conv: crate::types::FriendshipId, why: &str) -> bool {
        let Some(ci) = self.pair_contact_of(conv) else {
            return false;
        };
        let Some(us) = self.our_party_id(&self.contacts[ci]) else {
            return false;
        };
        let them = self.contacts[ci].handle_hash;
        let Some((era, cur_tag, root, hk, token, lineage, pending)) = self.friendship_chains.iter().find(|(id, _)| *id == conv).and_then(|(_, c)| Some((c.era_index, c.era_tag()?, *c.lane_root()?, *c.history_key()?, c.conversation_token, c.era_lineage, c.pending_era().map(|p| p.tag)))) else {
            return false;
        };
        if self.bond_fid_of(ci).is_none() {
            crate::logf!("ERA: no bond yet with the other party of {} — its next era waits for the bond ({})", hex::encode(&conv.as_bytes()[..4]), why);
            return false;
        }
        if let Some(p) = pending {
            if self.offer_outstanding(conv) || self.deliveries_in_flight.iter().any(|(_, _, c)| *c == conv) {
                crate::logf!("ERA: {} already offers a pending era — its answer finishes it ({} waits)", hex::encode(&conv.as_bytes()[..4]), why);
                return false;
            }
            if self.pending_minter.get(&conv) == Some(&them) {
                crate::logf!("ERA: {} holds the other party's pending era — their first frame on it cuts over ({} waits)", hex::encode(&conv.as_bytes()[..4]), why);
                return false;
            }
            crate::logf!("ERA: {}'s pending era#{:08x} was never answered — this mint replaces it ({})", hex::encode(&conv.as_bytes()[..4]), p, why);
        }
        // PAST EVERY INDEX SEEN (2026-11-13): a friend seen at an index past ours is on another era, and a mint from ours would land on an index they already hold. The era goes whole, past all of them.
        let floor = self.era_index_floor(conv);
        if floor > era {
            crate::logf!("ERA: {} was seen at era index {} while we hold {} — a mint from our era would collide; the next era goes whole past every index ({})", hex::encode(&conv.as_bytes()[..4]), floor, era, why);
            return self.redeliver_pair_era(conv, why);
        }
        let next = era + 1;
        let (nonce, mut fresh): ([u8; 32], [u8; 32]) = (rand::random(), rand::random());
        let transcript = crate::crypto::era::derive_era_transcript(&token, next, &nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
        let (next_root, next_hk) = crate::crypto::era::derive_era_keys(conv.as_bytes(), next, &root, Some(&hk), &fresh, &transcript);
        let tag = crate::crypto::clutch::era_tag(&next_root);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: next, lane_root: next_root, history_key: Some(next_hk), tag, resp_osc: None });
        }
        self.pending_minter.insert(conv, us);
        self.era_offers.retain(|o| o.conv != conv);
        self.era_offers.push(EraOffer { conv, nonce, tag });
        self.persist_chains_async(&conv);
        let ts = crate::network::time_base::stamp_osc();
        // The mint names the era it derives from, so a recipient on another era at our index refuses instead of deriving keys we do not share.
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: conv.0, era: next, era_lineage: lineage, nonce, secret: fresh.to_vec(), prior_tag: Some(cur_tag) }), answer: None };
        use zeroize::Zeroize;
        fresh.zeroize();
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        if let Some(bfid) = self.bond_fid_of(ci) {
            self.deliveries_in_flight.push((bfid, ts, conv));
        }
        crate::logf!("ERA: minted era#{} of {} ({}) — {} over the bond; pending until they accept it", next, hex::encode(&conv.as_bytes()[..4]), why, if sent { "delivered" } else { "queued" });
        true
    }

    /// The bond ACKed one of our era deliveries. Transport only (2026-11-13, Brittany's era split: the desktop refused her era 13 and still ACKed the row, and her phone cut over on that ACK to an era the desktop never held): the minter waits for the other side's accept or its first frame on the era.
    pub(super) fn on_bond_ack(&mut self, bond: crate::types::FriendshipId, acked: i64) {
        let Some(pos) = self.deliveries_in_flight.iter().position(|(b, t, _)| *b == bond && *t == acked) else {
            return;
        };
        let (_, _, conv) = self.deliveries_in_flight.remove(pos);
        let pending = self.friendship_chains.iter().find(|(id, _)| *id == conv).and_then(|(_, c)| c.pending_era().map(|p| p.tag));
        if let Some(offer) = self.era_offers.iter().find(|o| o.conv == conv).copied() {
            // OLD-PEER ERA CUTOVER, MIGRATION-EXPIRES: 2027-02-01. A friend whose build predates era answers never accepts or refuses, so a delivery to it would wait forever; for a one-to-one friend never seen answering, the plain ACK cuts us over as it did before 2026-11-13.
            // A friend on a build with answers that has not answered yet this session can be cut over here early; its refusal, if it sends one, starts the heal (the lower party delivers its own era past every index), so the pair still converges.
            if let Some(pci) = self.pair_contact_of(conv).filter(|&pci| self.contacts.get(pci).is_some_and(|c| !self.era_answerers.contains(&c.handle_proof))) {
                if pending == Some(offer.tag) {
                    self.era_offers.retain(|o| o.conv != conv);
                    self.deliveries_in_flight.retain(|(_, _, c)| *c != conv);
                    self.pending_minter.remove(&conv);
                    let cut = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv).and_then(|(_, chains)| chains.cut_over_to_pending().map(|r| (r, chains.era_index)));
                    if let Some(((old_tag, new_tag, retired), era)) = cut {
                        crate::logf!("ERA: {} cut over {:08x} → {:08x} (era#{}) on the bond ACK of a friend never seen answering (an older build) — {} pending(s) re-serve on the fresh lane", hex::encode(&conv.as_bytes()[..4]), old_tag, new_tag, era, retired);
                        self.era_cutover_flush(pci, &conv);
                    }
                    return;
                }
            }
            debug_assert!(!minter_cuts_over(MinterEvent::BondAck, offer.tag, pending));
            crate::logf!("ERA: the bond ACKed our delivery of era#{:08x} for {} — no cutover on an ACK; it waits for their accept or their first frame on it", offer.tag, hex::encode(&conv.as_bytes()[..4]));
        }
    }

    /// The other party's ANSWER to an era we delivered (2026-11-13): an accept of the era still pending here cuts us over; a refusal drops that pending, keeps the era we hold, and learns the index and tag they hold, so the lower party can heal.
    pub(super) fn on_era_answer(&mut self, ci: usize, a: crate::network::message_package::EraAnswerWire) {
        let sender = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let fid = crate::types::FriendshipId::from_bytes(a.conversation);
        let conv4 = hex::encode(&fid.as_bytes()[..4]);
        let Some((us, them)) = self.contacts.get(ci).and_then(|c| self.our_party_id(c).map(|us| (us, c.handle_hash))) else {
            return;
        };
        // Only a member answers for a conversation, exactly as only a member delivers one.
        let pair = fid == crate::types::FriendshipId::derive(&[us, them]);
        let group = self.molecule_rosters.iter().find(|(g, _)| g.0 == fid.0).is_some_and(|(_, r)| r.is_standing(&them));
        if !pair && !group {
            crate::logf!("ERA: {} answered for {}, a conversation they do not stand in — ignored", sender, conv4);
            return;
        }
        // This friend's build answers eras: its plain bond ACK no longer cuts us over (OLD-PEER ERA CUTOVER).
        if let Some(hp) = self.contacts.get(ci).map(|c| c.handle_proof) {
            if !self.era_answerers.contains(&hp) {
                self.era_answerers.push(hp);
            }
        }
        self.note_era_index(fid, a.held_era);
        let Some(offer) = self.era_offers.iter().find(|o| o.conv == fid && o.nonce == a.nonce).copied() else {
            crate::logf!("ERA: {} answered {} for a delivery of {} we no longer offer — nothing to do", sender, a.verdict.word(), conv4);
            return;
        };
        let pending = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.pending_era().map(|p| p.tag));
        match a.verdict {
            crate::network::message_package::EraVerdict::Accept => {
                if !minter_cuts_over(MinterEvent::Answer { accepted: true }, offer.tag, pending) {
                    self.era_offers.retain(|o| *o != offer);
                    crate::logf!("ERA: {} accepted era#{:08x} of {}, which is no longer pending here — nothing to cut over", sender, offer.tag, conv4);
                    return;
                }
                // The first accept cuts over; every other offer of this conversation is spent with it.
                self.era_offers.retain(|o| o.conv != fid);
                self.deliveries_in_flight.retain(|(_, _, c)| *c != fid);
                self.pending_minter.remove(&fid);
                let cut = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid).and_then(|(_, chains)| chains.cut_over_to_pending().map(|r| (r, chains.era_index)));
                if let Some(((old_tag, new_tag, retired), era)) = cut {
                    crate::logf!("ERA: {} cut over {:08x} → {:08x} (era#{}) on {}'s accept — {} pending(s) re-serve on the fresh lane", conv4, old_tag, new_tag, era, sender, retired);
                    if let Some(pci) = self.pair_contact_of(fid) {
                        self.era_cutover_flush(pci, &fid);
                    } else if self.friendship_chains.iter().any(|(id, c)| *id == fid && c.molecule) {
                        // A GROUP cut over by a delivered era (the fork heal, molecules.rs): its held rows go out again on the new era, as after a mint's cutover.
                        self.after_molecule_cutover(crate::types::molecule::MoleculeId(fid.0));
                    } else {
                        self.persist_chains_async(&fid);
                    }
                }
            }
            crate::network::message_package::EraVerdict::Refuse => {
                if group {
                    // One member's refusal is not the group's: the others may still accept, and the group's own fork heal settles the rest.
                    self.era_offers.retain(|o| *o != offer);
                    crate::logf!("ERA: {} refused our era#{:08x} of group {} (they hold era {} {:08x})", sender, offer.tag, conv4, a.held_era, a.held_tag);
                    return;
                }
                self.era_offers.retain(|o| o.conv != fid);
                self.deliveries_in_flight.retain(|(_, _, c)| *c != fid);
                if pending == Some(offer.tag) {
                    if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                        chains.clear_pending();
                    }
                    self.pending_minter.remove(&fid);
                    self.persist_chains_async(&fid);
                }
                crate::logf!("ERA: {} refused our era#{:08x} of {} — they hold era {} {:08x}; our pending is dropped and we stay on the era we hold", sender, offer.tag, conv4, a.held_era, a.held_tag);
                // The era they hold, if we do not, is a disagreement the lower party heals; the heal decides which side that is.
                if !self.chains_know_era(fid, a.held_tag) {
                    self.heal_era_mismatch(fid, a.held_tag);
                }
            }
        }
    }

    /// Answer a delivery of `conv`'s era from contact `ci` (2026-11-13): typed, over the bond, naming the delivery by its nonce and the era we now hold.
    pub(super) fn send_era_answer(&mut self, ci: usize, conv: crate::types::FriendshipId, nonce: [u8; 32], verdict: crate::network::message_package::EraVerdict) {
        let Some(us) = self.contacts.get(ci).and_then(|c| self.our_party_id(c)) else {
            return;
        };
        let Some((held_era, held_tag)) = self.friendship_chains.iter().find(|(id, _)| *id == conv).and_then(|(_, c)| Some((c.era_index, c.era_tag()?))) else {
            return;
        };
        let answer = crate::network::message_package::EraAnswerWire { conversation: conv.0, nonce, verdict, held_era, held_tag };
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: None, answer: Some(answer) };
        let sent = self.bond_transmit_with(ci, crate::network::time_base::stamp_osc(), None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        crate::logf!("ERA: answered {} to {}'s era delivery for {} (we hold era {} {:08x}) — {}", verdict.word(), self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default(), hex::encode(&conv.as_bytes()[..4]), held_era, held_tag, if sent { "sent" } else { "queued" });
    }

    /// Record an era index seen for a conversation (a delivery, an answer): the next mint goes past it.
    pub(super) fn note_era_index(&mut self, conv: crate::types::FriendshipId, index: u64) {
        let high = self.era_index_seen.entry(conv).or_insert(0);
        *high = (*high).max(index);
    }

    /// The highest era index known for a conversation: ours, every index the other side's devices advertised, delivered or answered. A new era goes past it (2026-11-13: her phone minted index 13 without having seen the desktop's 13).
    pub(super) fn era_index_floor(&self, conv: crate::types::FriendshipId) -> u64 {
        let ours = self.friendship_chains.iter().find(|(id, _)| *id == conv).map_or(0, |(_, c)| c.era_index);
        let adverts = self.peer_era_seen.iter().filter(|((f, _), _)| *f == conv).map(|(_, (idx, _))| *idx).max().unwrap_or(0);
        let seen = self.era_index_seen.get(&conv).copied().unwrap_or(0);
        ours.max(adverts).max(seen)
    }

    /// Is an era we offered still pending here and unanswered?
    pub(super) fn offer_outstanding(&self, conv: crate::types::FriendshipId) -> bool {
        let pending = self.friendship_chains.iter().find(|(id, _)| *id == conv).and_then(|(_, c)| c.pending_era().map(|p| p.tag));
        self.era_offers.iter().any(|o| o.conv == conv && pending == Some(o.tag))
    }
}

impl PhotonApp {
    /// RE-DELIVER a two-member conversation's era WHOLE over the bond (docs/conversations.md §7): the repair for keys that diverged (garbage past verify). A fresh root and history key past every index seen, installed as pending and delivered whole, so the other side needs nothing it may have lost.
    /// Like a mint, it cuts over on their accept or their first frame on it; two sides repairing at once converge on the lower party's.
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
        let Some(lineage) = self.friendship_chains.iter().find(|(id, _)| *id == conv).map(|(_, c)| c.era_lineage) else {
            return false;
        };
        // Past EVERY index seen: a whole era must land beyond the index the other party holds too, or a peer ahead of us refuses it as not new.
        let next = self.era_index_floor(conv) + 1;
        let (root, hk, nonce): ([u8; 32], [u8; 32], [u8; 32]) = (rand::random(), rand::random(), rand::random());
        let tag = crate::crypto::clutch::era_tag(&root);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: next, lane_root: root, history_key: Some(hk), tag, resp_osc: None });
        }
        self.pending_minter.insert(conv, us);
        self.era_offers.retain(|o| o.conv != conv);
        self.era_offers.push(EraOffer { conv, nonce, tag });
        self.persist_chains_async(&conv);
        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(&root);
        secret.extend_from_slice(&hk);
        let ts = crate::network::time_base::stamp_osc();
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: conv.0, era: next, era_lineage: lineage, nonce, secret, prior_tag: None }), answer: None };
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        if let Some(bfid) = self.bond_fid_of(ci) {
            self.deliveries_in_flight.push((bfid, ts, conv));
        }
        crate::logf!("ERA: {} — era#{} re-delivered WHOLE over the bond ({}) — {}; pending until they accept it", hex::encode(&conv.as_bytes()[..4]), next, why, if sent { "sent" } else { "queued" });
        true
    }

    /// THE LOWER PARTY'S HEAL (2026-11-13): deliver the era we hold WHOLE, at an index past every index seen, and relabel our own copy to that index, so one era carries one index on both sides.
    /// The other party installs it and cuts over on our very next frame, which already carries its tag; this side has nothing to cut over to, so no accept is needed, and a build that predates answers takes it the same way.
    /// Any pending era here is dropped first: we are declaring the era we hold the one both sides converge on.
    pub(super) fn deliver_own_era_whole(&mut self, conv: crate::types::FriendshipId, why: &str) -> bool {
        let Some(ci) = self.pair_contact_of(conv) else {
            return false;
        };
        let Some(us) = self.our_party_id(&self.contacts[ci]) else {
            return false;
        };
        let Some(bfid) = self.bond_fid_of(ci) else {
            crate::logf!("ERA: {} needs our era delivered ({}) but holds no bond yet — the bond's ceremony comes first", hex::encode(&conv.as_bytes()[..4]), why);
            return false;
        };
        let Some((era, tag, root, hk, lineage)) = self.friendship_chains.iter().find(|(id, _)| *id == conv).and_then(|(_, c)| Some((c.era_index, c.era_tag()?, *c.lane_root()?, *c.history_key()?, c.era_lineage))) else {
            return false;
        };
        let next = self.era_index_floor(conv) + 1;
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == conv) {
            chains.clear_pending();
            chains.era_index = next;
            chains.mutated_osc = vsf::eagle_time_oscillations();
        }
        self.pending_minter.remove(&conv);
        self.era_offers.retain(|o| o.conv != conv);
        self.persist_chains_async(&conv);
        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(&root);
        secret.extend_from_slice(&hk);
        let ts = crate::network::time_base::stamp_osc();
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: conv.0, era: next, era_lineage: lineage, nonce: rand::random(), secret, prior_tag: None }), answer: None };
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        self.deliveries_in_flight.push((bfid, ts, conv));
        crate::logf!("ERA: {} — our era#{:08x} delivered WHOLE at index {} (ours relabelled from {}) ({}): the other party cuts over on our next frame — {}", hex::encode(&conv.as_bytes()[..4]), tag, next, era, why, if sent { "sent" } else { "queued" });
        true
    }
}

impl PhotonApp {
    /// Do these chains know the era `tag` — current, pending, or retired? An era they know is a straggler or a cutover in flight; one they do not is a real disagreement.
    pub(super) fn chains_know_era(&self, fid: crate::types::FriendshipId, tag: u32) -> bool {
        self.friendship_chains.iter().find(|(id, _)| *id == fid).is_some_and(|(_, c)| c.era_tag() == Some(tag) || c.pending_era().is_some_and(|p| p.tag == tag) || c.retired_era().is_some_and(|r| r.tag == tag))
    }

    /// THE ERA-MISMATCH HEAL (docs/conversations.md §7; Nick 2026-10-25, the Emma stall: one side waited on an answer that never came, the other waited on a repair the first never ran). A peer advertising an era our chains do not know at all is healed by the LOWER party, with nothing to wait for on either side:
    /// a BOND re-runs a fresh CLUTCH; a two-member CONVERSATION has its era re-delivered whole over the bond. The higher party just takes what arrives. Each disagreement acts once (per era seen, and per bond era for a conversation, so a re-keyed bond tries again). Returns whether this was a heal case (the caller's ordinary repair handles the rest).
    pub(super) fn heal_era_mismatch(&mut self, fid: crate::types::FriendshipId, peer_tag: u32) -> bool {
        if self.chains_know_era(fid, peer_tag) {
            return false;
        }
        let Some(us) = self.own_party_id() else {
            return false;
        };
        if let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && self.bond_chains_id(c) == Some(fid)) {
            let them = self.contacts[ci].handle_hash;
            let fp = crate::fp(&self.contacts[ci].handle_proof);
            // §4.2: one device of our fleet owns the ceremonies with this friend — the heal runs there, never on a parked sibling (its round would be dropped the moment its keygen landed).
            if super::ceremony_parked_by(&self.contacts[ci], self.device_keypair.as_ref().map(|k| *k.public.as_bytes()), &super::sibling_presence_snapshot(&self.contacts)) {
                crate::logf!("ERA: {} disagrees with us about era#{:08x} — the fleet's ceremony owner for them heals it; telling it what we saw", fp, peer_tag);
                self.pull_era_from_fleet(fid);
                self.ask_owner(fid, crate::network::fgtw::protocol::OwnerAsk { friend_tag: Some(peer_tag), fork: false });
                return true;
            }
            // THE FLEET FIRST (2026-11-01, Nick's Pixel and Emma): a device that booted on an old era sees the friend's era as unknown while a sibling holds it — the Pixel re-keyed here, moved the whole fleet off the era Emma was on, and every frame both ways was dropped.
            // A device with siblings only ever asks them; the era repair (era.rs) re-keys once every live sibling has answered that none holds an era the friend is on (FleetAllMissed), so a re-key happens only when the identity as a whole has lost the bond.
            if self.contacts.iter().any(|c| c.is_sibling && !c.locked_out) {
                crate::logf!("ERA: {}'s bond is on era#{:08x}, one we do not hold — asking our own devices, not re-keying", fp, peer_tag);
                self.pull_era_from_fleet(fid);
                return true;
            }
            if us > them {
                crate::logf!("ERA: {}'s bond advertises era#{:08x}, one ours never held — the lower party re-keys it; waiting", fp, peer_tag);
                return true;
            }
            // Once per disagreement AS WE STAND: keyed by our own era too, so a side whose era moved since its last try (a re-key that completed here and not there) tries again instead of waiting on itself forever.
            let our_tag = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.era_tag()).unwrap_or(0);
            if self.era_heals.insert((fid, peer_tag, our_tag)) {
                crate::logf!("ERA: {}'s bond advertises era#{:08x}, one ours never held — we are the lower party: a fresh CLUTCH re-keys the bond", fp, peer_tag);
                self.rekey_without_chains(ci);
            }
            return true;
        }
        if let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && c.friendship_id == Some(fid)) {
            let them = self.contacts[ci].handle_hash;
            let fp = crate::fp(&self.contacts[ci].handle_proof);
            // §4.2: one device of our fleet owns the ceremonies with this friend — the heal runs there, never on a parked sibling (its round would be dropped the moment its keygen landed).
            if super::ceremony_parked_by(&self.contacts[ci], self.device_keypair.as_ref().map(|k| *k.public.as_bytes()), &super::sibling_presence_snapshot(&self.contacts)) {
                crate::logf!("ERA: {} disagrees with us about era#{:08x} — the fleet's ceremony owner for them heals it; telling it what we saw", fp, peer_tag);
                self.pull_era_from_fleet(fid);
                self.ask_owner(fid, crate::network::fgtw::protocol::OwnerAsk { friend_tag: Some(peer_tag), fork: false });
                return true;
            }
            // A DEVICE BEHIND ITS FLEET ASKS THE FLEET (docs/lanes.md rule 3): a device of ours advertising the friend's very era means we are the stale one, and our era must not be pushed onto the friend.
            let fleet_holds_theirs = self.peer_era_seen.iter().any(|((f, dev), (_, tag))| *f == fid && *tag == peer_tag && self.contacts.iter().any(|c| c.is_sibling && !c.locked_out && c.knows_device(dev)));
            if fleet_holds_theirs {
                crate::logf!("ERA: our conversation with {} is on era#{:08x}, which a device of ours holds — asking our own devices, not delivering ours", fp, peer_tag);
                self.pull_era_from_fleet(fid);
                return true;
            }
            if us > them {
                crate::logf!("ERA: our conversation with {} advertises era#{:08x}, one we never held — the lower party delivers its era; waiting", fp, peer_tag);
                return true;
            }
            let bond_tag = self.bond_fid_of(ci).and_then(|b| self.friendship_chains.iter().find(|(id, _)| *id == b)).and_then(|(_, c)| c.era_tag()).unwrap_or(0);
            // The lower party's era wins (2026-11-13): it goes WHOLE past every index seen, relabelled here, and the other side cuts over on our next frame, so no answer is waited on and a build that predates answers converges the same way.
            if self.era_heals.insert((fid, peer_tag, bond_tag)) {
                crate::logf!("ERA: our conversation with {} advertises era#{:08x}, one we never held — we are the lower party: our era goes whole over the bond", fp, peer_tag);
                self.deliver_own_era_whole(fid, "era mismatch");
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

impl PhotonApp {
    /// A GROUP MEMBER still writing on an era we retired never received the wrap for the current one (lost, or sent under the era it was meant to replace): hand it the current era WHOLE over our bond with them, once per era per member.
    /// Harmless to a member merely straggling thru its cutover: a whole era at an index it already holds is refused as not new.
    pub(super) fn deliver_era_to_straggler(&mut self, fid: crate::types::FriendshipId, party: [u8; 32]) {
        let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && c.handle_hash == party) else {
            return;
        };
        let Some(us) = self.our_party_id(&self.contacts[ci]) else {
            return;
        };
        if self.bond_fid_of(ci).is_none() {
            return;
        }
        let Some((era, lineage, tag, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, c.era_tag()?, *c.lane_root()?, *c.history_key()?))) else {
            return;
        };
        let mut who = [0u8; 4];
        who.copy_from_slice(&party[..4]);
        if !self.era_heals.insert((fid, tag, u32::from_le_bytes(who))) {
            return;
        }
        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(&root);
        secret.extend_from_slice(&hk);
        let ts = crate::network::time_base::stamp_osc();
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: fid.0, era, era_lineage: lineage, nonce: rand::random(), secret, prior_tag: None }), answer: None };
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Deliver));
        crate::logf!("ERA: {} still writes {} on a retired era — era#{} handed over WHOLE on our bond ({})", crate::fp(&self.contacts[ci].handle_proof), hex::encode(&fid.as_bytes()[..4]), era, if sent { "sent" } else { "queued" });
    }

    /// Ask our own siblings for an era of these chains newer than ours (the era_pull): a device behind its own fleet catches up from its siblings by chain-sync, never by re-keying anything with the friend. Once per held era.
    pub(super) fn pull_era_from_fleet(&mut self, fid: crate::types::FriendshipId) {
        let Some((token, held, held_tag)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| (c.conversation_token, c.era_index, c.era_tag())) else {
            return;
        };
        if self.era_pull_sent.get(&token) == Some(&held) {
            return;
        }
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        if let Ok(frame) = crate::network::fgtw::protocol::build_chain_pull_vsf(&token, kp.public.as_bytes(), kp.secret.as_bytes(), Some(held), held_tag) {
            self.era_pull_sent.insert(token, held);
            self.dispatch_frame_to_siblings(frame);
            crate::logf!("ERA: era_pull for {} — asking our own devices for an era newer than #{}", hex::encode(&fid.as_bytes()[..4]), held);
        }
    }

    /// A device that does not own the ceremony hands the owner what it saw (2026-11-13, Brittany's era split and her 22:10 lane: the desktop logged "does not own the ceremony" and only waited).
    /// The ask is an era_pull carrying the evidence, so every sibling also serves a different era it holds; the owner acts on it as on its own observation (`on_owner_ask`). Once per disagreement as we stand.
    pub(super) fn ask_owner(&mut self, fid: crate::types::FriendshipId, ask: crate::network::fgtw::protocol::OwnerAsk) {
        let Some((token, held, our_tag)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.conversation_token, c.era_index, c.era_tag()?))) else {
            return;
        };
        if !self.owner_asks.insert((fid, ask.friend_tag.unwrap_or(our_tag), our_tag)) {
            return;
        }
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        if let Ok(frame) = crate::network::fgtw::protocol::build_chain_pull_ask_vsf(&token, kp.public.as_bytes(), kp.secret.as_bytes(), Some(held), Some(our_tag), ask) {
            self.dispatch_frame_to_siblings(frame);
            match ask.friend_tag {
                Some(t) => crate::logf!("ERA: told our own devices the friend writes {} on era#{:08x}, which we do not hold — the ceremony owner heals it", hex::encode(&fid.as_bytes()[..4]), t),
                None => crate::logf!("ERA: told our own devices the friend's frames on {} decrypt to garbage on era#{:08x}, the era we hold — the ceremony owner acts", hex::encode(&fid.as_bytes()[..4]), our_tag),
            }
        }
    }

    /// A sibling that does not own the ceremony told us what it saw (`ask_owner`). The owner acts as on its own evidence: the friend's unknown era goes to the era-mismatch heal; garbage on the era we share re-delivers a fresh era (a conversation) or re-keys (a bond). A device that is not the owner either leaves it to the owner.
    pub(super) fn on_owner_ask(&mut self, token: [u8; 32], from: [u8; 32], held_tag: Option<u32>, ask: crate::network::fgtw::protocol::OwnerAsk) {
        let Some((fid, our_tag, bond, molecule)) = self.friendship_chains.iter().find(|(_, c)| c.conversation_token == token).and_then(|(id, c)| Some((*id, c.era_tag()?, c.bond, c.molecule))) else {
            return;
        };
        if molecule {
            return;
        }
        let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && (c.friendship_id == Some(fid) || self.bond_chains_id(c) == Some(fid))) else {
            return;
        };
        if super::ceremony_parked_by(&self.contacts[ci], self.device_keypair.as_ref().map(|k| *k.public.as_bytes()), &super::sibling_presence_snapshot(&self.contacts)) {
            crate::logf!("ERA: device {} reported {} about {} — not the ceremony owner here either; the owner acts", crate::fp(&from), if ask.fork { "garbage" } else { "an unknown era" }, hex::encode(&fid.as_bytes()[..4]));
            return;
        }
        if let Some(t) = ask.friend_tag {
            crate::logf!("ERA: device {} saw the friend write {} on era#{:08x} — healing as the ceremony owner", crate::fp(&from), hex::encode(&fid.as_bytes()[..4]), t);
            self.heal_era_mismatch(fid, t);
        }
        // Garbage is evidence only on the era both devices hold; a sibling on another era catches up by the era_pull this ask also is.
        if ask.fork && held_tag == Some(our_tag) && self.owner_asks.insert((fid, our_tag, our_tag)) {
            crate::logf!("ERA: device {} reports garbage decrypts from the friend on {} at the era we share — acting as the ceremony owner", crate::fp(&from), hex::encode(&fid.as_bytes()[..4]));
            if bond {
                self.initiate_friend_rekey(ci);
            } else {
                self.redeliver_pair_era(fid, "a sibling's garbage decrypts");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{era_delivery_verdict, minter_cuts_over, DeliveryFacts, DeliveryVerdict, MinterEvent};

    /// The desktop's side of 2026-11-11 23:28: it holds era 13 (4325596e), her phone delivers ITS era 13 (801078d0) whole.
    fn same_index(deliverer_lower: bool) -> DeliveryFacts {
        DeliveryFacts {
            our_era: 13,
            our_tag: 0x4325_596e,
            pending_tag: None,
            pending_stands: false,
            retired_tag: Some(0xbe81_6e33),
            delivered_era: 13,
            delivered_tag: Some(0x8010_78d0),
            whole: true,
            prior_tag: None,
            deliverer_lower,
        }
    }

    /// ONE INDEX, TWO ERAS: the lower party's era wins on both sides. The higher party installs the lower's; the lower party refuses the higher's and heals with its own. Never "ignored".
    #[test]
    fn a_same_index_conflict_settles_on_the_lower_party_on_both_sides() {
        // Her phone (higher) delivered to the desktop (lower): refused, and the desktop heals.
        assert_eq!(era_delivery_verdict(&same_index(false)), DeliveryVerdict::Refuse { heal: true });
        // The desktop (lower) delivering its 13 to her phone (higher), which holds her own 13: installed.
        let mut hers = same_index(true);
        hers.our_tag = 0x8010_78d0;
        hers.delivered_tag = Some(0x4325_596e);
        assert_eq!(era_delivery_verdict(&hers), DeliveryVerdict::Install);
        // A mint (not whole) at our very index cannot be derived here: refused, the lower party heals.
        let mut fresh = same_index(false);
        fresh.whole = false;
        fresh.delivered_tag = None;
        assert_eq!(era_delivery_verdict(&fresh), DeliveryVerdict::Refuse { heal: true });
        let mut fresh_from_lower = fresh;
        fresh_from_lower.deliverer_lower = true;
        assert_eq!(era_delivery_verdict(&fresh_from_lower), DeliveryVerdict::Refuse { heal: false }, "the lower deliverer heals on our refusal");
    }

    /// The ordinary cases keep working: the next step installs, a retransmit of an era we hold is accepted again, a stale index is refused, a lower party's pending stands.
    #[test]
    fn the_next_step_installs_and_the_rest_is_answered() {
        let mut f = same_index(false);
        f.delivered_era = 14;
        assert_eq!(era_delivery_verdict(&f), DeliveryVerdict::Install, "a whole era past ours");
        f.pending_tag = Some(0x1111_1111);
        f.pending_stands = true;
        assert_eq!(era_delivery_verdict(&f), DeliveryVerdict::Refuse { heal: false }, "a lower party's pending stands against a higher minter");
        let mut held = same_index(false);
        held.delivered_tag = Some(0x4325_596e);
        held.delivered_era = 15;
        assert_eq!(era_delivery_verdict(&held), DeliveryVerdict::Held, "the era we hold, relabelled higher by its sender");
        let mut retired = same_index(true);
        retired.delivered_tag = Some(0xbe81_6e33);
        assert_eq!(era_delivery_verdict(&retired), DeliveryVerdict::Refuse { heal: false }, "an era we retired");
        let mut stale = same_index(true);
        stale.delivered_era = 12;
        assert_eq!(era_delivery_verdict(&stale), DeliveryVerdict::Refuse { heal: false });
        // A fresh mint at our index + 1: installed when it names our era or no era (an older build), refused when it names another.
        let mut mint = same_index(false);
        mint.whole = false;
        mint.delivered_era = 14;
        mint.delivered_tag = Some(0x2222_2222);
        mint.prior_tag = Some(0x4325_596e);
        assert_eq!(era_delivery_verdict(&mint), DeliveryVerdict::Install);
        mint.prior_tag = None;
        assert_eq!(era_delivery_verdict(&mint), DeliveryVerdict::Install, "a build before prior_tag is taken as before");
        mint.prior_tag = Some(0xeab1_e92e);
        mint.delivered_tag = None;
        assert_eq!(era_delivery_verdict(&mint), DeliveryVerdict::Refuse { heal: true }, "a mint from her era 12, which we never held");
    }

    /// NO SWITCH ON A REFUSED DELIVERY (2026-11-11 23:28:15: her phone cut over on the desktop's ACK of a delivery it refused). The ACK never cuts over, a refusal never does, and an accept cuts over only to the very era still pending.
    #[test]
    fn the_minter_never_switches_on_an_ack_or_a_refusal() {
        let offered = 0x8010_78d0;
        assert!(!minter_cuts_over(MinterEvent::BondAck, offered, Some(offered)), "the bond row's ACK is transport only");
        assert!(!minter_cuts_over(MinterEvent::Answer { accepted: false }, offered, Some(offered)), "a refusal keeps the era we hold");
        assert!(minter_cuts_over(MinterEvent::Answer { accepted: true }, offered, Some(offered)));
        assert!(!minter_cuts_over(MinterEvent::Answer { accepted: true }, offered, Some(0x3f54_aba5)), "an accept of an era no longer pending here");
        assert!(!minter_cuts_over(MinterEvent::Answer { accepted: true }, offered, None));
    }
}
