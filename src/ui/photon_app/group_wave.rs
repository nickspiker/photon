//! GROUP WAVES (docs/group-waves.md) — a wave in a molecule, any size: one offer on the group's lane, a join per member device, and a media link between every pair of joined devices.
//!
//! Each link is a whole 1:1 wave engine (keys, ladder, loss loop, fills) on its own far party of the one audio session (platform::audio PARTIES, as many as the wave has); the speaker mixes the slots, the microphone feeds them all.
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
#[derive(Clone, Copy)]
pub(crate) struct GroupMember {
    pub device: [u8; 32],
    pub party: [u8; 32],
    /// Its join nonce (the origin's offer nonce for the origin).
    pub nonce: [u8; 32],
    /// The stamp of the row that joined it: a person is ONE device in a wave, and only a newer join from the same person moves them (the hand-off).
    pub at: i64,
}

/// A GROUP WAVE RUNNING WITHOUT US (Nick 2026-11-01: "We need a way for a member to join in after the beam or wave has started. Leave/join at will."): what joining it needs — the offer's key and who is in.
/// Learned from the offer whether or not it rang here, kept current by every join and leave on the group's lane, and gone when the last member leaves; leaving a wave keeps it, so a member can come back.
pub(crate) struct LiveGroupWave {
    pub gid: MoleculeId,
    pub wave_id: [u8; 16],
    pub offer_osc: i64,
    /// The lane key the offer was sealed under — every link's secret derives from it.
    pub key: [u8; 32],
    /// Every other device in the wave, the origin first while it stays.
    pub joined: Vec<GroupMember>,
}

/// A live media link to one member device.
pub(crate) struct GroupLinkUp {
    pub device: [u8; 32],
    pub slot: usize,
    pub lead: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub engine: crate::wave::engine::EngineHandle,
    /// The link's beam receiver: up with the engine, costing nothing until the peer beams; its picture lands in the link's slot.
    pub _beam_rx: crate::wave::beam_session::Receiver,
    /// The link's drought state (group_drought_tick): reconnecting, the candidate the walk is on, when it last moved.
    pub reconnecting: bool,
    pub probe: u32,
    pub last_probe_osc: i64,
    /// The link's recording spool: our side and this link's far party (docs/group-waves.md); collected into the group keep when the link or the wave ends.
    pub spool: Option<crate::wave::spool::SpoolTicket>,
}

/// One face on the wave screen's roll.
pub(crate) struct RollFace {
    pub device: [u8; 32],
    /// Our own face (the one labelled "You", ringed by what we send).
    pub us: bool,
    pub party: [u8; 32],
    /// Their contact when we friended them.
    pub ci: Option<usize>,
    /// The far slot their voice plays in, once a link is up.
    pub slot: Option<usize>,
    /// What we call them: a friend's display name, or the name a never-friended member's record grants.
    pub name: String,
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
    /// Our camera going out to every link (the fan), and a desktop camera still opening.
    pub beam_tx: Option<crate::wave::beam_session::Sender>,
    pub beam_opening: Option<std::sync::mpsc::Receiver<Result<crate::wave::beam_session::Sender, String>>>,
    /// Spools of links that have ended (with their engine threads, which the keep joins first), and how many spools this wave has minted (each link's file is named apart).
    pub spent: Vec<(crate::wave::spool::SpoolTicket, Option<std::thread::JoinHandle<()>>, [u8; 32])>,
    pub spools_minted: u8,
    /// The wave's one encode, shared by every link (engine::GroupTx).
    pub tx: std::sync::Arc<crate::wave::engine::GroupTx>,
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
            beam_tx: None,
            beam_opening: None,
            spent: Vec::new(),
            spools_minted: 0,
            tx: std::sync::Arc::new(crate::wave::engine::GroupTx::new()),
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
        // A join or a leave also goes express (an offer waits for its egg: capture_group_offer_key).
        if !matches!(sig, WaveSignal::Offer { .. }) {
            self.send_group_express(gid, &sig, ts, None, None);
        }
        let row = self.conv_by_id(id).and_then(|v| v.messages.iter().find(|m| m.is_outgoing && m.timestamp == ts && m.control.as_ref() == Some(&ctl)).cloned());
        if let Some(row) = row {
            self.push_conversation_rows_to_siblings(id, std::slice::from_ref(&row), None);
        }
        true
    }

    /// THE GROUP EXPRESS COPY (docs/group-waves.md): the same signal as a sealed datagram under the group's express key, signed by our device, to every other member's devices — straight to each device's addresses and over the relay, so a ring, a join or a leave beats the group's lane as a pair's does. Every signal also seals under the retired era (a member one cutover behind still opens it).
    /// `to` names one device instead of every member's: the roll call answers only the newcomer.
    pub(super) fn send_group_express(&self, gid: MoleculeId, sig: &WaveSignal, ts: i64, lane_key: Option<[u8; 32]>, to: Option<[u8; 32]>) {
        let Some(kp) = self.device_keypair.clone() else {
            return;
        };
        let fid = crate::types::FriendshipId::from_bytes(gid.0);
        let Some((_, chains)) = self.friendship_chains.iter().find(|(id, c)| *id == fid && c.molecule) else {
            return;
        };
        let mut keys: Vec<[u8; 32]> = Vec::with_capacity(2);
        if let (Some(root), Some(hk)) = (chains.lane_root(), chains.history_key()) {
            keys.push(crate::wave::signal::group_express_key(root, hk));
        }
        if let Some(r) = chains.retired_era() {
            if let Some(hk) = r.history_key.as_ref() {
                keys.push(crate::wave::signal::group_express_key(&r.lane_root, hk));
            }
        }
        // A group beam describes itself in-band once its links are up, so a group offer never carries the beam flag.
        let frames: Vec<Vec<u8>> = keys.iter().filter_map(|k| crate::wave::signal::seal_group_express(k, ts, lane_key.as_ref(), sig, false, &kp)).collect();
        if frames.is_empty() {
            return;
        }
        let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        let Some((_, roster)) = self.molecule_rosters.iter().find(|(g, _)| *g == gid) else {
            return;
        };
        let devices: Vec<[u8; 32]> = match to {
            Some(dev) => vec![dev],
            None => roster.standing().into_iter().filter(|p| Some(*p) != us).flat_map(|p| super::molecules::member_devices(&self.contacts, &self.molecule_peers, &p)).collect(),
        };
        // PEER TO PEER ONLY (Nick 2026-11-01: "Relay peer-peer only, no thru fgtw.org and only if no direct path"): express goes straight to each device's own addresses and never thru the fgtw.org relay. A device with no address here still gets the signal on the group's lane row, which members carry to each other (grp_relay) — the express copy is only ever the fast lane.
        let mut direct = 0usize;
        let mut unreached = 0usize;
        for dev in &devices {
            let mut any = false;
            for a in self.device_addresses(dev) {
                if a.ip().to_canonical().is_loopback() || a.ip().is_unspecified() {
                    continue;
                }
                for f in &frames {
                    crate::wave::send_media(f.clone(), a);
                }
                direct += 1;
                any = true;
            }
            if !any {
                unreached += 1;
            }
        }
        crate::logf!("WAVE: group express {} fired → {} device(s), {} direct path(s) × {} era key(s); {} with no address ride the lane row (id {})", sig.kind(), devices.len(), direct, frames.len(), unreached, hex::encode(&sig.wave_id()[..4]));
    }

    /// An express frame no pair opened, tried against every molecule we stand in (current era, then retired): the signing device names the member, who need not be our contact, and the signal goes where the group's lane row would send it. `false` = no group opened it.
    pub(super) fn open_group_express_frame(&mut self, bytes: &[u8], nonce: [u8; crate::wave::signal::EXPRESS_NONCE_LEN]) -> bool {
        let mut opened: Option<(MoleculeId, (i64, Option<[u8; 32]>, WaveSignal, bool, [u8; 32]))> = None;
        for (fid, chains) in self.friendship_chains.iter().filter(|(_, c)| c.molecule) {
            let mut keys: Vec<[u8; 32]> = Vec::with_capacity(2);
            if let (Some(root), Some(hk)) = (chains.lane_root(), chains.history_key()) {
                keys.push(crate::wave::signal::group_express_key(root, hk));
            }
            if let Some(r) = chains.retired_era() {
                if let Some(hk) = r.history_key.as_ref() {
                    keys.push(crate::wave::signal::group_express_key(&r.lane_root, hk));
                }
            }
            if let Some(r) = keys.iter().find_map(|k| crate::wave::signal::open_group_express(k, bytes)) {
                opened = Some((MoleculeId(*fid.as_bytes()), r));
                break;
            }
        }
        let Some((gid, (ts, lane_key, sig, _, device))) = opened else {
            return false;
        };
        // The same freshness gate and nonce cache as a pair's frame: sealed means unforgeable, not unreplayable.
        // WHY/PROOF: `ts` is the sender's — abs_diff is exact for every pair of stamps, a crafted one included.
        let skew = vsf::eagle_time_oscillations().abs_diff(ts);
        if skew > crate::wave::signal::EXPRESS_MAX_SKEW_OSC as u64 {
            crate::logf!("WAVE: group express {} stamped {}s away — dropped as stale (replay guard)", sig.kind(), skew / vsf::OSCILLATIONS_PER_SECOND as u64);
            return true;
        }
        if self.express_seen.len() >= 256 {
            self.express_seen.drain(..64);
        }
        self.express_seen.push(nonce);
        // The group key proves the sender stands in the group's era; the device signature proves which device — and the roster must still say that device's person stands.
        let Some((party, ci)) = super::molecules::group_member_of(&self.contacts, &self.molecule_peers, &self.molecule_rosters, &gid, &device) else {
            crate::logf!("WAVE: group express {} signed by {}, no standing member's device — dropped", sig.kind(), crate::fp(&device));
            return true;
        };
        crate::logf!("WAVE: group express {} from {} (device {}) in {} (jumped the lane; id {})", sig.kind(), crate::fp(&party), crate::fp(&device), hex::encode(&gid.0[..4]), hex::encode(&sig.wave_id()[..4]));
        self.on_group_wave_signal(gid, party, ci, device, sig, lane_key, ts);
        true
    }

    /// The offer's send commit (messaging.rs drain_braid_tx): the origin's own copy of the doomed egg.
    pub(super) fn capture_group_offer_key(&mut self, wave_id: &[u8; 16], key: [u8; 32]) -> bool {
        match self.group_wave.as_mut() {
            Some(w) if w.wave_id == *wave_id && w.offer_lane_key.is_none() => {
                w.offer_lane_key = Some(key);
                crate::log("WAVE: group offer lane key captured at commit — basket egg secured");
                // The egg is ours now, so the offer's express copy can carry it: every member rings at datagram speed, the lane row following as the record.
                let (gid, ts, nonce) = (w.gid, w.offer_osc, w.our_nonce);
                if let (Some(nonce), Some(device)) = (nonce, self.our_device()) {
                    self.send_group_express(gid, &WaveSignal::Offer { wave_id: *wave_id, nonce, device: Some(device) }, ts, Some(key), None);
                }
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
        // MUTED COMING IN FROM THE THIRD (Nick 2026-11-01: "First and second person in, unmuted, 3 and up, mute coming in"): two or more other people already in makes us the third or later, and we join muted — the Mute pill shows Unmute, every link starts silent (group_link_up applies wave_muted).
        let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        let mut parties: Vec<[u8; 32]> = Vec::new();
        for m in self.group_wave.as_ref().map(|w| w.joined.as_slice()).unwrap_or(&[]) {
            if Some(m.party) != us && !parties.contains(&m.party) {
                parties.push(m.party);
            }
        }
        if parties.len() >= 2 {
            self.wave_muted = true;
            crate::logf!("WAVE: joining as person {} of the group wave — muted coming in (tap Unmute to speak)", parties.len() + 1);
        }
        let members: Vec<[u8; 32]> = self.group_wave.as_ref().map(|w| w.joined.iter().map(|m| m.device).collect()).unwrap_or_default();
        for dev in members {
            self.group_link_up(dev);
        }
        crate::logf!("WAVE: joined group wave {} — {} link(s) up", hex::encode(&wave_id[..4]), self.group_wave.as_ref().map_or(0, |w| w.links.len()));
        self.scene_dirty = true;
    }

    /// An offer seen in a group, rung here or not: the group's running wave is this one, unless a newer offer already stands.
    fn note_live_offer(&mut self, gid: MoleculeId, wave_id: [u8; 16], offer_osc: i64, key: [u8; 32], origin: GroupMember) {
        match self.live_group_waves.iter_mut().find(|l| l.gid == gid) {
            Some(l) if l.wave_id == wave_id => {}
            Some(l) if l.offer_osc < offer_osc => *l = LiveGroupWave { gid, wave_id, offer_osc, key, joined: vec![origin] },
            Some(_) => {}
            None => self.live_group_waves.push(LiveGroupWave { gid, wave_id, offer_osc, key, joined: vec![origin] }),
        }
    }

    /// A join on the group's lane: the device is in (a fresh nonce replaces its old one — it left and came back).
    fn note_live_join(&mut self, gid: MoleculeId, wave_id: [u8; 16], m: GroupMember) {
        if let Some(l) = self.live_group_waves.iter_mut().find(|l| l.gid == gid && l.wave_id == wave_id) {
            // One device per person, the newer join winning (the hand-off).
            match l.joined.iter_mut().find(|j| j.device == m.device || j.party == m.party) {
                Some(j) if j.at > m.at => {}
                Some(j) => *j = m,
                None => l.joined.push(m),
            }
        }
    }

    /// A leave on the group's lane: the device is out, and the wave with it once nobody is left.
    fn note_live_leave(&mut self, gid: MoleculeId, wave_id: [u8; 16], device: [u8; 32]) {
        if let Some(l) = self.live_group_waves.iter_mut().find(|l| l.gid == gid && l.wave_id == wave_id) {
            l.joined.retain(|j| j.device != device);
        }
        self.live_group_waves.retain(|l| !l.joined.is_empty());
    }

    /// Can this device join the group's running wave now: not in a wave of its own, and someone in that wave online.
    pub(super) fn live_group_wave_joinable(&self, gi: usize) -> bool {
        if self.active_wave.is_some() || self.group_wave.is_some() {
            return false;
        }
        let Some(gid) = self.molecule_rosters.get(gi).map(|(g, _)| *g) else {
            return false;
        };
        self.live_group_waves.iter().find(|l| l.gid == gid).is_some_and(|l| l.joined.iter().any(|m| self.contacts.iter().any(|c| !c.is_sibling && c.handle_hash == m.party && c.is_online)))
    }

    /// Join the group's running wave (late, or again after leaving): the record becomes this device's group wave and the ordinary join runs — a fresh nonce, so every link's secret is fresh too.
    pub(super) fn join_live_group_wave(&mut self, gi: usize) {
        if self.active_wave.is_some() || self.group_wave.is_some() {
            crate::log("WAVE: already in a wave");
            return;
        }
        let Some(gid) = self.molecule_rosters.get(gi).map(|(g, _)| *g) else {
            return;
        };
        let Some(pos) = self.live_group_waves.iter().position(|l| l.gid == gid) else {
            return;
        };
        let l = self.live_group_waves.remove(pos);
        // Our own devices are never linked to: if one of them is in, our join moves the wave here (the hand-off) and it leaves.
        let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        let others: Vec<GroupMember> = l.joined.iter().copied().filter(|m| Some(m.party) != us).collect();
        let Some(origin) = others.first().copied() else {
            crate::log("WAVE: the running group wave holds only our own devices — nothing to join here");
            return;
        };
        self.ended_waves.remove(&l.wave_id);
        self.rejected_waves.remove(&l.wave_id);
        self.freshen_clock_for_wave();
        let mut w = Self::ringing_group_wave(gid, l.wave_id, l.offer_osc, l.key, origin);
        w.joined = others;
        crate::logf!("WAVE: joining the running group wave {} in {} — {} device(s) in", hex::encode(&l.wave_id[..4]), hex::encode(&gid.0[..4]), w.joined.len());
        self.group_wave = Some(w);
        self.join_group_wave();
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
        self.send_group_wave_signal(gid, WaveSignal::Hangup { wave_id }, vsf::eagle_time_oscillations());
        let outcome = if links > 0 || self.group_wave.as_ref().is_some_and(|w| !w.joined.is_empty()) { WaveOutcome::Answered } else { WaveOutcome::Missed };
        self.end_group_wave(outcome);
    }

    /// A wave row in a molecule (docs/group-waves.md): `party` the sending member, `ci` their contact when we friended them (a member is a person, not a contact), `device` the device that signed the frame.
    pub(super) fn on_group_wave_signal(&mut self, gid: MoleculeId, party: [u8; 32], ci: Option<usize>, device: [u8; 32], sig: WaveSignal, rx_lane_key: Option<[u8; 32]>, row_ts: i64) {
        if ci.is_some_and(|ci| self.contacts.get(ci).is_none_or(|c| c.is_sibling)) {
            match sig {
                // Our own other device joining the wave we are in, by express: the hand-off (one stream per person).
                WaveSignal::Answer { wave_id, .. } => self.hand_off_group_wave(wave_id, Some(device), row_ts),
                // A wave our own other device started: never a ring here, but joinable (the wave moves here when we join).
                WaveSignal::Offer { wave_id, nonce, .. } => {
                    if let Some(key) = rx_lane_key {
                        self.note_live_offer(gid, wave_id, row_ts, key, GroupMember { device, party, nonce, at: row_ts });
                    }
                }
                _ => {}
            }
            return;
        }
        if Some(device) == self.our_device() {
            return;
        }
        match sig {
            WaveSignal::Offer { wave_id, nonce, .. } => {
                // Every offer seen in the group is its running wave, whether it rings here or not: a member who missed, declined or left joins it later.
                if let Some(key) = rx_lane_key {
                    self.note_live_offer(gid, wave_id, row_ts, key, GroupMember { device, party, nonce, at: row_ts });
                }
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
                            self.send_group_wave_signal(gid, WaveSignal::Hangup { wave_id: ours }, vsf::eagle_time_oscillations());
                            self.ended_waves.insert(ours);
                            self.group_wave = Some(Self::ringing_group_wave(gid, wave_id, row_ts, key, GroupMember { device, party, nonce, at: row_ts }));
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
                        self.group_wave = Some(Self::ringing_group_wave(gid, wave_id, row_ts, key, GroupMember { device, party, nonce, at: row_ts }));
                        let guard = match ci {
                            Some(ci) => self.ring_alert_guard(ci),
                            None => {
                                let name = self.member_granted_name(&gid, &party);
                                let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
                                us.and_then(|us| self.ring_alert_for(name, &party, &us))
                            }
                        };
                        if let Some(guard) = guard {
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
                self.note_live_join(gid, wave_id, GroupMember { device, party, nonce, at: row_ts });
                let Some(w) = self.group_wave.as_mut().filter(|w| w.wave_id == wave_id) else {
                    return;
                };
                // ONE DEVICE PER PERSON (the hand-off, Nick 2026-11-01): a join from another device of a person already in moves them there — the newer join wins and its predecessor's link goes; an older one (a late roll call from the device they left) is ignored.
                let moved_from = match w.joined.iter().position(|m| m.party == party && m.device != device) {
                    Some(i) if w.joined[i].at > row_ts => return,
                    Some(i) => Some(w.joined.remove(i).device),
                    None => None,
                };
                // A device already in with a DIFFERENT nonce left and came back (its leave may never have reached us): its old link goes, the fresh nonce brings a fresh secret.
                let rejoin = match w.joined.iter_mut().find(|m| m.device == device) {
                    Some(m) if m.nonce == nonce => return,
                    Some(m) => {
                        m.nonce = nonce;
                        true
                    }
                    None => {
                        w.joined.push(GroupMember { device, party, nonce, at: row_ts });
                        false
                    }
                };
                let joined_here = w.phase == GroupPhase::Joined;
                let (gid, our_nonce) = (w.gid, w.our_nonce);
                crate::logf!("WAVE: {} {} group wave {} ({} in besides us)", crate::fp(&party), if rejoin { "came back to" } else { "joined" }, hex::encode(&wave_id[..4]), w.joined.len());
                if rejoin {
                    self.group_link_down(device);
                }
                if let Some(old) = moved_from {
                    crate::logf!("WAVE: {} moved to device {} — the link to {} goes", crate::fp(&party), crate::fp(&device), crate::fp(&old));
                    self.group_link_down(old);
                }
                if joined_here {
                    self.group_link_up(device);
                    // ROLL CALL: the newcomer knows only the joins that reached it, and a member it never heard of could never link with it — everyone already in says so again (a device already known is ignored, so this never cascades).
                    // To the newcomer alone, by express (docs/group-media-scale.md, "A thousand people"): our own join is already a row on the lane, so a second row and a copy to every device made each join cost the square of the group — a thousand joins, a million answers.
                    if let (Some(nonce), Some(dev)) = (our_nonce, self.our_device()) {
                        self.send_group_express(gid, &WaveSignal::Answer { wave_id, nonce, device: Some(dev) }, vsf::eagle_time_oscillations(), None, Some(device));
                    }
                }
                self.scene_dirty = true;
            }
            WaveSignal::Hangup { wave_id } => {
                self.note_live_leave(gid, wave_id, device);
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
    pub(super) fn on_group_wave_merge(&mut self, sig: WaveSignal, row_ts: i64) {
        if let WaveSignal::Answer { wave_id, device, .. } = sig {
            if self.group_wave.as_ref().is_some_and(|w| w.wave_id == wave_id && w.phase == GroupPhase::Ringing) {
                crate::log("WAVE: a sibling joined the group wave — ring stops here");
                self.ended_waves.insert(wave_id);
                self.group_wave = None;
                Self::stop_group_ring_platform();
                self.scene_dirty = true;
                return;
            }
            self.hand_off_group_wave(wave_id, device, row_ts);
        }
    }

    /// HAND-OFF (Nick 2026-11-01: "handoff is proper. per identity seems as tho we should only have one stream"): another of our devices joined the wave this device is in, LATER than we did — the wave moves there.
    /// We leave on the wire (our hangup takes this device's links down everywhere; the members already swapped us for the newer device on its join), the camera and microphone close here, and the wave stays joinable on this device (a Join pulls it back).
    /// Only a NEWER join moves it: a copy of an older one (a sibling's history arriving late) never sends the wave backwards.
    fn hand_off_group_wave(&mut self, wave_id: [u8; 16], device: Option<[u8; 32]>, row_ts: i64) {
        let ours = self.our_device();
        let Some(w) = self.group_wave.as_ref().filter(|w| w.wave_id == wave_id && w.phase == GroupPhase::Joined) else {
            return;
        };
        if device.is_none() || device == ours || row_ts <= w.phase_osc {
            return;
        }
        crate::logf!("WAVE: our other device {} joined group wave {} — the wave moves there; leaving here", crate::fp(&device.unwrap_or_default()), hex::encode(&wave_id[..4]));
        self.leave_group_wave();
    }

    /// A wave ROW from our own fleet for the offer we are ringing for (stamped offer_osc + 1): another of our devices declined it, or lived it — the ring stops here too, silently.
    pub(super) fn on_group_wave_row_merged(&mut self, row_ts: i64) {
        let ringing = self.group_wave.as_ref().filter(|w| w.phase == GroupPhase::Ringing && w.offer_osc + 1 == row_ts).map(|w| w.wave_id);
        if let Some(wave_id) = ringing {
            crate::log("WAVE: our fleet answered the group ring elsewhere — ring stops here");
            self.ended_waves.insert(wave_id);
            self.group_wave = None;
            Self::stop_group_ring_platform();
            self.scene_dirty = true;
        }
    }

    /// The presence plane's offline verdict for a device (status.rs): while we RING, a waving device that went away is out of the wave, and a ring with nobody left waving ends missed — the group ring's lease, on an edge. A linked device is left to its link (its drought heal), since a busy media path can starve presence pings.
    pub(super) fn on_group_wave_device_offline(&mut self, device: [u8; 32]) {
        let Some(w) = self.group_wave.as_mut().filter(|w| w.phase == GroupPhase::Ringing) else {
            return;
        };
        if !w.joined.iter().any(|m| m.device == device) {
            return;
        }
        w.joined.retain(|m| m.device != device);
        crate::logf!("WAVE: {} went offline while waving the group — {} still waving", crate::fp(&device), w.joined.len());
        if w.joined.is_empty() {
            self.end_group_wave(WaveOutcome::Missed);
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
            beam_tx: None,
            beam_opening: None,
            spent: Vec::new(),
            spools_minted: 0,
            tx: std::sync::Arc::new(crate::wave::engine::GroupTx::new()),
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
            // The edge acts on the status itself (asks, or opens Settings and flags MIC_DENIED for the wave screen); the returned copy is not needed here.
            crate::platform::mic_permission::at_wave_start();
            let gen = crate::platform::audio::start_owned();
            if gen.is_none() {
                crate::log("WAVE: audio session did NOT open — this group wave has no microphone and no speaker");
            }
            crate::platform::audio::set_named_playout(true);
            crate::wave::live::ensure_started();
            #[cfg(target_os = "android")]
            crate::platform::jni_android::wave_service_void("waveWentActive");
            if let Some(w) = self.group_wave.as_mut() {
                w.session = gen;
            }
        }
        // A party only after the session is open: its queue hygiene clears every party.
        let (slot, slot_gen) = crate::platform::audio::claim_slot();
        // The beam's video beside the audio, when the user saves it: this link's receive slot is track 1 + slot, named by its person (a slot past the u8 track space is not saved).
        let party = self.group_wave.as_ref().and_then(|w| w.joined.iter().find(|m| m.device == device).map(|m| m.party));
        if let (Some(p), Some(t)) = (party, u8::try_from(slot + 1).ok()) {
            let id8: [u8; 8] = wave_id[..8].try_into().unwrap();
            self.arm_beam_video(&id8, &[(t, p)]);
        }
        self.start_group_link(device, secret, our_device, slot, slot_gen);
    }

    fn start_group_link(&mut self, device: [u8; 32], secret: [u8; 32], our_device: [u8; 32], slot: usize, slot_gen: u64) {
        // The device's own best address (a wave is with a device, not a contact); none = the sentinel, and TX follows its first authenticated packet.
        let addr = self.device_addresses(&device).into_iter().next().unwrap_or(crate::network::status::RELAY_ADDR);
        let lead = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(self.group_wave.as_ref().is_some_and(|w| w.links.is_empty())));
        let origin = crate::wave::keys::group_link_origin(&our_device, &device);
        // Recording by default, as every wave is: each link its own spool, named apart by how many this wave has minted.
        let (spool_param, ticket) = match self.group_wave.as_mut() {
            Some(w) => {
                let mut id8: [u8; 8] = w.wave_id[..8].try_into().unwrap();
                id8[7] ^= 0x80 | w.spools_minted;
                w.spools_minted = w.spools_minted.wrapping_add(1);
                match crate::wave::spool::mint(&id8) {
                    Some((key, path, ticket)) => (Some((key, path)), Some(ticket)),
                    None => (None, None),
                }
            }
            None => (None, None),
        };
        let group_tx = match self.group_wave.as_ref() {
            Some(w) => w.tx.clone(),
            None => std::sync::Arc::new(crate::wave::engine::GroupTx::new()),
        };
        let engine = crate::wave::engine::start(crate::wave::engine::EngineParams {
            secret,
            we_are_origin: origin,
            peer_addr: addr,
            spool: spool_param,
            cal: self.cal_snapshot(),
            plaid_allowed: self.is_lan_for_plaid(addr),
            lan_prefixes: self.our_v6_prefixes(),
            slot,
            group: Some(crate::wave::engine::GroupLink { lead: lead.clone(), slot_gen, tx: group_tx }),
        });
        // A link that comes up while we are muted starts muted.
        engine.muted.store(self.wave_muted, std::sync::atomic::Ordering::Relaxed);
        crate::logf!("WAVE: group link up with {} on slot {} — media aimed at {}", crate::fp(&device), slot, addr);
        // No address for this device: push ours to it now (its answer is its record), as a 1:1 wave does when it starts without a route.
        if addr == crate::network::status::RELAY_ADDR {
            self.push_address_record_to(vec![device]);
        }
        // The link's beam, both ways: a receiver for their picture, a transport in the fan for ours (beaming or not, so its chain runs from the link's start).
        let beam_rx = crate::wave::beam_session::start_receiver_slot(&secret, origin, slot);
        crate::wave::beam_session::fan_add(slot, &secret, origin);
        if let Some(w) = self.group_wave.as_mut() {
            w.links.push(GroupLinkUp { device, slot, lead, engine, _beam_rx: beam_rx, reconnecting: false, probe: 0, last_probe_osc: 0, spool: ticket });
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
        let mut link = w.links.remove(i);
        crate::platform::audio::far_done_slot(link.slot);
        crate::wave::beam_session::fan_remove(link.slot);
        link.engine.stop();
        // Its recording joins the wave's keep, with the engine thread the keep waits on (the fill drain may still be writing).
        if let Some(t) = link.spool.take() {
            let party = w.joined.iter().find(|m| m.device == link.device).map_or([0u8; 32], |m| m.party);
            w.spent.push((t, link.engine.take_thread(), party));
        }
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
        // The next wave starts with the microphone on.
        self.wave_muted = false;
        // The wave goes on without us while anyone else is in it: leaving, declining or missing it keeps it joinable here (join_live_group_wave).
        if let Some(key) = w.offer_lane_key.filter(|_| !w.joined.is_empty()) {
            self.live_group_waves.retain(|l| l.gid != w.gid);
            self.live_group_waves.push(LiveGroupWave { gid: w.gid, wave_id: w.wave_id, offer_osc: w.offer_osc, key, joined: w.joined.clone() });
        }
        if self.ended_waves.len() > 256 {
            self.ended_waves.clear();
        }
        self.ended_waves.insert(w.wave_id);
        let had_links = !w.links.is_empty();
        if had_links {
            crate::platform::audio::play_end_sweep();
        }
        for mut link in w.links.drain(..) {
            link.engine.stop();
            if let Some(t) = link.spool.take() {
                let party = w.joined.iter().find(|m| m.device == link.device).map_or([0u8; 32], |m| m.party);
                w.spent.push((t, link.engine.take_thread(), party));
            }
        }
        // RECORDED BY DEFAULT: every link's spool goes to one keep off the UI thread, which joins the engines first; the recording lands against the wave's row.
        let spent = std::mem::take(&mut w.spent);
        // The beam video spool stops taking frames; its ticket rides the keep, or its file goes.
        crate::wave::spool::beam_disarm();
        let beam = self.beam_ticket.take();
        if w.ever_linked && !spent.is_empty() {
            self.spawn_group_keep(w.gid, w.offer_osc, w.wave_id, spent, beam);
        } else if let Some(b) = beam {
            crate::wave::spool::beam_discard(b);
        }
        // Our camera: stopped off the UI thread, the phone's pipeline told, the fan gone.
        if let Some(s) = w.beam_tx.take() {
            crate::wave::beam_session::retire(s);
        }
        if let Some(rx) = w.beam_opening.take() {
            crate::wave::beam_session::retire_opening(rx);
        }
        self.beam_armed = false;
        crate::wave::beam_session::android_stop();
        crate::wave::beam_session::preview_stop();
        self.wave_grant_note = None;
        crate::wave::beam_session::fan_clear();
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

    /// THE GROUP DROUGHT (docs/group-waves.md), the 1:1 wave's reconnect carried to each link: two seconds without an authenticated packet on a link marks it reconnecting and pushes our address to that device; a link that has never heard its peer walks the device's candidate addresses, one every two seconds; a link that has carried media keeps its path (a working path is never abandoned — the peer stopped, we are not aimed wrong). No give-up: a link waits until its peer leaves or we do.
    pub(super) fn group_drought_tick(&mut self) {
        const OSC: i64 = vsf::OSCILLATIONS_PER_SECOND as i64;
        use std::sync::atomic::Ordering::Relaxed;
        let Some(w) = self.group_wave.as_ref() else {
            return;
        };
        let now = vsf::eagle_time_oscillations();
        let mut pushes: Vec<[u8; 32]> = Vec::new();
        let mut walks: Vec<(usize, [u8; 32], u32)> = Vec::new();
        let mut changed = false;
        let states: Vec<(usize, bool, i64, i64)> = w
            .links
            .iter()
            .map(|l| {
                let start = crate::wave::SLOT_LINKS.at(l.slot).start_osc.load(Relaxed);
                let heard = crate::wave::SLOT_LINKS.at(l.slot).rx_osc.load(Relaxed);
                (l.slot, l.reconnecting, start, heard)
            })
            .collect();
        let Some(w) = self.group_wave.as_mut() else {
            return;
        };
        for (l, (slot, was, start, heard)) in w.links.iter_mut().zip(states) {
            if start == 0 {
                continue;
            }
            let drought = now - heard.max(start);
            if drought >= 2 * OSC {
                if !was {
                    crate::logf!("WAVE: group link {} (slot {}) silent {}s — reconnecting", crate::fp(&l.device), slot, drought / OSC);
                    l.reconnecting = true;
                    pushes.push(l.device);
                    changed = true;
                }
                if heard == 0 && now - l.last_probe_osc >= 2 * OSC {
                    l.last_probe_osc = now;
                    walks.push((slot, l.device, l.probe));
                    l.probe = l.probe.wrapping_add(1);
                }
            } else if was {
                crate::logf!("WAVE: group link {} (slot {}) — media resumed", crate::fp(&l.device), slot);
                l.reconnecting = false;
                l.probe = 0;
                changed = true;
            }
        }
        if !pushes.is_empty() {
            self.push_address_record_to(pushes);
            self.force_presence_sweep();
        }
        for (slot, dev, n) in walks {
            let cands = self.device_addresses(&dev);
            if !cands.is_empty() {
                let addr = cands[n as usize % cands.len()];
                crate::wave::set_slot_redirect(slot, addr);
                crate::logf!("WAVE: group link {} probe {} of {} — media aimed at {}", crate::fp(&dev), n as usize % cands.len() + 1, cands.len(), addr);
            }
        }
        if changed {
            self.scene_dirty = true;
        }
    }

    /// Where one member device can be reached, best first: a friend's candidates as a 1:1 wave gathers them; a member we never friended from the phonebook — the self-signed record the device published or pushed us (docs/group-waves.md: a member is a person, not a contact).
    pub(super) fn device_addresses(&self, device: &[u8; 32]) -> Vec<std::net::SocketAddr> {
        if let Some(c) = self.contacts.iter().find(|c| !c.is_sibling && c.knows_device(device)) {
            return crate::network::traverse::gather::gather_device_candidates(c, device).sorted().into_iter().map(|x| x.addr).collect();
        }
        let Some(store) = self.peer_store.as_ref() else {
            return Vec::new();
        };
        let Ok(store) = store.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for rec in store.get_all_peers().into_iter().filter(|r| r.device_pubkey.as_bytes() == device) {
            // Their LAN address only when it shares our LAN (the same rule the 1:1 gather keeps), then the public one.
            if let (Some(ip), true) = (rec.local_ip, rec.local_port != 0) {
                let lan = std::net::SocketAddr::new(ip, rec.local_port);
                if self.our_lan_ip.is_some_and(|ours| matches!(ip, std::net::IpAddr::V4(v4) if v4.octets()[..3] == ours.octets()[..3])) {
                    out.push(lan);
                }
            }
            if !crate::network::traverse::gather::is_bogus_addr(&rec.ip) && !out.contains(&rec.ip) {
                out.push(rec.ip);
            }
        }
        out
    }

    /// Is any link of the group wave reconnecting (the status line says so)?
    pub(super) fn group_reconnecting(&self) -> bool {
        self.group_wave.as_ref().is_some_and(|w| w.links.iter().any(|l| l.reconnecting))
    }

    /// The Beam button in a group wave: our camera to every link (the fan), or off again.
    pub(super) fn toggle_group_beam(&mut self) {
        let Some(w) = self.group_wave.as_mut() else {
            return;
        };
        if let Some(rx) = w.beam_opening.take() {
            crate::wave::beam_session::retire_opening(rx);
            crate::log("BEAM: camera open cancelled");
            return;
        }
        if let Some(s) = w.beam_tx.take() {
            crate::wave::beam_session::retire(s);
            crate::log("BEAM: stopped sending to the group");
            return;
        }
        #[cfg(target_os = "android")]
        {
            if crate::wave::beam_session::android_sending() {
                crate::wave::beam_session::android_stop();
                crate::log("BEAM: stopped sending to the group");
            } else if crate::wave::beam_session::android_arm(crate::wave::beam_session::Target::Group) {
                crate::log("BEAM: armed for the group — camera opening");
            } else {
                crate::log("BEAM: service bridge down — cannot beam");
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            w.beam_opening = Some(crate::wave::beam_session::start_desktop_async(crate::wave::beam_session::Target::Group));
        }
    }

    /// The desktop camera worker's answer for a group beam, an edge on the wave tick.
    pub(super) fn drain_group_beam_open(&mut self) {
        let Some(w) = self.group_wave.as_mut() else {
            return;
        };
        let Some(rx) = w.beam_opening.as_ref() else {
            return;
        };
        let got = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                w.beam_opening = None;
                return;
            }
        };
        w.beam_opening = None;
        self.scene_dirty = true;
        match got {
            Ok(tx) if w.phase == GroupPhase::Joined && w.beam_tx.is_none() => w.beam_tx = Some(tx),
            Ok(tx) => crate::wave::beam_session::retire(tx),
            Err(e) => {
                crate::logf!("BEAM: {}", e);
                #[cfg(target_os = "macos")]
                if e == crate::platform::camera_avf::ASKING {
                    self.beam_armed = true;
                }
            }
        }
    }

    /// Is our camera going out (or opening) in the group wave?
    pub(super) fn group_beaming(&self) -> bool {
        self.group_wave.as_ref().is_some_and(|w| w.beam_tx.is_some() || w.beam_opening.is_some()) || (cfg!(target_os = "android") && self.group_wave.is_some() && crate::wave::beam_session::android_sending())
    }

    /// A desktop camera switch mid-beam: the sender restarts on the next camera, the fan carries on.
    #[cfg(not(target_os = "android"))]
    pub(super) fn restart_group_beam(&mut self) {
        let Some(w) = self.group_wave.as_mut() else {
            return;
        };
        if let Some(s) = w.beam_tx.take() {
            crate::wave::beam_session::retire(s);
            w.beam_opening = Some(crate::wave::beam_session::start_desktop_async(crate::wave::beam_session::Target::Group));
        }
    }

    /// Who the wave screen's roll shows, in order: us first once we are in, then every joined device (the origin first) — a friend, or a member we never friended under the name their member record grants.
    pub(super) fn group_roll(&self) -> Vec<RollFace> {
        let Some(w) = self.group_wave.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if w.phase == GroupPhase::Joined {
            if let (Some(d), Some(us)) = (self.our_device(), self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))) {
                out.push(RollFace { device: d, us: true, party: us, ci: None, slot: None, name: String::new() });
            }
        }
        for m in &w.joined {
            let ci = self.contacts.iter().position(|c| !c.is_sibling && c.handle_hash == m.party);
            let slot = w.links.iter().find(|l| l.device == m.device).map(|l| l.slot);
            let name = match ci {
                Some(i) => self.contacts[i].display_name(),
                None => self.member_granted_name(&w.gid, &m.party),
            };
            out.push(RollFace { device: m.device, us: false, party: m.party, ci, slot, name });
        }
        out
    }

    /// Keep every roll face scaled to `diameter` (before the paint borrows `self`): a face is scaled once per person per layout.
    pub(super) fn refresh_group_roll_avatars(&mut self, diameter: usize) {
        let roll = self.group_roll();
        self.group_roll_avatars.retain(|(d, diam, _)| *diam == diameter && roll.iter().any(|r| r.device == *d));
        for face in roll {
            if self.group_roll_avatars.iter().any(|(d, _, _)| *d == face.device) {
                continue;
            }
            let px = if face.us {
                let proof = self.session.as_ref().map(|s| s.handle_proof).unwrap_or_default();
                match self.device_avatar_pixels.as_ref() {
                    Some(base) => crate::ui::avatar_render::update_avatar_scaled(base, crate::ui::avatar::AVATAR_SIZE, diameter),
                    None => gradient_avatar_rgb(proof_gradient_seed(&proof), diameter),
                }
            } else {
                match face.ci.map(|i| &self.contacts[i]) {
                    Some(c) => match c.avatar_pixels.as_ref() {
                        Some(base) => crate::ui::avatar_render::update_avatar_scaled(base, crate::ui::avatar::AVATAR_SIZE, diameter),
                        None => gradient_avatar_rgb(proof_gradient_seed(&c.handle_proof), diameter),
                    },
                    // A member we never friended: their party's gradient, as their person page draws them.
                    None => gradient_avatar_rgb(proof_gradient_seed(&face.party), diameter),
                }
            };
            self.group_roll_avatars.push((face.device, diameter, px));
        }
    }

    /// The group keep, off the UI thread: wait for every link's engine (the fill drain may still be writing), merge the spools into one recording, store it; the drain lands the row.
    fn spawn_group_keep(&mut self, gid: MoleculeId, offer_osc: i64, wave_id: [u8; 16], spent: Vec<(crate::wave::spool::SpoolTicket, Option<std::thread::JoinHandle<()>>, [u8; 32])>, beam: Option<crate::wave::spool::BeamTicket>) {
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let tx = self.wave_keep_sender();
        let wake = self.event_proxy.clone();
        self.pending_keep_hold = 1;
        let wave_id8: [u8; 8] = wave_id[..8].try_into().unwrap();
        // The recording's people in channel order — ours on channel 0, then each spool's far party in the order merge_group_records lays them out (zero = a link whose party was not known).
        let us = crate::crypto::clutch::identity_party_id(&seed);
        let parties: Vec<[u8; 32]> = std::iter::once(us).chain(spent.iter().map(|s| s.2)).collect();
        let spawned = std::thread::Builder::new()
            .name("wave-keep".into())
            .spawn(move || {
                #[cfg(target_os = "android")]
                {
                    // SAFETY: setpriority on the calling thread (who 0) takes no pointers.
                    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -4) } != 0 {
                        crate::logf!("WAVE: group keep thread priority raise refused: {}", std::io::Error::last_os_error());
                    }
                }
                let mut tickets = Vec::new();
                for (t, th, _) in spent {
                    if let Some(th) = th {
                        if th.join().is_err() {
                            crate::log("WAVE: a link engine thread panicked before the group keep — its spool merges as it stands");
                        }
                    }
                    tickets.push(t);
                }
                let result = crate::wave::record::finalize_group(tickets, &seed);
                // The beam's video beside it, when it was saved.
                let video = beam.and_then(|b| crate::wave::beam_keep::keep(b, &seed));
                crate::log_err!(tx.send(super::wave_ui::WaveKeepResult { peer: [0u8; 32], offer_osc, wave_id8, result, group: Some(gid), parties, video }), "the group wave keep result to the UI thread");
                if let Some(w) = wake {
                    super::wake_ui(&w);
                }
            })
            .is_ok();
        if !spawned {
            crate::log("WAVE: group keep thread failed to spawn — recording not kept");
        }
    }

    /// The group keep landed: the recording row in the molecule's conversation against the wave's row, carried to our siblings (each member's fleet keeps its own recording, as in a pair).
    pub(super) fn land_group_keep(&mut self, gid: MoleculeId, offer_osc: i64, kept: Option<crate::wave::record::Kept>, parties: Vec<[u8; 32]>, video: Option<crate::wave::beam_keep::VideoKept>) {
        let Some(kept) = kept else {
            crate::log("WAVE: group recording not kept — the WAVE: group keep line(s) above say why");
            return;
        };
        let id = crate::types::FriendshipId::from_bytes(gid.0);
        let file = crate::types::AttachRef { hash: kept.hash, name: String::new(), size: kept.size, role: crate::types::AttachRole::WaveAudio, head: None, wrap: None, parties };
        let mut row = ChatMessage::attachment(file, true, offer_osc + 2).with_reference(crate::types::RefKind::Wave, offer_osc + 1);
        row.notified = true;
        row.delivered = true;
        row.author = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        let mut pushed = vec![row.clone()];
        // The beams' video (saved only when the user turned it on): its own fleet-internal row at +4, each track named by its person.
        if let Some(v) = video {
            let vfile = crate::types::AttachRef { hash: v.hash, name: String::new(), size: v.size, role: crate::types::AttachRole::BeamVideo, head: None, wrap: None, parties: v.tracks };
            let mut vrow = ChatMessage::attachment(vfile, true, offer_osc + 4).with_reference(crate::types::RefKind::Wave, offer_osc + 1);
            vrow.notified = true;
            vrow.delivered = true;
            vrow.author = row.author;
            pushed.push(vrow.clone());
            if let Some(conv) = self.conv_by_id_mut(id) {
                conv.insert_message_sorted(vrow);
            }
        }
        if let Some(conv) = self.conv_by_id_mut(id) {
            conv.insert_message_sorted(row);
            if let Some(w) = conv.messages.iter_mut().find(|m| m.timestamp == offer_osc + 1 && m.wave.is_some()) {
                let before = w.wave;
                crate::types::merge_wave_fields(w, Some(WaveInfo { outcome: WaveOutcome::Answered, secs: kept.secs }), &[]);
                if w.wave != before {
                    pushed.push(w.clone());
                }
            }
        }
        self.persist_and_push_rows(id, &pushed);
        crate::logf!("WAVE: group recording kept — {} bytes as blob {}…, {} s", kept.size, hex::encode(&kept.hash[..4]), kept.secs);
        self.space_check("a group recording kept");
        self.scene_dirty = true;
    }

    fn stop_group_ring_platform() {
        #[cfg(target_os = "android")]
        crate::platform::jni_android::cancel_wave_notification();
    }

    /// The wave screen's name for a group wave: the molecule's title.
    pub(super) fn group_wave_title(&self) -> Option<String> {
        let w = self.group_wave.as_ref()?;
        self.molecule_rosters.iter().find(|(g, _)| *g == w.gid).map(|(_, r)| r.display_title())
    }

    /// The contact the wave screen shows for a group wave: the first other device's person (the origin while ringing).
    pub(super) fn group_wave_face(&self) -> Option<usize> {
        let w = self.group_wave.as_ref()?;
        let party = w.links.first().and_then(|l| w.joined.iter().find(|m| m.device == l.device)).or_else(|| w.joined.first())?.party;
        self.contacts.iter().position(|c| !c.is_sibling && c.handle_hash == party)
    }
}
