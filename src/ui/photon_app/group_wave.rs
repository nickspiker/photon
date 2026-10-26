//! GROUP WAVES (docs/group-waves.md) — a wave in a molecule, any size: one offer on the group's lane, a join per member device, and a media link between every pair of joined devices.
//!
//! Each link is a whole 1:1 wave engine (keys, ladder, loss loop, fills) on its own far slot of the one audio session (platform::audio FAR_SLOTS); the speaker mixes the slots, the microphone feeds them all.
//! Signalling is the 1:1 vocabulary riding the group's lane: the Offer is the origin's join, an Answer is a member's join, a Hangup is a member leaving — the frame's signing device says who.
//! The doctrine carries over: no ring timer, ring on direct decrypt only, the offer's lane key is the basket's doomed egg, and every link derives its own secret from it bound to both devices.

use super::*;
use crate::types::molecule::MoleculeId;
use crate::types::{WaveInfo, WaveOutcome};
use crate::wave::signal::WaveSignal;

/// Where this device stands in a group wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupPhase {
    /// A member's offer reached us; we are ringing.
    Ringing,
    /// We are in it: the origin from its offer on, a member from its join on.
    Joined,
}

/// Another device in the wave.
pub(crate) struct GroupMember {
    pub device: [u8; 32],
    pub party: [u8; 32],
    /// Its join nonce (the origin's offer nonce for the origin).
    pub nonce: [u8; 32],
}

/// A live media link to one member device.
pub(crate) struct GroupLinkUp {
    pub device: [u8; 32],
    pub slot: usize,
    pub lead: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub engine: crate::wave::engine::EngineHandle,
}

/// The one live group wave (one wave at a time on a device, group or pair).
pub(crate) struct GroupWave {
    pub gid: MoleculeId,
    pub wave_id: [u8; 16],
    /// The offer row's stamp, shared by every device that saw the offer: each mints the wave's row at offer_osc + 1.
    pub offer_osc: i64,
    pub we_are_origin: bool,
    pub phase: GroupPhase,
    /// When this device joined (the duration base).
    pub phase_osc: i64,
    /// The lane key the offer was sealed under — the doomed egg every link's secret derives from.
    pub offer_lane_key: Option<[u8; 32]>,
    /// Our join nonce (the offer's nonce for the origin), once we are in.
    pub our_nonce: Option<[u8; 32]>,
    /// Every OTHER device that joined, the origin first.
    pub joined: Vec<GroupMember>,
    pub links: Vec<GroupLinkUp>,
    /// The desktop ring loop's stopper while ringing.
    pub ring: Option<crate::wave::RingGuard>,
    /// The audio session this wave opened for its links (start_owned's generation).
    pub session: Option<u64>,
    /// A link has been up at some point: the origin's wave ends when the last member leaves, not while it still waits for the first.
    pub ever_linked: bool,
}

impl GroupWave {
    /// The wave screen's phase: ringing in, waiting for the first join, or talking.
    pub(crate) fn screen_phase(&self) -> crate::wave::WavePhase {
        match self.phase {
            GroupPhase::Ringing => crate::wave::WavePhase::Ringing,
            GroupPhase::Joined if self.links.is_empty() => crate::wave::WavePhase::Outgoing,
            GroupPhase::Joined => crate::wave::WavePhase::Active,
        }
    }
}

impl PhotonApp {
    fn our_device(&self) -> Option<[u8; 32]> {
        self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes())
    }

    /// Place a wave in the open molecule (roster index `gi`): the offer rides the group's lane to every standing member, and its lane key is captured at the send commit.
    pub(super) fn start_group_wave(&mut self, gi: usize) {
        if self.active_wave.is_some() || self.group_wave.is_some() {
            crate::log("WAVE: already in a wave");
            return;
        }
        let Some((gid, _)) = self.molecule_rosters.get(gi).cloned() else {
            return;
        };
        let Some(device) = self.our_device() else {
            return;
        };
        if self.our_reflexive.map_or(true, |h| crate::network::traverse::is_lan_scope(&h)) {
            self.reseed_reflexive_from_fgtw();
        }
        let wave_id: [u8; 16] = rand::random();
        let nonce: [u8; 32] = rand::random();
        let now = vsf::eagle_time_oscillations();
        self.wave_minimized = false;
        self.wave_playback.take();
        self.freshen_clock_for_wave();
        // The record exists BEFORE the offer goes out: its lane key is captured at the send commit, which can drain during the send itself.
        self.group_wave = Some(GroupWave {
            gid,
            wave_id,
            offer_osc: now,
            we_are_origin: true,
            phase: GroupPhase::Joined,
            phase_osc: now,
            offer_lane_key: None,
            our_nonce: Some(nonce),
            joined: Vec::new(),
            links: Vec::new(),
            ring: None,
            session: None,
            ever_linked: false,
        });
        self.dialed_wave_ids.insert(wave_id);
        let sig = WaveSignal::Offer { wave_id, nonce, device: Some(device) };
        if !self.send_group_wave_signal(gid, sig, now) {
            crate::log("WAVE: group offer send failed (no group keys) — not dialing");
            self.group_wave = None;
            self.dialed_wave_ids.remove(&wave_id);
            return;
        }
        crate::logf!("WAVE: group wave {} offered in {}", hex::encode(&wave_id[..4]), hex::encode(&gid.0[..4]));
        self.scene_dirty = true;
    }

    /// One signal on the group's lane, kept as our own hidden row (the fleet's ring/stop fan-out reads it from the sibling push).
    fn send_group_wave_signal(&mut self, gid: MoleculeId, sig: WaveSignal, ts: i64) -> bool {
        let id = crate::types::FriendshipId::from_bytes(gid.0);
        let ctl = crate::types::RowControl::Wave(sig);
        if !self.send_control_in_conversation(id, ctl.clone(), ts) {
            return false;
        }
        let row = self.conv_by_id(id).and_then(|v| v.messages.iter().find(|m| m.is_outgoing && m.timestamp == ts && m.control.as_ref() == Some(&ctl)).cloned());
        if let Some(row) = row {
            self.push_conversation_rows_to_siblings(id, std::slice::from_ref(&row), None);
        }
        true
    }

    /// The offer's send commit (messaging.rs drain_braid_tx): the origin's own copy of the doomed egg.
    pub(super) fn capture_group_offer_key(&mut self, wave_id: &[u8; 16], key: [u8; 32]) -> bool {
        match self.group_wave.as_mut() {
            Some(w) if w.wave_id == *wave_id && w.offer_lane_key.is_none() => {
                w.offer_lane_key = Some(key);
                crate::log("WAVE: group offer lane key captured at commit — basket egg secured");
                true
            }
            _ => false,
        }
    }

    /// Join the ringing group wave: our join goes to every member, and a link comes up to every device already in.
    pub(super) fn join_group_wave(&mut self) {
        let Some(w) = self.group_wave.as_ref().filter(|w| w.phase == GroupPhase::Ringing) else {
            return;
        };
        let (gid, wave_id) = (w.gid, w.wave_id);
        if w.offer_lane_key.is_none() {
            crate::log("WAVE: joining a group wave without the offer lane key — declining instead (basket incomplete)");
            self.decline_group_wave();
            return;
        }
        let Some(device) = self.our_device() else {
            return;
        };
        let nonce: [u8; 32] = rand::random();
        if !self.send_group_wave_signal(gid, WaveSignal::Answer { wave_id, nonce, device: Some(device) }, vsf::eagle_time_oscillations()) {
            crate::log("WAVE: group join send failed — the group can't carry a frame right now");
            return;
        }
        self.wave_playback.take();
        self.wave_minimized = false;
        if let Some(w) = self.group_wave.as_mut() {
            w.phase = GroupPhase::Joined;
            w.phase_osc = vsf::eagle_time_oscillations();
            w.our_nonce = Some(nonce);
            w.ring = None;
        }
        Self::stop_group_ring_platform();
        let members: Vec<[u8; 32]> = self.group_wave.as_ref().map(|w| w.joined.iter().map(|m| m.device).collect()).unwrap_or_default();
        for dev in members {
            self.group_link_up(dev);
        }
        crate::logf!("WAVE: joined group wave {} — {} link(s) up", hex::encode(&wave_id[..4]), self.group_wave.as_ref().map_or(0, |w| w.links.len()));
        self.scene_dirty = true;
    }

    /// Decline the ring here: no signal (the others' wave goes on without us), the wave's row says we missed it.
    pub(super) fn decline_group_wave(&mut self) {
        if self.group_wave.as_ref().is_some_and(|w| w.phase == GroupPhase::Ringing) {
            self.end_group_wave(WaveOutcome::Declined);
        }
    }

    /// Leave: our hangup to every member, every link down, the wave's row minted.
    pub(super) fn leave_group_wave(&mut self) {
        let Some(w) = self.group_wave.as_ref() else {
            return;
        };
        let (gid, wave_id, phase, links) = (w.gid, w.wave_id, w.phase, w.links.len());
        if phase == GroupPhase::Ringing {
            self.decline_group_wave();
            return;
        }
        let _ = self.send_group_wave_signal(gid, WaveSignal::Hangup { wave_id }, vsf::eagle_time_oscillations());
        let outcome = if links > 0 || self.group_wave.as_ref().is_some_and(|w| !w.joined.is_empty()) { WaveOutcome::Answered } else { WaveOutcome::Missed };
        self.end_group_wave(outcome);
    }

    /// A wave row in a molecule (docs/group-waves.md): `ci` is the sending member's contact, `device` the device that signed the frame.
    pub(super) fn on_group_wave_signal(&mut self, gid: MoleculeId, ci: usize, device: [u8; 32], sig: WaveSignal, rx_lane_key: Option<[u8; 32]>, row_ts: i64) {
        let Some(party) = self.contacts.get(ci).filter(|c| !c.is_sibling).map(|c| c.handle_hash) else {
            return;
        };
        if Some(device) == self.our_device() {
            return;
        }
        match sig {
            WaveSignal::Offer { wave_id, nonce, .. } => {
                if self.ended_waves.contains(&wave_id) || self.rejected_waves.contains(&wave_id) {
                    return;
                }
                match self.group_wave.as_ref() {
                    Some(w) if w.wave_id == wave_id => {}
                    // GLARE: two members waved the same group in the same breath — the smaller wave id is THE wave, and the other origin folds into joining it (both pressed Wave; consent is mutual).
                    Some(w) if w.gid == gid && w.we_are_origin && w.links.is_empty() && w.joined.is_empty() => {
                        if wave_id < w.wave_id {
                            let Some(key) = rx_lane_key else { return };
                            let ours = w.wave_id;
                            crate::logf!("WAVE: group glare — folding our {} into joining {}", hex::encode(&ours[..4]), hex::encode(&wave_id[..4]));
                            let _ = self.send_group_wave_signal(gid, WaveSignal::Hangup { wave_id: ours }, vsf::eagle_time_oscillations());
                            self.ended_waves.insert(ours);
                            self.group_wave = Some(Self::ringing_group_wave(gid, wave_id, row_ts, key, GroupMember { device, party, nonce }));
                            self.join_group_wave();
                        }
                    }
                    // Already in a wave (group or pair): this ring is not for now.
                    Some(_) => {}
                    None if self.active_wave.is_some() => {}
                    None => {
                        let Some(key) = rx_lane_key else { return };
                        // An offer this old is history (a re-served row), not a doorbell — an age check on starting a ring, not a ring timer.
                        // WHY/PROOF: `row_ts` is the origin's clock off the wire; an absurd stamp reads as ancient, never as a wrapped negative age.
                        let age = vsf::eagle_time_oscillations().saturating_sub(row_ts);
                        if age > 60 * vsf::OSCILLATIONS_PER_SECOND as i64 {
                            crate::logf!("WAVE: group offer {} arrived {}s stale — recorded, not ringing", hex::encode(&wave_id[..4]), age / vsf::OSCILLATIONS_PER_SECOND as i64);
                            return;
                        }
                        self.freshen_clock_for_wave();
                        self.group_wave = Some(Self::ringing_group_wave(gid, wave_id, row_ts, key, GroupMember { device, party, nonce }));
                        if let Some(guard) = self.ring_alert_guard(ci) {
                            if let Some(w) = self.group_wave.as_mut() {
                                w.ring = Some(guard);
                            }
                        }
                        self.change_focus(None);
                        crate::logf!("WAVE: RING — group wave {} from {} in {}", hex::encode(&wave_id[..4]), crate::fp(&party), hex::encode(&gid.0[..4]));
                        self.scene_dirty = true;
                    }
                }
            }
            WaveSignal::Answer { wave_id, nonce, .. } => {
                let Some(w) = self.group_wave.as_mut().filter(|w| w.wave_id == wave_id) else {
                    return;
                };
                if w.joined.iter().any(|m| m.device == device) {
                    return;
                }
                w.joined.push(GroupMember { device, party, nonce });
                let joined_here = w.phase == GroupPhase::Joined;
                crate::logf!("WAVE: {} joined group wave {} ({} in besides us)", crate::fp(&party), hex::encode(&wave_id[..4]), w.joined.len());
                if joined_here {
                    self.group_link_up(device);
                }
                self.scene_dirty = true;
            }
            WaveSignal::Hangup { wave_id } => {
                let Some(w) = self.group_wave.as_mut().filter(|w| w.wave_id == wave_id) else {
                    return;
                };
                w.joined.retain(|m| m.device != device);
                let (phase, left) = (w.phase, w.joined.len());
                crate::logf!("WAVE: {} left group wave {} ({} in besides us)", crate::fp(&party), hex::encode(&wave_id[..4]), left);
                self.group_link_down(device);
                // Nobody left to wave with: a ring ends missed, a joined wave ends answered — the origin waiting for its first join waits on.
                let we_are_origin = self.group_wave.as_ref().is_some_and(|w| w.we_are_origin);
                let links = self.group_wave.as_ref().map_or(0, |w| w.links.len());
                if left == 0 && links == 0 {
                    match phase {
                        GroupPhase::Ringing => self.end_group_wave(WaveOutcome::Missed),
                        GroupPhase::Joined if !we_are_origin || self.group_wave.as_ref().is_some_and(|w| w.ever_linked) => self.end_group_wave(WaveOutcome::Answered),
                        GroupPhase::Joined => {}
                    }
                }
                self.scene_dirty = true;
            }
            // Decline, Busy, Taken, Anchor: a member's own business in a group — the wave goes on with whoever joins.
            _ => {}
        }
    }

    /// OUR FLEET's row for a group wave, merged from a sibling: our identity answered on another device, so this one stops ringing.
    pub(super) fn on_group_wave_merge(&mut self, sig: WaveSignal) {
        if let WaveSignal::Answer { wave_id, .. } = sig {
            if self.group_wave.as_ref().is_some_and(|w| w.wave_id == wave_id && w.phase == GroupPhase::Ringing) {
                crate::log("WAVE: a sibling joined the group wave — ring stops here");
                self.ended_waves.insert(wave_id);
                self.group_wave = None;
                Self::stop_group_ring_platform();
                self.scene_dirty = true;
            }
        }
    }

    fn ringing_group_wave(gid: MoleculeId, wave_id: [u8; 16], offer_osc: i64, key: [u8; 32], origin: GroupMember) -> GroupWave {
        GroupWave {
            gid,
            wave_id,
            offer_osc,
            we_are_origin: false,
            phase: GroupPhase::Ringing,
            phase_osc: vsf::eagle_time_oscillations(),
            offer_lane_key: Some(key),
            our_nonce: None,
            joined: vec![origin],
            links: Vec::new(),
            ring: None,
            session: None,
            ever_linked: false,
        }
    }

    /// Bring up the media link to one joined device: its secret from the offer egg and both join nonces, a far slot of its own, the session opened on the first link.
    fn group_link_up(&mut self, device: [u8; 32]) {
        let Some(our_device) = self.our_device() else {
            return;
        };
        let Some(w) = self.group_wave.as_ref() else {
            return;
        };
        if w.links.iter().any(|l| l.device == device) {
            return;
        }
        let (Some(key), Some(our_nonce)) = (w.offer_lane_key, w.our_nonce) else {
            crate::log("WAVE: group link without the offer key or our nonce — not started");
            return;
        };
        let Some(their_nonce) = w.joined.iter().find(|m| m.device == device).map(|m| m.nonce) else {
            return;
        };
        let wave_id = w.wave_id;
        let secret = crate::wave::keys::derive_group_link_secret(&key, &wave_id, (&our_device, &our_nonce), (&device, &their_nonce));
        if w.session.is_none() {
            // The first link opens the session the whole wave shares; the links only claim slots in it.
            let _ = crate::platform::mic_permission::at_wave_start();
            let gen = crate::platform::audio::start_owned();
            if gen.is_none() {
                crate::log("WAVE: audio session did NOT open — this group wave has no microphone and no speaker");
            }
            crate::platform::audio::set_named_playout(true);
            crate::wave::live::ensure_started();
            #[cfg(target_os = "android")]
            let _ = crate::platform::jni_android::wave_service_void("waveWentActive");
            if let Some(w) = self.group_wave.as_mut() {
                w.session = gen;
            }
        }
        // A slot only after the session is open: its queue hygiene clears every slot.
        let Some((slot, slot_gen)) = crate::platform::audio::claim_slot() else {
            crate::logf!("WAVE: no far slot left for {} — {} is the most one wave can hold", crate::fp(&device), crate::platform::audio::FAR_SLOTS - 1);
            return;
        };
        self.start_group_link(device, secret, our_device, slot, slot_gen);
    }

    fn start_group_link(&mut self, device: [u8; 32], secret: [u8; 32], our_device: [u8; 32], slot: usize, slot_gen: u64) {
        // The device's own best address (a wave is with a device, not a contact); none = the sentinel, and TX follows its first authenticated packet.
        let addr = self
            .contacts
            .iter()
            .find(|c| !c.is_sibling && c.knows_device(&device))
            .and_then(|c| crate::network::traverse::gather::gather_device_candidates(c, &device).sorted().into_iter().map(|x| x.addr).next())
            .unwrap_or(crate::network::status::RELAY_ADDR);
        let lead = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(self.group_wave.as_ref().is_some_and(|w| w.links.is_empty())));
        let engine = crate::wave::engine::start(crate::wave::engine::EngineParams {
            secret,
            we_are_origin: crate::wave::keys::group_link_origin(&our_device, &device),
            peer_addr: addr,
            spool: None,
            cal: self.cal_snapshot(),
            plaid_allowed: self.is_lan_for_plaid(addr),
            slot,
            group: Some(crate::wave::engine::GroupLink { lead: lead.clone(), slot_gen }),
        });
        crate::logf!("WAVE: group link up with {} on slot {} — media aimed at {}", crate::fp(&device), slot, addr);
        if let Some(w) = self.group_wave.as_mut() {
            w.links.push(GroupLinkUp { device, slot, lead, engine });
            w.ever_linked = true;
        }
        self.scene_dirty = true;
    }

    /// Take one link down; the lead passes to the next link if it was this one.
    fn group_link_down(&mut self, device: [u8; 32]) {
        let Some(w) = self.group_wave.as_mut() else {
            return;
        };
        let Some(i) = w.links.iter().position(|l| l.device == device) else {
            return;
        };
        let link = w.links.remove(i);
        crate::platform::audio::far_done_slot(link.slot);
        link.engine.stop();
        if link.lead.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(next) = w.links.first() {
                next.lead.store(true, std::sync::atomic::Ordering::Relaxed);
                crate::logf!("WAVE: lead link passes to {}", crate::fp(&next.device));
            }
        }
    }

    /// The wave's end on this device: every link down, the session closed, the wave's row in the group's conversation.
    fn end_group_wave(&mut self, outcome: WaveOutcome) {
        let Some(mut w) = self.group_wave.take() else {
            return;
        };
        crate::logf!("WAVE: group wave {} ends here — {}, {} link(s)", hex::encode(&w.wave_id[..4]), format!("{:?}", outcome), w.links.len());
        if self.ended_waves.len() > 256 {
            self.ended_waves.clear();
        }
        self.ended_waves.insert(w.wave_id);
        let had_links = !w.links.is_empty();
        if had_links {
            crate::platform::audio::play_end_sweep();
        }
        for link in w.links.drain(..) {
            link.engine.stop();
        }
        w.ring = None;
        Self::stop_group_ring_platform();
        if w.session.is_some() {
            crate::wave::live::stop();
            crate::platform::audio::stop();
        }
        self.wave_minimized = false;
        let secs: u32 = if had_links {
            // WHY/PROOF: the wall clock can step back past the join — the age is 0, not a u32 wrapped from a negative
            ((vsf::eagle_time_oscillations() - w.phase_osc).max(0) / vsf::OSCILLATIONS_PER_SECOND as i64) as u32
        } else {
            0
        };
        let id = crate::types::FriendshipId::from_bytes(w.gid.0);
        let mut row = ChatMessage::new_with_timestamp(String::new(), w.we_are_origin, w.offer_osc + 1);
        row.wave = Some(WaveInfo { outcome, secs });
        row.notified = true;
        row.delivered = true;
        row.author = if w.we_are_origin {
            self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        } else {
            w.joined.first().map(|m| m.party)
        };
        if let Some(conv) = self.conv_by_id_mut(id) {
            conv.insert_message_sorted(row.clone());
        }
        self.persist_and_push_rows(id, std::slice::from_ref(&row));
        self.scene_dirty = true;
    }

    fn stop_group_ring_platform() {
        #[cfg(target_os = "android")]
        crate::platform::jni_android::cancel_wave_notification();
    }

    /// The wave screen's name for a group wave: the molecule's title.
    pub(super) fn group_wave_title(&self) -> Option<String> {
        let w = self.group_wave.as_ref()?;
        let gi = self.molecule_rosters.iter().position(|(g, _)| *g == w.gid)?;
        self.molecule_view_of(gi).map(|c| c.display_name())
    }

    /// The contact the wave screen shows for a group wave: the first other device's person (the origin while ringing).
    pub(super) fn group_wave_face(&self) -> Option<usize> {
        let w = self.group_wave.as_ref()?;
        let party = w.links.first().and_then(|l| w.joined.iter().find(|m| m.device == l.device)).or_else(|| w.joined.first())?.party;
        self.contacts.iter().position(|c| !c.is_sibling && c.handle_hash == party)
    }
}
