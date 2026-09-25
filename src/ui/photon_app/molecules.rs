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

impl PhotonApp {
    /// A `MoleculeSignal` control row landed (deferred from commit_braid_rx): `ci` = the sender's contact, `cp` = the conversation the row landed in, `wire` = the package's typed group extras.
    pub(super) fn on_molecule_signal(&mut self, ci: usize, cp: usize, sig: MoleculeSignal, wire: Option<crate::network::message_package::MoleculeWire>, kem: Option<crate::crypto::era::EraKemWire>, ts: i64) {
        let blob = wire.as_ref().and_then(|g| g.blob.clone());
        let in_group = self.conversations.get(cp).map_or(false, |c| c.is_molecule());
        match (sig, in_group) {
            (MoleculeSignal::Offer, false) => self.park_bond_offer(ci, blob, ts),
            (MoleculeSignal::Join, false) => self.answer_bind(ci, blob),
            (MoleculeSignal::Wrap, false) => self.install_bind_wrap(ci, wire, kem),
            (MoleculeSignal::Wrap, true) => self.install_molecule_wrap(cp, wire, kem),
            (MoleculeSignal::Records, true) => self.merge_molecule_records(ci, cp, blob),
            (other, _) => {
                let sender = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
                crate::logf!("MOLECULE: {} row from {} landed {} a group conversation — ignored", format!("{other:?}"), sender, if in_group { "inside" } else { "outside" });
            }
        }
    }

    /// OFFER (§10.1 None → Offered): park the sponsor's snapshot beside the friendship row it rode in on. No secret arrives; the Join pill on that row is the consent. A re-offer for the same group replaces the parked snapshot (the sponsor's refresh); an offer for a group we already hold merges the snapshot as records.
    fn park_bond_offer(&mut self, ci: usize, blob: Option<Vec<u8>>, row_osc: i64) {
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
        if let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) {
            let mut changed = self.merge_snapshot_into(pos, snapshot);
            changed |= self.sync_molecule_conversation(&gid);
            if changed {
                self.persist_molecule(&gid);
            }
            crate::logf!("MOLECULE: offer for held group {} from {} merged as records", hex::encode(&gid.0[..4]), sponsor_fp);
            return;
        }
        let members = snapshot.standing().len();
        let offer = BondOffer { molecule_id: gid, sponsor, row_osc, snapshot, accepted: false };
        self.bond_offers.retain(|o| o.molecule_id != gid);
        self.bond_offers.push(offer.clone());
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::molecule::park_bond_offer(&offer, storage) {
                crate::logf!("MOLECULE: offer park failed for {}: {}", hex::encode(&gid.0[..4]), e);
            }
        }
        crate::logf!("MOLECULE: offered \"{}\" ({}) by {} — {} standing — Join is the consent", title, hex::encode(&gid.0[..4]), sponsor_fp, members);
    }

    /// The genesis must be present, name this group, and verify under its own signer before anything persists. Returns the title suggestion.
    fn verify_snapshot_birth(gid: &MoleculeId, snapshot: &Roster) -> Option<String> {
        let g = snapshot.genesis.as_ref()?;
        if g.molecule_id != *gid || !crate::types::molecule::verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) {
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
        if offer.accepted && self.molecule_rosters.iter().any(|(g, _)| *g == gid) {
            crate::logf!("MOLECULE: join of {} — already joined", hex::encode(&gid.0[..4]));
            return false;
        }
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
        let mut holder = crate::types::FriendshipChains::from_molecule_root(gid, &offer.snapshot.standing(), [0u8; 32], [0u8; 32], era, offer.snapshot.genesis.as_ref().map_or([0u8; 32], |g| g.molecule_id.0));
        holder.push_molecule_kem(eph.export_decaps(era));
        self.friendship_chains.retain(|(id, _)| *id != fid);
        self.friendship_chains.push((fid, holder));
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
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: Some(blob), wrap: None };
        let ts = vsf::eagle_time_oscillations();
        let sent = self.chain_transmit_with(ci, "", ts, None, None, None, Some(&wire), Some(&crate::types::RowControl::Molecule(MoleculeSignal::Join)));
        if sent {
            self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Joining);
            self.persist_chains_of(&fid);
            if let Some(o) = self.bond_offers.iter_mut().find(|o| o.molecule_id == gid) {
                o.accepted = true;
                if let Some(storage) = self.storage.as_ref() {
                    let _ = crate::storage::molecule::save_bond_offer(o, storage);
                }
            }
        }
        crate::logf!("MOLECULE: join of {} {} to {} — waiting for the wrap", hex::encode(&gid.0[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" }, crate::fp(&self.contacts[ci].handle_proof));
        sent
    }

    /// JOIN (the sponsor's side): verify the invitee's records, vouch for them, post everything into the group, and answer with a wrap of the current era (from-genesis) or of a freshly minted next era (from-join, D5 — the joiner never reads a row from before it stood).
    fn answer_bind(&mut self, ci: usize, blob: Option<Vec<u8>>) {
        let joiner_fp = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
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
        // The records go into the group for everyone; the joiner is standing from the merge above, so the fan-out reaches it too once it holds the era.
        self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: Some(crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok().unwrap_or_default()), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        if from_genesis {
            // Re-wrap the CURRENT era whole to each of the joiner's devices, over the friendship.
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let Some((era, lineage, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, *c.lane_root()?, *c.history_key()?))) else {
                crate::logf!("MOLECULE: {} holds no root here — cannot answer the join", hex::encode(&gid.0[..4]));
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
            self.mint_molecule_era(gid, Some((ci, bundles)));
        }
    }

    /// Seal `payload` to one bundle and send it as a WRAP row over the friendship with `ci`.
    fn send_wrap_over_friendship(&mut self, ci: usize, our_pid: [u8; 32], bundle: &crate::types::molecule::BundleRecord, era: u64, lineage: [u8; 32], payload: &[u8]) -> bool {
        let nonce: [u8; 32] = rand::random();
        let Some((cts, sealed)) = crate::crypto::era::wrap_to_bundle(&bundle.pubkeys(), bundle.kem_set, &bundle.device, era, &nonce, payload) else {
            crate::logf!("MOLECULE: wrap to {} failed to seal", hex::encode(&bundle.device[..4]));
            return false;
        };
        let wrap = crate::network::message_package::BondWrapWire { recipient_device: bundle.device, bundle_id: bundle.pubkeys().bundle_id(), era, era_lineage: lineage, nonce, sealed };
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: None, wrap: Some(wrap) };
        let ts = vsf::eagle_time_oscillations();
        let sent = self.chain_transmit_with(ci, "", ts, None, None, Some(&cts), Some(&wire), Some(&crate::types::RowControl::Molecule(MoleculeSignal::Wrap)));
        crate::logf!("MOLECULE: wrap (era {}) for device {} {} over the friendship", era, hex::encode(&bundle.device[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" });
        sent
    }

    /// MINT (§10.4): a fresh secret, the next era derived from it, one WRAP per standing device posted inside the group on the OLD era — and, for a join under from-join, a wrap over the friendship to each of the joiner's devices. The minter installs the era as pending; the cutover edge is its first ACK of any wrap row (step 9 wires the edge and the triggers beyond a join).
    pub(super) fn mint_molecule_era(&mut self, gid: MoleculeId, joiner: Option<(usize, Vec<crate::types::molecule::BundleRecord>)>) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((era, lineage, root, hk)) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| Some((c.era_index, c.era_lineage, *c.lane_root()?, *c.history_key()?))) else {
            crate::logf!("MOLECULE: {} holds no root here — cannot mint", hex::encode(&gid.0[..4]));
            return;
        };
        let Some(pos) = self.molecule_rosters.iter().position(|(id, _)| *id == gid) else {
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
        let targets: Vec<crate::types::molecule::BundleRecord> = self.molecule_rosters[pos].1.standing_bundles().into_iter().cloned().collect();
        let mut inside = 0usize;
        for b in &targets {
            if Some(b.device) == our_device {
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
        let Some(offer) = self.bond_offers.iter().find(|o| self.contacts.get(ci).map_or(false, |c| c.handle_hash == o.sponsor)).cloned() else {
            crate::logf!("MOLECULE: wrap from {} but no parked offer from them — ignored", sponsor_fp);
            return;
        };
        let gid = offer.molecule_id;
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some(secret) = self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.molecule_kems().iter().rev().find_map(|k| crate::crypto::era::unwrap_from_bundle(k, &w.bundle_id, cts, &our_device, w.era, &w.nonce, &w.sealed))) else {
            crate::logf!("MOLECULE: wrap from {} for {} does not open against our bundle — ignored", sponsor_fp, hex::encode(&gid.0[..4]));
            return;
        };
        // A join answer always carries the era WHOLE (root ‖ history key): era N under from-genesis, the freshly minted N+1 under from-join — either way the joiner held nothing to derive from.
        if secret.len() != 64 {
            crate::logf!("MOLECULE: join wrap for {} must carry the era whole (got {} bytes) — ignored", hex::encode(&gid.0[..4]), secret.len());
            return;
        }
        let (root, hk) = (<[u8; 32]>::try_from(&secret[..32]).unwrap(), <[u8; 32]>::try_from(&secret[32..]).unwrap());
        let kems: Vec<crate::crypto::era::EraDecapKeys> = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c.molecule_kems().to_vec()).unwrap_or_default();
        let mut chains = crate::types::FriendshipChains::from_molecule_root(gid, &offer.snapshot.standing(), root, hk, w.era, w.era_lineage);
        chains.set_molecule_kems(kems);
        self.friendship_chains.retain(|(id, _)| *id != fid);
        self.friendship_chains.push((fid, chains));
        if !self.conversations.iter().any(|v| v.id() == fid) {
            self.conversations.push(crate::types::Conversation::new_molecule(gid, offer.snapshot.standing()));
        }
        self.molecule_rosters.retain(|(id, _)| *id != gid);
        self.molecule_rosters.push((gid, offer.snapshot.clone()));
        // The offer stays parked, accepted: it is what names the group the friendship row is about (the card reads "joined" from here on).
        self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Standing);
        self.persist_molecule(&gid);
        self.persist_chains_of(&fid);
        // Our first frame in the group: our own member record + bundle, so every member holds them even if the sponsor's records post is still in flight.
        if let (Some(our_pid), Some(seed), Some(hp)) = (self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) {
            let (name, avatar) = self.own_name_grant();
            let mut ours = Roster::default();
            ours.merge_member(crate::types::molecule::join_record(our_pid, hp, &name, avatar, offer.sponsor, &seed));
            if let Some(b) = self.molecule_rosters.iter().find(|(id, _)| *id == gid).and_then(|(_, r)| r.newest_bundle(&our_device).cloned()) {
                ours.merge_bundle(b);
            }
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: crate::storage::molecule::roster_to_vsf_bytes(&gid, &ours).ok(), wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        }
        crate::logf!("MOLECULE: joined \"{}\" ({}) via {} at era {}", offer.snapshot.title(), hex::encode(&gid.0[..4]), sponsor_fp, w.era);
    }

    /// WRAP inside the group (an existing member, §10.4): a mint's wrap addressed to our device — open it against our persisted bundle, derive the next era from the one we hold, install it as pending. Rows for other devices are ignored. A wrap set that omits a member we hold standing is WITHHELD (the exclusion tripwire of the vouch model); it re-evaluates on the next records edge.
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
        let transcript = crate::crypto::era::derive_era_transcript(&gid.token(), w.era, &w.nonce, crate::crypto::era::KEM_SET_DEFAULT, &crate::crypto::era::EraKemWire::default(), &crate::crypto::era::EraKemWire::default());
        let (next_root, next_hk) = crate::crypto::era::derive_era_keys(&gid.0, w.era, &root, Some(&hk), &fresh32, &transcript);
        if let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
            chains.install_pending(crate::types::friendship::PendingEra { era_index: w.era, lane_root: next_root, history_key: Some(next_hk), tag: crate::crypto::clutch::era_tag(&next_root), resp_osc: None });
        }
        self.persist_chains_of(&fid);
        crate::logf!("MOLECULE: era {} for {} installed as pending — cutover on the first frame tagged with it or our next send", w.era, hex::encode(&gid.0[..4]));
    }

    /// Records posted inside a group (§4): merge every record from the blob under the roster's one law, then follow the standing set into the conversation. Invalid signatures are skipped loudly; merge decides precedence.
    fn merge_molecule_records(&mut self, ci: usize, cp: usize, blob: Option<Vec<u8>>) {
        let sender = self.contacts.get(ci).map(|c| crate::fp(&c.handle_proof)).unwrap_or_default();
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
        let mut changed = self.merge_snapshot_into(pos, snapshot);
        changed |= self.sync_molecule_conversation(&gid);
        if changed {
            self.persist_molecule(&gid);
            crate::logf!("MOLECULE: {} roster advanced by records from {}", hex::encode(&gid.0[..4]), sender);
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
                for p in &departed {
                    crate::logf!("MOLECULE: {} left {} — {} standing remain; minting the next era", crate::fp(p), hex::encode(&gid.0[..4]), standing_after.len());
                }
                self.mint_molecule_era(gid, None);
            }
        }
    }

    /// Merge a snapshot's records into a held roster — signature-checked per record (the subject's own claimed device; the fold check deepens with MoleculePeer in step 5), precedence entirely the roster merge's newest-wins law.
    pub(super) fn merge_snapshot_into(&mut self, pos: usize, snapshot: Roster) -> bool {
        use crate::types::molecule::verify_record;
        let mut verified = Roster::default();
        if let Some(g) = snapshot.genesis {
            if verify_record(&g.signing_bytes(), &g.signature, &g.signer_device) {
                verified.genesis = Some(g);
            }
        }
        // merge_vouch needs the genesis for the founder self-vouch rule: seed it from the held roster when the snapshot lacks it.
        if verified.genesis.is_none() {
            verified.genesis = self.molecule_rosters[pos].1.genesis.clone();
        }
        for (_, m) in snapshot.members {
            if verify_record(&m.signing_bytes(), &m.signature, &m.signer_device) {
                verified.merge_member(m);
            } else {
                crate::logf!("MOLECULE: member record for {} fails its signature — skipped", crate::fp(&m.party));
            }
        }
        for (_, l) in snapshot.leaves {
            if verify_record(&l.signing_bytes(), &l.signature, &l.signer_device) {
                verified.merge_leave(l);
            } else {
                crate::logf!("MOLECULE: leave record for {} fails its signature — skipped", crate::fp(&l.party));
            }
        }
        for (_, list) in snapshot.vouches {
            for v in list {
                if verify_record(&v.signing_bytes(), &v.signature, &v.signer_device) {
                    verified.merge_vouch(v);
                } else {
                    crate::logf!("MOLECULE: vouch {} → {} fails its signature — skipped", crate::fp(&v.voucher), crate::fp(&v.subject));
                }
            }
        }
        for (_, b) in snapshot.bundles {
            if verify_record(&b.signing_bytes(), &b.signature, &b.device) {
                verified.merge_bundle(b);
            } else {
                crate::logf!("MOLECULE: bundle for device {} fails its signature — skipped", hex::encode(&b.device[..4]));
            }
        }
        if let Some(t) = snapshot.title {
            if verify_record(&t.signing_bytes(), &t.signature, &t.signer_device) {
                verified.title = Some(t);
            }
        }
        let held_genesis = self.molecule_rosters[pos].1.genesis.is_some();
        if held_genesis {
            verified.genesis = None; // never re-merge a genesis we hold (merge_genesis refuses anyway; this keeps `changed` honest)
        }
        self.molecule_rosters[pos].1.merge_all(verified)
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
        let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: Some(blob), wrap: None };
        let ts = crate::network::time_base::stamp_osc();
        // The offer row lands in the friendship conversation FIRST (the sponsor-side card: "you brought … into …"), then rides the wire.
        let invitee = self.contacts[ci].handle_hash;
        if let Some(conv) = self.conv_mut_of(ci) {
            conv.insert_message_sorted(crate::types::ChatMessage::control(crate::types::RowControl::Molecule(MoleculeSignal::Offer), true, ts));
        }
        self.persist_messages_async(ci);
        let sent = self.chain_transmit_with(ci, "", ts, None, None, None, Some(&wire), Some(&crate::types::RowControl::Molecule(MoleculeSignal::Offer)));
        self.set_molecule_local(&gid, |l| {
            l.offered.retain(|(p, _)| *p != invitee);
            l.offered.push((invitee, ts));
        });
        crate::logf!("MOLECULE: offer for {} {} to {}", hex::encode(&gid.0[..4]), if sent { "sent" } else { "NOT sent — the next edge retries" }, fp);
        sent
    }

    /// FOUND an ATOM (docs/molecules.md §0: you can only ever create an atom): a conversation with you alone in it, titled or not. Nothing is offered to anyone; binding people in later makes it a molecule.
    pub(super) fn found_atom_alone(&mut self, title: &str, history_from_genesis: bool) -> Option<MoleculeId> {
        let (Some(our_pid), Some(seed), Some(hp)) = (self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)), self.device_seed(), self.session.as_ref().map(|s| s.handle_proof)) else {
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

    /// Mutate + persist this device's local state for a group.
    pub(super) fn set_molecule_local(&mut self, gid: &MoleculeId, f: impl FnOnce(&mut MoleculeLocal)) {
        let pos = match self.molecule_locals.iter().position(|(id, _)| id == gid) {
            Some(p) => p,
            None => {
                self.molecule_locals.push((*gid, MoleculeLocal::default()));
                self.molecule_locals.len() - 1
            }
        };
        f(&mut self.molecule_locals[pos].1);
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::molecule::save_molecule_local(gid, &self.molecule_locals[pos].1, storage) {
                crate::logf!("MOLECULE: local state write failed for {}: {}", hex::encode(&gid.0[..4]), e);
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
        let done = chains.process_group_ack(acked_eagle_time, party);
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

    /// Compose in a GROUP (docs/molecules.md step 4): bubble first, wire second — the row lands in the group conversation now, the encrypt + fan-out runs on the next tick thru `drain_molecule_posts` (the same frame fence the friendship path keeps).
    pub(super) fn send_molecule_message(&mut self, gid: MoleculeId, text: &str, reference: Option<(crate::types::RefKind, i64)>) -> bool {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some(our_pid) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            return false;
        };
        let eagle_time = crate::network::time_base::stamp_osc();
        let mut msg = crate::types::ChatMessage::new_with_timestamp(text.to_string(), true, eagle_time);
        msg.marks = self.marks_for_send(text);
        msg.reference = reference;
        msg.author = Some(our_pid);
        if let Some((a, p, f)) = self.attach_stage.take() {
            msg.attach = Some(a);
            msg.preview = p;
            msg.file = Some(f);
        }
        let quiet = matches!(reference, Some((crate::types::RefKind::Edit | crate::types::RefKind::React, _)));
        if let Some(conv) = self.conversations.iter_mut().find(|v| v.id() == fid) {
            conv.insert_message_sorted(msg);
            if !quiet {
                conv.scroll_offset = 0.0;
            }
        }
        self.persist_conversation_async(fid);
        self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: None, wrap: None, kem: None, text: Some((text.to_string(), eagle_time, reference)), queued: self.tick_serial, control: false });
        true
    }

    /// Run the deferred wire half of every queued group post — chat rows and control rows alike — one tick after they were queued. A post that cannot go yet (no root, nobody reachable, the encrypt gate busy) stays queued; the retransmit sweep and the next edge retry it. Returns whether anything went out.
    pub(super) fn drain_molecule_posts(&mut self) -> bool {
        if self.pending_molecule_posts.is_empty() {
            return false;
        }
        let serial = self.tick_serial;
        let (ready, keep): (Vec<MoleculePost>, Vec<MoleculePost>) = std::mem::take(&mut self.pending_molecule_posts).into_iter().partition(|p| p.queued.wrapping_add(1) < serial);
        self.pending_molecule_posts = keep;
        if ready.is_empty() {
            return false;
        }
        let Some(our_pid) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            self.pending_molecule_posts.extend(ready);
            return false;
        };
        let mut sent_any = false;
        for post in ready {
            let gid = post.gid;
            let fid = crate::types::FriendshipId::from_bytes(gid.0);
            let (text, ts, reference, control) = match post.text.as_ref() {
                Some((t, ts, r)) => (t.clone(), *ts, *r, None),
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
            let wire = crate::network::message_package::MoleculeWire { from: our_pid, woven_authors: Vec::new(), blob: post.blob.clone(), wrap: post.wrap.clone() };
            if self.molecule_transmit(gid, &text, ts, reference, post.kem.as_ref(), Some(&wire), control.as_ref()) {
                sent_any = true;
                if post.text.is_none() {
                    self.persist_conversation_async(fid);
                }
                // Our own row exists only on this device until a sibling hears of it — the group's token carries it (docs/molecules.md step 6).
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
                self.pending_molecule_posts.push(held);
            }
        }
        sent_any
    }

    /// LEAVE (§10.1 Standing → Left, D9): post our signed leave record into the group and go read-only at once — the token drops from the admission list, compose closes, the row greys. The root is zeroized when the leave row's first ACK proves a survivor holds it (`on_molecule_ack`), so a lost frame cannot strand the others without the news; alone, at once.
    pub(super) fn leave_molecule(&mut self, gid: MoleculeId) -> bool {
        let (Some(our_pid), Some(seed)) = (self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)), self.device_seed()) else {
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

    /// RENAME (D7): a title record signed by us, merged here and posted into the group; newest wins everywhere.
    pub(super) fn rename_molecule(&mut self, gid: MoleculeId, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let (Some(our_pid), Some(seed)) = (self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)), self.device_seed()) else {
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

    /// After a GROUP cutover (docs/molecules.md §10.4), on either side: the pendings the cutover cleared re-serve on the fresh lane, a fresh KEM bundle publishes for the new era (the next mint wraps to it), and Catching-up devices are Standing again.
    pub(super) fn after_molecule_cutover(&mut self, gid: MoleculeId) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        self.set_molecule_local(&gid, |l| {
            if l.phase == MoleculePhase::CatchingUp {
                l.phase = MoleculePhase::Standing;
            }
        });
        // Fresh bundle for the new era.
        if let (Some(our_pid), Some(seed)) = (self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)), self.device_seed()) {
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
        self.resend_held_molecule_rows(gid);
    }

    /// Re-queue every undelivered outgoing chat row of a group (a cutover cleared its pendings; a relaunch found them held) — control rows never re-serve bare.
    pub(super) fn resend_held_molecule_rows(&mut self, gid: MoleculeId) {
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let held: Vec<(String, i64, Option<(crate::types::RefKind, i64)>)> = self
            .conversations
            .iter()
            .find(|v| v.id() == fid)
            .map(|v| v.messages.iter().filter(|m| m.is_outgoing && !m.delivered && !m.is_control() && (!m.content.is_empty() || m.reference.is_some() || m.file.is_some())).map(|m| (m.content.clone(), m.timestamp, m.reference)).collect())
            .unwrap_or_default();
        let already: Vec<i64> = self.pending_molecule_posts.iter().filter(|p| p.gid == gid).filter_map(|p| p.text.as_ref().map(|(_, ts, _)| *ts)).collect();
        let pending: Vec<i64> = self.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| c.pending_messages.iter().map(|m| m.eagle_time).collect()).unwrap_or_default();
        let mut n = 0usize;
        for (text, ts, reference) in held {
            if already.contains(&ts) || pending.contains(&ts) {
                continue;
            }
            self.queue_molecule_post(MoleculePost { gid, signal: MoleculeSignal::Records, blob: None, wrap: None, kem: None, text: Some((text, ts, reference)), queued: self.tick_serial, control: false });
            n += 1;
        }
        if n > 0 {
            crate::logf!("MOLECULE: {} held row(s) re-queued for {}", n, hex::encode(&gid.0[..4]));
        }
    }

    /// Persist a group's index entry + roster. Chains persist thru the standard chains path on their own mutation edges.
    pub(super) fn persist_molecule(&mut self, gid: &MoleculeId) {
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
                self.set_molecule_local(&gid, |l| l.phase = MoleculePhase::Standing);
                self.molecule_rosters.len() - 1
            }
        };
        let mut changed = self.merge_snapshot_into(pos, snapshot);
        changed |= self.sync_molecule_conversation(&gid);
        if changed {
            self.persist_molecule(&gid);
        } else if let Some(storage) = self.storage.as_ref() {
            let _ = crate::storage::molecule::index_molecule(&gid, storage);
        }
        changed
    }
}
