//! Call state machine (docs/calls.md) — offer/answer/hangup edges over the in-lane signaling rows.
//!
//! Doctrine, restated where it bites:
//! - **No ring timer.** Ringing stops on answer/decline edges, a sibling's answer, or the caller's hangup — the caller's patience IS the timeout, and an unanswered hangup mints the missed-call row.
//! - **Ring requires DIRECT decrypt.** Every online callee device receives the offer frame itself (chat frames already fan to the whole fold via the relay list) and captures the offer's lane key at decrypt — the basket's doomed egg. A signal arriving via SIBLING MERGE is history, not a doorbell: merge signals only ever STOP rings, which also kills the whole stale-offer-rings-days-later class (a woke device replays old signals and correctly rings for none of them).
//! - **The summary row is the call's visible record**: minted at the end by the devices that lived the call, stamped offer_osc+1 on BOTH fleets (each side mints its own copy; the shared stamp makes sibling-merge dedup fold the copies).

use super::*;
use crate::call::signal::CallSignal;
use crate::call::{ActiveCall, CallPhase};

/// A finished keep-transcode, posted from the worker thread back to the UI thread to mint the `call.audio` row. `result` is `None` when the recording was empty or the transcode failed (treated as delete).
pub(super) struct CallKeepResult {
    peer: [u8; 32],
    offer_osc: i64,
    result: Option<([u8; 32], u64)>,
}

impl PhotonApp {
    /// Format a call duration as `M:SS`, base-aware (the About-page dozenal toggle). Rendered in the Oxanium face so the dozenal `+glyphs` control-block glyphs resolve. Dozenal seconds pad to two dozenal digits (0–4B).
    pub(super) fn fmt_duration(&self, secs: i64) -> String {
        let secs = secs.max(0);
        let (m, s) = ((secs / 60) as u32, (secs % 60) as u32);
        if crate::dozenal_ui() {
            let mut ss = crate::dozenal_glyphs(s);
            if s < 12 {
                ss = format!("{}{}", char::from(0x10), ss); // two-digit pad, dozenal zero
            }
            format!("{}:{}", crate::dozenal_glyphs(m), ss)
        } else {
            format!("{}:{:02}", m, s)
        }
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
            match phase {
                Some(CallPhase::Ringing) => self.answer_call(),
                Some(CallPhase::Ended) => self.keep_recording(),
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
                Some(CallPhase::Ended) => self.delete_recording(),
                _ => {}
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
        // Speaker toggle (Active) — visual state + a stubbed route intent (no real device switch in v1).
        if self
            .call_speaker_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            if matches!(phase, Some(CallPhase::Active)) {
                self.call_speaker_on = !self.call_speaker_on;
                let route = if self.call_speaker_on {
                    crate::platform::audio::AudioRoute::Speaker
                } else {
                    crate::platform::audio::AudioRoute::Earpiece
                };
                crate::platform::audio::set_route(route);
            }
            any = true;
        }
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
        // Play (Ended) — preview the recording before the Keep/Delete decision (plays the live spool through the same mono downmix).
        if self
            .call_play_btn
            .as_mut()
            .map(|b| b.take_click())
            .unwrap_or(false)
        {
            if matches!(phase, Some(CallPhase::Ended)) {
                self.preview_recording();
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

    /// Preview the in-flight recording on the Ended screen (before Keep/Delete finalizes a blob) — plays the live spool through the mono downmix. Holds the handle so the worker keeps running.
    fn preview_recording(&mut self) {
        // Stop any prior preview first (one owner of the audio session).
        self.call_playback.take();
        if let Some(call) = self.active_call.as_ref() {
            if let Some(ticket) = call.spool.as_ref() {
                self.call_playback = crate::call::playback::play_spool(ticket);
            }
        }
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
        let call_id: [u8; 16] = rand::random();
        let caller_nonce: [u8; 32] = rand::random();
        let sig = CallSignal::Offer {
            call_id,
            nonce: caller_nonce,
        };
        if !self.send_call_signal(ci, sig) {
            crate::log("CALL: offer send failed (no lane) — not dialing");
            return;
        }
        // Fresh call: clear any stale minimize / speaker state and stop a recording preview (the call owns the audio session).
        self.call_minimized = false;
        self.call_speaker_on = false;
        self.call_playback.take();
        let now = vsf::eagle_time_oscillations();
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
        });
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
        if !self.send_call_signal(ci, CallSignal::Answer { call_id, nonce: callee_nonce }) {
            crate::log("CALL: answer send failed");
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
            let _ = self.send_call_signal(ci, CallSignal::Decline { call_id });
        }
        self.end_call("\u{260E} call declined", offer_osc);
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
        if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
            let _ = self.send_call_signal(ci, CallSignal::Hangup { call_id });
        }
        let summary = match phase {
            CallPhase::Outgoing if we_are_caller => "\u{260E} missed call",
            CallPhase::Active => {
                let _ = phase_osc; // duration rendering rides the summary-row polish (dozenal digits at the edge)
                "\u{260E} call"
            }
            _ => "\u{260E} call",
        };
        self.end_call(summary, offer_osc);
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
        match sig {
            CallSignal::Offer { call_id, nonce } if !row_is_outgoing => {
                match &self.active_call {
                    Some(c) if c.call_id == call_id => {} // duplicate/retransmit
                    Some(_) => {
                        // Busy: only the direct receiver replies (merge is history, and every sibling replying would triplicate it).
                        if !from_merge {
                            let _ = self.send_call_signal(ci, CallSignal::Busy { call_id });
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
            CallSignal::Offer { .. } => {} // our own fleet's outgoing offer echoed via merge — bookkeeping only
            CallSignal::Answer { call_id, nonce } => {
                let Some(call) = self.active_call.as_mut() else {
                    // An answer for a call we don't know (stale offer retransmit rang them after our crash): kill it loudly.
                    if !from_merge && !row_is_outgoing {
                        let _ = self.send_call_signal(ci, CallSignal::Hangup { call_id });
                    }
                    return;
                };
                if call.call_id != call_id {
                    return;
                }
                if row_is_outgoing {
                    // OUR FLEET answered somewhere. If that somewhere isn't here, stop this device's ring — the call lives on the answering device.
                    if call.phase == CallPhase::Ringing && call.callee_nonce != Some(nonce) {
                        crate::log("CALL: a sibling answered — ring stops here");
                        self.active_call = None;
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
                        call.phase = CallPhase::Active;
                        call.phase_osc = vsf::eagle_time_oscillations();
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
                            let _ = self.send_call_signal(ci, CallSignal::Taken { call_id });
                        }
                    }
                    // Ringing/Ended: an answer arriving after we've already left the live window is stale — ignore it.
                    CallPhase::Ringing | CallPhase::Ended => {}
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
                    let text = if matches!(sig, CallSignal::Busy { .. }) {
                        "\u{260E} busy"
                    } else {
                        "\u{260E} call declined"
                    };
                    self.end_call(text, offer_osc);
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
                        // Caller gave up before we answered — the missed-call row, on every device that was ringing (same stamp, merge-folds to one).
                        self.end_call("\u{260E} missed call", offer_osc);
                    }
                    CallPhase::Active => {
                        self.end_call("\u{260E} call", offer_osc);
                    }
                    CallPhase::Outgoing if !we_are_caller => {}
                    CallPhase::Outgoing => {
                        // Friend-side auto-hangup (e.g. answer hit their dead call) — treat as declined-ish end.
                        self.end_call("\u{260E} call", offer_osc);
                    }
                    // Already in the keep/delete window — a late peer hangup changes nothing here.
                    CallPhase::Ended => {}
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
                    self.scene_dirty = true;
                }
            }
        }
    }

    /// Send one signal on the lane (hidden wire content, probe-pattern) AND store it as a hidden OUTGOING row pushed to our siblings — the fleet's ring/stop fan-out (a sibling seeing our Answer row stops its own ring).
    fn send_call_signal(&mut self, ci: usize, sig: CallSignal) -> bool {
        let content = sig.to_content();
        let ts = vsf::eagle_time_oscillations();
        let sent = self.chain_transmit(ci, &content, ts, None, None);
        if sent {
            let mut row = ChatMessage::new_with_timestamp(content, true, ts);
            row.notified = true;
            if let Some(conv) = self.conv_mut_of(ci) {
                conv.insert_message_sorted(row.clone());
            }
            self.persist_messages_async(ci);
            self.push_rows_to_siblings(ci, std::slice::from_ref(&row), None);
        }
        sent
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
        let fid = self.contacts.get(ci)?.friendship_id?;
        let (_, chains) = self.friendship_chains.iter().find(|(id, _)| *id == fid)?;
        let lane_root = chains.lane_root()?;
        let history_key = chains.history_key()?;
        Some(crate::call::keys::derive_call_secret(
            lane_root,
            history_key,
            offer_lane_key,
            call_id,
            caller_nonce,
            callee_nonce,
        ))
    }

    /// Mint the visible summary row (offer_osc+1 — the shared stamp both fleets agree on, +1 clear of the hidden offer row), stop the engine, and either clear the call or park it in Ended for the keep/delete decision (recording by default — an Active call with a spool always gets the choice).
    fn end_call(&mut self, summary: &str, offer_osc: i64) {
        if let Some(call) = &self.active_call {
            if let Some(e) = &call.engine {
                e.stop(); // the engine thread zeroizes its chains, clears the sink, and releases audio
            }
        }
        crate::platform::audio::stop();
        Self::stop_ring_alert_platform();
        let peer = self.active_call.as_ref().map(|c| c.peer_handle_hash);
        let was_caller = self.active_call.as_ref().map(|c| c.we_are_caller).unwrap_or(false);
        let keep_pending = self
            .active_call
            .as_ref()
            .map(|c| c.phase == CallPhase::Active && c.spool.is_some())
            .unwrap_or(false);
        if keep_pending {
            if let Some(call) = self.active_call.as_mut() {
                call.phase = CallPhase::Ended;
                call.final_osc = Some(vsf::eagle_time_oscillations()); // freeze the duration for the end-screen summary
                call.engine = None;
                call.secret = None;
            }
        } else {
            self.active_call = None;
        }
        if let Some(peer) = peer {
            if let Some(ci) = self.contact_index_by_handle_hash(&peer) {
                let mut row =
                    ChatMessage::new_with_timestamp(summary.to_string(), was_caller, offer_osc + 1);
                row.notified = true;
                row.delivered = true;
                if let Some(conv) = self.conv_mut_of(ci) {
                    conv.insert_message_sorted(row.clone());
                }
                self.persist_messages_async(ci);
                self.push_rows_to_siblings(ci, std::slice::from_ref(&row), None);
            }
        }
        self.scene_dirty = true;
    }

    /// The ring alert: platform notification + the relationship RING — the identity chirp's instrument conjugated into a call phrase (`chirp::Chirp::ring_from_hash`: 2×9 jittered hits, τ/3 envelopes), so ears know who's calling before eyes do. Deliberately BYPASSES the will_ding gates: a call is the one always-ring event (design decision 2026-08-18).
    ///
    /// Desktop loops the cadence until the [`crate::call::RingGuard`] stored on the ActiveCall flips — every teardown edge stops it (no timers). Android posts one cadence per offer; looping there is Kotlin's to own (tracked in docs/calls.md).
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
        #[cfg(target_os = "android")]
        {
            // App on screen → the full-screen ring panel IS the alert (and the in-process chirp below rings) — posting the notification too was the double-alert whose heads-up banner covered the old top-bar Answer button. Backgrounded/locked → the CALL-CLASS notification (fullScreenIntent + Answer/Decline actions) is the whole surface.
            if crate::platform::jni_android::app_in_foreground() {
                crate::platform::jni_android::play_ring_chirp(&digest);
            } else {
                crate::platform::jni_android::notify_incoming_call(
                    &ring_hp,
                    &digest,
                    &sender_name,
                    "\u{260E} incoming call",
                );
            }
        }
        // Honor the Notifications "Ring on incoming call" tick: unchecked = the notification still posts (the call is never invisible) but the audible ring loop stays silent.
        let ring_audible = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("notify.ring_call"))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(true);
        #[cfg(not(any(target_os = "android", target_os = "redox")))]
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            crate::platform::desktop_notify::notify_new_message(
                &ring_hp,
                &sender_name,
                "\u{260E} incoming call",
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
    pub(super) fn keep_recording(&mut self) {
        self.call_playback.take(); // stop any preview — the decision is made
        self.call_minimized = false;
        let Some(call) = self.active_call.as_mut() else {
            return;
        };
        if call.phase != CallPhase::Ended {
            return;
        }
        let Some(ticket) = call.spool.take() else {
            self.active_call = None;
            return;
        };
        let (peer, offer_osc) = (call.peer_handle_hash, call.offer_osc);
        let seed = self
            .session
            .as_ref()
            .map(|s| s.identity_seed)
            .unwrap_or([0u8; 32]);
        self.active_call = None;
        // Transcode to the N-channel keep file OFF the UI thread — decode+re-encode is O(call length) and would freeze the frame for seconds on a long call. The worker posts the (hash, size) back over `call_keep_rx`, drained in `tick` → `drain_call_keep`, where the fleet-internal row is minted. Dropping the ticket on any failure still crypto-shreds the spool key (its Drop zeroizes), so a failed transcode degrades safely to delete.
        let tx = self.call_keep_sender();
        let wake = self.event_proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("call-keep".into())
            .spawn(move || {
                let result = crate::call::record::finalize_nchannel(ticket, &seed);
                let _ = tx.send(CallKeepResult {
                    peer,
                    offer_osc,
                    result,
                });
                if let Some(w) = wake {
                    let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                }
            })
            .is_ok();
        if !spawned {
            crate::log("CALL: keep-transcode thread failed to spawn — recording shredded");
        }
        self.scene_dirty = true;
    }

    /// Lazily mint the keep-transcode result channel (worker → UI), mirroring `update_sender`.
    fn call_keep_sender(&mut self) -> std::sync::mpsc::Sender<CallKeepResult> {
        if self.call_keep_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.call_keep_tx = Some(tx);
            self.call_keep_rx = Some(rx);
        }
        self.call_keep_tx.as_ref().unwrap().clone()
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
            match r.result {
                Some((hash, size)) => {
                    if let Some(ci) = self.contact_index_by_handle_hash(&r.peer) {
                        // "call.audio" (video calls will mint "call.video") — no POTS in Photon, so nothing here is a "phone call".
                        let content = crate::types::attachment_content(&hash, "call.audio", size);
                        let mut row =
                            ChatMessage::new_with_timestamp(content, true, r.offer_osc + 2);
                        row.notified = true;
                        row.delivered = true;
                        if let Some(conv) = self.conv_mut_of(ci) {
                            conv.insert_message_sorted(row.clone());
                        }
                        self.persist_messages_async(ci);
                        self.push_rows_to_siblings(ci, std::slice::from_ref(&row), None);
                        crate::logf!(
                            "CALL: recording kept — {} bytes as blob {}…",
                            size,
                            hex::encode(&hash[..4])
                        );
                    }
                }
                None => crate::log("CALL: recording was empty — nothing kept"),
            }
        }
        self.scene_dirty = true;
        true
    }

    /// DELETE the recording: the ticket drops, the key zeroizes, the spool file is ciphertext-garbage — true crypto-shred, instant.
    pub(super) fn delete_recording(&mut self) {
        self.call_playback.take(); // stop any preview — the decision is made
        self.call_minimized = false;
        let Some(call) = self.active_call.as_mut() else {
            return;
        };
        if call.phase != CallPhase::Ended {
            return;
        }
        if let Some(ticket) = call.spool.take() {
            crate::call::spool::shred(ticket);
        }
        self.active_call = None;
        crate::log("CALL: recording shredded");
        self.scene_dirty = true;
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
        // No validated direct address is NOT no engine (field 2026-08-20, the "stuck call": Emma's validated path to Nick expired mid-session, this bail left her engine down, and the call sat Active-and-silent BOTH ways even though Nick held a valid path and its media was arriving — with no engine there was no sink to decode it and no TX to answer with). Start on the RELAY_ADDR sentinel instead: RX needs no address at all (the engine installs the sink), sends to the sentinel are swallowed harmlessly, and the peer's FIRST authenticated packet re-points TX at its real source (address-follows-auth). Media stays dead only when NEITHER side holds an address.
        let addr = self
            .contacts
            .get(ci)
            .and_then(|c| c.race_addrs())
            .map(|(a, _)| a)
            .unwrap_or_else(|| {
                crate::log("CALL: no direct address — engine up on the sentinel; TX will follow the peer's first authenticated packet");
                crate::network::status::RELAY_ADDR
            });
        let call_id8: [u8; 8] = call_id[..8].try_into().unwrap();
        // Recording by default (docs/calls.md): the spool key lives ONLY in this ticket; the engine writes sealed records; keep/delete decides at hangup.
        let (spool_param, ticket) = match crate::call::spool::mint(&call_id8) {
            Some((key, path, ticket)) => (Some((key, path)), Some(ticket)),
            None => (None, None),
        };
        // No call_id in the engine params — the media wire dropped it (the basket-derived key IS the call identity; see packet.rs); the id's only job here is naming the spool above.
        let handle = crate::call::engine::start(crate::call::engine::EngineParams {
            secret,
            we_are_caller,
            peer_addr: addr,
            spool: spool_param,
        });
        (Some(handle), ticket)
    }

    pub(super) fn contact_index_by_handle_hash(&self, hh: &[u8; 32]) -> Option<usize> {
        self.contacts.iter().position(|c| c.handle_hash == *hh)
    }
}
