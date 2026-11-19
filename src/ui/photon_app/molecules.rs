//! Group flows (docs/molecules.md §10): founding, the offer over a friendship, the explicit Join, the wrap that delivers an era, and roster record merges.
//! The receive half rides `commit_braid_rx`'s control-row ladder (a `MoleculeSignal` row defers here after the chains borrow ends, the era-signal discipline); the send half posts bare `MOLECULE_PREFIX` rows with roster blobs on the package's typed `gpl` field and sealed secrets on its typed wrap fields — the text is the row, and the row persists, so nothing secret ever rides the text.
//! Consent doctrine (§10 D8/D10): an OFFER carries a roster snapshot and no secret; the invitee's JOIN sends its member record + KEM bundle back; the sponsor vouches, posts the records into the group, and answers with a WRAP sealed to that bundle. The joiner never holds an era it is not in. Nothing here auto-joins anyone.

use super::PhotonApp;
use crate::storage::molecule::{MoleculeLocal, BondOffer, MoleculePhase};
use crate::types::molecule::{MoleculeId, MoleculeSignal, Roster};

/// A group-side post waiting for the fan-out send (step 4): a `Records` or `Wrap` row to put on our lane inside the group. Queued so the flows below stay pure of the send path until it speaks group.
#[derive(Clone, Debug)]
pub(super) struct MoleculePost {
    pub gid: MoleculeId,
    pub signal: MoleculeSignal,
    pub blob: Option<Vec<u8>>,
    pub wrap: Option<crate::network::message_package::BondWrapWire>,
    pub kem: Option<crate::crypto::era::EraKemWire>,
    /// A chat row (text, stamp, reference) whose bubble already landed; `None` = a control row minted at drain time.
    pub text: Option<(String, i64, Option<(crate::types::RefKind, i64)>)>,
    /// The tick the post was queued on — the frame fence (a post drains one tick after its bubble rendered).
    pub queued: u64,
    /// A control row that was held after its row landed (so a retry re-sends the SAME row, never mints a second).
    pub control: bool,
}

/// A GROUP frame exactly as its author's device signed it, kept so this device can hand it to a member who missed it (`grp_relay`).
pub(super) struct GroupFrame {
    pub gid: MoleculeId,
    /// The author's lane label (a member's sync record names its head per label).
    pub lane: [u8; 32],
    pub ts: i64,
    pub author_device: [u8; 32],
    /// The era tag the frame was sent under (`None` = untagged); only a member on that era can open it.
    pub era: Option<u32>,
    pub raw: Vec<u8>,
}

/// A MEMBER IS A PERSON, NOT A CONTACT (Nick 2026-10-26, "I add Frank, Frank adds Mike, who I do not know"): the standing member of a group that a signing device belongs to — a friend's contact first, then a member we never friended, by the devices their public fleet chain folds to (`MoleculePeer`).
/// Returns the member's party id and their contact when they are one. A free function over the fields, because the receive arms call it while the group's chains are borrowed.
pub(super) fn group_member_of(contacts: &[crate::types::Contact], peers: &[crate::types::molecule::MoleculePeer], rosters: &[(MoleculeId, Roster)], gid: &MoleculeId, device: &[u8; 32]) -> Option<([u8; 32], Option<usize>)> {
    let roster = &rosters.iter().find(|(g, _)| g == gid)?.1;
    if let Some(ci) = contacts.iter().position(|c| !c.is_sibling && !c.locked_out && c.knows_device(device) && roster.is_standing(&c.handle_hash)) {
        return Some((contacts[ci].handle_hash, Some(ci)));
    }
    peers.iter().find(|p| p.knows_device(device) && roster.is_standing(&p.party)).map(|p| (p.party, None))
}

/// ONE SENDER ATTRIBUTION, any size (docs/conversations.md Phase 2; Nick 2026-10-26: "we know who's in it"): the member of the conversation these chains key who signed with `device` — (their party, their contact when we friended them). A roster conversation resolves thru its standing members' folds ([`group_member_of`]); a pair, a bond or our own fleet's chains thru their two participants, the one that is not `us`. The caller still holds the signer to the member's trusted devices.
pub(super) fn member_by_device(contacts: &[crate::types::Contact], peers: &[crate::types::molecule::MoleculePeer], rosters: &[(MoleculeId, Roster)], chains: &crate::types::FriendshipChains, us: &[u8; 32], device: &[u8; 32]) -> Option<([u8; 32], Option<usize>)> {
    if chains.molecule {
        return group_member_of(contacts, peers, rosters, &MoleculeId(*chains.friendship_id.as_bytes()), device);
    }
    let party = *chains.other_participant(us)?;
    Some((party, contacts.iter().position(|c| c.handle_hash == party)))
}

/// Where an ACK for a frame goes, any size: the very device that signed it (its lane's pending lives there — per-device lanes), with every device of that member on the relay leg.
pub(super) fn ack_target(contacts: &[crate::types::Contact], peers: &[crate::types::molecule::MoleculePeer], party: &[u8; 32], ci: Option<usize>, signer: &[u8; 32]) -> ([u8; 32], Vec<[u8; 32]>) {
    let relay = match ci.and_then(|ci| contacts.get(ci)) {
        Some(c) => c.relay_device_list(),
        None => member_devices(contacts, peers, party),
    };
    (*signer, relay)
}

/// The devices a group member's photons and ACKs go to: a friend's relay list, or a never-friended member's folded devices.
pub(super) fn member_devices(contacts: &[crate::types::Contact], peers: &[crate::types::molecule::MoleculePeer], party: &[u8; 32]) -> Vec<[u8; 32]> {
    match contacts.iter().find(|c| !c.is_sibling && c.handle_hash == *party) {
        Some(c) => c.relay_device_list(),
        None => peers.iter().find(|p| p.party == *party).map(|p| p.devices.clone()).unwrap_or_default(),
    }
}

/// The handle proofs of the standing members whose devices we hold no fold for: not us, not a friend (a contact folds as a contact), and no MoleculePeer naming any device yet.
/// `named` is every standing member's proof across the groups we stand in; the answer is sorted and deduped, one fold per person however many groups name them.
pub(super) fn unfolded_member_proofs(named: &[[u8; 32]], us: Option<[u8; 32]>, friends: &[[u8; 32]], peers: &[crate::types::molecule::MoleculePeer]) -> Vec<[u8; 32]> {
    let mut out: Vec<[u8; 32]> = named
        .iter()
        .copied()
        .filter(|hp| Some(*hp) != us && !friends.contains(hp) && !peers.iter().any(|p| p.handle_proof == *hp && !p.devices.is_empty()))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// A mint's (nonce, fresh): derived from the era it is answered in for a join (`join_mint_secrets`, the same on every device of ours), random for every other mint.
pub(super) fn mint_secrets(join: bool, root: &[u8; 32], hk: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    if join {
        crate::crypto::era::join_mint_secrets(root, hk)
    } else {
        (rand::random(), rand::random())
    }
}

/// A group's next era from the one it is minted in, the one formula the minter and every member opening its wrap run: the transcript binds the group, the index and the nonce.
/// The wrap set is not in the transcript (a member is meant to verify the set against its own roster and withhold a short one — that check is not built yet, docs/conversations.md Phase 3).
pub(super) fn molecule_next_era(gid: &MoleculeId, next: u64, root: &[u8; 32], hk: &[u8; 32], nonce: &[u8; 32], fresh: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let transcript = crate::crypto::era::derive_era_transcript(&gid.token(), next, nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
    crate::crypto::era::derive_era_keys(&gid.0, next, root, Some(hk), fresh, &transcript)
}

/// Does the pending era we hold (`held` = its index and root) stand against an incoming one at `era` with `root`: the same index under the same or a lower root does, so every device keeps the lower of two mints of one index whatever order their wraps land in.
pub(super) fn pending_stands(held: Option<(u64, [u8; 32])>, era: u64, root: &[u8; 32]) -> bool {
    held.is_some_and(|(e, r)| e == era && r <= *root)
}

/// Was this Join already answered by one of our devices: the joiner stands, and every bundle it carries is held at that stamp or a newer one (the answering device's records carried them back to us).
/// A re-sent Join carries a fresh bundle, so it is never mistaken for one already answered.
pub(super) fn join_already_answered(roster: &Roster, joiner: &[u8; 32], bundles: &[crate::types::molecule::BundleRecord]) -> bool {
    roster.is_standing(joiner) && bundles.iter().all(|b| roster.bundles.get(&(b.party, b.device)).is_some_and(|held| held.signed_osc >= b.signed_osc))
}

/// The groups our fleet still owes `invitee` an offer for: invited, not standing yet, in a group we hold and have not left.
pub(super) fn invites_due(locals: &[(MoleculeId, MoleculeLocal)], rosters: &[(MoleculeId, Roster)], invitee: &[u8; 32]) -> Vec<MoleculeId> {
    locals
        .iter()
        .filter(|(gid, l)| l.phase != MoleculePhase::Left && l.offered.iter().any(|(p, _)| p == invitee) && rosters.iter().any(|(g, r)| g == gid && !r.is_standing(invitee)))
        .map(|(gid, _)| *gid)
        .collect()
}

/// One invite's state as the page row our devices hand each other: an OUTGOING `Offer` control row stamped with the offer, `deleted` once it has ended, and a roster naming the group and holding no records in `gpl` (a complete VSF document, verified on read).
pub(super) fn invite_row(gid: &MoleculeId, osc: i64, ended: bool) -> Option<crate::network::history_pages::HistoryRow> {
    let blob = crate::storage::molecule::roster_to_vsf_bytes(gid, &Roster::default()).ok()?;
    let mut msg = crate::types::ChatMessage::control(crate::types::RowControl::Molecule(MoleculeSignal::Offer), true, osc);
    msg.deleted = ended;
    let mut row = crate::network::history_pages::HistoryRow::from_message(&msg);
    row.gpl = blob;
    Some(row)
}

/// The invite a sibling's page row carries, when it is one: (group, offer stamp, ended). An incoming Offer row is a received invitation, never ours.
pub(super) fn invite_of_row(row: &crate::network::history_pages::HistoryRow) -> Option<(MoleculeId, i64, bool)> {
    if !row.sender_outgoing || row.gpl.is_empty() || !matches!(row.control, Some(crate::types::RowControl::Molecule(MoleculeSignal::Offer))) {
        return None;
    }
    let (gid, _) = crate::storage::molecule::roster_from_vsf_bytes(&row.gpl).ok()?;
    Some((gid, row.timestamp, row.deleted))
}

/// Frames kept per group for relaying: the newest win.
const GROUP_FRAMES_KEPT: usize = 256;
/// At most this many frames handed to one member device per sync record (oldest first; the next record carries on).
const GROUP_RELAY_BURST: usize = 16;

impl PhotonApp {
    /// The name a member's own record grants them in a group — what the author line, the ring and the wave screen call someone we never friended.
    pub(super) fn member_granted_name(&self, gid: &MoleculeId, party: &[u8; 32]) -> String {
        self.molecule_rosters.iter().find(|(g, _)| g == gid).and_then(|(_, r)| r.members.get(party)).map(|m| m.name.clone()).unwrap_or_default()
    }

    /// Keep a group frame for relaying (once per author device and stamp).
    pub(super) fn store_group_frame(&mut self, frame: GroupFrame) {
        if frame.raw.is_empty() || self.molecule_frames.iter().any(|f| f.gid == frame.gid && f.ts == frame.ts && f.author_device == frame.author_device) {
            return;
        }
        let gid = frame.gid;
        self.molecule_frames.push(frame);
        let held = self.molecule_frames.iter().filter(|f| f.gid == gid).count();
        if held > GROUP_FRAMES_KEPT {
            if let Some(oldest) = self.molecule_frames.iter().enumerate().filter(|(_, f)| f.gid == gid).min_by_key(|(_, f)| f.ts).map(|(i, _)| i) {
                self.molecule_frames.remove(oldest);
            }
        }
    }

    /// MEMBER RELAY (Nick 2026-10-25: "if steve sends and only mike is online, then steve goes offline and jan comes online, jan should get steve's messages"): a member device `peer` just advertised its lane heads for group `gid`.
    /// Every frame we hold that it is behind on, from an author whose own devices are away, goes to it byte for byte inside a `grp_relay`, so it reaches every member by whoever is online, hop by hop. Only frames on the era that device is on (it opens nothing else), at most three tries per frame per device.
    pub(super) fn relay_group_frames(&mut self, gid: MoleculeId, peer: [u8; 32], peer_era: u32, heads: &[([u8; 32], i64)]) {
        // Photon on the alias alone holds the group's keys for reading only: a member's device takes no frame from it yet.
        if self.alias_alone() {
            return;
        }
        if self.device_keypair.as_ref().is_some_and(|k| *k.public.as_bytes() == peer) || self.contacts.iter().any(|c| c.is_sibling && c.knows_device(&peer)) {
            return;
        }
        let Some(roster) = self.molecule_rosters.iter().find(|(g, _)| *g == gid).map(|(_, r)| r) else {
            return;
        };
        // The device must belong to a standing member: a friend's device thru the contact fold, a never-friended member's thru its folded devices.
        let route = match self.contacts.iter().find(|c| !c.is_sibling && c.knows_device(&peer) && roster.is_standing(&c.handle_hash)) {
            Some(c) => {
                let relay_to = if c.relay_device_list().contains(&peer) { vec![peer] } else { Vec::new() };
                match (c.device_key() == Some(peer)).then(|| c.race_addrs()).flatten() {
                    Some((a, b)) if !c.direct_parked() => (a, b, relay_to),
                    _ => (crate::network::status::RELAY_ADDR, None, vec![peer]),
                }
            }
            None if self.molecule_peers.iter().any(|p| p.devices.contains(&peer) && roster.is_standing(&p.party)) => (crate::network::status::RELAY_ADDR, None, vec![peer]),
            None => return,
        };
        let head_of = |lane: &[u8; 32]| heads.iter().find(|(l, _)| l == lane).map_or(0, |(_, t)| *t);
        let author_here = |dev: &[u8; 32]| self.contacts.iter().any(|c| c.knows_device(dev) && c.is_online);
        let mut owed: Vec<(i64, usize)> = self
            .molecule_frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.gid == gid && f.author_device != peer && f.ts > head_of(&f.lane))
            .filter(|(_, f)| peer_era == 0 || f.era.is_none_or(|t| t == peer_era))
            .filter(|(_, f)| !author_here(&f.author_device))
            .filter(|(_, f)| self.molecule_relay_tries.get(&(peer, f.ts)).copied().unwrap_or(0) < 3)
            .map(|(i, f)| (f.ts, i))
            .collect();
        // Frames it has caught up on stop being tracked.
        self.molecule_relay_tries.retain(|(d, ts), _| *d != peer || self.molecule_frames.iter().any(|f| f.gid == gid && f.ts == *ts && f.ts > head_of(&f.lane)));
        if owed.is_empty() {
            return;
        }
        owed.sort_unstable();
        owed.truncate(GROUP_RELAY_BURST);
        let (Some(kp), Some(checker)) = (self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let token = gid.token();
        let dispatch = checker.history_dispatch();
        let mut sent = 0usize;
        for (ts, i) in &owed {
            let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_grp_relay_vsf(&token, &self.molecule_frames[*i].raw, kp.public.as_bytes(), kp.secret.as_bytes()) else {
                continue;
            };
            crate::log_err!(dispatch.send(crate::network::status::HistorySendRequest { peer_addr: route.0, alt_addr: route.1, recipient_pubkey: peer, vsf_bytes, relay_to: route.2.clone(), tag: None }), "queue a group relay send");
            *self.molecule_relay_tries.entry((peer, *ts)).or_insert(0) += 1;
            sent += 1;
        }
        crate::logf!("RELAY: handed {} frame(s) of {} to member device {} — their authors are away", sent, hex::encode(&gid.0[..4]), crate::fp(&peer));
    }

    /// A `MoleculeSignal` control row landed (deferred from commit_braid_rx): `ci` = the sender's contact, `cp` = the conversation the row landed in, `wire` = the package's typed group extras.
    /// `ci` is the sender's contact — `None` for a member we never friended (docs/group-waves.md, "a member is a person, not a contact"): inside the group a wrap or records row needs no contact; the rows that ride a friendship always have one.
    pub(super) fn on_molecule_signal(&mut self, ci: Option<usize>, cp: usize, sig: MoleculeSignal, wire: Option<crate::network::message_package::MoleculeWire>, kem: Option<crate::crypto::era::EraKemWire>, ts: i64) {
        let blob = wire.as_ref().and_then(|g| g.blob.clone());
        let in_group = self.conversations.get(cp).map_or(false, |c| c.is_molecule());
        match (sig, in_group, ci) {
            (MoleculeSignal::Offer, false, Some(ci)) => self.park_bond_offer(ci, blob, ts),
            (MoleculeSignal::Join, false, Some(ci)) => self.answer_bind(ci, blob),
            (MoleculeSignal::Wrap, false, Some(ci)) => self.install_bind_wrap(ci, wire, kem),
            (MoleculeSignal::Wrap, true, _) => self.install_molecule_wrap(cp, wire, kem),
            (MoleculeSignal::Records, true, _) => self.merge_molecule_records(ci, cp, blob),
            (MoleculeSignal::Decline, false, Some(ci)) => self.on_decline(ci, blob),
            (MoleculeSignal::Deliver, false, Some(ci)) => self.on_era_delivery(ci, wire),
            (other, _, _) => {
                let sender = ci.and_then(|ci| self.contacts.get(ci)).map(|c| crate::fp(&c.handle_proof)).unwrap_or_else(|| "a member".into());
                crate::logf!("MOLECULE: {} row from {} landed {} a group conversation — ignored", format!("{other:?}"), sender, if in_group { "inside" } else { "outside" });
            }
        }
    }

    /// OFFER (§10.1 None → Offered): park the sponsor's snapshot beside the friendship row it rode in on. No secret arrives; the Join pill on that row is the consent. A re-offer for the same group replaces the parked snapshot (the sponsor's refresh); an offer for a group we already hold merges the snapshot as records.
    fn park_bond_offer(&mut self, ci: usize, blob: Option<Vec<u8>>, row_osc: i64) {
        self.park_bond_offer_from(ci, blob, row_osc, false);
    }

    /// An invitation one of OUR devices received from `ci`, handed over in a sibling page: parked here the same way, silently (the device that received it already rang), and never handed on again.
    pub(super) fn park_fleet_offer(&mut self, ci: usize, blob: Vec<u8>, row_osc: i64) {
        if self.contacts.get(ci).is_some_and(|c| c.is_sibling) {
            return;
        }
        self.park_bond_offer_from(ci, Some(blob), row_osc, true);
    }

    fn park_bond_offer_from(&mut self, ci: usize, blob: Option<Vec<u8>>, row_osc: i64, from_fleet: bool) {
        let sponsor_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let Some(sponsor) = self.contacts.get(ci).map(|c| c.handle_hash) else {
            return;
        };
        let Some(blob) = blob else {
            crate::logf!("MOLECULE: offer from {} without a roster snapshot — refused", sponsor_fp);
            return;
        };
        let (gid, snapshot) = match crate::storage::molecule::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("MOLECULE: offer snapshot from {} failed verified read: {} — refused", sponsor_fp, e);
                return;
            }
        };
        let Some(title) = Self::verify_snapshot_birth(&gid, &snapshot) else {
            crate::logf!("MOLECULE: offer from {} for {} carries no verifiable genesis — refused", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        };
        if !snapshot.is_standing(&sponsor) {
            crate::logf!("MOLECULE: offer from {} for {} — the sponsor is not standing in its own snapshot; refused", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        }
        if snapshot.genesis.as_ref().is_some_and(|g| !g.is_derived() && self.record_signer_verdict(&g.founder, &g.signer_device) == Some(false)) {
            crate::logf!("MOLECULE: offer from {} for {} — its genesis is signed outside the founder's fold; refused", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        }
        // Fold every never-friended member now, while the offer waits for its Join, so the snapshot verifies whole the moment it is merged.
        self.fetch_member_folds(&snapshot);
        if let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) {
            let mut changed = self.merge_snapshot_into(pos, snapshot, false);
            changed |= self.sync_molecule_conversation(&gid);
            if changed {
                self.persist_molecule(&gid);
            }
            crate::logf!("MOLECULE: offer for held group {} from {} merged as records", hex::encode(&gid.0[..4]), sponsor_fp);
            // NOT YET HOLDING THE KEYS: a re-offer is the sponsor saying it never saw our Join (2026-11-17, Jon: joined, saw the members, never any content, while the sponsor re-offered on every return). The Join rides the bond once, and one lost on the way (the sponsor's device asleep, no mailbox) left both sides waiting forever: send it again (bind_offer sends while Joining, a fresh bundle the sponsor wraps to).
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let keys_held = self.friendship_chains.iter().any(|(id, c)| *id == fid && !c.awaiting_root());
            let we_left = self.molecule_locals.iter().any(|(g, l)| *g == gid && l.phase == MoleculePhase::Left);
            if !keys_held && !we_left {
                if !self.bond_offers.iter().any(|o| o.molecule_id == gid) {
                    if let Some((_, roster)) = self.molecule_rosters.iter().find(|(g, _)| *g == gid) {
                        self.bond_offers.push(BondOffer { molecule_id: gid, sponsor, row_osc, snapshot: roster.clone(), accepted: true });
                    }
                }
                crate::logf!("MOLECULE: re-offer for {} from {} and its keys are not held here; our Join went missing, sending it again", hex::encode(&gid.0[..4]), sponsor_fp);
                self.bind_offer(gid);
            }
            return;
        }
        let members = snapshot.standing().len();
        let offer = BondOffer { molecule_id: gid, sponsor, row_osc, snapshot, accepted: false };
        // The newest offer row stands: a copy from another of the sponsor's devices, or a sibling's hand-over of one we already parked, changes nothing and never rings twice.
        let Some(fresh) = crate::storage::molecule::park_offer_into(&mut self.bond_offers, offer.clone()) else {
            crate::logf!("MOLECULE: offer for {} from {} — a newer copy is parked already", hex::encode(&gid.0[..4]), sponsor_fp);
            return;
        };
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::molecule::park_bond_offer(&offer, storage) {
                crate::logf!("MOLECULE: offer park failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
        }
        crate::logf!("MOLECULE: offered \"{}\" ({}) by {} — {} standing — Join is the consent", title, hex::encode(&gid.0[..4]), sponsor_fp, members);
        if fresh && !from_fleet {
            self.alert_invitation(ci, gid, row_osc, &title);
            self.share_offer_with_fleet(ci, &blob, row_osc);
        }
        self.scene_dirty = true;
    }

    /// Hand an invitation to our own devices as a page row carrying its snapshot (`gpl`), under the bond's token: whichever of our devices the sponsor reached, every one of them parks it.
    fn share_offer_with_fleet(&self, ci: usize, blob: &[u8], row_osc: i64) {
        let Some(sponsor) = self.contacts.get(ci).map(|c| c.handle_hash) else {
            return;
        };
        let Some(token) = self.bond_fid_of(ci).and_then(|b| self.friendship_chains.iter().find(|(id, _)| *id == b)).map(|(_, c)| c.conversation_token) else {
            return;
        };
        let mut msg = crate::types::ChatMessage::control(crate::types::RowControl::Molecule(MoleculeSignal::Offer), false, row_osc);
        msg.author = Some(sponsor);
        let mut row = crate::network::history_pages::HistoryRow::from_message(&msg);
        row.gpl = blob.to_vec();
        self.push_history_rows_to_siblings(token, "an invitation", vec![row], None);
    }

    /// An invitation is something a person sent us: it rings and alerts like a photon from them (Nick 2026-10-24: "it didn't ding for the recipients"), with the sponsor's own chirp and their name as the title.
    fn alert_invitation(&mut self, ci: usize, gid: MoleculeId, row_osc: i64, title: &str) {
        let Some(c) = self.contacts.get(ci) else {
            return;
        };
        let sender = c.display_name();
        let named = if title.is_empty() { crate::ui::lang::tr(crate::ui::lang::Msg::ConversationTitle).into_owned() } else { title.to_string() };
        let body = crate::ui::lang::tr(crate::ui::lang::Msg::InvitedYouTo { title: &named }).into_owned();
        let mut key = gid.0.to_vec();
        key.extend_from_slice(&row_osc.to_le_bytes());
        let alert_hp = *blake3::hash(&key).as_bytes();
        let us = self.our_party_id(c).unwrap_or([0u8; 32]);
        let digest = super::relationship_digest(&c.handle_hash, &us);
        crate::logf!("NOTIFY: invitation to {} from {}", hex::encode(&gid.0[..4]), crate::fp(&c.handle_proof));
        #[cfg(target_os = "android")]
        crate::platform::jni_android::notify_new_message(&alert_hp, &digest, &sender, &body, None);
        #[cfg(not(any(target_os = "android", target_os = "ferros")))]
        {
            crate::platform::desktop_notify::notify_new_message(&alert_hp, &sender, &body, None);
            if crate::platform::notify_prefs::chime() {
                std::thread::spawn(move || {
                    chirp::Chirp::from_hash(digest).play_blocking_at(crate::wave::chirp_gain()).unwrap_or_else(|e| crate::logf!("CHIME: {}", e));
                });
            }
        }
    }

    /// The genesis must be present, name this group, and verify under its own signer before anything persists. Returns the title suggestion.
    fn verify_snapshot_birth(gid: &MoleculeId, snapshot: &Roster) -> Option<String> {
        let g = snapshot.genesis.as_ref()?;
        let born = if g.is_derived() { g.self_certifies() } else { crate::types::molecule::verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) };
        if g.molecule_id != *gid || !born {
            return None;
        }
        Some(snapshot.title())
    }

    /// JOIN (§10.1 Offered → Joining, the invitee's act): mint this device's KEM bundle, keep the decapsulation half in a chains blob that holds NO root yet, send our member record + bundle back over the friendship, and wait for the sponsor's wrap. Returns whether the Join went out.
    pub(super) fn bind_offer(&mut self, gid: MoleculeId) -> bool {
        let Some(offer) = self.bond_offers.iter().find(|o| o.molecule_id == gid).cloned() else {
            crate::logf!("MOLECULE: join of {} — no parked offer", hex::encode(&gid.0[..4]));
            return false;
        };
        // A sibling already bound and its keys reached us: joining again would replace them with a placeholder and make the sponsor mint the group a needless era.
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        if self.friendship_chains.iter().any(|(id, c)| *id == fid && !c.awaiting_root()) {
            crate::logf!("MOLECULE: join of {} — our fleet already holds its keys; nothing to send", hex::encode(&gid.0[..4]));
            self.fleet_holds_molecule(gid);
            return false;
        }
        // Joined = the group's keys are held, which the check above already answers. A held roster or an accepted offer is not joined: the roster arrives with the offer's snapshot, and a Join whose wrap never came (2026-11-17, Jon: "Neither one of his devices is letting him bind") was refused here as "already joined" forever, so a tap must send it again.
        let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == offer.sponsor && !c.is_sibling) else {
            crate::logf!("MOLECULE: join of {} — the sponsor is no longer a contact; offer expired", hex::encode(&gid.0[..4]));
            return false;
        };
        let (Some(our_pid), Some(seed), Some(hp)) = (self.our_party_id(&self.contacts[ci]), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) else {
            return false;
        };
        let (name, avatar) = self.own_name_grant();
        // The bundle's published_era is informational (the minter always wraps to a device's newest bundle); an invitee knows no era yet.
        let era = 0u64;
        let eph = crate::crypto::era::era_keygen(era, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let bundle = crate::types::molecule::bundle_record(our_pid, era, &eph, &seed);
        let member = crate::types::molecule::join_record(our_pid, hp, &name, avatar, offer.sponsor, &seed);
        // A rootless chains blob holds the decapsulation keys until the wrap arrives; from_molecule_root with the wrap's secrets replaces it, carrying the kems across.
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        // A Join sent again (a re-offer, from any of the sponsor's devices) keeps the last Join's keys beside the new ones: either Join may be the one answered, and its wrap must still open (two of the sponsor's devices re-offering on one edge sent two Joins, and the second replaced the holder the first answer was sealed to).
        // Only a rootless holder can be here: one holding the group's keys returned above.
        if let Some((_, held)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            held.push_molecule_kem(eph.export_decaps(era));
        } else {
            let mut holder = crate::types::FriendshipChains::from_molecule_root(gid, &offer.snapshot.standing(), [0u8; 32], [0u8; 32], era, offer.snapshot.genesis.as_ref().map_or([0u8; 32], |g| g.molecule_id.0));
            holder.push_molecule_kem(eph.export_decaps(era));
            self.friendship_chains.push((fid, holder));
        }
        let mut ours = Roster::default();
        ours.merge_member(member);
        ours.merge_bundle(bundle);
        let blob = match crate::storage::molecule::roster_to_vsf_bytes(&gid, &ours) {
            Ok(b) => b,
            Err(e) => {
                crate::logf!("MOLECULE: join records for {} failed to encode: {}", hex::encode(&gid.0[..4]), e);
                return false;
            }
        };
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: Some(blob), wrap: None, deliver: None, answer: None };
        let ts = vsf::eagle_time_oscillations();
        let sent = self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(MoleculeSignal::Join));
        if sent {
            self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Joining);
            // THE JOINING ROW (docs/molecules.md §10.5, "dashed while Joining"; Emma 2026-10-24: "once bonded, doesn't show up on contact screen"): the group joins our list now, from the offer's snapshot.
            // It used to appear only when the wrap arrived, so a join whose answer was slow (the sponsor's only holding device asleep) left nothing on screen; install_bind_wrap replaces the snapshot and makes it Standing.
            if !self.conversations.iter().any(|v| v.id() == fid) {
                self.conversations.push(crate::types::Conversation::new_molecule(gid, offer.snapshot.standing()));
            }
            if !self.molecule_rosters.iter().any(|(g, _)| *g == gid) {
                // The offer's snapshot enters thru the fold-checked merge like any records: what verifies now is held, what waits on a fold is parked.
                self.molecule_rosters.push((gid, Roster::default()));
                let pos = self.molecule_rosters.len() - 1;
                self.merge_snapshot_into(pos, offer.snapshot.clone(), false);
                self.sync_molecule_conversation(&gid);
            }
            self.persist_molecule(&gid);
            self.persist_chains_of(&fid);
            if let Some(o) = self.bond_offers.iter_mut().find(|o| o.molecule_id == gid) {
                o.accepted = true;
                if let Some(storage) = self.storage.as_ref() {
                    crate::log_err!(crate::storage::molecule::save_bond_offer(o, storage), "save an accepted bond offer");
                }
            }
        }
        crate::logf!("MOLECULE: join of {} {} to {} — waiting for the wrap", hex::encode(&gid.0[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" }, crate::fp(&self.contacts[ci].handle_proof));
        sent
    }

    /// JOIN (the sponsor's side): verify the invitee's records, vouch for them, post everything into the group, and answer with a wrap of the current era (from-genesis) or of a freshly minted next era (from-join, D5 — the joiner never reads a row from before it stood).
    fn answer_bind(&mut self, ci: usize, blob: Option<Vec<u8>>) {
        let joiner_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        // Photon on the alias alone speaks to no friend yet (transmit_core refuses the wrap), so it vouches and posts nothing either: its fleet siblings answer.
        if self.alias_alone() {
            crate::logf!("MOLECULE: join from {} — Photon on the alias alone answers no Join; a fleet sibling does", joiner_fp);
            return;
        }
        let Some(joiner) = self.contacts.get(ci).map(|c| c.handle_hash) else {
            return;
        };
        let Some(blob) = blob else {
            crate::logf!("MOLECULE: join from {} without records — ignored", joiner_fp);
            return;
        };
        let (gid, records) = match crate::storage::molecule::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("MOLECULE: join records from {} failed verified read: {} — ignored", joiner_fp, e);
                return;
            }
        };
        let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) else {
            crate::logf!("MOLECULE: join from {} for {} — we do not hold that group; ignored", joiner_fp, hex::encode(&gid.0[..4]));
            return;
        };
        let Some(member) = records.members.get(&joiner).cloned() else {
            crate::logf!("MOLECULE: join from {} carries no member record for the sender — ignored", joiner_fp);
            return;
        };
        if !self.contacts[ci].knows_device(&member.signer_device) || !crate::types::molecule::verify_record(&member.signing_bytes(), &member.signature, &member.signer_device) {
            crate::logf!("MOLECULE: join from {} — member record not signed by one of their folded devices; ignored", joiner_fp);
            return;
        }
        let bundles: Vec<crate::types::molecule::BundleRecord> = records
            .bundles
            .values()
            .filter(|b| b.party == joiner && self.contacts[ci].knows_device(&b.device) && crate::types::molecule::verify_record(&b.signing_bytes(), &b.signature, &b.device))
            .cloned()
            .collect();
        if bundles.is_empty() {
            crate::logf!("MOLECULE: join from {} carries no verifiable bundle — ignored", joiner_fp);
            return;
        }
        let (Some(our_pid), Some(seed)) = (self.our_party_id(&self.contacts[ci]), self.device_seed()) else {
            return;
        };
        if !self.molecule_rosters[pos].1.is_standing(&our_pid) {
            crate::logf!("MOLECULE: join from {} for {} — we are not standing; cannot vouch", joiner_fp, hex::encode(&gid.0[..4]));
            return;
        }
        // ANY DEVICE OF OUR FLEET ANSWERS (Nick 2026-11-18): the Join reaches only the device the joiner's route names, so the old one-device-acts gate dropped it whenever that device was not the one that acts, and nobody answered (Jon, 2026-11-18, waiting on the MacBook's re-offer).
        // Two answers no longer compete: a from-join mint derives its era from the era it is answered in (join_mint_secrets), so every answer in era N hands out the one same N+1.
        // A Join one of our devices already answered (its bundles came back to us in that device's records, the joiner standing) is not answered again: the joiner holds that answer, and a lost one comes back as a fresh Join with a fresh bundle.
        if join_already_answered(&self.molecule_rosters[pos].1, &joiner, &bundles) {
            crate::logf!("MOLECULE: join from {} for {} — one of our devices already answered it", joiner_fp, hex::encode(&gid.0[..4]));
            return;
        }
        // ANOTHER DEVICE OF A MEMBER already standing: the party has read every era already, so it takes the current one whole like a from-genesis join — a from-join mint here only forked the era away from that member's other devices.
        let already_standing = self.molecule_rosters[pos].1.is_standing(&joiner);
        let vouch = crate::types::molecule::vouch_record(our_pid, joiner, false, &seed);
        let mut posted = Roster::default();
        posted.merge_member(member.clone());
        for b in &bundles {
            posted.merge_bundle(b.clone());
        }
        posted.merge_vouch(vouch.clone());
        {
            let roster = &mut self.molecule_rosters[pos].1;
            roster.merge_member(member);
            for b in &bundles {
                roster.merge_bundle(b.clone());
            }
            roster.merge_vouch(vouch);
        }
        self.sync_molecule_conversation(&gid);
        self.persist_molecule(&gid);
        let from_genesis = self.molecule_rosters[pos].1.genesis.as_ref().map_or(false, |g| g.history_from_genesis);
        // THE BOND RULE (docs/conversations.md §7): an era at or below the conversation's bond_split_era derived a bond, so it is never handed out — a join into such a conversation mints first, whatever its history policy.
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let split_bars = self.friendship_chains.iter().find(|(id, _)| *id == fid).is_some_and(|(_, c)| c.bond_split_era.is_some_and(|s| c.era_index <= s));
        if split_bars {
            crate::logf!("MOLECULE: {} current era derived a bond — the join mints a fresh era first", hex::encode(&gid.0[..4]));
        }
        // The records go into the group for everyone; the joiner is standing from the merge above, so the fan-out reaches it too once it holds the era.
        self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: Some(crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok().unwrap_or_default()), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        if (from_genesis || already_standing) && !split_bars {
            // Re-wrap the CURRENT era whole to each of the joiner's devices, over the friendship.
            let Some((era, lineage, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, *c.lane_root()?, *c.history_key()?))) else {
                crate::logf!("MOLECULE: {} holds no root here — cannot answer the join", hex::encode(&gid.0[..4]));
                return;
            };
            let mut payload = Vec::with_capacity(64);
            payload.extend_from_slice(&root);
            payload.extend_from_slice(&hk);
            for b in &bundles {
                self.send_wrap_over_friendship(gid, ci, our_pid, b, era, lineage, &payload);
            }
            use zeroize::Zeroize;
            payload.zeroize();
        } else {
            self.mint_molecule_era(gid, Some((ci, bundles)));
        }
    }

    /// Seal `payload` to one bundle and send it as a WRAP row over the friendship with `ci`.
    fn send_wrap_over_friendship(&mut self, gid: MoleculeId, ci: usize, our_pid: [u8; 32], bundle: &crate::types::molecule::BundleRecord, era: u64, lineage: [u8; 32], payload: &[u8]) -> bool {
        let nonce: [u8; 32] = rand::random();
        let Some((cts, sealed)) = crate::crypto::era::wrap_to_bundle(&bundle.pubkeys(), bundle.kem_set, &bundle.device, era, &nonce, payload) else {
            crate::logf!("MOLECULE: wrap to {} failed to seal", hex::encode(&bundle.device[..4]));
            return false;
        };
        let wrap = crate::network::message_package::BondWrapWire { recipient_device: bundle.device, bundle_id: bundle.pubkeys().bundle_id(), era, era_lineage: lineage, nonce, sealed };
        // The join answer carries the group's WHOLE current roster: the invitation's snapshot may predate other joins, and the records that named them rode an era the joiner never held (Nick 2026-10-25: his rows went to Emma alone, Brittany unknown to him).
        let roster = self.molecule_rosters.iter().find(|(g, _)| *g == gid).and_then(|(_, r)| crate::storage::molecule::roster_to_vsf_bytes(&gid, r).ok());
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: roster, wrap: Some(wrap), deliver: None, answer: None };
        let ts = vsf::eagle_time_oscillations();
        let sent = self.bond_transmit_with(ci, ts, Some(&cts), Some(&wire), &crate::types::RowControl::Molecule(MoleculeSignal::Wrap));
        crate::logf!("MOLECULE: wrap (era {}) for device {} {} over the friendship", era, hex::encode(&bundle.device[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" });
        sent
    }

    /// MINT (§10.4): a fresh secret, the next era derived from it, one WRAP per standing device posted inside the group on the OLD era — and, for a join under from-join, a wrap over the friendship to each of the joiner's devices. The minter installs the era as pending; the cutover edge is its first ACK of any wrap row (step 9 wires the edge and the triggers beyond a join).
    pub(super) fn mint_molecule_era(&mut self, gid: MoleculeId, joiner: Option<(usize, Vec<crate::types::molecule::BundleRecord>)>) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        // The joiner holds only the era this mint hands it: our rows reach it once we are on that era too.
        if let Some(party) = joiner.as_ref().and_then(|(ci, _)| self.contacts.get(*ci)).map(|c| c.handle_hash) {
            if !self.molecule_era_joiners.contains(&(gid, party)) {
                self.molecule_era_joiners.push((gid, party));
            }
        }
        // A MINT IS ALREADY IN FLIGHT (its era is pending until the first member's frame on it): a second mint from the same current era would overwrite it and split the group (Brittany 2026-10-25: two joins 12 s apart minted era 1 twice — Nick held the first, Emma and Brittany the second).
        // A joiner takes the pending era whole — it reads nothing from before it, the from-join promise kept; any other mint waits for the cutover.
        let pending = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.pending_era().map(|p| (p.era_index, p.lane_root, p.history_key, c.era_lineage)));
        if let Some((p_era, p_root, p_hk, lineage)) = pending {
            match (joiner, p_hk) {
                (Some((ci, bundles)), Some(p_hk)) => {
                    let Some(our_pid) = self.own_party_id() else {
                        return;
                    };
                    let mut whole = Vec::with_capacity(64);
                    whole.extend_from_slice(&p_root);
                    whole.extend_from_slice(&p_hk);
                    for b in &bundles {
                        self.send_wrap_over_friendship(gid, ci, our_pid, b, p_era, lineage, &whole);
                    }
                    use zeroize::Zeroize;
                    whole.zeroize();
                    crate::logf!("MOLECULE: {} has era {} pending — the joiner takes it whole, no second mint", hex::encode(&gid.0[..4]), p_era);
                }
                _ => {
                    self.molecule_mint_after_cutover.insert(gid);
                    crate::logf!("MOLECULE: {} has era {} pending — the next mint waits for its cutover", hex::encode(&gid.0[..4]), p_era);
                }
            }
            return;
        }
        let Some((era, lineage, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, *c.lane_root()?, *c.history_key()?))) else {
            crate::logf!("MOLECULE: {} holds no root here — cannot mint", hex::encode(&gid.0[..4]));
            return;
        };
        let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) else {
            return;
        };
        let Some(our_pid) = self.own_party_id() else {
            return;
        };
        let our_device = self.device_keypair.as_ref().map(|k| *k.public.as_bytes());
        let next = era + 1;
        // A join mint is derived from the era it is answered in, so every device of ours answering any Join in this era mints the same next era; every other mint stays random.
        let (nonce, mut fresh) = mint_secrets(joiner.is_some(), &root, &hk);
        let (next_root, next_hk) = molecule_next_era(&gid, next, &root, &hk, &nonce, &fresh);
        // The wrap set is the standing bundles whose device is still in its owner's fold: a device removed from a member's fleet, or a bundle planted under a party by a device that was never theirs, receives no era from here on.
        // A bundle in the roster whose owner we hold no fold for was merged before records were fold-checked (new ones park until the fold lands), so it still receives rather than stranding that member.
        let targets: Vec<crate::types::molecule::BundleRecord> = self.molecule_rosters[pos]
            .1
            .standing_bundles()
            .into_iter()
            .filter(|b| {
                let keep = self.record_signer_verdict(&b.party, &b.device) != Some(false);
                if !keep {
                    crate::logf!("MOLECULE: mint {} — bundle for {} device {} is outside that member's fold; not wrapped", hex::encode(&gid.0[..4]), crate::fp(&b.party), hex::encode(&b.device[..4]));
                }
                keep
            })
            .cloned()
            .collect();
        let mut inside = 0usize;
        // The joiner never held the era these inside wraps ride on, so a wrap to it there is unreadable — it takes the new era whole over the bond below.
        let joiner_party = joiner.as_ref().and_then(|(ci, _)| self.contacts.get(*ci)).map(|c| c.handle_hash);
        for b in &targets {
            if Some(b.device) == our_device || Some(b.party) == joiner_party {
                continue;
            }
            let Some((cts, sealed)) = crate::crypto::era::wrap_to_bundle(&b.pubkeys(), b.kem_set, &b.device, next, &nonce, &fresh) else {
                crate::logf!("MOLECULE: mint {} era {} — wrap to {} failed to seal", hex::encode(&gid.0[..4]), next, hex::encode(&b.device[..4]));
                continue;
            };
            let wrap = crate::network::message_package::BondWrapWire { recipient_device: b.device, bundle_id: b.pubkeys().bundle_id(), era: next, era_lineage: lineage, nonce, sealed };
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Wrap, blob: None, wrap: Some(wrap), kem: Some(cts), text: None, queued: self.tick_serial, control: false });
            inside += 1;
        }
        let joiner_wraps = joiner.as_ref().map_or(0, |(_, b)| b.len());
        if let Some((ci, bundles)) = joiner {
            // The joiner never held era N, so it cannot derive N+1 from fresh: it receives the next era WHOLE (root ‖ history key), the same shape a from-genesis join answer carries.
            let mut whole = Vec::with_capacity(64);
            whole.extend_from_slice(&next_root);
            whole.extend_from_slice(&next_hk);
            for b in &bundles {
                self.send_wrap_over_friendship(gid, ci, our_pid, b, next, lineage, &whole);
            }
            use zeroize::Zeroize;
            whole.zeroize();
        }
        use zeroize::Zeroize;
        fresh.zeroize();
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: next, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
        }
        self.persist_chains_of(&fid);
        crate::logf!("MOLECULE: minted era {} for {} — {} wrap(s) inside, {} to a joiner; pending until the first wrap ACKs", next, hex::encode(&gid.0[..4]), inside, joiner_wraps);
    }

    /// WRAP over the friendship (the joiner's side, §10.1 Joining → Standing): open it against the bundle we minted at Join, install the era, build chains + conversation + roster from the parked snapshot, and post our records into the group as our first frame.
    fn install_bind_wrap(&mut self, ci: usize, wire: Option<crate::network::message_package::MoleculeWire>, kem: Option<crate::crypto::era::EraKemWire>) {
        let sponsor_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let (Some(w), Some(cts)) = (wire.as_ref().and_then(|g| g.wrap.as_ref()), kem.as_ref()) else {
            crate::logf!("MOLECULE: wrap from {} without its typed fields — ignored", sponsor_fp);
            return;
        };
        let Some(our_device) = self.device_keypair.as_ref().map(|k| *k.public.as_bytes()) else {
            return;
        };
        if w.recipient_device != our_device {
            return; // for a sibling of ours — it opens its own
        }
        // The wrap does not name its group: it belongs to whichever of this sponsor's offers holds the bundle it was sealed to (Emma 2026-10-24: the FIRST offer from the sponsor was taken — an old one she had left — and the new group's wrap was ignored as "does not open").
        let sponsor = self.contacts.get(ci).map(|c| c.handle_hash);
        let offers: Vec<BondOffer> = self.bond_offers.iter().filter(|o| Some(o.sponsor) == sponsor).cloned().collect();
        if offers.is_empty() {
            crate::logf!("MOLECULE: wrap from {} but no parked offer from them — ignored", sponsor_fp);
            return;
        }
        let opened = offers.into_iter().find_map(|offer| {
            let fid = crate::types::FriendshipId::from_bytes(offer.molecule_id.0);
            let secret = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.molecule_kems().iter().rev().find_map(|k| crate::crypto::era::unwrap_from_bundle(k, &w.bundle_id, cts, &our_device, w.era, &w.nonce, &w.sealed)))?;
            Some((offer, secret))
        });
        let Some((offer, secret)) = opened else {
            crate::logf!("MOLECULE: wrap from {} opens against none of our bundles for their offers — ignored", sponsor_fp);
            return;
        };
        let gid = offer.molecule_id;
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        // A join answer always carries the era WHOLE (root ‖ history key): era N under from-genesis, the freshly minted N+1 under from-join — either way the joiner held nothing to derive from.
        if secret.len() != 64 {
            crate::logf!("MOLECULE: join wrap for {} must carry the era whole (got {} bytes) — ignored", hex::encode(&gid.0[..4]), secret.len());
            return;
        }
        let (root, hk) = (<[u8; 32]>::try_from(&secret[..32]).unwrap(), <[u8; 32]>::try_from(&secret[32..]).unwrap());
        // A SECOND ANSWER to our Join (2026-11-01, Brittany: two of Emma's devices answered, each with its own era 1, and the last to land replaced the first): an era we already hold at a higher index, or at the same index under a lower root, stands — the lower root wins on every device (era_superseded_by), so we keep the one the sponsor's fleet converges on.
        if let Some((held_era, held_root)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, *c.lane_root()?))) {
            if held_era > w.era || (held_era == w.era && held_root <= root) {
                crate::logf!("MOLECULE: a second join answer for {} (era {}, root {}) — the era we hold (era {}, root {}) stands", hex::encode(&gid.0[..4]), w.era, hex::encode(&root[..4]), held_era, hex::encode(&held_root[..4]));
                return;
            }
        }
        let kems: Vec<crate::crypto::era::EraDecapKeys> = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c.molecule_kems().to_vec()).unwrap_or_default();
        let mut chains = crate::types::FriendshipChains::from_molecule_root(gid, &offer.snapshot.standing(), root, hk, w.era, w.era_lineage);
        chains.set_molecule_kems(kems);
        self.friendship_chains.retain(|(id, _)| *id != fid);
        self.friendship_chains.push((fid, chains));
        if !self.conversations.iter().any(|v| v.id() == fid) {
            self.conversations.push(crate::types::Conversation::new_molecule(gid, offer.snapshot.standing()));
        }
        // The offer's snapshot enters thru the fold-checked merge (the Join already seeded it the same way; a re-merge is a no-op for what it holds).
        let pos = match self.molecule_rosters.iter().position(|(id, _)| *id == gid) {
            Some(p) => p,
            None => {
                self.molecule_rosters.push((gid, Roster::default()));
                self.molecule_rosters.len() - 1
            }
        };
        self.merge_snapshot_into(pos, offer.snapshot.clone(), false);
        // The sponsor's current roster rides the answer: everyone who joined since the invitation, fold-checked like any records.
        if let Some(Ok((bid, current))) = wire.as_ref().and_then(|g| g.blob.as_ref()).map(|b| crate::storage::molecule::roster_from_vsf_bytes(b)) {
            if bid == gid {
                self.merge_snapshot_into(pos, current, false);
            }
        }
        self.sync_molecule_conversation(&gid);
        // The offer stays parked, accepted: it is what names the group the friendship row is about (the card reads "joined" from here on).
        self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Standing);
        self.unpark_molecule_posts(gid);
        self.persist_molecule(&gid);
        self.persist_chains_of(&fid);
        // Our first frame in the group: our own member record + bundle, so every member holds them even if the sponsor's records post is still in flight.
        if let (Some(our_pid), Some(seed), Some(hp)) = (self.own_party_id(), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) {
            let (name, avatar) = self.own_name_grant();
            let mut ours = Roster::default();
            ours.merge_member(crate::types::molecule::join_record(our_pid, hp, &name, avatar, offer.sponsor, &seed));
            if let Some(b) = self.molecule_rosters.iter().find(|(id, _)| *id == gid).and_then(|(_, r)| r.newest_bundle(&our_device).cloned()) {
                ours.merge_bundle(b);
            }
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: crate::storage::molecule::roster_to_vsf_bytes(&gid, &ours).ok(), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        }
        crate::logf!("MOLECULE: joined \"{}\" ({}) via {} at era {}", offer.snapshot.title(), hex::encode(&gid.0[..4]), sponsor_fp, w.era);
        self.release_molecule_frames(gid);
    }

    /// WRAP inside the group (an existing member, §10.4): a mint's wrap addressed to our device — open it against our persisted bundle, derive the next era from the one we hold, install it as pending. Rows for other devices are ignored. NOT YET BUILT: withholding a mint whose wrap set omits a member we hold standing (the exclusion tripwire of the vouch model, docs/conversations.md Phase 3) — today any wrap that opens is installed.
    fn install_molecule_wrap(&mut self, cp: usize, wire: Option<crate::network::message_package::MoleculeWire>, kem: Option<crate::crypto::era::EraKemWire>) {
        let (Some(w), Some(cts)) = (wire.as_ref().and_then(|g| g.wrap.as_ref()), kem.as_ref()) else {
            return;
        };
        let Some(our_device) = self.device_keypair.as_ref().map(|k| *k.public.as_bytes()) else {
            return;
        };
        if w.recipient_device != our_device {
            return;
        }
        let Some(gid) = self.conversations.get(cp).filter(|c| c.is_molecule()).map(|c| MoleculeId(*c.id().as_bytes())) else {
            return;
        };
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((era, root, hk, fresh)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| {
            let fresh = c.molecule_kems().iter().rev().find_map(|k| crate::crypto::era::unwrap_from_bundle(k, &w.bundle_id, cts, &our_device, w.era, &w.nonce, &w.sealed))?;
            Some((c.era_index, *c.lane_root()?, *c.history_key()?, fresh))
        }) else {
            crate::logf!("MOLECULE: wrap for era {} in {} does not open against our bundle — ignored", w.era, hex::encode(&gid.0[..4]));
            return;
        };
        if w.era != era + 1 || fresh.len() != 32 {
            crate::logf!("MOLECULE: wrap for era {} in {} while we hold {} — not the next era; ignored (re-serve carries the one we need)", w.era, hex::encode(&gid.0[..4]), era);
            return;
        }
        let fresh32: [u8; 32] = fresh.as_slice().try_into().unwrap();
        let (next_root, next_hk) = molecule_next_era(&gid, w.era, &root, &hk, &w.nonce, &fresh32);
        // TWO MINTS OF ONE INDEX (a random leave or cadence mint meeting a join mint): the lower root stands on every device, the rule era_superseded_by keeps for held eras, so a pending era never flips with the order wraps land in.
        let held = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.pending_era().map(|p| (p.era_index, p.lane_root)));
        if pending_stands(held, w.era, &next_root) {
            crate::logf!("MOLECULE: wrap for era {} in {} — the pending era we hold at that index stands (same or lower root)", w.era, hex::encode(&gid.0[..4]));
            return;
        }
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: w.era, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
        }
        self.persist_chains_of(&fid);
        crate::logf!("MOLECULE: era {} for {} installed as pending — cutover on the first frame tagged with it or our next send", w.era, hex::encode(&gid.0[..4]));
        self.release_molecule_frames(gid);
    }

    /// Records posted inside a group (§4): merge every record from the blob under the roster's one law, then follow the standing set into the conversation. Invalid signatures are skipped loudly; merge decides precedence.
    fn merge_molecule_records(&mut self, ci: Option<usize>, cp: usize, blob: Option<Vec<u8>>) {
        let sender = ci.and_then(|ci| self.contacts.get(ci)).map(|c| crate::fp(&c.handle_proof)).unwrap_or_else(|| "a member".into());
        let Some(gid) = self.conversations.get(cp).filter(|c| c.is_molecule()).map(|c| MoleculeId(*c.id().as_bytes())) else {
            crate::logf!("MOLECULE: records row from {} landed outside a group conversation — ignored", sender);
            return;
        };
        let Some(blob) = blob else {
            crate::logf!("MOLECULE: records row from {} without a payload — ignored", sender);
            return;
        };
        let (blob_gid, snapshot) = match crate::storage::molecule::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("MOLECULE: records payload from {} failed verified read: {} — ignored", sender, e);
                return;
            }
        };
        if blob_gid != gid {
            crate::logf!("MOLECULE: records payload names {} but landed in {} — ignored", hex::encode(&blob_gid.0[..4]), hex::encode(&gid.0[..4]));
            return;
        }
        let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) else {
            crate::logf!("MOLECULE: records for unheld group {} — ignored", hex::encode(&gid.0[..4]));
            return;
        };
        let standing_before = self.molecule_rosters[pos].1.standing();
        // A records row from one of our own devices is our fleet replicating what it already held.
        let own_fleet = ci.and_then(|i| self.contacts.get(i)).is_some_and(|c| c.is_sibling);
        let merged = self.merge_snapshot_into(pos, snapshot, own_fleet);
        if merged || self.sync_molecule_conversation(&gid) {
            crate::logf!("MOLECULE: {} roster advanced by records from {}", hex::encode(&gid.0[..4]), sender);
            self.after_roster_advanced(gid, pos, &standing_before);
        }
    }

    /// A roster advanced (a records row, or parked records a fold just verified): follow the standing set into the conversation, persist, and if anyone lost standing settle what was owed to them and mint the next era without them.
    fn after_roster_advanced(&mut self, gid: MoleculeId, pos: usize, standing_before: &[[u8; 32]]) {
        self.sync_molecule_conversation(&gid);
        self.persist_molecule(&gid);
        let standing_after = self.molecule_rosters[pos].1.standing();
        let departed: Vec<[u8; 32]> = standing_before.iter().filter(|p| standing_after.binary_search(p).is_err()).copied().collect();
        if !departed.is_empty() {
            // A member lost standing (a leave, or a withdrawn vouch): nothing is owed to it any more, and the survivors mint the next era (step 9).
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let done = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid).map(|(_, c)| c.retarget_pendings(&standing_after)).unwrap_or_default();
            if !done.is_empty() {
                if let Some(cp) = self.conversations.iter().position(|v| v.id() == fid) {
                    for m in self.conversations[cp].messages.iter_mut() {
                        if m.is_outgoing && done.contains(&m.timestamp) {
                            m.delivered = true;
                        }
                    }
                }
                self.persist_conversation_async(fid);
            }
            // ONE MINTER for the whole group: the lowest standing party, on one device of its fleet — every member's every device used to mint here, each a different era.
            let us = self.own_party_id();
            let mints_here = us.is_some() && us == standing_after.first().copied() && self.fleet_device_acts(None);
            for p in &departed {
                crate::logf!("MOLECULE: {} left {} — {} standing remain; {}", crate::fp(p), hex::encode(&gid.0[..4]), standing_after.len(), if mints_here { "minting the next era" } else { "the group's minter mints the next era" });
            }
            if mints_here {
                self.mint_molecule_era(gid, None);
            }
        }
    }

    /// Is THIS the one device of our fleet that acts on an event every device of ours sees (a join over a bond, a group's leave or cadence)?
    /// For a friend's event it is our fleet's ceremony owner for them (§4.2) while that owner is present; otherwise, and for a group-wide event, the lowest device key among ours that are online.
    /// THE GROUP ERA HEAL (2026-11-01, Emma and Brittany: one group held two era-1 roots, each side dropping the other's frames for good — a group advert reached no repair at all): a standing member's device advertising an era of this group we never held is a fork. The group's lowest standing party, on one device of its fleet, mints a fresh era WHOLE past every index seen and delivers it over each member's bond (`on_era_delivery` takes a whole era from any standing member); both sides of the fork install it as pending and cut over on its first frame, so the group converges whichever root each held. Once per (group, advertised tag). A member we never friended has no bond to carry it — it follows when a friend's frame on the new era reaches it thru the group.
    pub(super) fn heal_group_era_fork(&mut self, fid: crate::types::FriendshipId, device: [u8; 32], tag: u32) {
        let gid = MoleculeId(fid.0);
        let Some(standing) = self.molecule_rosters.iter().find(|(g, _)| *g == gid).map(|(_, r)| r.standing()) else {
            return;
        };
        let member = self.contacts.iter().any(|c| !c.is_sibling && c.knows_device(&device) && standing.contains(&c.handle_hash));
        if !member || self.chains_know_era(fid, tag) {
            return;
        }
        let us = self.own_party_id();
        let Some(us) = us.filter(|u| standing.first() == Some(u)) else {
            return; // the lowest standing party heals; everyone else takes what it delivers
        };
        if !self.fleet_device_acts(None) || !self.era_heals.insert((fid, tag, 0)) {
            return;
        }
        let Some(lineage) = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c.era_lineage) else {
            return;
        };
        let next = self.era_index_floor(fid) + 1;
        let (root, hk): ([u8; 32], [u8; 32]) = (rand::random(), rand::random());
        let new_tag = crate::crypto::clutch::era_tag(&root);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: next, lane_root: root, history_key: Some(hk), tag: new_tag, resp_osc: None });
        }
        self.pending_minter.insert(fid, us);
        self.persist_chains_async(&fid);
        let mut secret = Vec::with_capacity(64);
        secret.extend_from_slice(&root);
        secret.extend_from_slice(&hk);
        let nonce: [u8; 32] = rand::random();
        // The first member's accept cuts the group over (bonds.rs `on_era_answer`); a delivery's ACK never does (2026-11-13).
        self.era_offers.retain(|o| o.conv != fid);
        self.era_offers.push(super::bonds::EraOffer { conv: fid, nonce, tag: new_tag });
        let wire = crate::network::message_package::MoleculeWire { from: us, woven_authors: Vec::new(), blob: None, wrap: None, deliver: Some(crate::network::message_package::EraDeliveryWire { conversation: fid.0, era: next, era_lineage: lineage, nonce, secret, prior_tag: None }), answer: None };
        let members: Vec<usize> = (0..self.contacts.len()).filter(|&ci| !self.contacts[ci].is_sibling && self.contacts[ci].handle_hash != us && standing.contains(&self.contacts[ci].handle_hash)).collect();
        let mut sent = 0usize;
        for ci in members {
            let ts = crate::network::time_base::stamp_osc();
            if self.bond_transmit_with(ci, ts, None, Some(&wire), &crate::types::RowControl::Molecule(MoleculeSignal::Deliver)) {
                sent += 1;
            }
            if let Some(bfid) = self.bond_fid_of(ci) {
                self.deliveries_in_flight.push((bfid, ts, fid));
            }
        }
        crate::logf!("MOLECULE: {} holds a fork — device {} is on era#{:08x}, which we never held; era#{} minted WHOLE and delivered over {} member bond(s), so both sides converge on it", hex::encode(&gid.0[..4]), crate::fp(&device), tag, next, sent);
    }

    pub(super) fn fleet_device_acts(&self, ci: Option<usize>) -> bool {
        let Some(ours) = self.device_keypair.as_ref().map(|k| *k.public.as_bytes()) else {
            return false;
        };
        let siblings = super::sibling_presence_snapshot(&self.contacts);
        if let Some(owner) = ci.and_then(|ci| self.contacts.get(ci)).and_then(|c| c.ceremony_owner) {
            if owner == ours {
                return true;
            }
            if siblings.iter().any(|(pk, online, probed)| *pk == owner && (*online || !*probed)) {
                return false;
            }
        }
        siblings.iter().filter(|(_, online, _)| *online).all(|(pk, _, _)| *pk > ours)
    }

    /// Is `device` one of `party`'s folded devices: `Some(true)` it is, `Some(false)` it is not, `None` we hold no fold for them yet.
    /// One resolver for every roster subject — ourselves (this device plus our siblings), a friend (the fold-respecting `knows_device`), or a never-friended member (its MoleculePeer fold).
    pub(super) fn record_signer_verdict(&self, party: &[u8; 32], device: &[u8; 32]) -> Option<bool> {
        let us = self.own_party_id();
        if us.as_ref() == Some(party) {
            return Some(self.own_fleet_devices().contains(device));
        }
        if let Some(c) = self.contacts.iter().find(|c| !c.is_sibling && c.handle_hash == *party) {
            return Some(c.knows_device(device));
        }
        self.molecule_peers.iter().find(|p| p.party == *party && !p.devices.is_empty()).map(|p| p.knows_device(device))
    }

    /// The verdict for a record signed at `osc`, when the records came from `own_fleet` (one of our devices replicating what it already held).
    /// A device that has since left its fleet honestly (a phone that moved to a new key, a device that departed) is judged by the fleet it signed in: our own device verified the record while that device was a member (Nick 2026-11-06, a reinstalled phone refusing its group's genesis, signed by the phone's own retired key).
    /// Records from anyone else still need a current member, and a locked device vouches for nothing at any date: its record's date is its own word, so a stolen key could backdate.
    /// `None` while the party's chain is not held here: the records park, and the chain's landing judges them again.
    fn record_signer_verdict_at(&self, party: &[u8; 32], device: &[u8; 32], osc: i64, own_fleet: bool) -> Option<bool> {
        let now = self.record_signer_verdict(party, device);
        if now != Some(false) || !own_fleet {
            return now;
        }
        let us = if self.own_party_id() == Some(*party) { self.our_handle_proof() } else { None };
        let hp = us
            .or_else(|| self.contacts.iter().find(|c| !c.is_sibling && c.handle_hash == *party).map(|c| c.handle_proof))
            .or_else(|| self.molecule_peers.iter().find(|p| p.party == *party).map(|p| p.handle_proof))?;
        crate::network::fgtw::fleet::held_chain(&hp).map(|chain| chain.was_member_at(device, osc))
    }

    /// The party a never-friended handle proof belongs to, as any roster we hold, park, or have been offered names it — so its fold lands as a MoleculePeer and the records waiting on it can verify.
    pub(super) fn molecule_party_for_proof(&self, hp: &[u8; 32]) -> Option<[u8; 32]> {
        let named = |r: &Roster| r.members.values().find(|m| m.handle_proof == *hp).map(|m| m.party);
        self.molecule_rosters
            .iter()
            .find_map(|(_, r)| r.members.values().find(|m| m.handle_proof == *hp && r.is_standing(&m.party)).map(|m| m.party))
            .or_else(|| self.molecule_parked.iter().find_map(|(_, r, _)| named(r)))
            .or_else(|| self.bond_offers.iter().find_map(|o| named(&o.snapshot)))
    }

    /// FOLD EVERY MEMBER WE NEVER FRIENDED (field 2026-11-11, Jeff and Joshua in Nick's group): a MoleculePeer lives in RAM only, and the roster edges were the only thing that folded one, so after a restart this device named no device of such a member.
    /// Without the fold their frames are dropped as a stranger's, our rows and wave joins are routed to nobody of theirs, and a group wave between them is two calls with the one friend they share.
    /// Called on the load edges (resume and attest) and on every group wave edge (an offer, a ring, a join, a frame from a device we cannot name); a member already folded is skipped, so a repeat costs nothing.
    pub(super) fn fold_molecule_members(&mut self, why: &str) {
        let mut named: Vec<[u8; 32]> = Vec::new();
        for (gid, roster) in &self.molecule_rosters {
            let phase = self.molecule_locals.iter().find(|(g, _)| g == gid).map(|(_, l)| l.phase).unwrap_or_default();
            if matches!(phase, MoleculePhase::Left) {
                continue;
            }
            named.extend(roster.standing().iter().filter_map(|p| roster.members.get(p).map(|m| m.handle_proof)));
        }
        let friends: Vec<[u8; 32]> = self.contacts.iter().map(|c| c.handle_proof).collect();
        let proofs = unfolded_member_proofs(&named, self.our_handle_proof(), &friends, &self.molecule_peers);
        if proofs.is_empty() {
            return;
        }
        crate::logf!("MOLECULE: folding {} member(s) we never friended ({}) — their devices are where our group frames and wave joins go", proofs.len(), why);
        self.spawn_contact_fleet_refresh(proofs);
    }

    /// Ask for the folds of every never-friended member a roster names, so its records can be verified against them.
    pub(super) fn fetch_member_folds(&mut self, roster: &Roster) {
        let us = self.session.as_ref().map(|s| s.handle_proof);
        let mut proofs: Vec<[u8; 32]> = roster
            .members
            .values()
            .map(|m| m.handle_proof)
            .filter(|hp| Some(*hp) != us && !self.contacts.iter().any(|c| c.handle_proof == *hp) && !self.molecule_peers.iter().any(|p| p.handle_proof == *hp && !p.devices.is_empty()))
            .collect();
        proofs.sort_unstable();
        proofs.dedup();
        if !proofs.is_empty() {
            self.spawn_contact_fleet_refresh(proofs);
        }
    }

    /// Merge a snapshot's records into a held roster under verify-or-withhold (docs/conversations.md Phase 0): `partition_by_fold` judges every record against its subject's fold.
    /// What verifies merges; what waits on a fold we do not hold is parked, its fold is fetched, and the fold's landing re-merges it; what is signed outside a known fold is refused.
    /// Precedence is entirely the roster merge's newest-wins law.
    /// `own_fleet`: the records came from one of our own devices replicating what it already held (see `record_signer_verdict_at`).
    pub(super) fn merge_snapshot_into(&mut self, pos: usize, snapshot: Roster, own_fleet: bool) -> bool {
        let gid = self.molecule_rosters[pos].0;
        let held_genesis = self.molecule_rosters[pos].1.genesis.clone();
        let part = crate::types::molecule::partition_by_fold(snapshot, held_genesis.as_ref(), |p, d, osc| self.record_signer_verdict_at(p, d, osc, own_fleet), crate::network::time_base::from_the_future);
        for why in &part.refused {
            crate::logf!("MOLECULE: {} — {} — refused", hex::encode(&gid.0[..4]), why);
        }
        let waiting = part.parked_count();
        let mut verified = part.verified;
        if held_genesis.is_some() {
            verified.genesis = None; // never re-merge a genesis we hold (merge_genesis refuses anyway; this keeps `changed` honest)
        }
        let changed = self.molecule_rosters[pos].1.merge_all(verified);
        self.molecule_parked.retain(|(g, _, _)| *g != gid);
        if waiting > 0 {
            crate::logf!("MOLECULE: {} — {} record(s) wait on folds we do not hold yet; fetching them", hex::encode(&gid.0[..4]), waiting);
            self.fetch_member_folds(&part.parked);
            self.molecule_parked.push((gid, part.parked, own_fleet));
        }
        changed
    }

    /// A fold landed: every parked record gets its verdict again, and whatever now verifies joins its roster exactly as a records row would.
    pub(super) fn remerge_parked_records(&mut self) {
        let parked = std::mem::take(&mut self.molecule_parked);
        for (gid, records, own_fleet) in parked {
            let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) else {
                continue; // the group is gone (left, or never held): its parked records go with it
            };
            let standing_before = self.molecule_rosters[pos].1.standing();
            if self.merge_snapshot_into(pos, records, own_fleet) {
                crate::logf!("MOLECULE: {} roster advanced by records a fold just verified", hex::encode(&gid.0[..4]));
                self.after_roster_advanced(gid, pos, &standing_before);
            }
        }
    }

    /// Follow the roster's standing set into the group conversation (§4: the set follows the roster; the id never moves), re-seed the group's admission list, and fold any standing member we never friended into the MoleculePeer trust source (step 5). Returns whether the set changed.
    pub(super) fn sync_molecule_conversation(&mut self, gid: &MoleculeId) -> bool {
        let Some((standing, proofs)) = self.molecule_rosters.iter().find(|(id, _)| id == gid).map(|(_, r)| {
            let standing = r.standing();
            let proofs: Vec<[u8; 32]> = standing.iter().filter_map(|p| r.members.get(p).map(|m| m.handle_proof)).collect();
            (standing, proofs)
        }) else {
            return false;
        };
        let changed = self.conversations.iter_mut().find(|c| c.id().as_bytes() == &gid.0).map(|c| c.set_participants(standing.clone())).unwrap_or(false);
        // The group's chains carry the same standing set (a joiner's were born from the invitation's snapshot, which never names the joiner).
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        // We are in the set while we hold the keys and have not left, whether or not our own records have reached our roster yet.
        let mut members = standing.clone();
        if let Some(us) = self.own_party_id() {
            let keyed = self.friendship_chains.iter().any(|(id, c)| *id == fid && !c.awaiting_root());
            if keyed && !self.molecule_is_left(gid) {
                members.push(us);
            }
        }
        let chains_moved = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid).is_some_and(|(_, c)| c.set_molecule_participants(&members));
        if chains_moved || changed {
            self.release_molecule_frames(*gid);
        }
        if chains_moved {
            self.persist_chains_of(&fid);
        }
        self.reseed_molecule_pubkeys();
        // Members with no contact row need their public chain folded — the same refresh a contact gets; the drain lands them in `molecule_peers`.
        let us = self.session.as_ref().map(|s| s.handle_proof);
        let unfriended: Vec<[u8; 32]> = proofs
            .into_iter()
            .filter(|hp| Some(*hp) != us && !self.contacts.iter().any(|c| c.handle_proof == *hp) && !self.molecule_peers.iter().any(|p| p.handle_proof == *hp))
            .collect();
        if !unfriended.is_empty() {
            self.spawn_contact_fleet_refresh(unfriended);
        }
        changed
    }

    /// Send (or refresh) a group OFFER to a contact over the pairwise braid (§10.1): the roster snapshot rides the package's gpl field so the invitee sees who is in it before consenting. Carries no secret. The invitee's consent comes back as a Join.
    pub(super) fn send_bond_offer(&mut self, gid: MoleculeId, ci: usize) -> bool {
        let Some(contact) = self.contacts.get(ci) else {
            return false;
        };
        if contact.is_sibling {
            crate::log("MOLECULE: a sibling is already every group we are — offer refused");
            return false;
        }
        let fp = crate::fp(&contact.handle_proof);
        let Some(our_pid) = self.our_party_id(contact) else {
            return false;
        };
        let blob = {
            let Some((_, roster)) = self.molecule_rosters.iter().find(|(g, _)| *g == gid) else {
                crate::logf!("MOLECULE: no roster for {} — cannot offer", hex::encode(&gid.0[..4]));
                return false;
            };
            if !roster.is_standing(&our_pid) {
                crate::logf!("MOLECULE: not standing in {} — cannot offer", hex::encode(&gid.0[..4]));
                return false;
            }
            match crate::storage::molecule::roster_to_vsf_bytes(&gid, roster) {
                Ok(b) => b,
                Err(e) => {
                    crate::logf!("MOLECULE: roster snapshot for {} failed to encode: {}", hex::encode(&gid.0[..4]), e);
                    return false;
                }
            }
        };
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: Some(blob), wrap: None, deliver: None, answer: None };
        let ts = crate::network::time_base::stamp_osc();
        // The offer rides the BOND (docs/conversations.md §7): its row lands in the bond's hidden conversation first, then goes on the wire — nothing lands in any chat; the invitee sees it as an invitation row.
        let invitee = self.contacts[ci].handle_hash;
        let sent = self.send_bond_row(ci, crate::types::RowControl::Molecule(MoleculeSignal::Offer), Some(&wire), ts);
        self.set_molecule_local(&gid, |l| l.sent_invite(invitee, ts));
        // Every offer, first or re-sent, is fleet state: our other devices learn the invite and its newest stamp, so any of them re-offers and any of them can undo it.
        self.share_invites_with_fleet(invitee, &[(gid, ts, false)]);
        crate::logf!("MOLECULE: offer for {} {} to {}", hex::encode(&gid.0[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" }, fp);
        sent
    }

    /// THE NEXT EDGE (field 2026-11-04): a contact came online, so every group our fleet offered them and has not seen them join or decline is offered again. An offer rides the wire once and friend history pages carry no control rows, so an invitee offline at that moment never received it; a re-offer replaces the parked snapshot on their side, so one they did receive costs nothing.
    /// Joined invitees are standing in the roster and skipped; a decline already removed the invitee from `offered`.
    /// The invites are the fleet's (2026-11-19): one sent from a laptop that then sleeps is re-offered here by whichever of our devices sees the invitee.
    pub(super) fn reoffer_pending_bonds(&mut self, ci: usize) {
        let Some(invitee) = self.contacts.get(ci).filter(|c| !c.is_sibling).map(|c| c.handle_hash) else {
            return;
        };
        for gid in invites_due(&self.molecule_locals, &self.molecule_rosters, &invitee) {
            crate::logf!("MOLECULE: {} is back — offering {} again", crate::fp(&self.contacts[ci].handle_proof), hex::encode(&gid.0[..4]));
            self.send_bond_offer(gid, ci);
        }
    }

    /// NOT NOW — the invitee's decline, which is an UNDO (Nick 2026-10-24: "if they decline … undo the invite and remove it from my end so it's like I never sent it").
    /// The parked offer and its card go from this device now; the sponsor's device, on our Decline, forgets the invite and deletes its offer row for everyone, and that delete clears the card from our other devices too.
    pub(super) fn decline_offer(&mut self, gid: MoleculeId) -> bool {
        let Some(offer) = self.bond_offers.iter().find(|o| o.molecule_id == gid && !o.accepted).cloned() else {
            crate::logf!("MOLECULE: decline of {} — no open offer", hex::encode(&gid.0[..4]));
            return false;
        };
        let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == offer.sponsor && !c.is_sibling) else {
            crate::logf!("MOLECULE: decline of {} — the sponsor is no longer a contact; forgotten here", hex::encode(&gid.0[..4]));
            self.forget_offer(None, gid, offer.row_osc);
            return false;
        };
        self.forget_offer(Some(ci), gid, offer.row_osc);
        let Some(our_pid) = self.our_party_id(&self.contacts[ci]) else {
            return false;
        };
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: Some(gid.0.to_vec()), wrap: None, deliver: None, answer: None };
        let ts = crate::network::time_base::stamp_osc();
        let sent = self.send_bond_row(ci, crate::types::RowControl::Molecule(MoleculeSignal::Decline), Some(&wire), ts);
        crate::logf!("MOLECULE: declined {} — {} {}", hex::encode(&gid.0[..4]), if sent { "the undo sent to" } else { "the undo NOT sent (the next edge retries) to" }, crate::fp(&self.contacts[ci].handle_proof));
        sent
    }

    /// Forget a parked offer on this device: off the list, off the vault, and its card tombstoned in the sponsor's conversation (when that conversation is known).
    pub(super) fn forget_offer(&mut self, ci: Option<usize>, gid: MoleculeId, row_osc: i64) {
        self.bond_offers.retain(|o| o.molecule_id != gid);
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::molecule::unpark_bond_offer(&gid, storage) {
                crate::logf!("MOLECULE: offer {} unpark failed: {}", hex::encode(&gid.0[..4]), e);
            }
        }
        let Some(ci) = ci else { return };
        if let Some(conv) = self.bond_conv_mut(ci) {
            if let Some(m) = conv.messages.iter_mut().find(|m| m.timestamp == row_osc && super::is_bond_offer_row(m) && !m.deleted) {
                m.deleted = true;
            }
        }
        if let Some(bfid) = self.bond_fid_of(ci) {
            self.persist_conversation_async(bfid);
        }
        self.scene_dirty = true;
    }

    /// SPONSOR, on the invitee's Decline: the undo — forget the invite, tombstone our offer row, and delete it for everyone (the marker clears the card from each of the invitee's devices).
    fn on_decline(&mut self, ci: usize, blob: Option<Vec<u8>>) {
        let Some(gid) = blob.and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()).map(MoleculeId) else {
            crate::log("MOLECULE: decline without a group id — ignored");
            return;
        };
        self.undo_invite(ci, gid, "declined");
    }

    /// SPONSOR: UNDO an invite we sent — on the invitee's decline, or our own withdrawal from the Bonds page. Forget it, tombstone our offer row, and delete that row for everyone (the marker clears the card from each of the invitee's devices). Nothing new lands in either chat.
    pub(super) fn undo_invite(&mut self, ci: usize, gid: MoleculeId, why: &str) -> bool {
        let invitee = self.contacts[ci].handle_hash;
        let row_ts = self.molecule_locals.iter().find(|(g, _)| *g == gid).and_then(|(_, l)| l.offered.iter().find(|(p, _)| *p == invitee).map(|(_, t)| *t));
        let Some(row_ts) = row_ts else {
            crate::logf!("MOLECULE: invite of {} into {} {} — no invite of ours (never sent, or already undone)", crate::fp(&self.contacts[ci].handle_proof), hex::encode(&gid.0[..4]), why);
            return false;
        };
        self.set_molecule_local(&gid, |l| {
            l.clear_invite(invitee, row_ts);
        });
        // The end is fleet state too: our other devices stop re-offering it.
        self.share_invites_with_fleet(invitee, &[(gid, row_ts, true)]);
        // The offer row lives in the bond's hidden conversation (docs/conversations.md §7): tombstone it there, and delete it for everyone on the bond.
        if let Some(conv) = self.bond_conv_mut(ci) {
            if let Some(m) = conv.messages.iter_mut().find(|m| m.timestamp == row_ts && super::is_bond_offer_row(m)) {
                m.deleted = true;
            }
        }
        let deleted = self.send_bond_row(ci, crate::types::RowControl::Delete { target: row_ts }, None, crate::network::time_base::stamp_osc());
        crate::logf!("MOLECULE: invite of {} into {} {} — undone{}", crate::fp(&self.contacts[ci].handle_proof), hex::encode(&gid.0[..4]), why, if deleted { ", offer deleted for everyone" } else { " here (no chain for the delete marker)" });
        self.scene_dirty = true;
        true
    }

    /// SEND THE BONDS EDITS when the Bonds page is left (Nick 2026-10-24: "Don't send out any invites until the user has navigated away so they can select/unselect").
    /// Leaving the page — another page, Back, the panel closed, the app sent to the background — is the edge; a staged invite goes out as an offer, a staged withdrawal undoes its invite. An app killed first sends nothing, the safe way round.
    pub(super) fn commit_bonds_if_left(&mut self) {
        if self.bonds_staged.is_empty() {
            return;
        }
        let still_here = matches!(self.state, crate::ui::AppState::ConversationPanel(crate::ui::state::ConversationPage::Bonds)) && crate::platform::app_watching();
        if still_here {
            return;
        }
        let staged = std::mem::take(&mut self.bonds_staged);
        crate::logf!("MOLECULE: Bonds page left — sending {} staged edit(s)", staged.len());
        for (gid, party) in staged {
            let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == party && !c.is_sibling) else {
                continue;
            };
            let invited = self.molecule_locals.iter().any(|(g, l)| *g == gid && l.offered.iter().any(|(p, _)| *p == party));
            if invited {
                self.undo_invite(ci, gid, "withdrawn");
            } else {
                self.send_bond_offer(gid, ci);
            }
        }
    }

    /// FOUND an ATOM (docs/molecules.md §0: you can only ever create an atom): a conversation with you alone in it, titled or not. Nothing is offered to anyone; binding people in later makes it a molecule.
    pub(super) fn found_atom_alone(&mut self, title: &str, history_from_genesis: bool) -> Option<MoleculeId> {
        let (Some(our_pid), Some(seed), Some(hp)) = (self.own_party_id(), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) else {
            return None;
        };
        let (name, avatar) = self.own_name_grant();
        let birth = crate::types::molecule::found_atom(our_pid, hp, &name, avatar, title, history_from_genesis, &seed);
        let gid = birth.molecule_id;
        let eph = crate::crypto::era::era_keygen(0, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let bundle = crate::types::molecule::bundle_record(our_pid, 0, &eph, &seed);
        let mut roster = Roster::default();
        roster.merge_genesis(birth.genesis.clone());
        roster.merge_member(birth.founder_member.clone());
        roster.merge_vouch(birth.founder_vouch.clone());
        roster.merge_bundle(bundle);
        let mut chains = crate::types::FriendshipChains::from_molecule_root(gid, &roster.standing(), birth.molecule_root, birth.molecule_history_key, 0, birth.era_lineage);
        chains.push_molecule_kem(eph.export_decaps(0));
        let fid = *chains.id();
        self.friendship_chains.push((fid, chains));
        self.conversations.push(crate::types::Conversation::new_molecule(gid, roster.standing()));
        self.molecule_rosters.push((gid, roster));
        self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Standing);
        self.persist_molecule(&gid);
        self.persist_chains_of(&fid);
        crate::logf!("MOLECULE: atom \"{}\" ({}) founded — {} history", title, hex::encode(&gid.0[..4]), if history_from_genesis { "from-genesis" } else { "from-join" });
        Some(gid)
    }

    /// OLD-SELF IMPORT — MIGRATION-EXPIRES: 2027-02-01 (Nick 2026-10-27: "that import of old will be english only and version out in 12 versions, no reason to keep old non atoms around in the code"); grep the tag to find every piece.
    /// Boot: the old self conversation (`Conversation::new([us])`, its own table) becomes an atom in place when it holds a photon and is no atom yet; an empty one is never founded. Runs after the atoms load, so a device that converted earlier finds its atom already held.
    pub(super) fn adopt_old_self_conversation(&mut self, storage: &crate::storage::FlatStorage) {
        let Some(us) = self.own_party_id() else {
            return;
        };
        let mut legacy = crate::types::Conversation::new([us]);
        if self.molecule_rosters.iter().any(|(g, _)| g.0 == *legacy.id().as_bytes()) {
            return;
        }
        crate::storage::contacts::load_conversation_state(&mut legacy, &us, storage);
        match crate::storage::contacts::load_messages(&mut legacy, storage) {
            Ok(_) if legacy.messages.iter().any(|m| !m.is_control()) => {
                self.found_self_atom(legacy);
            }
            Ok(_) => {}
            Err(e) => crate::logf!("MOLECULE: the old self conversation failed to load: {}", e),
        }
    }

    /// OLD-SELF IMPORT — MIGRATION-EXPIRES: 2027-02-01 (Nick 2026-10-27: "that import of old will be english only and version out in 12 versions, no reason to keep old non atoms around in the code"); grep the tag to find every piece.
    /// THE CONVERSATION WITH ONLY US IN IT BECOMES AN ATOM (Nick 2026-10-27: "You still keep baking weird shit into self conversations that don't behave like the others"): the old self conversation — `Conversation::new([us])`, no roster, its own hidden-until-opened rule — is founded IN PLACE as an atom like any other, listed under Atoms, renamable, bindable, leavable.
    /// In place: a DERIVED genesis for founders [us] names exactly the id the old conversation already had, so its table and its rows stay where they are.
    /// The same atom on every device of ours with no race: the era-0 root and history key derive from our identity seed (which only our devices hold, one-way), the genesis certifies itself, and each device adds its own member grant and bundle the way any member device does.
    /// Called only when the old conversation holds a photon; an empty one is simply never founded.
    pub(super) fn found_self_atom(&mut self, legacy: crate::types::Conversation) -> Option<MoleculeId> {
        let (Some(identity_seed), Some(hp), Some(seed)) = (self.session.as_ref().map(|s| s.identity_seed), self.session.as_ref().map(|s| s.handle_proof), self.device_seed()) else {
            return None;
        };
        let us = crate::crypto::clutch::identity_party_id(&identity_seed);
        let gid = MoleculeId(*legacy.id().as_bytes());
        if self.molecule_rosters.iter().any(|(g, _)| *g == gid) {
            return Some(gid);
        }
        let genesis = crate::types::molecule::GenesisRecord::derived(&[us]);
        if genesis.molecule_id != gid {
            crate::logf!("MOLECULE: the self conversation {} is not the derivation of [us] — left as it is", hex::encode(&gid.0[..4]));
            return None;
        }
        let (name, avatar) = self.own_name_grant();
        let eph = crate::crypto::era::era_keygen(0, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let mut roster = Roster::default();
        roster.merge_genesis(genesis);
        roster.merge_member(crate::types::molecule::join_record(us, hp, &name, avatar, us, &seed));
        roster.merge_bundle(crate::types::molecule::bundle_record(us, 0, &eph, &seed));
        // Every atom has a name (Nick 2026-10-27): the old conversation never had one, so its import takes our preferred name as its title — or "Self" when none is set ("and if blank, just title it 'Self'") — a real title, renamed like any other.
        let title = if name.is_empty() { "Self".to_string() } else { name.clone() };
        roster.merge_title(crate::types::molecule::title_record(us, &title, &seed));
        let root = blake3::derive_key("photon 2026-10-27 self atom era 0 root", &identity_seed);
        let history_key = blake3::derive_key("photon 2026-10-27 self atom era 0 history key", &identity_seed);
        let mut chains = crate::types::FriendshipChains::from_molecule_root(gid, &roster.standing(), root, history_key, 0, crate::crypto::clutch::era_lineage(&root));
        chains.push_molecule_kem(eph.export_decaps(0));
        let fid = *chains.id();
        if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
            self.friendship_chains.push((fid, chains));
        }
        let mut conv = crate::types::Conversation::new_molecule(gid, roster.standing());
        conv.messages = legacy.messages;
        conv.unread_count = legacy.unread_count;
        conv.hydrated = legacy.hydrated;
        let rows = conv.messages.len();
        self.conversations.retain(|v| v.id() != fid);
        self.conversations.push(conv);
        self.molecule_rosters.push((gid, roster));
        self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Standing);
        self.persist_molecule(&gid);
        self.persist_chains_of(&fid);
        crate::logf!("MOLECULE: the conversation with only us in it is an atom now ({}) — {} row(s) kept in place", hex::encode(&gid.0[..4]), rows);
        Some(gid)
    }

    /// The default newcomer-history policy for groups this identity founds (Settings → Conversations; a linked fleet setting). Absent = from join (D5).
    pub(super) fn molecule_default_from_genesis(&self) -> bool {
        self.fleet_settings.as_ref().and_then(|fs| fs.effective("molecule.history_from_genesis")).and_then(|v| v.as_u64()).map_or(false, |v| v != 0)
    }

    /// The name + avatar pin we grant a group: the published profile name and the session avatar pin's lookup half (zeroes when unset).
    fn own_name_grant(&self) -> (String, [u8; 32]) {
        let name = self.fleet_settings.as_ref().and_then(|fs| fs.effective("profile.name")).and_then(crate::storage::fleet_settings::as_text).unwrap_or_default();
        (name, [0u8; 32])
    }

    /// Our device's Ed25519 seed for record signing — the same key that signs every frame.
    pub(super) fn device_seed(&self) -> Option<[u8; 32]> {
        self.device_keypair.as_ref().map(|k| *k.secret.as_bytes())
    }

    /// Queue a group-side post for the fan-out send (drained by step 4's group transmit).
    pub(super) fn queue_molecule_post(&mut self, post: MoleculePost) {
        self.pending_molecule_posts.push(post);
    }

    /// Hand roster records (our leave) to our own devices as a page row under the group's token, the records in the `gpl` column.
    pub(super) fn share_molecule_records_with_fleet(&self, gid: MoleculeId, blob: Vec<u8>) {
        let Some(us) = self.own_party_id() else {
            return;
        };
        let mut msg = crate::types::ChatMessage::control(crate::types::RowControl::Molecule(MoleculeSignal::Records), true, crate::network::time_base::stamp_osc());
        msg.author = Some(us);
        let mut row = crate::network::history_pages::HistoryRow::from_message(&msg);
        row.gpl = blob;
        self.push_history_rows_to_siblings(gid.token(), &hex::encode(&gid.0[..4]), vec![row], None);
    }

    /// A sibling came online: every group we have left hands it our leave again (a no-op merge on a device that already left), so a device that was away when we left still leaves.
    pub(super) fn reshare_fleet_leaves(&self) {
        let Some(us) = self.own_party_id() else {
            return;
        };
        for (gid, roster) in &self.molecule_rosters {
            if !self.molecule_is_left(gid) {
                continue;
            }
            let Some(leave) = roster.leaves.get(&us).cloned() else {
                continue;
            };
            let mut posted = Roster::default();
            posted.merge_leave(leave);
            if let Ok(b) = crate::storage::molecule::roster_to_vsf_bytes(gid, &posted) {
                self.share_molecule_records_with_fleet(*gid, b);
            }
        }
    }

    /// THE FLEET'S INVITES (2026-11-19, the owner: "Oh, well that's dumb"): an invite sent from a rarely used laptop lived only there, so only the laptop re-offered, and the desktop that talked to the invitee all day never did.
    /// Hand our other devices the state of our invites to `invitee`, one page row per group under our pair token with them (it names the invitee on every device of ours): an outgoing `Offer` row, its stamp the offer's, `deleted` when the invite has ended, and a records-free roster naming the group in `gpl`.
    /// A received invitation rides the same column as an INCOMING Offer row (`share_offer_with_fleet`); the direction tells the two apart.
    pub(super) fn share_invites_with_fleet(&self, invitee: [u8; 32], invites: &[(MoleculeId, i64, bool)]) {
        let Some(us) = self.own_party_id() else {
            return;
        };
        let token = crate::crypto::clutch::derive_conversation_token(&[us, invitee]);
        let rows: Vec<crate::network::history_pages::HistoryRow> = invites.iter().filter_map(|(gid, osc, ended)| invite_row(gid, *osc, *ended)).collect();
        self.push_history_rows_to_siblings(token, "our invites", rows, None);
    }

    /// A sibling came online: every invite this device holds, outstanding or ended, goes to our devices again (a no-op merge on one that holds it already).
    /// This is also how invites sent before the ledger replicated reach the rest of the fleet: the first time the device that sent one runs this build and sees a sibling.
    pub(super) fn reshare_fleet_invites(&self) {
        let mut by_invitee: Vec<([u8; 32], Vec<(MoleculeId, i64, bool)>)> = Vec::new();
        for (gid, l) in &self.molecule_locals {
            let roster = self.molecule_rosters.iter().find(|(g, _)| g == gid).map(|(_, r)| r);
            // An invitee standing already is ended on every device by its own roster edge; it is not handed on as outstanding.
            let outstanding = l.offered.iter().filter(|(p, _)| !roster.is_some_and(|r| r.is_standing(p))).map(|(p, o)| (*p, *o, false));
            let ended = l.withdrawn.iter().map(|(p, w)| (*p, *w, true));
            for (party, osc, is_ended) in outstanding.chain(ended) {
                match by_invitee.iter_mut().find(|(p, _)| *p == party) {
                    Some((_, v)) => v.push((*gid, osc, is_ended)),
                    None => by_invitee.push((party, vec![(*gid, osc, is_ended)])),
                }
            }
        }
        for (invitee, invites) in by_invitee {
            crate::logf!("MOLECULE: handing our {} invite record(s) for {} to our devices", invites.len(), crate::fp(&invitee));
            self.share_invites_with_fleet(invitee, &invites);
        }
    }

    /// One of our devices handed us the state of an invite to contact `ci` (a page row from `share_invites_with_fleet`): take it into the ledger, newest stamp wins, an end at or past an offer's stamp ends it.
    /// An outstanding invite this device did not hold is offered again at once when the invitee is online now: the online edge it would wait for may be hours away (the desktop that talks to the invitee all day).
    /// Never handed on again; the device that sent it reaches the others.
    pub(super) fn adopt_fleet_invite(&mut self, ci: usize, gid: MoleculeId, osc: i64, ended: bool) {
        let Some((invitee, online, fp)) = self.contacts.get(ci).filter(|c| !c.is_sibling).map(|c| (c.handle_hash, c.is_online, crate::fp(&c.handle_proof))) else {
            return;
        };
        let standing = self.molecule_rosters.iter().any(|(g, r)| *g == gid && r.is_standing(&invitee));
        if !ended && standing {
            return;
        }
        let mut local = self.molecule_locals.iter().find(|(g, _)| *g == gid).map(|(_, l)| l.clone()).unwrap_or_default();
        let new_here = !local.offered.iter().any(|(p, _)| *p == invitee);
        let changed = if ended { local.clear_invite(invitee, osc) } else { local.note_invite(invitee, osc) };
        if !changed {
            return;
        }
        self.set_molecule_local(&gid, |l| {
            l.offered = local.offered;
            l.withdrawn = local.withdrawn;
        });
        self.scene_dirty = true;
        crate::logf!("MOLECULE: our fleet's invite of {} into {} {}", fp, hex::encode(&gid.0[..4]), if ended { "ended — no longer offered here" } else { "taken here" });
        if !ended && new_here && online && !self.molecule_is_left(&gid) {
            crate::logf!("MOLECULE: {} is online now — offering {} from this device", fp, hex::encode(&gid.0[..4]));
            self.send_bond_offer(gid, ci);
        }
    }

    /// The invitees of `gid` standing in its roster now: their invites have ended on this device.
    fn clear_stood_invites(&mut self, gid: &MoleculeId) {
        let Some(roster) = self.molecule_rosters.iter().find(|(g, _)| g == gid).map(|(_, r)| r) else {
            return;
        };
        let stood = self.molecule_locals.iter().find(|(g, _)| g == gid).map(|(_, l)| l.stood_invites(roster)).unwrap_or_default();
        if stood.is_empty() {
            return;
        }
        for (p, _) in &stood {
            crate::logf!("MOLECULE: {} stands in {} — the invite is done", crate::fp(p), hex::encode(&gid.0[..4]));
        }
        self.set_molecule_local(gid, |l| {
            for (p, o) in stood {
                l.clear_invite(p, o);
            }
        });
    }

    /// We left `gid`: nothing of ours is offered into it any more (each of our devices leaves, and ends them, on its own).
    fn clear_group_invites(&mut self, gid: MoleculeId) {
        let open: Vec<([u8; 32], i64)> = self.molecule_locals.iter().find(|(g, _)| *g == gid).map(|(_, l)| l.offered.clone()).unwrap_or_default();
        if open.is_empty() {
            return;
        }
        crate::logf!("MOLECULE: left {} — {} open invite(s) ended", hex::encode(&gid.0[..4]), open.len());
        self.set_molecule_local(&gid, |l| {
            for (p, o) in open {
                l.clear_invite(p, o);
            }
        });
    }

    /// Hold a group frame whose signer the roster does not name yet; the oldest go first past 64 for one group (a flood from a device that never joins cannot grow without bound).
    pub(super) fn hold_molecule_frame(&mut self, gid: MoleculeId, update: crate::network::status::StatusUpdate) {
        const HELD_PER_GROUP: usize = 64;
        if self.molecule_held_frames.iter().filter(|(g, _)| *g == gid).count() >= HELD_PER_GROUP {
            if let Some(i) = self.molecule_held_frames.iter().position(|(g, _)| *g == gid) {
                self.molecule_held_frames.remove(i);
            }
        }
        self.molecule_held_frames.push((gid, update));
    }

    /// A member device sent a frame stamped `ts` on an era we hold: anything it stamped no later on an era we do not hold is an older copy (a device moves eras forward, and a row it re-sends keeps its stamp).
    pub(super) fn note_device_on_held_era(&mut self, gid: MoleculeId, device: [u8; 32], ts: i64) {
        match self.molecule_device_era.iter_mut().find(|(g, d, _)| *g == gid && *d == device) {
            Some(e) => e.2 = e.2.max(ts),
            None => self.molecule_device_era.push((gid, device, ts)),
        }
    }

    /// CATCHING UP, DERIVED (Petunia 2026-10-27: a frame on the era from before Nick joined was held, replayed on every roster edge and re-flagged the group each time, and only a cutover ever cleared the flag — the header said Catching up forever while every row opened): a group is Catching up exactly while a held frame waits on an era we do not hold.
    /// A held frame stamped no later than a frame the same device sent on an era we hold is an old copy — a from-join member is never given the era it rides, and a row we never ACK is re-sent on the sender's current lane — so it is dropped here, never waited on; a newer stamp on an unknown era is the real wait (our wrap is in flight).
    /// Returns whether a phase moved.
    pub(super) fn settle_catching_up(&mut self, gids: &[MoleculeId]) -> bool {
        let mut changed = false;
        for &gid in gids {
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let Some(chains) = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c) else {
                continue;
            };
            // The signer and stamp of a held frame on an era we hold no slot for.
            let waits = |u: &crate::network::status::StatusUpdate| match u {
                crate::network::status::StatusUpdate::ChatMessage { era: Some(t), sender_pubkey, timestamp, .. } if chains.era_slot_for_tag(Some(*t)).is_none() => Some((sender_pubkey.key, *timestamp)),
                _ => None,
            };
            let mut dropped = 0usize;
            let mut waiting = 0usize;
            let device_era = &self.molecule_device_era;
            self.molecule_held_frames.retain(|(g, u)| {
                if *g != gid {
                    return true;
                }
                let Some((dev, ts)) = waits(u) else { return true };
                if device_era.iter().any(|(eg, ed, seen)| *eg == gid && *ed == dev && *seen >= ts) {
                    dropped += 1;
                    return false;
                }
                waiting += 1;
                true
            });
            if dropped > 0 {
                crate::logf!("MOLECULE: {} held frame(s) in {} dropped — their senders have since sent on an era we hold, so these ride an older one", dropped, hex::encode(&gid.0[..4]));
            }
            let was = self.molecule_locals.iter().find(|(g, _)| *g == gid).map(|(_, l)| l.phase).unwrap_or_default();
            let now = match was {
                MoleculePhase::Standing if waiting > 0 => MoleculePhase::CatchingUp,
                MoleculePhase::CatchingUp if waiting == 0 => MoleculePhase::Standing,
                p => p,
            };
            if now != was {
                crate::logf!("MOLECULE: {} is {} — {} held frame(s) wait on an era we do not hold", hex::encode(&gid.0[..4]), if now == MoleculePhase::CatchingUp { "catching up" } else { "standing again" }, waiting);
                self.set_molecule_local(&gid, |l| l.phase = now);
                changed = true;
            }
        }
        changed
    }

    /// An edge a held frame waits on (the roster's standing set moved, an era installed, the chains landed): every frame held for this group re-enters the receive arm, full gates and all.
    pub(super) fn release_molecule_frames(&mut self, gid: MoleculeId) {
        if !self.molecule_held_frames.iter().any(|(g, _)| *g == gid) {
            return;
        }
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.molecule_held_frames).into_iter().partition(|(g, _)| *g == gid);
        self.molecule_held_frames = rest;
        crate::logf!("MOLECULE: {} held frame(s) in {} replayed — the group moved", mine.len(), hex::encode(&gid.0[..4]));
        self.chat_replay_queue.extend(mine.into_iter().map(|(_, u)| u));
    }

    /// The edge a parked group post waits for (keys landed, a cutover): every post parked for this group drains on the next tick.
    pub(super) fn unpark_molecule_posts(&mut self, gid: MoleculeId) {
        let serial = self.tick_serial;
        for p in self.pending_molecule_posts.iter_mut().filter(|p| p.gid == gid && p.queued == u64::MAX) {
            p.queued = serial;
        }
        // …and the group's chat rows, parked in the one send queue.
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        for p in self.pending_sends.iter_mut().filter(|p| p.conv == fid && p.queued == u64::MAX) {
            p.queued = serial;
        }
    }

    /// Mutate + persist this device's local state for a group.
    /// Have we LEFT this molecule (our leave record posted, phase Left)? A left molecule is out of the contact list (Nick 2026-10-01: "You left this molecule" and the row stayed) — its records stay on disk, read-only, exactly as the doctrine keeps every record, but nothing shows and nothing is posted.
    pub(super) fn molecule_is_left(&self, gid: &MoleculeId) -> bool {
        self.molecule_locals.iter().any(|(g, l)| g == gid && l.phase == MoleculePhase::Left)
    }

    pub(super) fn set_molecule_local(&mut self, gid: &MoleculeId, f: impl FnOnce(&mut MoleculeLocal)) {
        let pos = match self.molecule_locals.iter().position(|(id, _)| id == gid) {
            Some(p) => p,
            None => {
                self.molecule_locals.push((*gid, MoleculeLocal::default()));
                self.molecule_locals.len() - 1
            }
        };
        f(&mut self.molecule_locals[pos].1);
        // The write rides the seal worker (the UI thread makes no vault call): one worker in order, so the last snapshot queued is the one on disk.
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let (gid, local) = (*gid, self.molecule_locals[pos].1.clone());
        let pending = self.durable_pending.clone();
        *self.durable_pending.0.lock().unwrap() += 1;
        super::queue_job(&self.seal_job_tx, move || {
            if let Err(e) = crate::storage::molecule::save_molecule_local(&gid, &local, &storage) {
                crate::logf!("MOLECULE: local state write failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
            // Quit-drain accounting, the drafts writer's law: a group's state must not die with a quitting process.
            let (n, cv) = &*pending;
            *n.lock().unwrap() -= 1; // counted in above, before the queue
            cv.notify_all();
        });
    }

    /// Persist one chains blob by id (group or friendship) on the calling thread — small, rare edges (found, join, wrap install).
    pub(super) fn persist_chains_of(&self, fid: &crate::types::FriendshipId) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if let Some((_, c)) = self.friendship_chains.iter().find(|(id, _)| id == fid) {
            if let Err(e) = crate::storage::friendship::save_friendship_chains(c, storage) {
                crate::logf!("MOLECULE: chains persist failed for {}: {}", hex::encode(&fid.as_bytes()[..4]), e);
            }
        }
    }

    /// A GROUP ACK (docs/molecules.md step 4): resolve the acking device to a standing party (a friend's fold, else a MoleculePeer's), mark it on the per-member ledger — the implied-ACK rule per party rides inside — and flip `delivered` on every row whose target set is now covered. Returns whether a row changed.
    pub(super) fn on_molecule_ack(&mut self, gid: MoleculeId, device: [u8; 32], acked_eagle_time: i64) -> bool {
        let Some(pos) = self.molecule_rosters.iter().position(|(g, _)| *g == gid) else {
            return false;
        };
        let standing = self.molecule_rosters[pos].1.standing();
        let party = self
            .contacts
            .iter()
            .find(|c| !c.is_sibling && c.knows_device(&device) && standing.binary_search(&c.handle_hash).is_ok())
            .map(|c| c.handle_hash)
            .or_else(|| self.molecule_peers.iter().find(|p| p.knows_device(&device) && standing.binary_search(&p.party).is_ok()).map(|p| p.party));
        let Some(party) = party else {
            crate::logf!("MOLECULE: ACK for {} from device {} — no standing member's device; ignored", hex::encode(&gid.0[..4]), hex::encode(&device[..4]));
            return false;
        };
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) else {
            return false;
        };
        // THE MINTER'S CUTOVER EDGE (docs/molecules.md §10.4): the first ACK of any wrap row proves a member holds the new era — the pending era becomes current here.
        let is_wrap_row = self.conversations.iter().find(|v| v.id() == fid).and_then(|v| v.messages.iter().find(|m| m.is_outgoing && m.timestamp == acked_eagle_time)).map_or(false, |m| m.control == Some(crate::types::RowControl::Molecule(MoleculeSignal::Wrap)));
        let mut cut_over = false;
        if is_wrap_row && chains.pending_era().is_some() {
            if let Some((old_tag, new_tag, retired)) = chains.cut_over_to_pending() {
                crate::logf!("MOLECULE: era cut over {:08x} → {:08x} in {} on the first wrap ACK — {} pending(s) re-serve on the fresh lane", old_tag, new_tag, hex::encode(&gid.0[..4]), retired);
                cut_over = true;
            }
        }
        let done = chains.process_ack_by(acked_eagle_time, Some(party), None);
        let progress = chains.pending_progress(acked_eagle_time);
        // The leaver's edge (D9): once ANY member ACKs our leave row, a survivor holds the news — the root dies here.
        if self.molecule_locals.iter().any(|(g, l)| *g == gid && l.phase == MoleculePhase::Left) {
            let is_leave_row = self.conversations.iter().find(|v| v.id() == fid).and_then(|v| v.messages.iter().find(|m| m.is_outgoing && m.timestamp == acked_eagle_time)).map_or(false, |m| m.control == Some(crate::types::RowControl::Molecule(MoleculeSignal::Records)));
            if is_leave_row {
                crate::logf!("MOLECULE: leave of {} acknowledged by {} — root zeroized", hex::encode(&gid.0[..4]), crate::fp(&party));
                self.zeroize_molecule_root(gid);
                return false;
            }
        }
        crate::logf!("MOLECULE: ACK from {} for {} in {} — {}", crate::fp(&party), acked_eagle_time, hex::encode(&gid.0[..4]), match progress { Some((a, n)) => format!("{} of {}", a, n), None => "complete".to_string() });
        self.persist_chains_async(&fid);
        if cut_over {
            self.after_molecule_cutover(gid);
        }
        // The delivery ring: this member holds the row now, whatever the rest still owe.
        let mut changed = false;
        let mut ringed: Option<crate::types::ChatMessage> = None;
        if let Some(m) = self.conversations.iter_mut().find(|v| v.id() == fid).and_then(|v| v.messages.iter_mut().find(|m| m.is_outgoing && m.timestamp == acked_eagle_time)) {
            if crate::types::merge_delivered_to(&mut m.delivered_to, &[party]) {
                changed = true;
                ringed = Some(m.clone());
            }
        }
        // Our other devices learn who holds it too (their rings fill the same way).
        if let Some(row) = ringed {
            self.push_conversation_rows_to_siblings(fid, std::slice::from_ref(&row), None);
        }
        if done.is_empty() {
            if changed {
                self.persist_conversation_async(fid);
                self.scene_dirty = true;
            }
            return changed;
        }
        if let Some(cp) = self.conversations.iter().position(|v| v.id() == fid) {
            for m in self.conversations[cp].messages.iter_mut() {
                if m.is_outgoing && !m.delivered && done.contains(&m.timestamp) {
                    m.delivered = true;
                    changed = true;
                }
            }
        }
        if changed {
            self.persist_conversation_async(fid);
        }
        changed
    }

    /// Run the deferred wire half of every queued group post — chat rows and control rows alike — one tick after they were queued. A post that cannot go yet (no root, nobody reachable, the encrypt gate busy) stays queued; the retransmit sweep and the next edge retry it. Returns whether anything went out.
    pub(super) fn drain_molecule_posts(&mut self) -> bool {
        if self.pending_molecule_posts.is_empty() {
            return false;
        }
        let serial = self.tick_serial;
        // WHY/PROOF: a PARKED post is queued at u64::MAX, our own sentinel — a plain `+ 1` wraps it to 0, which is below every serial, and the parked post would fire.
        let (ready, keep): (Vec<MoleculePost>, Vec<MoleculePost>) = std::mem::take(&mut self.pending_molecule_posts).into_iter().partition(|p| p.queued.saturating_add(1) < serial); // u64 tick serials — a PARKED post (u64::MAX) waits for its edge
        self.pending_molecule_posts = keep;
        if ready.is_empty() {
            return false;
        }
        let Some(our_pid) = self.own_party_id() else {
            self.pending_molecule_posts.extend(ready);
            return false;
        };
        let mut sent_any = false;
        for post in ready {
            let gid = post.gid;
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let (text, ts, reference, control) = match post.text.as_ref() {
                // A HELD control row re-sends as the same control row: dropping its kind sent a wrap as an empty chat row, and the era it carried never reached anyone.
                Some((t, ts, r)) => (t.clone(), *ts, *r, post.control.then(|| crate::types::RowControl::Molecule(post.signal))),
                None => {
                    // A control row: it lands in the group conversation as a hidden outgoing row FIRST (re-ACK durability, the era-row pattern), then rides the wire.
                    let ts = crate::network::time_base::stamp_osc();
                    let control = crate::types::RowControl::Molecule(post.signal);
                    let mut row = crate::types::ChatMessage::control(control.clone(), true, ts);
                    row.author = Some(our_pid);
                    if let Some(conv) = self.conversations.iter_mut().find(|v| v.id() == fid) {
                        conv.insert_message_sorted(row);
                    }
                    (String::new(), ts, None, Some(control))
                }
            };
            let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: post.blob.clone(), wrap: post.wrap.clone(), deliver: None, answer: None };
            if self.molecule_transmit(gid, &text, ts, reference, post.kem.as_ref(), Some(&wire), control.as_ref()) {
                sent_any = true;
                if post.text.is_none() {
                    self.persist_conversation_async(fid);
                }
                // Our own row exists only on this device until a sibling hears of it — the group's token carries it (docs/molecules.md step 6).
                if let Some(row) = self.conversations.iter().find(|v| v.id() == fid).and_then(|v| v.messages.iter().find(|m| m.is_outgoing && m.timestamp == ts).cloned()) {
                    self.push_conversation_rows_to_siblings(crate::types::FriendshipId::from_bytes(gid.0), std::slice::from_ref(&row), None);
                }
            } else {
                // Held: keep the post (with its stamp, so a control row re-sends as the SAME row).
                let mut held = post;
                if held.text.is_none() {
                    held.text = Some((text, ts, None));
                    held.control = true;
                }
                // A group we cannot send in yet (Joining, no root) or any more (Left) PARKS the post until the edge that changes that (`unpark_molecule_posts`): retrying it every tick was a busy loop. A left group whose root is already gone drops it — nothing will ever carry it.
                let phase = self.molecule_locals.iter().find(|(g, _)| *g == gid).map(|(_, l)| l.phase).unwrap_or_default();
                let keyed = self.friendship_chains.iter().any(|(id, c)| *id == fid && c.lane_capable() && !c.awaiting_root());
                if phase == MoleculePhase::Left && !keyed {
                    crate::logf!("MOLECULE: dropped a held post in {} — we left and hold no root", hex::encode(&gid.0[..4]));
                    continue;
                }
                // A post whose every member waits for a pending era (never a wrap — those are never held) parks for its cutover too.
                let minting = held.signal != MoleculeSignal::Wrap && self.friendship_chains.iter().any(|(id, c)| *id == fid && c.pending_era().is_some());
                held.queued = if phase == MoleculePhase::Joining || !keyed || minting { u64::MAX } else { serial };
                self.pending_molecule_posts.push(held);
            }
        }
        sent_any
    }

    /// LEAVE (§10.1 Standing → Left, D9): post our signed leave record into the group and go read-only at once — the token drops from the admission list, compose closes, the row greys. The root is zeroized when the leave row's first ACK proves a survivor holds it (`on_molecule_ack`), so a lost frame cannot strand the others without the news; alone, at once.
    pub(super) fn leave_molecule(&mut self, gid: MoleculeId) -> bool {
        let (Some(our_pid), Some(seed)) = (self.own_party_id(), self.device_seed()) else {
            return false;
        };
        let Some(pos) = self.molecule_rosters.iter().position(|(g, _)| *g == gid) else {
            return false;
        };
        let leave = crate::types::molecule::LeaveRecord { party: our_pid, signed_osc: vsf::eagle_time_oscillations(), signature: [0u8; 64], signer_device: crate::types::molecule::device_pubkey(&seed) };
        let mut leave = leave;
        leave.signature = crate::types::molecule::sign_record(&leave.signing_bytes(), &seed);
        let alone = self.molecule_rosters[pos].1.standing().iter().all(|p| *p == our_pid);
        let mut posted = Roster::default();
        posted.merge_leave(leave.clone());
        let blob = crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok();
        self.molecule_rosters[pos].1.merge_leave(leave);
        self.sync_molecule_conversation(&gid);
        self.persist_molecule(&gid);
        self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Left);
        self.clear_group_invites(gid);
        // Our other devices hear the leave directly, not only by the group's chains syncing: those chains die with the root, and a group we were alone in never syncs at all (Nick 2026-10-25: leaving did not reach the whole fleet).
        if let Some(b) = blob.as_ref() {
            self.share_molecule_records_with_fleet(gid, b.clone());
        }
        if alone {
            self.zeroize_molecule_root(gid);
            crate::logf!("MOLECULE: left {} — alone, root zeroized at once", hex::encode(&gid.0[..4]));
        } else {
            // The leave row is the LAST frame on our lane; the root dies on its first ACK.
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob, wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
            crate::logf!("MOLECULE: leaving {} — leave record posted; the root dies on its first ACK", hex::encode(&gid.0[..4]));
        }
        self.reseed_molecule_pubkeys();
        true
    }

    /// Drop the group's chains blob (root, lanes, bundles) — the leaver's local zeroize. The conversation stays, read-only.
    pub(super) fn zeroize_molecule_root(&mut self, gid: MoleculeId) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        self.friendship_chains.retain(|(id, _)| *id != fid);
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::friendship::delete_friendship_chains(&fid, storage) {
                crate::logf!("MOLECULE: chains delete failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
        }
    }

    /// Our own group records signed by a device that has since left our fleet, signed again here with the same content and posted into the group.
    /// A root conversion moves a device to a new key (the desktop's 90e571bf became 469e79b8, 2026-11): our own devices still take a record its old key signed, as signed in the fleet of its day, but a friend judges every record against our CURRENT fold and refuses it ("signed by a device outside its subject's fold"). Jon, invited to a group, saw its members but no content, and his acceptance never showed here: our vouches for three members were the old key's (2026-11-16).
    /// Newest wins everywhere, so the re-signed record replaces the old one for every member; our own devices converge, since once one has re-signed the signer is a member and the rest find nothing to do.
    /// A bundle is not re-signed: it is a device's own era keys, and the departed device's bundle is rightly unused.
    pub(super) fn resign_departed_own_records(&mut self) {
        let (Some(us), Some(seed)) = (self.own_party_id(), self.device_seed()) else {
            return;
        };
        let fold = self.fleet_members_held();
        // No held chain, no judgement: a record is never re-signed on a fold we do not hold.
        if fold.is_empty() {
            return;
        }
        let departed = |d: &[u8; 32]| !fold.contains(d);
        let gids: Vec<MoleculeId> = self.molecule_rosters.iter().map(|(g, _)| *g).collect();
        for gid in gids {
            let Some(pos) = self.molecule_rosters.iter().position(|(g, _)| *g == gid) else {
                continue;
            };
            let roster = &self.molecule_rosters[pos].1;
            let mut posted = Roster::default();
            // The genesis in place while the records merge (the founder's self-vouch is taken only against it), out again before the post, as a rename does.
            posted.genesis = roster.genesis.clone();
            let mut count = 0usize;
            if let Some(m) = roster.members.get(&us).filter(|m| departed(&m.signer_device)) {
                posted.merge_member(crate::types::molecule::join_record(us, m.handle_proof, &m.name, m.avatar_pin, m.sponsor, &seed));
                count += 1;
            }
            for ((voucher, subject), recs) in roster.vouches.iter() {
                // The list is sorted by its signed moment: the last is the vouch that stands.
                if let Some(v) = recs.last().filter(|v| *voucher == us && departed(&v.signer_device)) {
                    posted.merge_vouch(crate::types::molecule::vouch_record(us, *subject, v.withdrawn, &seed));
                    count += 1;
                }
            }
            if let Some(t) = roster.title.as_ref().filter(|t| t.party == us && departed(&t.signer_device)) {
                posted.merge_title(crate::types::molecule::title_record(us, &t.title, &seed));
                count += 1;
            }
            if let Some(a) = roster.avatar.as_ref().filter(|a| a.party == us && departed(&a.signer_device)) {
                posted.merge_avatar(crate::types::molecule::avatar_record(us, a.image, &seed));
                count += 1;
            }
            posted.genesis = None;
            if count == 0 {
                continue;
            }
            {
                // The founder's self-vouch merges only against the genesis, so the held roster takes the records with its own genesis in place.
                let roster = &mut self.molecule_rosters[pos].1;
                for m in posted.members.values() {
                    roster.merge_member(m.clone());
                }
                for recs in posted.vouches.values() {
                    for v in recs {
                        roster.merge_vouch(v.clone());
                    }
                }
                if let Some(t) = posted.title.clone() {
                    roster.merge_title(t);
                }
                if let Some(a) = posted.avatar.clone() {
                    roster.merge_avatar(a);
                }
            }
            self.persist_molecule(&gid);
            let blob = crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok();
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob, wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
            crate::logf!("MOLECULE: {} — {} of our record(s) signed by a device that left our fleet, signed again here and posted", hex::encode(&gid.0[..4]), count);
        }
    }

    /// RENAME (D7): a title record signed by us, merged here and posted into the group; newest wins everywhere.
    pub(super) fn rename_molecule(&mut self, gid: MoleculeId, title: &str) -> bool {
        // A name is byte-precise (Nick 2026-10-27: "could be a line return, tab, space, something"): never trimmed, only an empty one refused.
        if title.is_empty() {
            return false;
        }
        let (Some(our_pid), Some(seed)) = (self.own_party_id(), self.device_seed()) else {
            return false;
        };
        let Some(pos) = self.molecule_rosters.iter().position(|(g, _)| *g == gid) else {
            return false;
        };
        let rec = crate::types::molecule::title_record(our_pid, title, &seed);
        let mut posted = Roster::default();
        posted.genesis = self.molecule_rosters[pos].1.genesis.clone();
        posted.merge_title(rec.clone());
        posted.genesis = None;
        if !self.molecule_rosters[pos].1.merge_title(rec) {
            return false;
        }
        self.persist_molecule(&gid);
        let blob = crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok();
        self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob, wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        crate::logf!("MOLECULE: {} renamed \"{}\"", hex::encode(&gid.0[..4]), title);
        true
    }

    /// MUTE (D12): this device only — never in the roster.
    pub(super) fn toggle_molecule_mute(&mut self, gid: MoleculeId) {
        self.set_molecule_local(&gid, |l| l.muted = !l.muted);
    }

    /// Is this group muted on this device?
    pub(super) fn molecule_muted(&self, gid: &MoleculeId) -> bool {
        self.molecule_locals.iter().find(|(g, _)| g == gid).map_or(false, |(_, l)| l.muted)
    }

    /// After a GROUP cutover (docs/molecules.md §10.4), on either side: the pendings the cutover cleared re-serve on the fresh lane, a fresh KEM bundle publishes for the new era (the next mint wraps to it), and a Catching-up device settles from what it still holds.
    pub(super) fn after_molecule_cutover(&mut self, gid: MoleculeId) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        // Catching up settles from what is still held: a frame on the era just installed no longer waits.
        self.settle_catching_up(&[gid]);
        // The joiners hold the era we are on now: their rows go out from here (the held flush below reaches them on the fresh lane).
        self.molecule_era_joiners.retain(|(g, _)| *g != gid);
        // Fresh bundle for the new era.
        if let (Some(our_pid), Some(seed)) = (self.own_party_id(), self.device_seed()) {
            let era = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c.era_index).unwrap_or(0);
            let eph = crate::crypto::era::era_keygen(era, 0, crate::crypto::era::KEM_SET_DEFAULT);
            let bundle = crate::types::molecule::bundle_record(our_pid, era, &eph, &seed);
            if let Some((_, c)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                c.push_molecule_kem(eph.export_decaps(era));
            }
            if let Some(pos) = self.molecule_rosters.iter().position(|(g, _)| *g == gid) {
                self.molecule_rosters[pos].1.merge_bundle(bundle.clone());
                self.persist_molecule(&gid);
            }
            let mut posted = Roster::default();
            posted.merge_bundle(bundle);
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok(), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        }
        self.persist_chains_of(&fid);
        self.unpark_molecule_posts(gid);
        self.resend_held_molecule_rows(gid);
        if self.molecule_mint_after_cutover.remove(&gid) {
            crate::logf!("MOLECULE: {} cut over — the mint that waited on it runs now", hex::encode(&gid.0[..4]));
            self.mint_molecule_era(gid, None);
        }
        // A cutover may finish a group's lockout re-key: the next group's may go (era.rs).
        self.lockout_rekey_step("a group cut over");
    }

    /// Re-queue every undelivered outgoing chat row of a group (a cutover cleared its pendings; a relaunch found them held) — control rows never re-serve bare.
    pub(super) fn resend_held_molecule_rows(&mut self, gid: MoleculeId) {
        self.resend_held_rows(crate::types::FriendshipId::from_bytes(gid.0));
    }

    /// Persist a group's index entry + roster. Chains persist thru the standard chains path on their own mutation edges.
    pub(super) fn persist_molecule(&mut self, gid: &MoleculeId) {
        // Every roster change passes here: its picture may have changed, so the sweep fetches it and the next tick reads it (group_avatar.rs).
        self.group_faces_dirty = true;
        // …and an invitee may have come to stand in it, which ends that invite on this device (each of our devices ends it on its own roster edge).
        self.clear_stood_invites(gid);
        self.replicate_dirty = true;
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if let Err(e) = crate::storage::molecule::index_molecule(gid, storage) {
            crate::logf!("MOLECULE: index write failed for {}: {}", hex::encode(&gid.0[..4]), e);
        }
        if let Some((_, roster)) = self.molecule_rosters.iter().find(|(id, _)| id == gid) {
            if let Err(e) = crate::storage::molecule::save_roster(gid, roster, storage) {
                crate::logf!("MOLECULE: roster write failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
            // The roster rides the chains blob to our siblings (step 6): stamp it, and the replication pass pushes on its mutated_osc.
            if let Ok(bytes) = crate::storage::molecule::roster_to_vsf_bytes(gid, roster) {
                let fid = crate::types::FriendshipId::from_bytes(gid.0);
                if let Some((_, c)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                    c.set_molecule_roster(bytes);
                }
            }
        }
    }

    /// OUR FLEET HOLDS THIS GROUP (a sibling's replication carried its roster): the invitation is answered on every device, never bound twice, and the group is Standing wherever its keys have landed — Joining until then (Nick 2026-10-24: a bind showed on one device only, and a second device's own Bind minted the group an extra era).
    pub(super) fn fleet_holds_molecule(&mut self, gid: MoleculeId) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        // OUR LEAVE, made on a sibling: the roster carries our leave record and we no longer stand, so this device leaves too (Nick 2026-10-24: a leave stayed on the device it was made on).
        let us = self.own_party_id();
        let we_left = us.is_some_and(|us| self.molecule_rosters.iter().any(|(g, r)| *g == gid && r.leaves.contains_key(&us) && !r.is_standing(&us)));
        if we_left {
            if !self.molecule_is_left(&gid) {
                self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Left);
                self.zeroize_molecule_root(gid);
                self.clear_group_invites(gid);
                self.pending_molecule_posts.retain(|p| p.gid != gid);
                self.reseed_molecule_pubkeys();
                self.scene_dirty = true;
                crate::logf!("MOLECULE: our fleet left {} on another device — left here too", hex::encode(&gid.0[..4]));
            }
            return;
        }
        let keyed = self.friendship_chains.iter().any(|(id, c)| *id == fid && !c.awaiting_root());
        if keyed {
            self.unpark_molecule_posts(gid);
        }
        let phase = self.molecule_locals.iter().find(|(g, _)| *g == gid).map(|(_, l)| l.phase);
        match phase {
            None | Some(MoleculePhase::Joining) if keyed => self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Standing),
            None => self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Joining),
            _ => {}
        }
        if !self.conversations.iter().any(|v| v.id() == fid) {
            let standing = self.molecule_rosters.iter().find(|(g, _)| *g == gid).map(|(_, r)| r.standing()).unwrap_or_default();
            self.conversations.push(crate::types::Conversation::new_molecule(gid, standing));
        }
        if let Some(o) = self.bond_offers.iter_mut().find(|o| o.molecule_id == gid && !o.accepted) {
            o.accepted = true;
            if let Some(storage) = self.storage.as_ref() {
                crate::log_err!(crate::storage::molecule::save_bond_offer(o, storage), "save an accepted bond offer");
            }
            self.scene_dirty = true;
        }
    }

    /// A SIBLING's replicated group blob landed (docs/molecules.md step 6): merge the roster it carries; if this device never held the group, boot it — index, conversation, local state — so a phone that slept thru a join wakes holding the group. Returns whether the roster changed.
    pub(super) fn adopt_replicated_molecule(&mut self, gid: MoleculeId, roster_bytes: &[u8]) -> bool {
        if roster_bytes.is_empty() {
            return false;
        }
        let (bid, snapshot) = match crate::storage::molecule::roster_from_vsf_bytes(roster_bytes) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("MOLECULE: sibling roster for {} failed verified read: {}", hex::encode(&gid.0[..4]), e);
                return false;
            }
        };
        if bid != gid {
            return false;
        }
        let pos = match self.molecule_rosters.iter().position(|(id, _)| *id == gid) {
            Some(p) => p,
            None => {
                let Some(_) = Self::verify_snapshot_birth(&gid, &snapshot) else {
                    crate::logf!("MOLECULE: sibling roster for {} carries no verifiable genesis — refused", hex::encode(&gid.0[..4]));
                    return false;
                };
                crate::logf!("MOLECULE: booted \"{}\" ({}) from a sibling's replication", snapshot.title(), hex::encode(&gid.0[..4]));
                self.molecule_rosters.push((gid, Roster { genesis: snapshot.genesis.clone(), ..Default::default() }));
                let fid = crate::types::FriendshipId::from_bytes(gid.0);
                if !self.conversations.iter().any(|v| v.id() == fid) {
                    self.conversations.push(crate::types::Conversation::new_molecule(gid, std::iter::empty()));
                }
                self.molecule_rosters.len() - 1
            }
        };
        let mut changed = self.merge_snapshot_into(pos, snapshot, true);
        changed |= self.sync_molecule_conversation(&gid);
        self.fleet_holds_molecule(gid);
        if changed {
            self.persist_molecule(&gid);
        } else if let Some(storage) = self.storage.as_ref() {
            crate::log_err!(crate::storage::molecule::index_molecule(&gid, storage), "index a molecule in the vault");
        }
        changed
    }
}

#[cfg(test)]
mod fold_tests {
    use super::unfolded_member_proofs;
    use crate::types::molecule::MoleculePeer;

    /// The 2026-11-11 group from Joshua's side: Nick is a friend (folds as a contact), Jeff is a member Joshua never friended and holds no fold for after a restart, and Joshua is us; only Jeff is folded, once, however many groups name him.
    #[test]
    fn only_members_with_no_fold_are_asked_for() {
        let (nick, jeff, joshua) = ([0xA0u8; 32], [0xB0u8; 32], [0xC0u8; 32]);
        let named = [nick, jeff, joshua, jeff];
        assert_eq!(unfolded_member_proofs(&named, Some(joshua), &[nick], &[]), vec![jeff]);
        // Once Jeff's fold names a device, nothing is asked again.
        let folded = MoleculePeer { party: [0xB1; 32], handle_proof: jeff, devices: vec![[0xE3; 32]], fold_ts: 1 };
        assert!(unfolded_member_proofs(&named, Some(joshua), &[nick], &[folded]).is_empty());
        // A peer whose chain vanished holds no devices: it is asked for again.
        let emptied = MoleculePeer { party: [0xB1; 32], handle_proof: jeff, devices: Vec::new(), fold_ts: 1 };
        assert_eq!(unfolded_member_proofs(&named, Some(joshua), &[nick], &[emptied]), vec![jeff]);
    }
}

#[cfg(test)]
mod join_answer_tests {
    use super::{join_already_answered, mint_secrets, molecule_next_era, pending_stands};
    use crate::types::molecule::{found_atom, join_record, vouch_record, BundleRecord, MoleculeId, Roster};

    fn era_n() -> (MoleculeId, [u8; 32], [u8; 32]) {
        (MoleculeId([0x42; 32]), [0x5A; 32], [0xC3; 32])
    }

    /// The sponsor's desktop and laptop each receive the same Join in era 4 and each answer it: both mint byte-identical era 5 roots and history keys, and a member opening either device's wrap derives that same era.
    #[test]
    fn two_devices_answering_one_join_mint_one_era() {
        let (gid, root, hk) = era_n();
        let (nonce_a, fresh_a) = mint_secrets(true, &root, &hk);
        let (nonce_b, fresh_b) = mint_secrets(true, &root, &hk);
        assert_eq!((nonce_a, fresh_a), (nonce_b, fresh_b));
        let desktop = molecule_next_era(&gid, 5, &root, &hk, &nonce_a, &fresh_a);
        let laptop = molecule_next_era(&gid, 5, &root, &hk, &nonce_b, &fresh_b);
        assert_eq!(desktop, laptop);
        // A member holding the desktop's pending era meets the laptop's wrap: the same root, so nothing changes.
        assert!(pending_stands(Some((5, desktop.0)), 5, &laptop.0));
    }

    /// A person's two devices each send a Join in era 4, answered by two different devices of ours: the join path takes no input from the Join, so both answers hand out the one era 5, and a device already holding that era pending hands the same one over.
    #[test]
    fn two_joins_in_one_era_get_one_era() {
        let (gid, root, hk) = era_n();
        let answer = || {
            let (nonce, fresh) = mint_secrets(true, &root, &hk);
            molecule_next_era(&gid, 5, &root, &hk, &nonce, &fresh)
        };
        let first_join = answer();
        let second_join = answer();
        assert_eq!(first_join, second_join);
        // The era after a cutover is another base, so a join answered there mints another era: convergence is per era, never one fixed secret.
        let (n6, f6) = mint_secrets(true, &first_join.0, &first_join.1);
        assert_ne!(molecule_next_era(&gid, 6, &first_join.0, &first_join.1, &n6, &f6), first_join);
    }

    /// A leave, a lost standing, a lockout and the cadence mint stay random: two such mints from one era differ, and neither is the join era.
    #[test]
    fn a_leave_mint_stays_random() {
        let (gid, root, hk) = era_n();
        let (n0, f0) = mint_secrets(false, &root, &hk);
        let (n1, f1) = mint_secrets(false, &root, &hk);
        assert_ne!((n0, f0), (n1, f1));
        let join = mint_secrets(true, &root, &hk);
        assert_ne!((n0, f0), join);
        assert_ne!(molecule_next_era(&gid, 5, &root, &hk, &n0, &f0), molecule_next_era(&gid, 5, &root, &hk, &n1, &f1));
    }

    /// Two mints of one index: the lower root stands whichever wrap lands first, so every device keeps the same pending era; another index is never held against.
    #[test]
    fn the_lower_pending_root_stands() {
        let (low, high) = ([0x10u8; 32], [0xF0u8; 32]);
        assert!(pending_stands(Some((5, low)), 5, &high));
        assert!(!pending_stands(Some((5, high)), 5, &low));
        assert!(!pending_stands(Some((4, low)), 5, &high));
        assert!(!pending_stands(None, 5, &high));
    }

    /// A Join one of our devices already answered (its bundle came back in that device's records, the joiner standing) is not answered again; the same person's fresh Join, with a fresh bundle, is.
    #[test]
    fn an_answered_join_is_answered_once() {
        let (founder, founder_seed) = ([0xF1u8; 32], [0x07u8; 32]);
        let birth = found_atom(founder, [0xF2; 32], "f", [0u8; 32], "t", false, &founder_seed);
        let mut roster = Roster { genesis: Some(birth.genesis.clone()), ..Roster::default() };
        roster.merge_member(birth.founder_member.clone());
        roster.merge_vouch(birth.founder_vouch.clone());
        let joiner = [0x0Au8; 32];
        let bundle = BundleRecord { party: joiner, device: [0x0D; 32], published_era: 0, kem_set: 0, mlkem_pk: Vec::new(), x_pk: Vec::new(), hqc_pk: Vec::new(), signed_osc: 1 << 8, signature: [0u8; 64] };
        assert!(!join_already_answered(&roster, &joiner, std::slice::from_ref(&bundle)), "nobody answered yet");
        // One of our devices answered: its records carry the joiner's member record, our vouch and the bundle.
        roster.merge_member(join_record(joiner, [0x0B; 32], "j", [0u8; 32], founder, &[0x09; 32]));
        roster.merge_bundle(bundle.clone());
        roster.merge_vouch(vouch_record(founder, joiner, false, &founder_seed));
        assert!(roster.is_standing(&joiner));
        assert!(join_already_answered(&roster, &joiner, std::slice::from_ref(&bundle)), "the same Join arriving again is answered already");
        let fresh = BundleRecord { signed_osc: 1 << 9, ..bundle.clone() };
        assert!(!join_already_answered(&roster, &joiner, &[fresh]), "a re-sent Join carries a fresh bundle and is answered");
        let other_device = BundleRecord { device: [0x0E; 32], ..bundle };
        assert!(!join_already_answered(&roster, &joiner, &[other_device]), "the person's other device joining is answered");
    }
}

#[cfg(test)]
mod fleet_invite_tests {
    use super::{invite_of_row, invite_row, invites_due};
    use crate::network::history_pages::{open_history_page, seal_history_page, HistoryPagePlain};
    use crate::storage::molecule::{park_offer_into, BondOffer, MoleculeLocal, MoleculePhase};
    use crate::types::molecule::{found_atom, join_record, vouch_record, MoleculeId, Roster};

    /// A group with its founder standing (us) and nobody else yet.
    fn group() -> (MoleculeId, Roster, [u8; 32], [u8; 32]) {
        let (founder, seed) = ([0xF1u8; 32], [0x07u8; 32]);
        let birth = found_atom(founder, [0xF2; 32], "f", [0u8; 32], "t", false, &seed);
        let mut roster = Roster { genesis: Some(birth.genesis.clone()), ..Roster::default() };
        roster.merge_member(birth.founder_member.clone());
        roster.merge_vouch(birth.founder_vouch.clone());
        (birth.molecule_id, roster, founder, seed)
    }

    /// The invitee joins: their member record and our vouch land in the roster.
    fn stand(roster: &mut Roster, founder: [u8; 32], seed: &[u8; 32], invitee: [u8; 32]) {
        roster.merge_member(join_record(invitee, [0x0B; 32], "j", [0u8; 32], founder, &[0x09; 32]));
        roster.merge_vouch(vouch_record(founder, invitee, false, seed));
        assert!(roster.is_standing(&invitee));
    }

    /// One device's rows as a sibling page, sealed and opened again: what the other device's drain reads.
    fn over_the_fleet(rows: Vec<crate::network::history_pages::HistoryRow>) -> Vec<(MoleculeId, i64, bool)> {
        let key = [0x77u8; 32];
        let page = HistoryPagePlain { oldest_osc: rows.iter().map(|r| r.timestamp).min().unwrap(), more: false, rows };
        let opened = open_history_page(&seal_history_page(&page, &key).expect("seal"), &key).expect("open");
        opened.rows.iter().filter_map(invite_of_row).collect()
    }

    /// The MacBook's invite reaches the desktop: the row it hands its siblings carries the group, the offer's stamp and its state, and the desktop's ledger takes it.
    #[test]
    fn an_invite_replicates() {
        let (gid, _, _, _) = group();
        let jon = [0x0Au8; 32];
        let mut macbook = MoleculeLocal::default();
        macbook.sent_invite(jon, 1 << 20);
        let landed = over_the_fleet(vec![invite_row(&gid, 1 << 20, false).expect("row")]);
        assert_eq!(landed, vec![(gid, 1 << 20, false)]);
        let mut desktop = MoleculeLocal::default();
        for (_, osc, ended) in &landed {
            assert!(!ended);
            assert!(desktop.note_invite(jon, *osc));
        }
        assert_eq!(desktop.offered, macbook.offered);
        // The same record again (the next sibling-online reshare) changes nothing.
        assert!(!desktop.note_invite(jon, 1 << 20));
        // An incoming Offer row (an invitation we received, handed over by a sibling) is never read as one of ours.
        let mut received = invite_row(&gid, 1 << 20, false).expect("row");
        received.sender_outgoing = false;
        assert!(invite_of_row(&received).is_none());
    }

    /// The desktop never sent the invite, yet on the invitee's online edge it owes them the offer once the ledger holds it; a device that left, or an invitee standing, owes nothing.
    #[test]
    fn a_second_device_reoffers_on_the_online_edge() {
        let (gid, mut roster, founder, seed) = group();
        let jon = [0x0Au8; 32];
        let mut desktop = MoleculeLocal::default();
        let rosters = vec![(gid, roster.clone())];
        assert!(invites_due(&[(gid, desktop.clone())], &rosters, &jon).is_empty(), "nothing held, nothing owed");
        for (_, osc, _) in over_the_fleet(vec![invite_row(&gid, 1 << 20, false).expect("row")]) {
            desktop.note_invite(jon, osc);
        }
        assert_eq!(invites_due(&[(gid, desktop.clone())], &rosters, &jon), vec![gid]);
        assert!(invites_due(&[(gid, desktop.clone())], &rosters, &[0x0Fu8; 32]).is_empty(), "another friend coming online is owed nothing");
        let left = MoleculeLocal { phase: MoleculePhase::Left, ..desktop.clone() };
        assert!(invites_due(&[(gid, left)], &rosters, &jon).is_empty());
        stand(&mut roster, founder, &seed, jon);
        assert!(invites_due(&[(gid, desktop)], &[(gid, roster)], &jon).is_empty());
    }

    /// Jon stands: every device ends the invite from its own roster, and a sibling's late copy of that same offer does not bring it back; a newer offer from any of our devices does.
    #[test]
    fn an_invite_clears_on_stand() {
        let (gid, mut roster, founder, seed) = group();
        let (jon, emma) = ([0x0Au8; 32], [0x0Eu8; 32]);
        let mut desktop = MoleculeLocal::default();
        desktop.note_invite(jon, 1 << 20);
        desktop.note_invite(emma, 1 << 21);
        assert!(desktop.stood_invites(&roster).is_empty());
        stand(&mut roster, founder, &seed, jon);
        let stood = desktop.stood_invites(&roster);
        assert_eq!(stood, vec![(jon, 1 << 20)]);
        for (p, o) in stood {
            assert!(desktop.clear_invite(p, o));
        }
        assert_eq!(desktop.offered, vec![(emma, 1 << 21)], "only the invitee who stood is done");
        assert!(!desktop.note_invite(jon, 1 << 20), "the laptop's late reshare of the same offer stays ended");
        // An undo on another device reaches this one as an ended row and ends Emma's invite too.
        for (_, osc, ended) in over_the_fleet(vec![invite_row(&gid, 1 << 21, true).expect("row")]) {
            assert!(ended);
            assert!(desktop.clear_invite(emma, osc));
        }
        assert!(desktop.offered.is_empty());
        assert!(!desktop.clear_invite(emma, 1 << 21), "the same end again changes nothing");
        // An end of an older offer never ends a newer one.
        desktop.note_invite(emma, 1 << 22);
        assert!(!desktop.clear_invite(emma, 1 << 21));
        assert_eq!(desktop.offered, vec![(emma, 1 << 22)]);
    }

    /// The joiner's side: the sponsor's laptop and desktop both re-offer on one online edge, in either order. One invitation is parked and rings once, the newest row stands (the one an undo deletes), and the older copy arriving last changes nothing.
    #[test]
    fn duplicate_offers_merge_harmlessly() {
        let (gid, roster, founder, _) = group();
        let offer = |row_osc: i64| BondOffer { molecule_id: gid, sponsor: founder, row_osc, snapshot: roster.clone(), accepted: false };
        let mut parked: Vec<BondOffer> = Vec::new();
        assert_eq!(park_offer_into(&mut parked, offer(1 << 20)), Some(true), "the first offer rings");
        assert_eq!(park_offer_into(&mut parked, offer(1 << 21)), Some(false), "a newer copy replaces it silently");
        assert_eq!(park_offer_into(&mut parked, offer(1 << 20)), None, "the older copy landing last changes nothing");
        assert_eq!(park_offer_into(&mut parked, offer(1 << 21)), None, "the same row again changes nothing");
        assert_eq!(parked.len(), 1);
        assert_eq!(parked[0].row_osc, 1 << 21);
        // Another group's offer parks beside it.
        let other = BondOffer { molecule_id: MoleculeId([0x55; 32]), ..offer(1 << 19) };
        assert_eq!(park_offer_into(&mut parked, other), Some(true));
        assert_eq!(parked.len(), 2);
    }
}
