//! Call state machine (docs/calls.md) — offer/answer/hangup edges over the in-lane signaling rows.
//!
//! Doctrine, restated where it bites:
//! - **No ring timer.** Ringing stops on answer/decline edges, a sibling's answer, or the caller's hangup — the caller's patience IS the timeout, and an unanswered hangup mints the missed-call row.
//! - **Ring requires DIRECT decrypt.** Every online callee device receives the offer frame itself (chat frames already fan to the whole fold via the relay list) and captures the offer's lane key at decrypt — the basket's doomed egg. A signal arriving via SIBLING MERGE is history, not a doorbell: merge signals only ever STOP rings, which also kills the whole stale-offer-rings-days-later class (a woke device replays old signals and correctly rings for none of them).
//! - **The summary row is the call's visible record**: minted at the end by the devices that lived the call, stamped offer_osc+1 on BOTH fleets (each side mints its own copy; the shared stamp makes sibling-merge dedup fold the copies).

use super::*;
use crate::call::signal::CallSignal;
use crate::call::{ActiveCall, CallPhase};
use crate::types::{WaveInfo, WaveOutcome};

/// A finished keep-transcode, posted from the worker thread back to the UI thread to mint the `call.audio` row. `result` is `None` when the recording was empty or the transcode failed (treated as delete).
pub(super) struct CallKeepResult {
    peer: [u8; 32],
    offer_osc: i64,
    call_id8: [u8; 8],
    result: Option<crate::call::record::Kept>,
}

/// Format a duration base-aware: `M:SS` in dozenal and arabic, the plain seconds count in hex (hex is linear everywhere, Nick 2026-09-11). A free function so the render can call it under its chrome borrow (the method form reads `&self`). Rendered in the Oxanium face so the dozenal `+glyphs` control-block glyphs resolve. Dozenal seconds pad to two dozenal digits (0–4B).
pub(super) fn fmt_duration_secs(secs: i64) -> String {
    let secs = secs.max(0);
    let (m, s) = ((secs / 60) as u32, (secs % 60) as u32);
    match crate::num_base() {
        crate::NumBase::Dozenal => {
            let mut ss = crate::dozenal_glyphs(s);
            if s < 12 {
                ss = format!("{}{}", char::from(0x10), ss); // two-digit pad, dozenal zero
            }
            format!("{}:{}", crate::dozenal_glyphs(m), ss)
        }
        crate::NumBase::Hex => crate::hex_linear(secs as u64),
        crate::NumBase::Arabic => format!("{}:{:02}", m, s),
    }
}

#[cfg(test)]
mod duration_kat {
    use super::fmt_duration_secs;
    use crate::base_kat::hold_base;
    use crate::NumBase;

    #[test]
    fn hex_is_the_seconds_count_and_the_others_are_minutes_and_seconds() {
        {
            let _g = hold_base(NumBase::Hex);
            assert_eq!(fmt_duration_secs(3661), "E4D");
            assert_eq!(fmt_duration_secs(0), "0");
        }
        {
            let _g = hold_base(NumBase::Arabic);
            assert_eq!(fmt_duration_secs(3661), "61:01");
            assert_eq!(fmt_duration_secs(0), "0:00");
        }
        {
            let _g = hold_base(NumBase::Dozenal);
            let d = |v: u8| char::from(0x10 + v);
            assert_eq!(fmt_duration_secs(3661), format!("{}{}:{}{}", d(5), d(1), d(0), d(1)));
        }
    }
}

/// The wave card's header line for a wave row: outcome word + live duration, in the current language and base.
pub(super) fn wave_header(w: crate::types::WaveInfo) -> String {
    use crate::types::WaveOutcome as O;
    let dur = fmt_duration_secs(w.secs as i64);
    match w.outcome {
        O::Answered => tr(Msg::CallEndedDur(&dur)).into_owned(),
        O::Dropped => tr(Msg::CallDroppedDur(&dur)).into_owned(),
        O::Missed => tr(Msg::MissedCallRow).into_owned(),
        O::Rejected => tr(Msg::RejectedWaveRow).into_owned(),
        O::Declined => tr(Msg::CallDeclinedRow).into_owned(),
        O::Busy => tr(Msg::BusyRow).into_owned(),
    }
}

impl PhotonApp {
    /// Format a call duration as `M:SS`, base-aware (the About-page dozenal toggle). Rendered in the Oxanium face so the dozenal `+glyphs` control-block glyphs resolve. Dozenal seconds pad to two dozenal digits (0–4B).
    pub(super) fn fmt_duration(&self, secs: i64) -> String {
        fmt_duration_secs(secs)
    }

    /// Poll the retained call Buttons' rising-edge clicks (docs/calls.md) — mirrors the attest/+/send pattern: `dispatch_release` (or a focused-key activation) fired `on_click`; we observe the edge here and run the phase's action. Called from BOTH the Released arm and the key path so pointer taps and Enter/Space on a focused call button both fire exactly once. The verb is phase-driven: action = Answer/Keep/Hang up, decline = Decline/Delete, start = place the call.
    pub(super) fn dispatch_call_button_clicks(&mut self, ctx: &mut Context) -> bool {
        let phase = self.active_call.as_ref().map(|c| c.phase);
        let mut any = false;
        if self
            .call_action_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            // Name the tap (field 2026-09-02): the SHARED action button means Answer/Keep/Hang-up by phase — a spurious click just after Ringing→Active would read as a hangup. Logging the phase at click catches a double-fire race.
            crate::logf!("CALL: action button clicked (phase {})", format!("{:?}", phase));
            match phase {
                Some(CallPhase::Ringing) => self.answer_call(),
                Some(_) => self.hangup_call(),
                None => {}
            }
            any = true;
        }
        if self
            .call_decline_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            match phase {
                Some(CallPhase::Ringing) => self.decline_call(),
                _ => {}
            }
            any = true;
        }
        if self
            .call_reject_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            if matches!(phase, Some(CallPhase::Ringing)) {
                self.reject_call();
            }
            any = true;
        }
        if self
            .call_start_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
            && self.active_call.is_none()
        {
            if let Some(ci) = self.active_contact() {
                self.start_call(ci);
                any = true;
            }
        }
        // Speaker toggle PARKED (Nick 2026-09-03): headset is the only mode for now — a fixed 4-stop output pad in the engine (OUTPUT_PAD_STOPS) with the duck dropping further from there; re-arm this handler when a real speaker route lands.
        // if self
        //     .call_speaker_btn
        //     .as_mut()
        //     .map(|b| b.take_click())
        //     .unwrap_or(false)
        // {
        //     if matches!(phase, Some(CallPhase::Active)) {
        //         self.call_speaker_on = !self.call_speaker_on;
        //         let route = if self.call_speaker_on {
        //             crate::platform::audio::AudioRoute::Speaker
        //         } else {
        //             crate::platform::audio::AudioRoute::Earpiece
        //         };
        //         crate::platform::audio::set_route(route);
        //     }
        //     any = true;
        // }
        // Add-handle (Active) — stubbed; multi-party call join is a follow-up.
        if self
            .call_addhandle_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            crate::log("CALL: add-handle tapped (stub — multi-party is a follow-up)");
            any = true;
        }
        // Back to contact (Active) — minimize the full-screen call to the app-wide strip / compact bar.
        if self
            .call_back_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            if matches!(phase, Some(CallPhase::Active)) {
                self.minimize_call_to_contact();
            }
            any = true;
        }
        if any {
            ctx.window.request_redraw();
        }
        any
    }

    /// Minimize the in-call full-screen to the app-wide strip / compact bar and navigate to the peer's conversation, so messaging + scrolling stay live during the call. The call keeps running (only the modal panel yields).
    fn minimize_call_to_contact(&mut self) {
        self.call_minimized = true;
        let peer = self.active_call.as_ref().map(|c| c.peer_handle_hash);
        if let Some(peer) = peer {
            if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
                self.open_conversation_with(ci);
            }
        }
        self.scene_dirty = true;
    }

    /// A recording finished playing on its own → drop the handle + the which-row marker so the bubble flips back to ▶ (edge poll on the worker's done flag, no timer).
    pub(super) fn tick_playback_done(&mut self) {
        if self.call_playback.as_ref().is_some_and(|p| p.is_finished()) {
            self.call_playback = None;
            self.call_playback_hash = None;
            self.scene_dirty = true;
        } else if self.call_playback.is_some() {
            // Still playing → repaint so the bubble's ■ progress % advances (a bounded, user-initiated activity; the per-tick cost is one repaint while a recording plays).
            self.scene_dirty = true;
        }
    }

    /// Toggle playback of a kept recording bubble: tap the playing one → stop; tap any other → play it. `blob` = the recording's content hash (the bubble's identity). No force-close to stop anymore (field 2026-09-08).
    pub(super) fn toggle_recording_playback(&mut self, blob: [u8; 32]) {
        if self.call_playback.is_some() && self.call_playback_hash == Some(blob) {
            self.call_playback = None; // drop = stop
            self.call_playback_hash = None;
            self.scene_dirty = true;
            return;
        }
        self.play_recording_from(blob, 0);
    }

    /// Start (or restart) playback of a kept recording at archive slot `slot` — the wave card's tap-to-seek and scrub-release edge. Any prior playback stops (the handle drop). The card itself is the feedback; the only toast is the refusal (a wave is live and owns the audio session).
    pub(super) fn play_recording_from(&mut self, blob: [u8; 32], slot: usize) {
        // Already playing THIS recording: seek the live stream in place — never tear down and respawn (the old worker's audio release raced the new spawn's "is a wave active?" check and the scrub read "can't play now").
        if self.call_playback_hash == Some(blob) {
            if let Some(h) = self.call_playback.as_ref() {
                h.seek(slot);
                self.scene_dirty = true;
                return;
            }
        }
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        self.call_playback = None;
        self.call_playback = crate::call::playback::play_blob_from(&seed, &blob, slot);
        self.call_playback_hash = self.call_playback.as_ref().map(|_| blob);
        if self.call_playback.is_none() {
            self.ready_toast = Some(tr(Msg::CantPlayNow).into_owned());
        }
        self.scene_dirty = true;
    }

    /// Place a call to the open (or named) contact. One live call at a time — v1 is singular by design.
    pub(super) fn start_call(&mut self, ci: usize) {
        if self.active_call.is_some() {
            crate::log("CALL: already in a call");
            return;
        }
        let Some(contact) = self.contacts.get(ci) else {
            return;
        };
        if contact.is_sibling {
            crate::log("CALL: sibling rows have no call plane (v1)");
            return;
        }
        if !contact.is_online {
            crate::log("CALL: contact offline — v1 rings online devices only");
            return;
        }
        let peer = contact.handle_hash;
        let contact_validated = contact.validated_path.is_some();
        // Ringback digest resolved here, while `contact` is still borrowed — the send below takes &mut self.
        let ringback_digest = self.our_party_id(contact).map(|us| relationship_digest(&peer, &us));
        let call_id: [u8; 16] = rand::random();
        let caller_nonce: [u8; 32] = rand::random();
        let sig = CallSignal::Offer {
            call_id,
            nonce: caller_nonce,
            // Name the originating device: the callee routes its answer at THIS device's freshest address (not the offer's possibly-stale source), and our siblings get a name for the wave-in-progress chip.
            device: self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()),
        };
        // The ring lease's audibility: the ringback rides the same relationship digest the callee's own ring does — what we hear IS their cadence — and honors the same "Ring on incoming call" tick as the inbound ring.
        let ring_audible = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("notify.ring_call"))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(true);
        // Fresh call: clear any stale minimize / speaker state and stop a recording preview (the call owns the audio session).
        self.call_minimized = false;
        self.call_speaker_on = false;
        self.call_playback.take();
        let now = vsf::eagle_time_oscillations();
        // THIS device placed this call — the only license to loud-kill a strayed answer for it later (see the Answer arm's sibling law).
        self.dialed_call_ids.insert(call_id);
        // The Outgoing record exists BEFORE the offer goes out: the offer's lane key is captured at its send COMMIT (messaging.rs drain_braid_tx), and that commit can drain during the send itself when the chain is already at its window (2026-09-10 Esme/Nick: two dials in a row committed inside the send, found no active call, captured nothing — the express offer never fired and the callee's ring lease lapsed at 3 s).
        self.active_call = Some(ActiveCall {
            call_id,
            peer_handle_hash: peer,
            we_are_caller: true,
            phase: CallPhase::Outgoing,
            phase_osc: now,
            final_osc: None,
            offer_osc: now,
            caller_nonce,
            callee_nonce: None,
            offer_lane_key: None, // filled by the drain_braid_tx capture when the offer commits
            secret: None,
            engine: None,
            spool: None,
            ring: None,
            ringback: None, // started below, once the offer is actually on its way
            express_addr: None,
            peer_device: None,
            reconnecting: false,
            last_anchor_osc: 0,
            last_beat_osc: now,
            express_key: None,
            express_beats: 0,
            reconnect_probe: 0,
        });
        if !self.send_call_signal(ci, sig, now) {
            crate::log("CALL: offer send failed (no lane) — not dialing");
            self.active_call = None;
            self.dialed_call_ids.remove(&call_id);
            return;
        }
        let Some(ringback_digest) = ringback_digest else {
            crate::log("CALL: no party id for the callee — dialing without ringback");
            self.scene_dirty = true;
            return;
        };
        // Ringback: the CALLEE's ring in OUR ear (their identity cadence, so we hear who we're waving), padded to the same headset level as the wave that follows. It also probes the room's coupling (the chirp anchor) so the echo filter has a fit when media lands.
        if ring_audible {
            let rb = crate::call::ringback::start(ringback_digest);
            if let Some(call) = self.active_call.as_mut() {
                call.ringback = rb;
            }
        }
        if contact_validated {
            crate::logf!("CALL: dialing {} (id {})", crate::fp(&peer), hex::encode(&call_id[..4]));
        } else {
            crate::logf!("CALL: dialing {} (id {}) — NO validated direct path; media waits on a punch or the peer's first packet (relay media does not exist yet)", crate::fp(&peer), hex::encode(&call_id[..4]));
        }
        self.scene_dirty = true;
    }

    /// Answer the ringing call — the take-the-ball edge.
    pub(super) fn answer_call(&mut self) {
        let Some(call) = self.active_call.as_ref() else {
            return;
        };
        if call.phase != CallPhase::Ringing {
            return;
        }
        let (call_id, peer, caller_nonce, offer_lane_key) = (
            call.call_id,
            call.peer_handle_hash,
            call.caller_nonce,
            call.offer_lane_key,
        );
        let Some(ci) = self.contact_index_by_handle_hash(&peer) else {
            return;
        };
        let Some(offer_key) = offer_lane_key else {
            // Should be unreachable: ring required direct decrypt, which captured the key.
            crate::log("CALL: answering without the offer lane key — declining instead (basket incomplete)");
            self.decline_call();
            return;
        };
        let callee_nonce: [u8; 32] = rand::random();
        let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        if !self.send_call_signal(ci, CallSignal::Answer { call_id, nonce: callee_nonce, device: our_device }, vsf::eagle_time_oscillations()) {
            // The reason is already logged by chain_transmit. Field 2026-09-08: a re-CLUTCH with the caller's fleet was mid-ceremony, the friendship had no chain, and the tap silently did nothing while the ring kept going — say so on screen.
            let name = self.contacts.get(ci).map(|c| c.display_name()).unwrap_or_default();
            crate::log("CALL: answer send failed — the friendship can't carry a frame right now (ceremony in progress?)");
            self.ready_toast = Some(tr(Msg::AnswerFailedReconnecting(&name)).into_owned());
            self.scene_dirty = true;
            return;
        }
        // Answering owns the audio session — stop any recording preview, and clear stale minimize/speaker state.
        self.call_playback.take();
        self.call_minimized = false;
        self.call_speaker_on = false;
        let secret = self.derive_secret_for(ci, &offer_key, &call_id, &caller_nonce, &callee_nonce);
        let (engine, spool) = self.spawn_call_engine(ci, &call_id, secret, false);
        if let Some(call) = self.active_call.as_mut() {
            call.callee_nonce = Some(callee_nonce);
            call.secret = secret;
            call.phase = CallPhase::Active;
            call.phase_osc = vsf::eagle_time_oscillations();
            call.engine = engine;
            call.spool = spool;
            call.ring = None; // Ringing → Active keeps the ActiveCall, so the guard needs an explicit stop here
        }
        crate::logf!("CALL: answered (id {})", hex::encode(&call_id[..4]));
        Self::stop_ring_alert_platform();
        self.scene_dirty = true;
    }

    /// Ring-stop edge, platform half: tear down the Android call-class notification (it's ongoing — never auto-cancels). Desktop's chirp loop stops via the RingGuard on ActiveCall teardown.
    fn stop_ring_alert_platform() {
        #[cfg(target_os = "android")]
        crate::platform::jni_android::cancel_call_notification();
    }

    /// Decline the ringing call.
    pub(super) fn decline_call(&mut self) {
        let Some(call) = self.active_call.as_ref() else {
            return;
        };
        if call.phase != CallPhase::Ringing {
            return;
        }
        let (call_id, peer, offer_osc) = (call.call_id, call.peer_handle_hash, call.offer_osc);
        if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
            let _ = self.send_call_signal(ci, CallSignal::Decline { call_id }, vsf::eagle_time_oscillations());
        }
        self.end_call(WaveOutcome::Declined, offer_osc);
    }

    /// REJECT (Nick 2026-09-09): dismiss the ring across the fleet WITHOUT telling the caller. No signal leaves the fleet; the caller's own patience ends their side (they mint the missed wave). The wave row's Rejected outcome rides the ordinary sibling push and stops every sibling's ring on merge; the call id and offer stamp are remembered so a re-expressed offer never rings again.
    pub(super) fn reject_call(&mut self) {
        let Some(call) = self.active_call.as_ref() else {
            return;
        };
        if call.phase != CallPhase::Ringing {
            return;
        }
        let (call_id, offer_osc) = (call.call_id, call.offer_osc);
        self.rejected_calls.insert(call_id);
        self.rejected_offers.insert(offer_osc);
        crate::logf!("CALL: rejected {} silently (fleet-wide stop, no signal)", hex::encode(&call_id[..4]));
        self.end_call(WaveOutcome::Rejected, offer_osc);
    }

    /// A sibling's REJECT reached this device as a wave row (outcome Rejected, stamped offer_osc+1): stop our ring for that offer silently and remember it, whether our ring has started yet or not.
    pub(super) fn on_sibling_reject(&mut self, wave_ts: i64) {
        let offer_osc = wave_ts - 1;
        self.rejected_offers.insert(offer_osc);
        let ringing = self.active_call.as_ref().filter(|c| c.phase == CallPhase::Ringing && c.offer_osc == offer_osc).map(|c| c.call_id);
        if let Some(call_id) = ringing {
            self.rejected_calls.insert(call_id);
            crate::logf!("CALL: sibling rejected {} — ring stops here", hex::encode(&call_id[..4]));
            self.active_call = None;
            Self::stop_ring_alert_platform();
            self.call_minimized = false;
            self.scene_dirty = true;
        }
    }

    /// Hang up — covers the caller abandoning an unanswered ring (the human timeout) AND either side ending an active call.
    pub(super) fn hangup_call(&mut self) {
        let Some(call) = self.active_call.as_ref() else {
            return;
        };
        let (call_id, peer, phase, offer_osc, phase_osc, we_are_caller) = (
            call.call_id,
            call.peer_handle_hash,
            call.phase,
            call.offer_osc,
            call.phase_osc,
            call.we_are_caller,
        );
        crate::logf!("CALL: hangup_call (phase {}, id {})", format!("{:?}", phase), hex::encode(&call_id[..4]));
        if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
            let _ = self.send_call_signal(ci, CallSignal::Hangup { call_id }, vsf::eagle_time_oscillations());
        }
        let _ = phase_osc; // the wave row's seconds come from the phase base inside end_call
        let outcome = match phase {
            CallPhase::Outgoing if we_are_caller => WaveOutcome::Missed,
            CallPhase::Active => WaveOutcome::Answered,
            // Hanging up a ringing call IS declining it.
            CallPhase::Ringing => WaveOutcome::Declined,
            _ => WaveOutcome::Answered,
        };
        self.end_call(outcome, offer_osc);
    }

    /// Launch-time wave recovery (record-by-default durability): finish any keep a crash/battery-death interrupted. Once per session, after the vault + session are up — orphaned spool files with surviving registers re-enter the NORMAL keep-transcode path and the wave appears in its conversation as if the hangup had completed.
    pub(super) fn recover_orphan_waves(&mut self) {
        if self.orphan_waves_swept || self.session.is_none() || crate::storage::device_vault().is_none() {
            return;
        }
        self.orphan_waves_swept = true;
        let seed = self.session.as_ref().map(|s| s.identity_seed).unwrap_or([0u8; 32]);
        for (ticket, peer, offer_osc, id8) in crate::call::spool::recover_orphans() {
            crate::logf!(
                "CALL: recovering orphaned wave spool ({}) — the crash interrupted its keep; transcoding now",
                hex::encode(id8)
            );
            self.spawn_keep_transcode(ticket, peer, offer_osc, seed, id8, None);
        }
    }

    /// The ring-lease heartbeat (Nick 2026-09-08). Caller: while Outgoing, re-express the offer every ~1s so every ringing callee device keeps its lease — the beat stops the instant we leave Outgoing (answered by anyone / hung up), which is the universal, loss-proof stop. Callee: a Ringing call with no offer beat for ~3s lapses SILENTLY (no row — the caller signs the missed-wave record, never the receiver). Instant stops (Hangup/Decline/sibling-answer) still fire on their edges; this is the backstop for when no edge is ever delivered (the desktop+mac forever-ring, 2026-09-08).
    pub(super) fn call_ring_tick(&mut self) {
        const OSC: i64 = vsf::OSCILLATIONS_PER_SECOND as i64;
        let now = vsf::eagle_time_oscillations();
        let Some(call) = self.active_call.as_ref() else {
            return;
        };
        match call.phase {
            CallPhase::Outgoing if call.we_are_caller => {
                if now - call.last_beat_osc < OSC {
                    return;
                }
                let (call_id, nonce, offer_key, peer) =
                    (call.call_id, call.caller_nonce, call.offer_lane_key, call.peer_handle_hash);
                let Some(offer_key) = offer_key else {
                    return; // no lane key captured yet — the first express already carries it once committed
                };
                let Some(ci) = self.contact_index_by_handle_hash(&peer) else {
                    return;
                };
                let device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
                let sig = CallSignal::Offer { call_id, nonce, device };
                self.send_express_signal(ci, &sig, now, Some(offer_key));
                if let Some(c) = self.active_call.as_mut() {
                    c.last_beat_osc = now;
                }
            }
            CallPhase::Ringing => {
                // The lease runs on the express cadence (3 s) only once an express beat has been seen; a ring that came by the lane alone keeps a 20 s lease, because a caller who vanishes also reaches us as a hangup row and a lane-only peer has no beats to miss.
                let lease_secs: i64 = if call.express_beats > 0 { 3 } else { 20 };
                if now - call.last_beat_osc >= lease_secs * OSC {
                    crate::logf!(
                        "CALL: ring lease lapsed — no offer beat for {}s ({} express beat(s) seen), caller stopped (id {})",
                        lease_secs,
                        call.express_beats,
                        hex::encode(&call.call_id[..4])
                    );
                    self.active_call = None;
                    Self::stop_ring_alert_platform();
                    self.scene_dirty = true;
                }
            }
            _ => {}
        }
    }

    /// Media-liveness measurement (edges-not-timers: packet arrival IS the event stream; this is a measurement cadence on it, like the learner tick or PT's RTO — never UI timing). Receive drought past the reconnect line → panel shows reconnecting + anchors fire at the peer's freshest paths; past the drop line → honest teardown with a dropped summary, because a silently-dead Active call the human must notice and kill is the worse experience. The engine's mute-transmits-zeros contract keeps a muted peer from ever reading as a drought.
    pub(super) fn call_drought_tick(&mut self) {
        if let Some(dev) = self.call_needs_addresses.take() {
            self.push_address_record_to(vec![dev]);
            self.force_presence_sweep();
        }
        const OSC: i64 = vsf::OSCILLATIONS_PER_SECOND as i64;
        let (reconnect_after, anchor_every, drop_after) = (5 * OSC, 2 * OSC, 30 * OSC);
        let Some(call) = self.active_call.as_ref() else {
            return;
        };
        if call.phase != CallPhase::Active || call.engine.is_none() {
            return;
        }
        let (call_id, peer, offer_osc, was_reconnecting, last_anchor) = (
            call.call_id,
            call.peer_handle_hash,
            call.offer_osc,
            call.reconnecting,
            call.last_anchor_osc,
        );
        use std::sync::atomic::Ordering::Relaxed;
        let start = crate::call::MEDIA_START_OSC.load(Relaxed);
        if start == 0 {
            return;
        }
        let last_rx = crate::call::LAST_MEDIA_RX_OSC.load(Relaxed).max(start);
        let now = vsf::eagle_time_oscillations();
        let drought = now - last_rx;
        if drought >= drop_after {
            crate::logf!(
                "CALL: dropped — no authenticated media for {}s (id {})",
                drought / OSC,
                hex::encode(&call_id[..4])
            );
            // The Hangup rides the durable lane row: a far end alive behind a dead path converges the moment any path heals, and the row tombstones the chip fleet-wide.
            if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
                let _ = self.send_call_signal(ci, CallSignal::Hangup { call_id }, vsf::eagle_time_oscillations());
            }
            self.end_call(WaveOutcome::Dropped, offer_osc);
            return;
        }
        if drought >= reconnect_after {
            if !was_reconnecting {
                crate::logf!(
                    "CALL: receive drought {}s — reconnecting, anchors firing (id {})",
                    drought / OSC,
                    hex::encode(&call_id[..4])
                );
                if let Some(c) = self.active_call.as_mut() {
                    c.reconnecting = true;
                }
                self.scene_dirty = true;
            }
            if now - last_anchor >= anchor_every {
                if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
                    // MEDIA FOLLOWS THE SEARCH (2026-09-11): the engine's TX is pinned to the address the call opened on; after a network change that address is a black hole and nothing re-points it, because re-pointing waits on media we can no longer receive. Each anchor round moves TX to the next candidate, so within a few rounds we are sending at the peer's live address — which is also what opens our NAT for its return path.
                    // THE CALL PEER'S DEVICE ONLY: a wave is with one device, and the first version of this probe walked the contact's whole roster — Emma's recovery aimed at Nick's DESKTOP while the wave was with his phone (field 2026-09-11).
                    let peer_dev = self.active_call.as_ref().and_then(|c| c.peer_device);
                    let cands: Vec<std::net::SocketAddr> = match (self.contacts.get(ci), peer_dev) {
                        (Some(c), Some(dev)) => crate::network::traverse::gather::gather_device_candidates(c, &dev).sorted().into_iter().map(|x| x.addr).collect(),
                        _ => Vec::new(),
                    };
                    if !cands.is_empty() {
                        let n = self.active_call.as_ref().map_or(0, |c| c.reconnect_probe) as usize;
                        let addr = cands[n % cands.len()];
                        crate::call::set_peer_redirect(addr);
                        crate::logf!("CALL: reconnect probe {} of {} — media aimed at {}", n % cands.len() + 1, cands.len(), addr);
                        if let Some(c) = self.active_call.as_mut() {
                            c.reconnect_probe = c.reconnect_probe.wrapping_add(1);
                        }
                    }
                    self.send_express_signal(ci, &CallSignal::Anchor { call_id }, now, None);
                }
                if let Some(c) = self.active_call.as_mut() {
                    c.last_anchor_osc = now;
                }
            }
        } else if was_reconnecting {
            crate::log("CALL: media resumed — reconnected");
            if let Some(c) = self.active_call.as_mut() {
                c.reconnecting = false;
                c.reconnect_probe = 0;
            }
            self.scene_dirty = true;
        }
    }

    /// One inbound signal — from the friend's lane directly (`rx_lane_key` present, this device decrypted it) or from a sibling's row push (merge; stop-edges only).
    pub(super) fn on_call_signal(
        &mut self,
        ci: usize,
        sig: CallSignal,
        rx_lane_key: Option<[u8; 32]>,
        row_ts: i64,
        from_merge: bool,
        row_is_outgoing: bool,
    ) {
        let peer = match self.contacts.get(ci) {
            Some(c) if !c.is_sibling => c.handle_hash,
            _ => return,
        };
        // Presence-chip tombstone: any terminal signal for the chip's call clears it, whichever direction it rode in (my sibling hung up, the friend hung up, decline, busy) — the summary/terminal rows replicate everywhere, so every device converges. Taken is deliberately NOT terminal (the call continues on the winner).
        if matches!(sig, CallSignal::Hangup { .. } | CallSignal::Decline { .. } | CallSignal::Busy { .. }) {
            if let Some((chip_id, _, _)) = self.fleet_call_elsewhere {
                if chip_id == *sig.call_id() {
                    self.fleet_call_elsewhere = None;
                    self.scene_dirty = true;
                }
            }
        }
        match sig {
            // A rejected call never rings again: not on a re-expressed offer, not on a late-arriving one after a sibling's reject.
            CallSignal::Offer { call_id, .. } if !row_is_outgoing && (self.rejected_calls.contains(&call_id) || self.rejected_offers.contains(&row_ts)) => {}
            CallSignal::Offer { call_id, nonce, device } if !row_is_outgoing => {
                match &self.active_call {
                    Some(c) if c.call_id == call_id => {
                        // A repeated offer for the LIVE ringing call is the caller's heartbeat — renew the ring lease (call_ring_tick lapses it after 3 missed beats).
                        if c.phase == CallPhase::Ringing {
                            if let Some(call) = self.active_call.as_mut() {
                                call.last_beat_osc = vsf::eagle_time_oscillations();
                            }
                        }
                    }
                    // GLARE (field 2026-09-02, Emma+Nick dialing each other in the same second → mutual auto-BUSY, no ring, no notification, two "missed call" rows and no call): an offer from the peer we are currently CALLING means both humans pressed the button — both consent, so CONNECT, never refuse. Deterministic fold from symmetric information: the smaller call_id is THE call; the larger-id side quietly drops its own outgoing (no hangup spray, no missed-call row — the peer is ignoring that offer by the same rule) and answers the winner. Both sides compute the same rule on the same two ids, so exactly one call survives.
                    Some(c)
                        if c.peer_handle_hash == peer
                            && matches!(c.phase, CallPhase::Outgoing)
                            && !from_merge =>
                    {
                        if call_id < c.call_id {
                            // Their call wins: fold ours into answering theirs.
                            let Some(offer_key) = rx_lane_key else {
                                return;
                            };
                            crate::logf!(
                                "CALL: glare with {} — folding our outgoing {} into answering their {}",
                                crate::fp(&peer),
                                hex::encode(&c.call_id[..4]),
                                hex::encode(&call_id[..4])
                            );
                            let peer_device = device
                                .filter(|d| self.contacts.get(ci).is_some_and(|c| c.knows_device(d)));
                            self.active_call = Some(ActiveCall {
                                call_id,
                                peer_handle_hash: peer,
                                we_are_caller: false,
                                phase: CallPhase::Ringing,
                                phase_osc: vsf::eagle_time_oscillations(),
                                final_osc: None,
                                offer_osc: row_ts,
                                caller_nonce: nonce,
                                callee_nonce: None,
                                offer_lane_key: Some(offer_key),
                                secret: None,
                                engine: None,
                                spool: None,
                                ring: None,
                                ringback: None, // glare fold: we become the CALLEE, so the ring plays — and dropping the old ActiveCall already stopped our ringback
                                express_addr: None,
                                peer_device,
                                reconnecting: false,
                                last_anchor_osc: 0,
                                last_beat_osc: vsf::eagle_time_oscillations(),
                            express_key: None,
            express_beats: 0,
            reconnect_probe: 0,
                            });
                            // Both users already pressed call — consent is mutual, connect NOW (a fold that merely rings would ask one of them to press the button twice). If the answer guard refuses (uncalibrated route), the call stays Ringing and the panel says why — still strictly better than BUSY.
                            self.answer_call();
                            self.scene_dirty = true;
                        } else {
                            // Ours wins: ignore their offer — their side folds by the same rule and Answers ours.
                            crate::logf!(
                                "CALL: glare with {} — our {} wins, expecting their answer to it",
                                crate::fp(&peer),
                                hex::encode(&c.call_id[..4])
                            );
                        }
                    }
                    Some(_) => {
                        // Busy: only the direct receiver replies (merge is history, and every sibling replying would triplicate it).
                        if !from_merge {
                            let _ = self.send_call_signal(ci, CallSignal::Busy { call_id }, vsf::eagle_time_oscillations());
                        }
                    }
                    None => {
                        // RING — direct decrypt only (see module doc). The offer's lane key is the basket egg this device will need if IT answers.
                        let Some(offer_key) = rx_lane_key else {
                            return;
                        };
                        if from_merge {
                            return;
                        }
                        // STALE-OFFER GATE (re-serve era, 2026-08-20): the durable-store re-serve can deliver an offer row the live retransmit ladder never landed — hours after the caller gave up. An offer that OLD is history, not a doorbell (the same physics the merge-path rule above encodes; a live caller's retransmits arrive within seconds). Not a ring TIMER — ringing still stops only on edges; this is an age check on STARTING one, made on the row's own stamp.
                        let age = vsf::eagle_time_oscillations().saturating_sub(row_ts);
                        if age > 60 * vsf::OSCILLATIONS_PER_SECOND as i64 {
                            crate::logf!(
                                "CALL: offer {} arrived {}s stale (re-served history) — recorded, not ringing",
                                hex::encode(&call_id[..4]),
                                age / vsf::OSCILLATIONS_PER_SECOND as i64
                            );
                            return;
                        }
                        // The offer's claimed origin device, honoured only if the friend's fold vouches for it (routing info, not authentication).
                        let peer_device = device
                            .filter(|d| self.contacts.get(ci).is_some_and(|c| c.knows_device(d)));
                        self.active_call = Some(ActiveCall {
                            call_id,
                            peer_handle_hash: peer,
                            we_are_caller: false,
                            phase: CallPhase::Ringing,
                            phase_osc: vsf::eagle_time_oscillations(),
                            final_osc: None,
                            offer_osc: row_ts,
                            caller_nonce: nonce,
                            callee_nonce: None,
                            offer_lane_key: Some(offer_key),
                            secret: None,
                            engine: None,
                            spool: None,
                            ring: None,
                            ringback: None, // inbound: we're the one being waved, the RING plays not the ringback
                            express_addr: None,
                            peer_device,
                            reconnecting: false,
                            last_anchor_osc: 0,
                            last_beat_osc: vsf::eagle_time_oscillations(),
                            express_key: None,
            express_beats: 0,
            reconnect_probe: 0,
                        });
                        self.ring_alert(ci);
                        crate::logf!(
                            "CALL: RING from {} (id {})",
                            crate::fp(&peer),
                            hex::encode(&call_id[..4])
                        );
                        self.scene_dirty = true;
                    }
                }
            }
            CallSignal::Offer { call_id, device, .. } => {
                // OUR fleet's outgoing offer, seen on a device that didn't dial it: a sibling is waving someone — light the presence chip ("wave on <device>"). The dialer itself skips (its own call UI is the display); the chip clears on the terminal rows above.
                if !self.dialed_call_ids.contains(&call_id)
                    && self.active_call.as_ref().is_none_or(|c| c.call_id != call_id)
                {
                    self.fleet_call_elsewhere = Some((call_id, device, peer));
                    self.scene_dirty = true;
                }
            }
            CallSignal::Answer { call_id, nonce, device } => {
                // The claimed answering device, fold-gated BEFORE any active_call borrow (routing + chip info, not authentication).
                let peer_dev = device.filter(|d| self.contacts.get(ci).is_some_and(|c| c.knows_device(d)));
                let Some(call) = self.active_call.as_mut() else {
                    // A sibling with no ring in progress (came online mid-call, or its ring row hasn't landed): the fleet's outgoing answer row still lights the chip.
                    if row_is_outgoing {
                        self.fleet_call_elsewhere = Some((call_id, device, peer));
                        self.scene_dirty = true;
                        return;
                    }
                    // An answer with no active call. Loud-kill ONLY if THIS device dialed that call this session (the caller dismissed/lost it and the answer strayed in late — without the hangup the friend sits Active on a dead call). Any other device is a fleet SIBLING hearing the friend's answer fan-out, and it must stay SILENT: the express key is per-friendship so its hangup authenticates as the whole identity, and on 2026-09-08 the non-calling sibling's "kill it loudly" tore down Brittany's engine 107ms after answer — 17s of nobody hearing anybody. Siblings observe, never destroy (fleetwide wave presence — join/switch mid-call — rides on exactly that). Trade accepted: a caller that CRASHED (RAM set gone) no longer kills its own stray answer; the friend hangs up a one-way call by hand, which beats every multi-device call dying at answer.
                    if !from_merge && !row_is_outgoing {
                        if self.dialed_call_ids.contains(&call_id) {
                            let _ = self.send_call_signal(ci, CallSignal::Hangup { call_id }, vsf::eagle_time_oscillations());
                        } else {
                            crate::logf!(
                                "CALL: answer for a call this device didn't place (id {}) — ignored (sibling's copy)",
                                hex::encode(&call_id[..4])
                            );
                        }
                    }
                    return;
                };
                if call.call_id != call_id {
                    return;
                }
                if row_is_outgoing {
                    // OUR FLEET answered somewhere. If that somewhere isn't here, stop this device's ring and light the chip — the call lives on the answering device.
                    if call.phase == CallPhase::Ringing && call.callee_nonce != Some(nonce) {
                        crate::log("CALL: a sibling answered — ring stops here");
                        self.active_call = None;
                        self.fleet_call_elsewhere = Some((call_id, device, peer));
                        Self::stop_ring_alert_platform();
                        self.scene_dirty = true;
                    }
                    return;
                }
                // The FRIEND answered our offer.
                match call.phase {
                    CallPhase::Outgoing => {
                        let offer_key = call.offer_lane_key;
                        let caller_nonce = call.caller_nonce;
                        call.callee_nonce = Some(nonce);
                        // Media routing pins to the ANSWERING device from here on (its endpoints outrank the pre-answer guesses).
                        if peer_dev.is_some() {
                            call.peer_device = peer_dev;
                        }
                        call.phase = CallPhase::Active;
                        call.phase_osc = vsf::eagle_time_oscillations();
                        // Answered: the ringback stops HERE, before the engine spawns — it must not still be queueing cadence frames into the live wave. Its probe (measured on this route, seconds ago) survives in the module and seeds the engine below.
                        call.ringback = None;
                        let Some(offer_key) = offer_key else {
                            crate::log("CALL: answer arrived before our offer commit capture — hanging up (basket incomplete)");
                            self.hangup_call();
                            return;
                        };
                        let secret = self.derive_secret_for(
                            ci,
                            &offer_key,
                            &call_id,
                            &caller_nonce,
                            &nonce,
                        );
                        let (engine, spool) = self.spawn_call_engine(ci, &call_id, secret, true);
                        if let Some(call) = self.active_call.as_mut() {
                            call.secret = secret;
                            call.engine = engine;
                            call.spool = spool;
                        }
                        crate::logf!("CALL: answered by {} — active", crate::fp(&peer));
                        self.scene_dirty = true;
                    }
                    CallPhase::Active => {
                        // A second device answered late — first won.
                        if call.callee_nonce != Some(nonce) && !from_merge {
                            let _ = self.send_call_signal(ci, CallSignal::Taken { call_id }, vsf::eagle_time_oscillations());
                        }
                    }
                    // Ringing/Ended: an answer arriving after we've already left the live window is stale — ignore it.
                    CallPhase::Ringing => {}
                }
            }
            CallSignal::Decline { call_id } | CallSignal::Busy { call_id } => {
                let Some(call) = self.active_call.as_ref() else {
                    return;
                };
                if call.call_id != call_id || row_is_outgoing && !from_merge {
                    return;
                }
                if row_is_outgoing {
                    // Our sibling declined for the fleet — ring stops silently here.
                    if call.phase == CallPhase::Ringing {
                        self.active_call = None;
                        Self::stop_ring_alert_platform();
                        self.scene_dirty = true;
                    }
                    return;
                }
                if call.we_are_caller {
                    let offer_osc = call.offer_osc;
                    let outcome = if matches!(sig, CallSignal::Busy { .. }) {
                        WaveOutcome::Busy
                    } else {
                        WaveOutcome::Declined
                    };
                    self.end_call(outcome, offer_osc);
                }
            }
            CallSignal::Hangup { call_id } => {
                let Some(call) = self.active_call.as_ref() else {
                    return;
                };
                if call.call_id != call_id {
                    return;
                }
                let (phase, offer_osc, we_are_caller) =
                    (call.phase, call.offer_osc, call.we_are_caller);
                match phase {
                    CallPhase::Ringing => {
                        // Caller gave up before we answered — the missed wave, on every device that was ringing (same stamp, merge-folds to one; a sibling that answered outranks it by outcome).
                        self.end_call(WaveOutcome::Missed, offer_osc);
                    }
                    CallPhase::Active => {
                        self.end_call(WaveOutcome::Answered, offer_osc);
                    }
                    CallPhase::Outgoing if !we_are_caller => {}
                    CallPhase::Outgoing => {
                        // Friend-side auto-hangup (e.g. answer hit their dead call) — treat as declined-ish end.
                        self.end_call(WaveOutcome::Declined, offer_osc);
                    }
                }
            }
            CallSignal::Taken { call_id } => {
                let Some(call) = self.active_call.as_ref() else {
                    return;
                };
                if call.call_id == call_id && call.phase == CallPhase::Active && !call.we_are_caller
                {
                    crate::log("CALL: another of our devices won the answer race");
                    if let Some(call) = &self.active_call {
                        if let Some(e) = &call.engine {
                            e.stop();
                        }
                    }
                    crate::platform::audio::stop();
                    self.active_call = None;
                    // The call continues on the winning sibling — chip it (device unknown from Taken; the answer row's merge fills the name when it lands).
                    self.fleet_call_elsewhere = Some((call_id, None, peer));
                    self.scene_dirty = true;
                }
            }
            // Anchor is transport plumbing: the re-point happens in drain_express_signals where the frame's SOURCE address is in hand; the row path never carries one.
            CallSignal::Anchor { .. } => {}
        }
    }

    /// Send one signal on the lane (hidden wire content, probe-pattern) AND store it as a hidden OUTGOING row pushed to our siblings — the fleet's ring/stop fan-out (a sibling seeing our Answer row stops its own ring).
    /// `ts` is the row's stamp, passed in rather than read here: the OFFER must be stamped with the caller's `offer_osc` — every device that lives the call mints its wave row at offer_osc+1, and the fold-by-shared-stamp dedup only works when the dialing device's local offer_osc IS the row's stamp. A second clock read here gave the dialer a private stamp and every wave showed twice on a multi-device fleet.
    fn send_call_signal(&mut self, ci: usize, sig: CallSignal, ts: i64) -> bool {
        let content = sig.to_content();
        let sent = self.chain_transmit(ci, &content, ts, None, None);
        if sent {
            let mut row = ChatMessage::new_with_timestamp(content, true, ts);
            row.notified = true;
            if let Some(conv) = self.conv_mut_of(ci) {
                conv.insert_message_sorted(row.clone());
            }
            self.persist_messages_async(ci);
            self.push_rows_to_siblings(ci, std::slice::from_ref(&row), None);
            // EXPRESS copy for everything except the offer — the offer must wait for its lane-key capture at the send commit (messaging.rs), because the express payload carries the doomed egg.
            if !matches!(sig, CallSignal::Offer { .. }) {
                self.send_express_signal(ci, &sig, ts, None);
            }
        }
        sent
    }

    /// Fire the EXPRESS out-of-band copy of a signal (signal.rs) at the peer's freshest direct paths: the address their last express signal arrived from (the answer rides back the offer's path), then the contact's punch-validated path. Fire-and-forget — the ordered lane row remains the canonical, retransmitted copy; this one just beats it past any chain gap. `lane_key` rides on offers only (the callee's basket egg).
    pub(super) fn send_express_signal(
        &self,
        ci: usize,
        sig: &CallSignal,
        ts: i64,
        lane_key: Option<[u8; 32]>,
    ) {
        let Some(contact) = self.contacts.get(ci) else {
            return;
        };
        let Some(fid) = contact.friendship_id else {
            return;
        };
        let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) else {
            return;
        };
        let (Some(lane_root), Some(history_key)) = (chains.lane_root(), chains.history_key())
        else {
            return;
        };
        // ERA KEYS (2026-09-10): an OFFER seals under our current era AND our retired one (a peer that has not followed our re-key opens the retired copy); every other signal seals FIRST under the key that opened this call's first express frame from the peer, then under our current era if that differs. One era of skew either way and the call still connects.
        let current = crate::call::signal::express_key(lane_root, history_key);
        let mut keys: Vec<[u8; 32]> = Vec::with_capacity(2);
        if matches!(sig, CallSignal::Offer { .. }) {
            keys.push(current);
            if let Some(r) = chains.retired_era() {
                if let Some(hk) = r.history_key.as_ref() {
                    keys.push(crate::call::signal::express_key(&r.lane_root, hk));
                }
            }
        } else {
            if let Some(call) = self.active_call.as_ref() {
                if call.call_id == *sig.call_id() {
                    if let Some(k) = call.express_key {
                        keys.push(k);
                    }
                }
            }
            if !keys.contains(&current) {
                keys.push(current);
            }
        }
        let frames: Vec<Vec<u8>> = keys
            .iter()
            .filter_map(|k| crate::call::signal::seal_express(k, ts, lane_key.as_ref(), sig))
            .collect();
        if frames.is_empty() {
            return;
        }
        // An OFFER fans wide by design; an ANCHOR fans wide because the path it would otherwise trust is the one in doubt.
        let wide = matches!(sig, CallSignal::Offer { .. } | CallSignal::Anchor { .. });
        // RING WANTS BREADTH, REPLIES WANT PRECISION (fleet lifecycle, 2026-09-08). An OFFER is the ding — it fans to every known endpoint of every fold-trusted device, so all the callee's devices ring at express speed instead of waiting on replication. Every other signal is a reply about one specific call: it targets the ONE peer device driving it — the call's freshest express source plus that device's own endpoint addresses (multiple addresses of one device is a race, not a misfire; multiple DEVICES was the 2026-09-08 sibling-hangup bug). validated_path is only the no-better-knowledge fallback: it's per-CONTACT (whichever device punch-validated last), not per-call.
        let mut targets: Vec<std::net::SocketAddr> = Vec::new();
        // Never a loopback or unspecified target: a frame sent to ourselves opens under our own friendship key and reads as the peer's (2026-09-10 anchor storm).
        let push = |t: &mut Vec<std::net::SocketAddr>, a: std::net::SocketAddr| {
            if !t.contains(&a) && !a.ip().to_canonical().is_loopback() && !a.ip().is_unspecified() {
                t.push(a);
            }
        };
        if wide {
            // Every candidate the phonebook knows, not just the address this call came in on — the one that still works is exactly the one the stale pin is hiding. An OFFER fans across the contact's devices by design (every device rings); an ANCHOR belongs to one device's wave, so it stays on that device's addresses.
            match (matches!(sig, CallSignal::Anchor { .. }), self.active_call.as_ref().and_then(|c| c.peer_device)) {
                (true, Some(dev)) => {
                    for c in crate::network::traverse::gather::gather_device_candidates(contact, &dev).sorted() {
                        push(&mut targets, c.addr);
                    }
                }
                _ => {
                    for c in crate::network::traverse::gather::gather_peer_candidates(contact).sorted() {
                        push(&mut targets, c.addr);
                    }
                }
            }
            for ep in &contact.device_endpoints {
                if !contact.knows_device(&ep.pubkey) {
                    continue;
                }
                if let Some(a) = ep.lan {
                    push(&mut targets, a);
                }
                if let Some(a) = ep.public {
                    push(&mut targets, a);
                }
            }
            if let Some((a, _)) = contact.validated_path {
                push(&mut targets, a);
            }
        } else {
            if let Some(call) = self.active_call.as_ref() {
                if call.peer_handle_hash == contact.handle_hash && call.call_id == *sig.call_id() {
                    if let Some(a) = call.express_addr {
                        push(&mut targets, a);
                    }
                    if let Some(dev) = call.peer_device {
                        if let Some(ep) = contact.device_endpoints.iter().find(|e| e.pubkey == dev) {
                            if let Some(a) = ep.lan {
                                push(&mut targets, a);
                            }
                            if let Some(a) = ep.public {
                                push(&mut targets, a);
                            }
                        }
                    }
                }
            }
            if targets.is_empty() {
                if let Some((a, _)) = contact.validated_path {
                    push(&mut targets, a);
                }
            }
        }
        // AN ANCHOR NEVER TRUSTS THE VALIDATED PATH (field 2026-09-11, Nick/Emma: a deliberate LAN→WAN switch mid-wave — the validated path stayed Some but was dead, so every anchor went to two stale addresses with no relay copy, and the wave dropped at the 30 s deadline with both sides still reachable over the relay). An anchor is fired precisely when the direct path is in doubt, so it goes to every candidate endpoint AND over the relay, always.
        // RELAY-CARRIED EXPRESS (field 2026-09-11, Brittany/Nick: three rings failed in a row — no direct UDP path between the phones, so every express beat and answer vanished, the lane offer sat behind an undelivered text row, and the one ring that did land died at 3 s for want of beats). When the contact has no validated DIRECT path, every express frame also goes to each of their devices thru the relay pipe; an injected pipe frame lands in the same express drain as a datagram would.
        let direct_ok = !matches!(sig, CallSignal::Anchor { .. }) && contact.validated_path.is_some_and(|(a, _)| a != crate::network::status::RELAY_ADDR);
        let relay_devs: Vec<[u8; 32]> = if direct_ok { Vec::new() } else { contact.relay_device_list() };
        if targets.is_empty() && relay_devs.is_empty() {
            crate::logf!("CALL: express {} skipped — no direct path and no relay device known (lane only)", sig.kind());
            return;
        }
        for a in &targets {
            for frame in &frames {
                let _ = crate::call::send_media(frame.clone(), *a);
            }
        }
        if let Some(kp) = self.device_keypair.clone().filter(|_| !relay_devs.is_empty()) {
            let kind = sig.kind();
            for dev in relay_devs.iter().copied() {
                for frame in frames.iter().cloned() {
                    let kp = kp.clone();
                    crate::network::http::runtime().spawn(async move {
                        if let Err(e) = crate::network::fgtw::relay::send_via_relay(&kp, &dev, &frame).await {
                            crate::logf!("CALL: express {} via relay to {} failed: {}", kind, crate::fp(&dev), e);
                        }
                    });
                }
            }
        }
        crate::logf!("CALL: express {} fired → {} path(s) + {} relay device(s) × {} era key(s)", sig.kind(), targets.len(), relay_devs.len(), frames.len());
    }

    /// Drain express frames the recv worker parked: trial-open against every friendship (a wrong key just fails the AEAD tag), dispatch as a direct non-merge signal, and remember the source address as the call's freshest direct path. Idempotent against the lane copy arriving later — dup call_ids are no-ops in `on_call_signal`.
    pub(super) fn drain_express_signals(&mut self) {
        let frames = crate::call::take_express_frames();
        for (bytes, src) in frames {
            // REPLAY GUARD (2026-09-11, before anchors carry routing): an express frame is sealed, so it cannot be forged — but a captured one could be replayed by anyone on path, and an anchor's SOURCE ADDRESS is what re-aims our media. Two cheap gates: the sealed stamp must be near now, and a nonce we have already opened is dropped. The nonce is checked before the trial decrypt, so a replay costs nothing.
            let Some(nonce) = crate::call::signal::express_nonce(&bytes) else {
                continue;
            };
            if self.express_seen.contains(&nonce) {
                crate::log("CALL: express frame replayed (nonce already opened) — dropped");
                continue;
            }
            let mut opened: Option<(usize, i64, Option<[u8; 32]>, CallSignal, [u8; 32])> = None;
            for (fid, chains) in &self.friendship_chains {
                // Current era first, then the retired one: a call offer minted on the old era that lands after our cutover must still open (it used to read as "opened by no friendship").
                let mut keys: Vec<[u8; 32]> = Vec::with_capacity(2);
                if let (Some(lane_root), Some(history_key)) = (chains.lane_root(), chains.history_key()) {
                    keys.push(crate::call::signal::express_key(lane_root, history_key));
                }
                if let Some(r) = chains.retired_era() {
                    if let Some(hk) = r.history_key.as_ref() {
                        keys.push(crate::call::signal::express_key(&r.lane_root, hk));
                    }
                }
                if keys.is_empty() {
                    continue;
                }
                if let Some((key, (ts, lane_key, sig))) = keys
                    .iter()
                    .find_map(|key| crate::call::signal::open_express(key, &bytes).map(|r| (*key, r)))
                {
                    if let Some(ci) = self
                        .contacts
                        .iter()
                        .position(|c| c.friendship_id == Some(*fid) && !c.is_sibling)
                    {
                        opened = Some((ci, ts, lane_key, sig, key));
                    }
                    break;
                }
            }
            let Some((ci, ts, lane_key, sig, opened_key)) = opened else {
                crate::log("CALL: express frame opened by no friendship — dropped (an era we do not hold: neither current nor retired)");
                continue;
            };
            // The stamp is INSIDE the seal, so only the friendship could have written it; all it has to prove is freshness.
            let skew = (vsf::eagle_time_oscillations() - ts).abs();
            if skew > crate::call::signal::EXPRESS_MAX_SKEW_OSC {
                crate::logf!("CALL: express {} stamped {}s away — dropped as stale (replay guard)", sig.kind(), skew / vsf::OSCILLATIONS_PER_SECOND as i64);
                continue;
            }
            if self.express_seen.len() >= 256 {
                self.express_seen.drain(..64);
            }
            self.express_seen.push(nonce);
            crate::logf!(
                "CALL: express {} from {} (jumped the lane)",
                sig.kind(),
                crate::fp(&self.contacts[ci].handle_hash)
            );
            self.on_call_signal(ci, sig, lane_key, ts, false, false);
            // An express offer beat for the ringing call: the lease may run on the express cadence from here on.
            if matches!(sig, CallSignal::Offer { .. }) {
                if let Some(call) = self.active_call.as_mut() {
                    if call.call_id == *sig.call_id() && call.phase == CallPhase::Ringing {
                        call.express_beats = call.express_beats.saturating_add(1);
                    }
                }
            }
            // Remember the era that opened it — replies for this call seal under it first (send_express_signal).
            if let Some(call) = self.active_call.as_mut() {
                if call.call_id == *sig.call_id() && call.express_key.is_none() {
                    call.express_key = Some(opened_key);
                }
            }
            // Remember the reply path — but never the relay-injection sentinel NOR a loopback/unspecified source: a pipe-injected frame arrives from 127.0.0.1, and re-pointing media there sent a whole call to ourselves (2026-09-10 Emma/Nick first call: 21,815 anchors in 30 s, every RX packet our own, "open-drop 581, decoded 0").
            let routable = src != crate::network::status::RELAY_ADDR && !src.ip().to_canonical().is_loopback() && !src.ip().is_unspecified();
            if routable {
                if let Some(call) = self.active_call.as_mut() {
                    if call.call_id == *sig.call_id() {
                        call.express_addr = Some(src);
                    }
                }
                // ANCHOR: the authenticated frame's source IS the peer's fresh address — re-point the live engine's TX there (the both-sides-moved heal). If we're droughted too, answer with our own anchor at that fresh address so the peer heals symmetrically.
                if matches!(sig, CallSignal::Anchor { .. }) {
                    let echo = self.active_call.as_ref().is_some_and(|call| {
                        call.call_id == *sig.call_id() && matches!(call.phase, CallPhase::Active)
                    });
                    if echo {
                        crate::call::set_peer_redirect(src);
                        crate::logf!("CALL: anchor received — media re-pointed at {}", src);
                        // Answer with our own anchor at most once a second (last_anchor_osc): two droughted sides otherwise anchor each other in a ms-cadence loop.
                        let now = vsf::eagle_time_oscillations();
                        let fire = self.active_call.as_ref().is_some_and(|c| c.reconnecting && now - c.last_anchor_osc >= vsf::OSCILLATIONS_PER_SECOND as i64);
                        if fire {
                            if let Some(c) = self.active_call.as_mut() {
                                c.last_anchor_osc = now;
                            }
                            let back = CallSignal::Anchor { call_id: *sig.call_id() };
                            self.send_express_signal(ci, &back, now, None);
                        }
                    }
                }
            }
        }
    }

    /// The basket, assembled (docs/calls.md, call/keys.rs): lane_root + history_key from the friendship, the offer's doomed lane key, the id, both nonces.
    fn derive_secret_for(
        &self,
        ci: usize,
        offer_lane_key: &[u8; 32],
        call_id: &[u8; 16],
        caller_nonce: &[u8; 32],
        callee_nonce: &[u8; 32],
    ) -> Option<[u8; 32]> {
        // v2: no era material beyond the offer lane key (keys.rs) — a one-era skew between the fleets used to split the secret. The contact is still required to exist (a call needs a friendship).
        let _ = self.contacts.get(ci)?.friendship_id?;
        Some(crate::call::keys::derive_call_secret(
            offer_lane_key,
            call_id,
            caller_nonce,
            callee_nonce,
        ))
    }

    /// Mint THE WAVE ROW (offer_osc+1 — the shared stamp both fleets agree on, +1 clear of the hidden offer row) with its typed outcome on EVERY end edge, stop the engine, and hand a live call's spool to the keep transcode. The wave card (2026-09-09): one row per wave; the recording, when it lands, REFERENCES this row and folds into its card — so the card exists at hangup, before the transcode finishes, and a sibling that only rang still shows the same event.
    fn end_call(&mut self, outcome: WaveOutcome, offer_osc: i64) {
        // WHO tore the call down (field 2026-09-02: Brittany's active call died at ~3s the instant an inbound text landed, no call signal from Nick — the trigger wasn't in any obvious path, so every teardown now names itself). The outcome doubles as the reason tag here.
        if let Some(call) = &self.active_call {
            crate::logf!(
                "CALL: end_call — phase {}, outcome {} (id {})",
                format!("{:?}", call.phase),
                format!("{:?}", outcome),
                hex::encode(&call.call_id[..4])
            );
        }
        // Live seconds from the Active phase base — the keep's slot count refines it when the recording lands (merge = max).
        let secs: u32 = self
            .active_call
            .as_ref()
            .filter(|c| c.phase == CallPhase::Active)
            .map(|c| ((vsf::eagle_time_oscillations() - c.phase_osc).max(0) / vsf::OSCILLATIONS_PER_SECOND as i64) as u32)
            .unwrap_or(0);
        // The engine thread outlives `stop()` by the fill drain (engine.rs: the peer hands back the windows we lost); the keep joins it before reading the spool.
        let engine_thread = self.active_call.as_ref().and_then(|call| {
            let e = call.engine.as_ref()?;
            e.stop(); // the engine thread zeroizes its chains, clears the sink, and releases audio
            e.take_thread()
        });
        crate::call::set_call_peer_device(None);
        crate::platform::audio::stop();
        Self::stop_ring_alert_platform();
        self.call_minimized = false;
        let peer = self.active_call.as_ref().map(|c| c.peer_handle_hash);
        let was_caller = self.active_call.as_ref().map(|c| c.we_are_caller).unwrap_or(false);
        // RECORDED BY DEFAULT (Nick 2026-09-08: "these are waves, not wireline calls" — a wave spools to disk like an email/attachment and you delete it LATER if you don't want it; NO keep/delete decision, NO ended screen). A completed wave with a spool auto-keeps: transcode off-thread → the call.audio row lands in the conversation. A missed/declined wave (no spool) mints the text summary instead.
        let call_id8: [u8; 8] = self
            .active_call
            .as_ref()
            .map(|c| c.call_id[..8].try_into().unwrap())
            .unwrap_or([0u8; 8]);
        let ticket = self.active_call.as_mut().and_then(|c| {
            (c.phase == CallPhase::Active).then(|| c.spool.take()).flatten()
        });
        self.active_call = None;
        let seed = self.session.as_ref().map(|s| s.identity_seed).unwrap_or([0u8; 32]);
        if let Some(peer) = peer {
            if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
                // The wave row, always: empty content, typed payload. Same stamp on every device that lived any part of the call → one row fleet-wide, outcome folded by rank.
                let mut row = ChatMessage::new_with_timestamp(String::new(), was_caller, offer_osc + 1);
                row.wave = Some(WaveInfo { outcome, secs });
                row.notified = true;
                row.delivered = true;
                if let Some(conv) = self.conv_mut_of(ci) {
                    conv.insert_message_sorted(row.clone());
                }
                self.persist_messages_async(ci);
                self.push_rows_to_siblings(ci, std::slice::from_ref(&row), None);
                if let Some(ticket) = ticket {
                    // Recorded by default: the transcode lands the recording row against this wave row. Land the user in the conversation so the card shows up where it lives.
                    self.spawn_keep_transcode(ticket, peer, offer_osc, seed, call_id8, engine_thread);
                    self.open_conversation_with(ci);
                }
            }
        }
        self.scene_dirty = true;
    }

    /// Transcode a kept spool to the durable N-channel blob OFF the UI thread (decode+re-encode is O(call length)); the worker posts (hash,size) back to `drain_call_keep`, which mints the fleet-internal call.audio row. A failed transcode drops the ticket → the spool key zeroizes → safe degrade to nothing kept.
    pub(super) fn spawn_keep_transcode(&mut self, ticket: crate::call::spool::SpoolTicket, peer: [u8; 32], offer_osc: i64, seed: [u8; 32], call_id8: [u8; 8], engine_thread: Option<std::thread::JoinHandle<()>>) {
        let tx = self.call_keep_sender();
        let wake = self.event_proxy.clone();
        // KEEP HOLD (2026-09-11, Emma's 13-minute wave kept 53 minutes after hangup): the phone dozed the moment the wave ended and the transcode crawled in maintenance windows. Kotlin holds a partial wake lock while a keep runs (+1 here, −1 in the drain), and the thread asks for a better-than-background priority.
        self.pending_keep_hold = 1;
        let spawned = std::thread::Builder::new()
            .name("call-keep".into())
            .spawn(move || {
                #[cfg(target_os = "android")]
                {
                    let _ = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -4) };
                }
                // The spool is still being written while the engine drains fills from the peer — wait for it to go quiet (bounded by engine.rs DRAIN_MAX).
                if let Some(t) = engine_thread {
                    let _ = t.join();
                }
                let result = crate::call::record::finalize_nchannel(ticket, &seed);
                let _ = tx.send(CallKeepResult { peer, offer_osc, call_id8, result });
                if let Some(w) = wake {
                    let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                }
            })
            .is_ok();
        if !spawned {
            crate::log("CALL: keep-transcode thread failed to spawn — recording shredded");
        }
    }

    /// The ring alert: platform notification + the relationship RING — the identity chirp's instrument conjugated into a call phrase (`chirp::Chirp::ring_from_hash`: the ding's chord HELD flat under a sin³ 0→9π arc, doubletted), so ears know who's calling before eyes do. Deliberately BYPASSES the will_ding gates: a call is the one always-ring event (design decision 2026-08-18).
    ///
    /// Both platforms loop the cadence until a stop edge, no timers: desktop by a thread watching the [`crate::call::RingGuard`] on the ActiveCall, Android by a MODE_STATIC AudioTrack looping in the HAL until `cancelCallNotification` (which every teardown edge already calls).
    fn ring_alert(&mut self, ci: usize) {
        let Some(contact) = self.contacts.get(ci) else {
            return;
        };
        let sender_name = contact.display_name();
        let from_hh = contact.handle_hash;
        let Some(us) = self.our_party_id(contact) else {
            return;
        };
        let digest = relationship_digest(&from_hh, &us);
        let ring_hp = *blake3::hash(&digest).as_bytes();
        // Honor the Notifications "Ring on incoming call" tick: unchecked = the notification still posts (the call is never invisible) but the audible ring loop stays silent. Read BEFORE the platform blocks — Android's ring loops now, and an unhonored mute there is a ring that cannot be silenced.
        let ring_audible = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("notify.ring_call"))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(true);
        #[cfg(target_os = "android")]
        {
            // App on screen → the full-screen ring panel IS the alert (and the in-process chirp below rings) — posting the notification too was the double-alert whose heads-up banner covered the old top-bar Answer button. Backgrounded/locked → the CALL-CLASS notification (fullScreenIntent + Answer/Decline actions) is the whole surface.
            if crate::platform::jni_android::app_in_foreground() {
                if ring_audible {
                    crate::platform::jni_android::play_ring_chirp(&digest);
                } else {
                    crate::log("CALL: ring muted by notify.ring_call — panel only");
                }
            } else {
                // Muted still posts the call-class notification (never an invisible call); only the looping audio/haptic is withheld.
                crate::platform::jni_android::notify_incoming_call(
                    &ring_hp,
                    &digest,
                    &sender_name,
                    &tr(Msg::IncomingCall),
                    ring_audible,
                );
            }
        }
        #[cfg(not(any(target_os = "android", target_os = "redox")))]
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            crate::platform::desktop_notify::notify_new_message(
                &ring_hp,
                &sender_name,
                &tr(Msg::IncomingCall),
            );
            if !ring_audible {
                crate::log("CALL: ring muted by notify.ring_call — notification only");
                return;
            }
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::spawn(move || {
                let ring = chirp::Chirp::ring_from_hash(digest);
                while !flag.load(Ordering::Relaxed) {
                    if let Err(e) = ring.clone().play_blocking() {
                        crate::logf!("CALL ring chirp: {}", e);
                        break;
                    }
                    // Gap between cadences, polled so a stop edge lands within ~50 ms of flipping.
                    for _ in 0..24 {
                        if flag.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            });
            if let Some(call) = self.active_call.as_mut() {
                call.ring = Some(crate::call::RingGuard(stop));
            }
        }
    }

    /// KEEP the recording: finalize the spool into the call container, store it as a content-addressed blob, and mint a FLEET-INTERNAL attachment row (local insert + sibling push — never chain-transmitted; the friend's fleet keeps its own recording). v1 gap, tracked in docs/calls.md: the blob itself lives on THIS device until sibling blob-fetch lands.
    /// Lazily mint the keep-transcode result channel (worker → UI), mirroring `update_sender`.
    fn call_keep_sender(&mut self) -> std::sync::mpsc::Sender<CallKeepResult> {
        if self.call_keep_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.call_keep_tx = Some(tx);
            self.call_keep_rx = Some(rx);
        }
        self.call_keep_tx.as_ref().unwrap().clone()
    }

    /// Land finished envelope reads in the cache.
    pub(super) fn drain_wave_env(&mut self) -> bool {
        let mut got = Vec::new();
        if let Some(rx) = self.wave_env_rx.as_ref() {
            while let Ok(ev) = rx.try_recv() {
                got.push(ev);
            }
        }
        let any = !got.is_empty();
        for (hash, env) in got {
            self.wave_env_pending.remove(&hash);
            // A miss caches the empty marker so the render's cached-is-none gate goes quiet instead of respawning the loader every frame.
            let (nchan, e) = env.unwrap_or((0, Vec::new()));
            self.wave_env.insert(hash, (nchan, std::sync::Arc::new(e)));
            self.scene_dirty = true;
        }
        any
    }

    /// Drain finished keep-transcodes and mint the fleet-internal `call.audio` attachment row on the UI thread (the mint needs `&mut self`, so results are collected before the row work — same borrow dance as `drain_update_events`). Called from `tick`.
    pub(super) fn drain_call_keep(&mut self) -> bool {
        let mut pending: Vec<CallKeepResult> = Vec::new();
        {
            let Some(rx) = self.call_keep_rx.as_ref() else {
                return false;
            };
            while let Ok(ev) = rx.try_recv() {
                pending.push(ev);
            }
        }
        if pending.is_empty() {
            return false;
        }
        for r in pending {
            self.pending_keep_hold = -1; // the keep finished, one way or the other — Kotlin drops the wake lock
            match r.result {
                Some(kept) => {
                    // Keep completed (blob stored) → the durable spool register has done its job; a crash from here on has nothing to recover.
                    crate::call::spool::drop_register(&r.call_id8);
                    if let Some(ci) = self.contact_index_by_handle_hash(&r.peer) {
                        // "call.audio" (video calls will mint "call.video") — no POTS in Photon, so nothing here is a "phone call". The row REFERENCES the wave row (offer_osc+1) and carries the envelope thumbnail, so every sibling folds it into the card and draws the shape before it holds the blob.
                        let content = crate::types::attachment_content(&kept.hash, "call.audio", kept.size);
                        let mut row = ChatMessage::new_with_timestamp(content, true, r.offer_osc + 2)
                            .with_reference(crate::types::RefKind::Wave, r.offer_osc + 1);
                        row.notified = true;
                        row.delivered = true;
                        row.envelope = kept.thumb.clone();
                        // The wave row's seconds refine to the recording's length (merge = max); push the upgraded copy too.
                        let mut pushed = vec![row.clone()];
                        if let Some(conv) = self.conv_mut_of(ci) {
                            conv.insert_message_sorted(row.clone());
                            if let Some(w) = conv.messages.iter_mut().find(|m| m.timestamp == r.offer_osc + 1 && m.wave.is_some()) {
                                let before = w.wave;
                                crate::types::merge_wave_fields(w, Some(WaveInfo { outcome: WaveOutcome::Answered, secs: kept.secs }), &[]);
                                if w.wave != before {
                                    pushed.push(w.clone());
                                }
                            }
                        }
                        self.persist_messages_async(ci);
                        self.push_rows_to_siblings(ci, &pushed, None);
                        crate::logf!(
                            "CALL: recording kept — {} bytes as blob {}…, {} s",
                            kept.size,
                            hex::encode(&kept.hash[..4]),
                            kept.secs
                        );
                    }
                }
                None => {
                    crate::call::spool::drop_register(&r.call_id8);
                    crate::log("CALL: recording was empty — nothing kept");
                }
            }
        }
        self.scene_dirty = true;
        true
    }

    /// Spin up the media engine for an Active call. None (call stays signaling-only + silent) when the basket never completed or the contact has no direct address — media-over-the-relay-pipe is explicitly deferred (docs/calls.md), and the transport dot already tells the human they're on relay.
    fn spawn_call_engine(
        &self,
        ci: usize,
        call_id: &[u8; 16],
        secret: Option<[u8; 32]>,
        we_are_caller: bool,
    ) -> (
        Option<crate::call::engine::EngineHandle>,
        Option<crate::call::spool::SpoolTicket>,
    ) {
        let Some(secret) = secret else {
            crate::log("CALL: no secret — media engine not started");
            return (None, None);
        };
        // DROUGHT BASELINE ON THIS THREAD, before the engine exists (field 2026-09-08 22:56, "dropped — no authenticated media for 1970s"): the engine thread re-baselines these at its own start, but call_drought_tick can run between `call.engine = Some` and that start and read the PREVIOUS call's clocks — 33 minutes of "drought" on a call that was 1 ms old.
        crate::call::MEDIA_START_OSC.store(vsf::eagle_time_oscillations(), std::sync::atomic::Ordering::Relaxed);
        crate::call::LAST_MEDIA_RX_OSC.store(0, std::sync::atomic::Ordering::Relaxed);
        // No validated direct address is NOT no engine (field 2026-08-20, the "stuck call": Emma's validated path to Nick expired mid-session, this bail left her engine down, and the call sat Active-and-silent BOTH ways even tho Nick held a valid path and its media was arriving — with no engine there was no sink to decode it and no TX to answer with). Start on the RELAY_ADDR sentinel instead: RX needs no address at all (the engine installs the sink), sends to the sentinel are swallowed harmlessly, and the peer's FIRST authenticated packet re-points TX at its real source (address-follows-auth). Media stays dead only when NEITHER side holds an address.
        // THE DEVICE IN THE WAVE, NOT THE CONTACT (field 2026-09-11, the green-circle wave: Emma's validated path to Nick pointed at his MACBOOK on her own LAN, because a validated path belongs to the CONTACT — so the engine aimed a wave with his PHONE at a laptop three feet away, and both sides logged zero packets in while the ring sat honestly green). A wave is with one device: take that device's own best address, and only fall back to the contact-level race when we do not know which device answered.
        let peer_dev = self.active_call.as_ref().and_then(|c| c.peer_device);
        let addr = self
            .contacts
            .get(ci)
            .and_then(|c| match peer_dev {
                // No device candidate survives the our-LAN filter (the peer's only known address is on a LAN we are not on): the contact-level race would hand back the same black hole, so take the sentinel and let TX follow the peer's first authenticated packet.
                Some(dev) => crate::network::traverse::gather::gather_device_candidates(c, &dev)
                    .sorted()
                    .into_iter()
                    .map(|x| x.addr)
                    .next(),
                None => c.race_addrs().map(|(a, _)| a),
            })
            .unwrap_or_else(|| {
                crate::log("CALL: no direct address — engine up on the sentinel; TX will follow the peer's first authenticated packet");
                crate::network::status::RELAY_ADDR
            });
        if let Some(dev) = peer_dev {
            if addr == crate::network::status::RELAY_ADDR {
                crate::logf!("CALL: no reachable address for the answering device {} — TX waits for its first packet", crate::fp(&dev));
                // CALL-TIME ADDRESS EXCHANGE (field 2026-09-11 20:47: the wave carried media both ways 18 s after answer, because our new public address was learned from a pong on the presence cadence mid-call, and they hung up the second it connected). Push whatever we hold to the answering device now — its answer is their record — and sweep presence at once so the pong that teaches our reflexive lands this tick; that edge pushes again with the public address.
                self.call_needs_addresses.set(Some(dev)); // drained by call_drought_tick (this fn holds only &self)
            } else {
                crate::logf!("CALL: media aimed at {} — the answering device {}", addr, crate::fp(&dev));
            }
        }
        let call_id8: [u8; 8] = call_id[..8].try_into().unwrap();
        // Recording by default (docs/calls.md): the engine writes sealed records as the wave runs; the register below is what makes that durable across a crash.
        let (spool_param, ticket) = match crate::call::spool::mint(&call_id8) {
            Some((key, path, ticket)) => {
                // Record-by-default durability (Nick 2026-09-08): persist {key, peer, offer_osc} at call START, so a battery death at 1h59m of a 2h wave recovers at next launch instead of vanishing (spool.rs recover_orphans; the RAM-only key was the old keep/delete model's "crash = delete").
                if let Some(c) = self.active_call.as_ref() {
                    crate::call::spool::persist_register(&call_id8, &ticket, &c.peer_handle_hash, c.offer_osc);
                }
                (Some((key, path)), Some(ticket))
            }
            None => (None, None),
        };
        // No call_id in the engine params — the media wire dropped it (the basket-derived key IS the call identity; see packet.rs); the id's only job here is naming the spool above.
        crate::call::set_call_peer_device(self.active_call.as_ref().and_then(|c| c.peer_device));
        let handle = crate::call::engine::start(crate::call::engine::EngineParams {
            secret,
            we_are_caller,
            peer_addr: addr,
            spool: spool_param,
            cal: self.ringback_seeded_cal(),
            plaid_allowed: is_lan_addr(addr),
        });
        (Some(handle), ticket)
    }

    /// The engine's calibration seed, ringback-first: a coupling this device measured on THIS route seconds ago outranks a stored profile measured on another day (the field-wave-1/2 failure was exactly a stale seed the in-call learner never had time to correct). The voice half (mic gain, floor) still comes from the stored profile — the ringback measures the room, not the user's voice, since the whole point is that nobody is talking during it.
    fn ringback_seeded_cal(&self) -> Option<crate::call::engine::CalSnapshot> {
        let stored = self.cal_snapshot();
        let Some(p) = crate::call::ringback::take_probe() else {
            return stored;
        };
        crate::logf!(
            "CALL: engine seeded from the ringback probe — g {:.4} delay {}ms (conf {:?}, {} window(s)); stored profile {}",
            p.g_norm,
            p.delay_bins * 10,
            format!("{:?}", p.confidence),
            p.windows,
            if stored.is_some() { "overridden" } else { "absent" }
        );
        Some(crate::call::engine::CalSnapshot {
            g_norm: p.g_norm,
            delay_bins: p.delay_bins,
            mic_gain: stored.as_ref().and_then(|c| c.mic_gain),
            floor: stored.as_ref().map(|c| c.floor).unwrap_or(p.floor),
        })
    }

    pub(super) fn contact_index_by_handle_hash(&self, hh: &[u8; 32]) -> Option<usize> {
        self.contacts.iter().position(|c| c.handle_hash == *hh)
    }
}

/// A LAN-class direct address: RFC 1918 / link-local IPv4, or link-local / unique-local IPv6 — the plaid rung's licence (raw PCM only where bandwidth is free and the path is one hop of radio).
fn is_lan_addr(a: std::net::SocketAddr) -> bool {
    match a.ip() {
        std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            let seg = v6.segments();
            (seg[0] & 0xffc0) == 0xfe80 || (seg[0] & 0xfe00) == 0xfc00 || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_private() || v4.is_link_local())
        }
    }
}
