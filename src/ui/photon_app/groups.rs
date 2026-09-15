//! Group flows (docs/groups.md §10): founding, the offer over a friendship, the explicit Join, the wrap that delivers an era, and roster record merges.
//! The receive half rides `commit_braid_rx`'s control-row ladder (a `GroupSignal` row defers here after the chains borrow ends, the era-signal discipline); the send half posts bare `GROUP_PREFIX` rows with roster blobs on the package's typed `gpl` field and sealed secrets on its typed wrap fields — the text is the row, and the row persists, so nothing secret ever rides the text.
//! Consent doctrine (§10 D8/D10): an OFFER carries a roster snapshot and no secret; the invitee's JOIN sends its member record + KEM bundle back; the sponsor vouches, posts the records into the group, and answers with a WRAP sealed to that bundle. The joiner never holds an era it is not in. Nothing here auto-joins anyone.

use super::PhotonApp;
use crate::storage::group::{GroupLocal, GroupOffer, GroupPhase};
use crate::types::group::{GroupId, GroupSignal, Roster};

/// A group-side post waiting for the fan-out send (step 4): a `Records` or `Wrap` row to put on our lane inside the group. Queued so the flows below stay pure of the send path until it speaks group.
#[derive(Clone, Debug)]
pub(super) struct GroupPost {
    pub gid: GroupId,
    pub signal: GroupSignal,
    pub blob: Option<Vec<u8>>,
    pub wrap: Option<crate::network::message_package::GroupWrapWire>,
    pub kem: Option<crate::crypto::era::EraKemWire>,
    /// A chat row (text, stamp, reference) whose bubble already landed; `None` = a control row minted at drain time.
    pub text: Option<(String, i64, Option<(crate::types::RefKind, i64)>)>,
    /// The tick the post was queued on — the frame fence (a post drains one tick after its bubble rendered).
    pub queued: u64,
    /// A control row that was held after its row landed (so a retry re-sends the SAME row, never mints a second).
    pub control: bool,
}

impl PhotonApp {
    /// A `GroupSignal` control row landed (deferred from commit_braid_rx): `ci` = the sender's contact, `cp` = the conversation the row landed in, `wire` = the package's typed group extras.
    pub(super) fn on_group_signal(&mut self, ci: usize, cp: usize, sig: GroupSignal, wire: Option<crate::network::message_package::GroupWire>, kem: Option<crate::crypto::era::EraKemWire>, ts: i64) {
        let blob = wire.as_ref().and_then(|g| g.blob.clone());
        let in_group = self.conversations.get(cp).map_or(false, |c| c.is_group());
        match (sig, in_group) {
            (GroupSignal::Offer, false) => self.park_group_offer(ci, blob, ts),
            (GroupSignal::Join, false) => self.answer_group_join(ci, blob),
            (GroupSignal::Wrap, false) => self.install_join_wrap(ci, wire, kem),
            (GroupSignal::Wrap, true) => self.install_group_wrap(cp, wire, kem),
            (GroupSignal::Records, true) => self.merge_group_records(ci, cp, blob),
            (other, _) => {
                let sender = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
                crate::logf!("GROUP: {} row from {} landed {} a group conversation — ignored", other.to_content().trim_start_matches(crate::types::group::GROUP_PREFIX), sender, if in_group { "inside" } else { "outside" });
            }
        }
    }

    /// OFFER (§10.1 None → Offered): park the sponsor's snapshot beside the friendship row it rode in on. No secret arrives; the Join pill on that row is the consent. A re-offer for the same group replaces the parked snapshot (the sponsor's refresh); an offer for a group we already hold merges the snapshot as records.
    fn park_group_offer(&mut self, ci: usize, blob: Option<Vec<u8>>, row_osc: i64) {
        let sponsor_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let Some(sponsor) = self.contacts.get(ci).map(|c| c.handle_hash) else {
            return;
        };
        let Some(blob) = blob else {
            crate::logf!("GROUP: offer from {} without a roster snapshot — refused", sponsor_fp);
            return;
        };
        let (gid, snapshot) = match crate::storage::group::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("GROUP: offer snapshot from {} failed verified read: {} — refused", sponsor_fp, e);
                return;
            }
        };
        let Some(title) = Self::verify_snapshot_birth(&gid, &snapshot) else {
            crate::logf!("GROUP: offer from {} for {} carries no verifiable genesis — refused", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        };
        if !snapshot.is_standing(&sponsor) {
            crate::logf!("GROUP: offer from {} for {} — the sponsor is not standing in its own snapshot; refused", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        }
        if let Some(pos) = self.group_rosters.iter().position(|(id, _)| *id == gid) {
            let mut changed = self.merge_snapshot_into(pos, snapshot);
            changed |= self.sync_group_conversation(&gid);
            if changed {
                self.persist_group(&gid);
            }
            crate::logf!("GROUP: offer for held group {} from {} merged as records", hex::encode(&gid.0[..4]), sponsor_fp);
            return;
        }
        let members = snapshot.standing().len();
        let offer = GroupOffer { group_id: gid, sponsor, row_osc, snapshot, accepted: false };
        self.group_offers.retain(|o| o.group_id != gid);
        self.group_offers.push(offer.clone());
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::group::park_group_offer(&offer, storage) {
                crate::logf!("GROUP: offer park failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
        }
        crate::logf!("GROUP: offered \"{}\" ({}) by {} — {} standing — Join is the consent", title, hex::encode(&gid.0[..4]), sponsor_fp, members);
    }

    /// The genesis must be present, name this group, and verify under its own signer before anything persists. Returns the title suggestion.
    fn verify_snapshot_birth(gid: &GroupId, snapshot: &Roster) -> Option<String> {
        let g = snapshot.genesis.as_ref()?;
        if g.group_id != *gid || !crate::types::group::verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) {
            return None;
        }
        Some(snapshot.title())
    }

    /// JOIN (§10.1 Offered → Joining, the invitee's act): mint this device's KEM bundle, keep the decapsulation half in a chains blob that holds NO root yet, send our member record + bundle back over the friendship, and wait for the sponsor's wrap. Returns whether the Join went out.
    pub(super) fn join_group_offer(&mut self, gid: GroupId) -> bool {
        let Some(offer) = self.group_offers.iter().find(|o| o.group_id == gid).cloned() else {
            crate::logf!("GROUP: join of {} — no parked offer", hex::encode(&gid.0[..4]));
            return false;
        };
        if offer.accepted && self.group_rosters.iter().any(|(g, _)| *g == gid) {
            crate::logf!("GROUP: join of {} — already joined", hex::encode(&gid.0[..4]));
            return false;
        }
        let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == offer.sponsor && !c.is_sibling) else {
            crate::logf!("GROUP: join of {} — the sponsor is no longer a contact; offer expired", hex::encode(&gid.0[..4]));
            return false;
        };
        let (Some(our_pid), Some(seed), Some(hp)) = (self.our_party_id(&self.contacts[ci]), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) else {
            return false;
        };
        let (name, avatar) = self.own_name_grant();
        // The bundle's published_era is informational (the minter always wraps to a device's newest bundle); an invitee knows no era yet.
        let era = 0u64;
        let eph = crate::crypto::era::era_keygen(era, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let bundle = crate::types::group::bundle_record(our_pid, era, &eph, &seed);
        let member = crate::types::group::join_record(our_pid, hp, &name, avatar, offer.sponsor, &seed);
        // A rootless chains blob holds the decapsulation keys until the wrap arrives; from_group_root with the wrap's secrets replaces it, carrying the kems across.
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let mut holder = crate::types::FriendshipChains::from_group_root(gid, &offer.snapshot.standing(), [0u8; 32], [0u8; 32], era, offer.snapshot.genesis.as_ref().map_or([0u8; 32], |g| g.group_id.0));
        holder.push_group_kem(eph.export_decaps(era));
        self.friendship_chains.retain(|(id, _)| *id != fid);
        self.friendship_chains.push((fid, holder));
        let mut ours = Roster::default();
        ours.merge_member(member);
        ours.merge_bundle(bundle);
        let blob = match crate::storage::group::roster_to_vsf_bytes(&gid, &ours) {
            Ok(b) => b,
            Err(e) => {
                crate::logf!("GROUP: join records for {} failed to encode: {}", hex::encode(&gid.0[..4]), e);
                return false;
            }
        };
        let wire = crate::network::message_package::GroupWire { from: our_pid, woven_authors: Vec::new(), blob: Some(blob), wrap: None };
        let ts = vsf::eagle_time_oscillations();
        let sent = self.chain_transmit_with(ci, &GroupSignal::Join.to_content(), ts, None, None, None, Some(&wire));
        if sent {
            self.set_group_local(&gid, |l| l.phase = GroupPhase::Joining);
            self.persist_chains_of(&fid);
            if let Some(o) = self.group_offers.iter_mut().find(|o| o.group_id == gid) {
                o.accepted = true;
                if let Some(storage) = self.storage.as_ref() {
                    let _ = crate::storage::group::save_group_offer(o, storage);
                }
            }
        }
        crate::logf!("GROUP: join of {} {} to {} — waiting for the wrap", hex::encode(&gid.0[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" }, crate::fp(&self.contacts[ci].handle_proof));
        sent
    }

    /// JOIN (the sponsor's side): verify the invitee's records, vouch for them, post everything into the group, and answer with a wrap of the current era (from-genesis) or of a freshly minted next era (from-join, D5 — the joiner never reads a row from before it stood).
    fn answer_group_join(&mut self, ci: usize, blob: Option<Vec<u8>>) {
        let joiner_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let Some(joiner) = self.contacts.get(ci).map(|c| c.handle_hash) else {
            return;
        };
        let Some(blob) = blob else {
            crate::logf!("GROUP: join from {} without records — ignored", joiner_fp);
            return;
        };
        let (gid, records) = match crate::storage::group::roster_from_vsf_bytes(&blob) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("GROUP: join records from {} failed verified read: {} — ignored", joiner_fp, e);
                return;
            }
        };
        let Some(pos) = self.group_rosters.iter().position(|(id, _)| *id == gid) else {
            crate::logf!("GROUP: join from {} for {} — we do not hold that group; ignored", joiner_fp, hex::encode(&gid.0[..4]));
            return;
        };
        let Some(member) = records.members.get(&joiner).cloned() else {
            crate::logf!("GROUP: join from {} carries no member record for the sender — ignored", joiner_fp);
            return;
        };
        if !self.contacts[ci].knows_device(&member.signer_device) || !crate::types::group::verify_record(&member.signing_bytes(), &member.signature, &member.signer_device) {
            crate::logf!("GROUP: join from {} — member record not signed by one of their folded devices; ignored", joiner_fp);
            return;
        }
        let bundles: Vec<crate::types::group::BundleRecord> = records
            .bundles
            .values()
            .filter(|b| b.party == joiner && self.contacts[ci].knows_device(&b.device) && crate::types::group::verify_record(&b.signing_bytes(), &b.signature, &b.device))
            .cloned()
            .collect();
        if bundles.is_empty() {
            crate::logf!("GROUP: join from {} carries no verifiable bundle — ignored", joiner_fp);
            return;
        }
        let (Some(our_pid), Some(seed)) = (self.our_party_id(&self.contacts[ci]), self.device_seed()) else {
            return;
        };
        if !self.group_rosters[pos].1.is_standing(&our_pid) {
            crate::logf!("GROUP: join from {} for {} — we are not standing; cannot vouch", joiner_fp, hex::encode(&gid.0[..4]));
            return;
        }
        let vouch = crate::types::group::vouch_record(our_pid, joiner, false, &seed);
        let mut posted = Roster::default();
        posted.merge_member(member.clone());
        for b in &bundles {
            posted.merge_bundle(b.clone());
        }
        posted.merge_vouch(vouch.clone());
        {
            let roster = &mut self.group_rosters[pos].1;
            roster.merge_member(member);
            for b in &bundles {
                roster.merge_bundle(b.clone());
            }
            roster.merge_vouch(vouch);
        }
        self.sync_group_conversation(&gid);
        self.persist_group(&gid);
        let from_genesis = self.group_rosters[pos].1.genesis.as_ref().map_or(false, |g| g.history_from_genesis);
        // The records go into the group for everyone; the joiner is standing from the merge above, so the fan-out reaches it too once it holds the era.
        self.queue_group_post(GroupPost { gid, signal: GroupSignal::Records, blob: Some(crate::storage::group::roster_to_vsf_bytes(&gid, &posted).ok().unwrap_or_default()), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        if from_genesis {
            // Re-wrap the CURRENT era whole to each of the joiner's devices, over the friendship.
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let Some((era, lineage, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, *c.lane_root()?, *c.history_key()?))) else {
                crate::logf!("GROUP: {} holds no root here — cannot answer the join", hex::encode(&gid.0[..4]));
                return;
            };
            let mut payload = Vec::with_capacity(64);
            payload.extend_from_slice(&root);
            payload.extend_from_slice(&hk);
            for b in &bundles {
                self.send_wrap_over_friendship(ci, our_pid, b, era, lineage, &payload);
            }
            use zeroize::Zeroize;
            payload.zeroize();
        } else {
            self.mint_group_era(gid, Some((ci, bundles)));
        }
    }

    /// Seal `payload` to one bundle and send it as a WRAP row over the friendship with `ci`.
    fn send_wrap_over_friendship(&mut self, ci: usize, our_pid: [u8; 32], bundle: &crate::types::group::BundleRecord, era: u64, lineage: [u8; 32], payload: &[u8]) -> bool {
        let nonce: [u8; 32] = rand::random();
        let Some((cts, sealed)) = crate::crypto::era::wrap_to_bundle(&bundle.pubkeys(), bundle.kem_set, &bundle.device, era, &nonce, payload) else {
            crate::logf!("GROUP: wrap to {} failed to seal", hex::encode(&bundle.device[..4]));
            return false;
        };
        let wrap = crate::network::message_package::GroupWrapWire { recipient_device: bundle.device, bundle_id: bundle.pubkeys().bundle_id(), era, era_lineage: lineage, nonce, sealed };
        let wire = crate::network::message_package::GroupWire { from: our_pid, woven_authors: Vec::new(), blob: None, wrap: Some(wrap) };
        let ts = vsf::eagle_time_oscillations();
        let sent = self.chain_transmit_with(ci, &GroupSignal::Wrap.to_content(), ts, None, None, Some(&cts), Some(&wire));
        crate::logf!("GROUP: wrap (era {}) for device {} {} over the friendship", era, hex::encode(&bundle.device[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" });
        sent
    }

    /// MINT (§10.4): a fresh secret, the next era derived from it, one WRAP per standing device posted inside the group on the OLD era — and, for a join under from-join, a wrap over the friendship to each of the joiner's devices. The minter installs the era as pending; the cutover edge is its first ACK of any wrap row (step 9 wires the edge and the triggers beyond a join).
    pub(super) fn mint_group_era(&mut self, gid: GroupId, joiner: Option<(usize, Vec<crate::types::group::BundleRecord>)>) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((era, lineage, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, *c.lane_root()?, *c.history_key()?))) else {
            crate::logf!("GROUP: {} holds no root here — cannot mint", hex::encode(&gid.0[..4]));
            return;
        };
        let Some(pos) = self.group_rosters.iter().position(|(id, _)| *id == gid) else {
            return;
        };
        let Some(our_pid) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            return;
        };
        let our_device = self.device_keypair.as_ref().map(|k| *k.public.as_bytes());
        let next = era + 1;
        let nonce: [u8; 32] = rand::random();
        let mut fresh: [u8; 32] = rand::random();
        // The transcript binds the mint to the group, the index and the nonce; the wrap set is not in it (a member verifies the set against its own roster instead, and withholds a short one).
        let transcript = crate::crypto::era::derive_era_transcript(&gid.token(), next, &nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
        let (next_root, next_hk) = crate::crypto::era::derive_era_keys(&gid.0, next, &root, Some(&hk), &fresh, &transcript);
        let targets: Vec<crate::types::group::BundleRecord> = self.group_rosters[pos].1.standing_bundles().into_iter().cloned().collect();
        let mut inside = 0usize;
        for b in &targets {
            if Some(b.device) == our_device {
                continue;
            }
            let Some((cts, sealed)) = crate::crypto::era::wrap_to_bundle(&b.pubkeys(), b.kem_set, &b.device, next, &nonce, &fresh) else {
                crate::logf!("GROUP: mint {} era {} — wrap to {} failed to seal", hex::encode(&gid.0[..4]), next, hex::encode(&b.device[..4]));
                continue;
            };
            let wrap = crate::network::message_package::GroupWrapWire { recipient_device: b.device, bundle_id: b.pubkeys().bundle_id(), era: next, era_lineage: lineage, nonce, sealed };
            self.queue_group_post(GroupPost { gid, signal: GroupSignal::Wrap, blob: None, wrap: Some(wrap), kem: Some(cts), text: None, queued: self.tick_serial, control: false });
            inside += 1;
        }
        let joiner_wraps = joiner.as_ref().map_or(0, |(_, b)| b.len());
        if let Some((ci, bundles)) = joiner {
            // The joiner never held era N, so it cannot derive N+1 from fresh: it receives the next era WHOLE (root ‖ history key), the same shape a from-genesis join answer carries.
            let mut whole = Vec::with_capacity(64);
            whole.extend_from_slice(&next_root);
            whole.extend_from_slice(&next_hk);
            for b in &bundles {
                self.send_wrap_over_friendship(ci, our_pid, b, next, lineage, &whole);
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
        crate::logf!("GROUP: minted era {} for {} — {} wrap(s) inside, {} to a joiner; pending until the first wrap ACKs", next, hex::encode(&gid.0[..4]), inside, joiner_wraps);
    }

    /// WRAP over the friendship (the joiner's side, §10.1 Joining → Standing): open it against the bundle we minted at Join, install the era, build chains + conversation + roster from the parked snapshot, and post our records into the group as our first frame.
    fn install_join_wrap(&mut self, ci: usize, wire: Option<crate::network::message_package::GroupWire>, kem: Option<crate::crypto::era::EraKemWire>) {
        let sponsor_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
        let (Some(w), Some(cts)) = (wire.as_ref().and_then(|g| g.wrap.as_ref()), kem.as_ref()) else {
            crate::logf!("GROUP: wrap from {} without its typed fields — ignored", sponsor_fp);
            return;
        };
        let Some(our_device) = self.device_keypair.as_ref().map(|k| *k.public.as_bytes()) else {
            return;
        };
        if w.recipient_device != our_device {
            return; // for a sibling of ours — it opens its own
        }
        let Some(offer) = self.group_offers.iter().find(|o| self.contacts.get(ci).map_or(false, |c| c.handle_hash == o.sponsor)).cloned() else {
            crate::logf!("GROUP: wrap from {} but no parked offer from them — ignored", sponsor_fp);
            return;
        };
        let gid = offer.group_id;
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some(secret) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.group_kems().iter().rev().find_map(|k| crate::crypto::era::unwrap_from_bundle(k, &w.bundle_id, cts, &our_device, w.era, &w.nonce, &w.sealed))) else {
            crate::logf!("GROUP: wrap from {} for {} does not open against our bundle — ignored", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        };
        // A join answer always carries the era WHOLE (root ‖ history key): era N under from-genesis, the freshly minted N+1 under from-join — either way the joiner held nothing to derive from.
        if secret.len() != 64 {
            crate::logf!("GROUP: join wrap for {} must carry the era whole (got {} bytes) — ignored", hex::encode(&gid.0[..4]), secret.len());
            return;
        }
        let (root, hk) = (<[u8; 32]>::try_from(&secret[..32]).unwrap(), <[u8; 32]>::try_from(&secret[32..]).unwrap());
        let kems: Vec<crate::crypto::era::EraDecapKeys> = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c.group_kems().to_vec()).unwrap_or_default();
        let mut chains = crate::types::FriendshipChains::from_group_root(gid, &offer.snapshot.standing(), root, hk, w.era, w.era_lineage);
        chains.set_group_kems(kems);
        self.friendship_chains.retain(|(id, _)| *id != fid);
        self.friendship_chains.push((fid, chains));
        if !self.conversations.iter().any(|v| v.id() == fid) {
            self.conversations.push(crate::types::Conversation::new_group(gid, offer.snapshot.standing()));
        }
        self.group_rosters.retain(|(id, _)| *id != gid);
        self.group_rosters.push((gid, offer.snapshot.clone()));
        // The offer stays parked, accepted: it is what names the group the friendship row is about (the card reads "joined" from here on).
        self.set_group_local(&gid, |l| l.phase = GroupPhase::Standing);
        self.persist_group(&gid);
        self.persist_chains_of(&fid);
        // Our first frame in the group: our own member record + bundle, so every member holds them even if the sponsor's records post is still in flight.
        if let (Some(our_pid), Some(seed), Some(hp)) = (self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) {
            let (name, avatar) = self.own_name_grant();
            let mut ours = Roster::default();
            ours.merge_member(crate::types::group::join_record(our_pid, hp, &name, avatar, offer.sponsor, &seed));
            if let Some(b) = self.group_rosters.iter().find(|(id, _)| *id == gid).and_then(|(_, r)| r.newest_bundle(&our_device).cloned()) {
                ours.merge_bundle(b);
            }
            self.queue_group_post(GroupPost { gid, signal: GroupSignal::Records, blob: crate::storage::group::roster_to_vsf_bytes(&gid, &ours).ok(), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        }
        crate::logf!("GROUP: joined \"{}\" ({}) via {} at era {}", offer.snapshot.title(), hex::encode(&gid.0[..4]), sponsor_fp, w.era);
    }

    /// WRAP inside the group (an existing member, §10.4): a mint's wrap addressed to our device — open it against our persisted bundle, derive the next era from the one we hold, install it as pending. Rows for other devices are ignored. A wrap set that omits a member we hold standing is WITHHELD (the exclusion tripwire of the vouch model); it re-evaluates on the next records edge.
    fn install_group_wrap(&mut self, cp: usize, wire: Option<crate::network::message_package::GroupWire>, kem: Option<crate::crypto::era::EraKemWire>) {
        let (Some(w), Some(cts)) = (wire.as_ref().and_then(|g| g.wrap.as_ref()), kem.as_ref()) else {
            return;
        };
        let Some(our_device) = self.device_keypair.as_ref().map(|k| *k.public.as_bytes()) else {
            return;
        };
        if w.recipient_device != our_device {
            return;
        }
        let Some(gid) = self.conversations.get(cp).filter(|c| c.is_group()).map(|c| GroupId(*c.id().as_bytes())) else {
            return;
        };
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((era, root, hk, fresh)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| {
            let fresh = c.group_kems().iter().rev().find_map(|k| crate::crypto::era::unwrap_from_bundle(k, &w.bundle_id, cts, &our_device, w.era, &w.nonce, &w.sealed))?;
            Some((c.era_index, *c.lane_root()?, *c.history_key()?, fresh))
        }) else {
            crate::logf!("GROUP: wrap for era {} in {} does not open against our bundle — ignored", w.era, hex::encode(&gid.0[..4]));
            return;
        };
        if w.era != era + 1 || fresh.len() != 32 {
            crate::logf!("GROUP: wrap for era {} in {} while we hold {} — not the next era; ignored (re-serve carries the one we need)", w.era, hex::encode(&gid.0[..4]), era);
            return;
        }
        let fresh32: [u8; 32] = fresh.as_slice().try_into().unwrap();
        let transcript = crate::crypto::era::derive_era_transcript(&gid.token(), w.era, &w.nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
        let (next_root, next_hk) = crate::crypto::era::derive_era_keys(&gid.0, w.era, &root, Some(&hk), &fresh32, &transcript);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: w.era, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
        }
        self.persist_chains_of(&fid);
        crate::logf!("GROUP: era {} for {} installed as pending — cutover on the first frame tagged with it or our next send", w.era, hex::encode(&gid.0[..4]));
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

    /// Merge a snapshot's records into a held roster — signature-checked per record (the subject's own claimed device; the fold check deepens with GroupPeer in step 5), precedence entirely the roster merge's newest-wins law.
    pub(super) fn merge_snapshot_into(&mut self, pos: usize, snapshot: Roster) -> bool {
        use crate::types::group::verify_record;
        let mut verified = Roster::default();
        if let Some(g) = snapshot.genesis {
            if verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) {
                verified.genesis = Some(g);
            }
        }
        // merge_vouch needs the genesis for the founder self-vouch rule: seed it from the held roster when the snapshot lacks it.
        if verified.genesis.is_none() {
            verified.genesis = self.group_rosters[pos].1.genesis.clone();
        }
        for (_, m) in snapshot.members {
            if verify_record(&m.signing_bytes(), &m.signature, &m.signer_device) {
                verified.merge_member(m);
            } else {
                crate::logf!("GROUP: member record for {} fails its signature — skipped", crate::fp(&m.party));
            }
        }
        for (_, l) in snapshot.leaves {
            if verify_record(&l.signing_bytes(), &l.signature, &l.signer_device) {
                verified.merge_leave(l);
            } else {
                crate::logf!("GROUP: leave record for {} fails its signature — skipped", crate::fp(&l.party));
            }
        }
        for (_, list) in snapshot.vouches {
            for v in list {
                if verify_record(&v.signing_bytes(), &v.signature, &v.signer_device) {
                    verified.merge_vouch(v);
                } else {
                    crate::logf!("GROUP: vouch {} → {} fails its signature — skipped", crate::fp(&v.voucher), crate::fp(&v.subject));
                }
            }
        }
        for (_, b) in snapshot.bundles {
            if verify_record(&b.signing_bytes(), &b.signature, &b.device) {
                verified.merge_bundle(b);
            } else {
                crate::logf!("GROUP: bundle for device {} fails its signature — skipped", hex::encode(&b.device[..4]));
            }
        }
        if let Some(t) = snapshot.title {
            if verify_record(&t.signing_bytes(), &t.signature, &t.signer_device) {
                verified.title = Some(t);
            }
        }
        let held_genesis = self.group_rosters[pos].1.genesis.is_some();
        if held_genesis {
            verified.genesis = None; // never re-merge a genesis we hold (merge_genesis refuses anyway; this keeps `changed` honest)
        }
        self.group_rosters[pos].1.merge_all(verified)
    }

    /// Follow the roster's standing set into the group conversation (§4: the set follows the roster; the id never moves), re-seed the group's admission list, and fold any standing member we never friended into the GroupPeer trust source (step 5). Returns whether the set changed.
    pub(super) fn sync_group_conversation(&mut self, gid: &GroupId) -> bool {
        let Some((standing, proofs)) = self.group_rosters.iter().find(|(id, _)| id == gid).map(|(_, r)| {
            let standing = r.standing();
            let proofs: Vec<[u8; 32]> = standing.iter().filter_map(|p| r.members.get(p).map(|m| m.handle_proof)).collect();
            (standing, proofs)
        }) else {
            return false;
        };
        let changed = self.conversations.iter_mut().find(|c| c.id().as_bytes() == &gid.0).map(|c| c.set_participants(standing.clone())).unwrap_or(false);
        self.reseed_group_pubkeys();
        // Members with no contact row need their public chain folded — the same refresh a contact gets; the drain lands them in `group_peers`.
        let us = self.session.as_ref().map(|s| s.handle_proof);
        let unfriended: Vec<[u8; 32]> = proofs
            .into_iter()
            .filter(|hp| Some(*hp) != us && !self.contacts.iter().any(|c| c.handle_proof == *hp) && !self.group_peers.iter().any(|p| p.handle_proof == *hp))
            .collect();
        if !unfriended.is_empty() {
            self.spawn_contact_fleet_refresh(unfriended);
        }
        changed
    }

    /// Send (or refresh) a group OFFER to a contact over the pairwise braid (§10.1): the roster snapshot rides the package's gpl field so the invitee sees who is in it before consenting. Carries no secret. The invitee's consent comes back as a Join.
    pub(super) fn send_group_offer(&mut self, gid: GroupId, ci: usize) -> bool {
        let Some(contact) = self.contacts.get(ci) else {
            return false;
        };
        if contact.is_sibling {
            crate::log("GROUP: a sibling is already every group we are — offer refused");
            return false;
        }
        let fp = crate::fp(&contact.handle_proof);
        let Some(our_pid) = self.our_party_id(contact) else {
            return false;
        };
        let blob = {
            let Some((_, roster)) = self.group_rosters.iter().find(|(g, _)| *g == gid) else {
                crate::logf!("GROUP: no roster for {} — cannot offer", hex::encode(&gid.0[..4]));
                return false;
            };
            if !roster.is_standing(&our_pid) {
                crate::logf!("GROUP: not standing in {} — cannot offer", hex::encode(&gid.0[..4]));
                return false;
            }
            match crate::storage::group::roster_to_vsf_bytes(&gid, roster) {
                Ok(b) => b,
                Err(e) => {
                    crate::logf!("GROUP: roster snapshot for {} failed to encode: {}", hex::encode(&gid.0[..4]), e);
                    return false;
                }
            }
        };
        let wire = crate::network::message_package::GroupWire { from: our_pid, woven_authors: Vec::new(), blob: Some(blob), wrap: None };
        let ts = crate::network::time_base::stamp_osc();
        // The offer row lands in the friendship conversation FIRST (the sponsor-side card: "you brought … into …"), then rides the wire.
        let invitee = self.contacts[ci].handle_hash;
        if let Some(conv) = self.conv_mut_of(ci) {
            conv.insert_message_sorted(crate::types::ChatMessage::new_with_timestamp(GroupSignal::Offer.to_content(), true, ts));
        }
        self.persist_messages_async(ci);
        let sent = self.chain_transmit_with(ci, &GroupSignal::Offer.to_content(), ts, None, None, None, Some(&wire));
        self.set_group_local(&gid, |l| {
            l.offered.retain(|(p, _)| *p != invitee);
            l.offered.push((invitee, ts));
        });
        crate::logf!("GROUP: offer for {} {} to {}", hex::encode(&gid.0[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" }, fp);
        sent
    }

    /// FOUND a group (§10.5: never empty — founding always offers to someone in the same act): mint everything, persist, and offer to `ci`.
    pub(super) fn found_group_with(&mut self, ci: usize, title: &str, history_from_genesis: bool) -> Option<GroupId> {
        let (Some(our_pid), Some(seed), Some(hp)) = (self.contacts.get(ci).and_then(|c| self.our_party_id(c)), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) else {
            return None;
        };
        let (name, avatar) = self.own_name_grant();
        let birth = crate::types::group::found_group(our_pid, hp, &name, avatar, title, history_from_genesis, &seed);
        let gid = birth.group_id;
        let eph = crate::crypto::era::era_keygen(0, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let bundle = crate::types::group::bundle_record(our_pid, 0, &eph, &seed);
        let mut roster = Roster::default();
        roster.merge_genesis(birth.genesis.clone());
        roster.merge_member(birth.founder_member.clone());
        roster.merge_vouch(birth.founder_vouch.clone());
        roster.merge_bundle(bundle);
        let mut chains = crate::types::FriendshipChains::from_group_root(gid, &roster.standing(), birth.group_root, birth.group_history_key, 0, birth.era_lineage);
        chains.push_group_kem(eph.export_decaps(0));
        let fid = *chains.id();
        self.friendship_chains.push((fid, chains));
        self.conversations.push(crate::types::Conversation::new_group(gid, roster.standing()));
        self.group_rosters.push((gid, roster));
        self.set_group_local(&gid, |l| l.phase = GroupPhase::Standing);
        self.persist_group(&gid);
        self.persist_chains_of(&fid);
        crate::logf!("GROUP: founded \"{}\" ({}) — {} history", title, hex::encode(&gid.0[..4]), if history_from_genesis { "from-genesis" } else { "from-join" });
        self.send_group_offer(gid, ci);
        Some(gid)
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
    pub(super) fn queue_group_post(&mut self, post: GroupPost) {
        self.pending_group_posts.push(post);
    }

    /// Mutate + persist this device's local state for a group.
    pub(super) fn set_group_local(&mut self, gid: &GroupId, f: impl FnOnce(&mut GroupLocal)) {
        let pos = match self.group_locals.iter().position(|(id, _)| id == gid) {
            Some(p) => p,
            None => {
                self.group_locals.push((*gid, GroupLocal::default()));
                self.group_locals.len() - 1
            }
        };
        f(&mut self.group_locals[pos].1);
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::group::save_group_local(gid, &self.group_locals[pos].1, storage) {
                crate::logf!("GROUP: local state write failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
        }
    }

    /// Persist one chains blob by id (group or friendship) on the calling thread — small, rare edges (found, join, wrap install).
    pub(super) fn persist_chains_of(&self, fid: &crate::types::FriendshipId) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if let Some((_, c)) = self.friendship_chains.iter().find(|(id, _)| id == fid) {
            if let Err(e) = crate::storage::friendship::save_friendship_chains(c, storage) {
                crate::logf!("GROUP: chains persist failed for {}: {}", hex::encode(&fid.as_bytes()[..4]), e);
            }
        }
    }

    /// A GROUP ACK (docs/groups.md step 4): resolve the acking device to a standing party (a friend's fold, else a GroupPeer's), mark it on the per-member ledger — the implied-ACK rule per party rides inside — and flip `delivered` on every row whose target set is now covered. Returns whether a row changed.
    pub(super) fn on_group_ack(&mut self, gid: GroupId, device: [u8; 32], acked_eagle_time: i64) -> bool {
        let Some(pos) = self.group_rosters.iter().position(|(g, _)| *g == gid) else {
            return false;
        };
        let standing = self.group_rosters[pos].1.standing();
        let party = self
            .contacts
            .iter()
            .find(|c| !c.is_sibling && c.knows_device(&device) && standing.binary_search(&c.handle_hash).is_ok())
            .map(|c| c.handle_hash)
            .or_else(|| self.group_peers.iter().find(|p| p.knows_device(&device) && standing.binary_search(&p.party).is_ok()).map(|p| p.party));
        let Some(party) = party else {
            crate::logf!("GROUP: ACK for {} from device {} — no standing member's device; ignored", hex::encode(&gid.0[..4]), hex::encode(&device[..4]));
            return false;
        };
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) else {
            return false;
        };
        let done = chains.process_group_ack(acked_eagle_time, party);
        let progress = chains.pending_progress(acked_eagle_time);
        crate::logf!("GROUP: ACK from {} for {} in {} — {}", crate::fp(&party), acked_eagle_time, hex::encode(&gid.0[..4]), match progress { Some((a, n)) => format!("{} of {}", a, n), None => "complete".to_string() });
        self.persist_chains_async(&fid);
        if done.is_empty() {
            return false;
        }
        let mut changed = false;
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

    /// Compose in a GROUP (docs/groups.md step 4): bubble first, wire second — the row lands in the group conversation now, the encrypt + fan-out runs on the next tick thru `drain_group_posts` (the same frame fence the friendship path keeps).
    pub(super) fn send_group_message(&mut self, gid: GroupId, text: &str, reference: Option<(crate::types::RefKind, i64)>) -> bool {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some(our_pid) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            return false;
        };
        let eagle_time = crate::network::time_base::stamp_osc();
        let mut msg = crate::types::ChatMessage::new_with_timestamp(text.to_string(), true, eagle_time);
        msg.marks = self.marks_for_send(text);
        msg.reference = reference;
        msg.author = Some(our_pid);
        if let Some((a, p)) = self.attach_stage.take() {
            msg.attach = Some(a);
            msg.preview = p;
        }
        let quiet = matches!(reference, Some((crate::types::RefKind::Edit | crate::types::RefKind::React, _)));
        if let Some(conv) = self.conversations.iter_mut().find(|v| v.id() == fid) {
            conv.insert_message_sorted(msg);
            if !quiet {
                conv.scroll_offset = 0.0;
            }
        }
        self.persist_conversation_async(fid);
        self.queue_group_post(GroupPost { gid, signal: GroupSignal::Records, blob: None, wrap: None, kem: None, text: Some((text.to_string(), eagle_time, reference)), queued: self.tick_serial, control: false });
        true
    }

    /// Run the deferred wire half of every queued group post — chat rows and control rows alike — one tick after they were queued. A post that cannot go yet (no root, nobody reachable, the encrypt gate busy) stays queued; the retransmit sweep and the next edge retry it. Returns whether anything went out.
    pub(super) fn drain_group_posts(&mut self) -> bool {
        if self.pending_group_posts.is_empty() {
            return false;
        }
        let serial = self.tick_serial;
        let (ready, keep): (Vec<GroupPost>, Vec<GroupPost>) = std::mem::take(&mut self.pending_group_posts).into_iter().partition(|p| p.queued.wrapping_add(1) < serial);
        self.pending_group_posts = keep;
        if ready.is_empty() {
            return false;
        }
        let Some(our_pid) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            self.pending_group_posts.extend(ready);
            return false;
        };
        let mut sent_any = false;
        for post in ready {
            let gid = post.gid;
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let (text, ts, reference) = match post.text.as_ref() {
                Some((t, ts, r)) => (t.clone(), *ts, *r),
                None => {
                    // A control row: it lands in the group conversation as a hidden outgoing row FIRST (re-ACK durability, the era-row pattern), then rides the wire.
                    let ts = crate::network::time_base::stamp_osc();
                    let content = post.signal.to_content();
                    let mut row = crate::types::ChatMessage::new_with_timestamp(content.clone(), true, ts);
                    row.author = Some(our_pid);
                    if let Some(conv) = self.conversations.iter_mut().find(|v| v.id() == fid) {
                        conv.insert_message_sorted(row);
                    }
                    (content, ts, None)
                }
            };
            let wire = crate::network::message_package::GroupWire { from: our_pid, woven_authors: Vec::new(), blob: post.blob.clone(), wrap: post.wrap.clone() };
            if self.group_transmit(gid, &text, ts, reference, post.kem.as_ref(), Some(&wire)) {
                sent_any = true;
                if post.text.is_none() {
                    self.persist_conversation_async(fid);
                }
                // Our own row exists only on this device until a sibling hears of it — the group's token carries it (docs/groups.md step 6).
                if let Some(row) = self.conversations.iter().find(|v| v.id() == fid).and_then(|v| v.messages.iter().find(|m| m.is_outgoing && m.timestamp == ts).cloned()) {
                    self.push_rows_to_siblings_token(gid.token(), &hex::encode(&gid.0[..4]), std::slice::from_ref(&row), None);
                }
            } else {
                // Held: keep the post (with its stamp, so a control row re-sends as the SAME row) for the next drain edge.
                let mut held = post;
                if held.text.is_none() {
                    held.text = Some((text, ts, None));
                    held.control = true;
                }
                held.queued = serial;
                self.pending_group_posts.push(held);
            }
        }
        sent_any
    }

    /// Persist a group's index entry + roster. Chains persist thru the standard chains path on their own mutation edges.
    pub(super) fn persist_group(&mut self, gid: &GroupId) {
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
            // The roster rides the chains blob to our siblings (step 6): stamp it, and the replication pass pushes on its mutated_osc.
            if let Ok(bytes) = crate::storage::group::roster_to_vsf_bytes(gid, roster) {
                let fid = crate::types::FriendshipId::from_bytes(gid.0);
                if let Some((_, c)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                    c.set_group_roster(bytes);
                }
            }
        }
    }

    /// A SIBLING's replicated group blob landed (docs/groups.md step 6): merge the roster it carries; if this device never held the group, boot it — index, conversation, local state — so a phone that slept thru a join wakes holding the group. Returns whether the roster changed.
    pub(super) fn adopt_replicated_group(&mut self, gid: GroupId, roster_bytes: &[u8]) -> bool {
        if roster_bytes.is_empty() {
            return false;
        }
        let (bid, snapshot) = match crate::storage::group::roster_from_vsf_bytes(roster_bytes) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("GROUP: sibling roster for {} failed verified read: {}", hex::encode(&gid.0[..4]), e);
                return false;
            }
        };
        if bid != gid {
            return false;
        }
        let pos = match self.group_rosters.iter().position(|(id, _)| *id == gid) {
            Some(p) => p,
            None => {
                let Some(_) = Self::verify_snapshot_birth(&gid, &snapshot) else {
                    crate::logf!("GROUP: sibling roster for {} carries no verifiable genesis — refused", hex::encode(&gid.0[..4]));
                    return false;
                };
                crate::logf!("GROUP: booted \"{}\" ({}) from a sibling's replication", snapshot.title(), hex::encode(&gid.0[..4]));
                self.group_rosters.push((gid, Roster { genesis: snapshot.genesis.clone(), ..Default::default() }));
                let fid = crate::types::FriendshipId::from_bytes(gid.0);
                if !self.conversations.iter().any(|v| v.id() == fid) {
                    self.conversations.push(crate::types::Conversation::new_group(gid, std::iter::empty()));
                }
                self.set_group_local(&gid, |l| l.phase = GroupPhase::Standing);
                self.group_rosters.len() - 1
            }
        };
        let mut changed = self.merge_snapshot_into(pos, snapshot);
        changed |= self.sync_group_conversation(&gid);
        if changed {
            self.persist_group(&gid);
        } else if let Some(storage) = self.storage.as_ref() {
            let _ = crate::storage::group::index_group(&gid, storage);
        }
        changed
    }
}
