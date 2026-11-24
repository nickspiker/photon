//! The Security page on the shared model (fgtw::security, owner 2026-11-24: "I'd like to get it synchronized in the FGTW crate so ferros and Photon render the same"): the rows, their order, when each is enabled and why not, the words and the page's state are fgtw's; what is here is Photon's half.
//! The facts the model reads, from RAM only (the render calls it every frame): the session, the sibling rows, the fleet-synced locked set, the fleet key's RAM copy and the Depart machine.
//! The identity confirmations, Photon's two ways: a revoke or a reinstatement is confirmed on the attest screen (this device de-attests and the act runs inside the next attest whose handle is this identity's, as the Fleet page's Lock out always was), a shred or an unbind by the handle typed in the box under its row, its memory-hard proof matched.
//! The hooks that carry each act out: the lock's session teardown, `clean_device_for_reuse` for a shred and a wipe, the Depart machine for an unbind, `revoke_device` and `reinstate_fleet_device` for the chain acts.
//! Photon's own rows (kill, the shell switch, load on startup, the lifeline, unattended) are drawn after the shared ones.

use super::*;
use fgtw::security::{Act, Do, Facts, Other, Phase, Row, Status, Tone};

/// The settings pill slot of the shared rows' first; the rows run on from it in [`fgtw::security::rows`]'s order. Below it: 4 kill, 5 the shell switch, 6 "Copy, then release".
pub(super) const SECURITY_ROW_SLOT0: HitId = 1 << 3;

/// A row's colour: the destructiveness ramp, least to most (green, yellow, orange, red).
pub(super) fn tone_fill(tone: Tone) -> (u32, u32) {
    match tone {
        Tone::Calm => *theme::PILL_GREEN,
        Tone::Caution => *theme::PILL_YELLOW,
        Tone::Warn => *theme::PILL_ORANGE,
        Tone::Danger => *theme::PILL_RED,
    }
}

impl PhotonApp {
    /// The fleet's other members as the page shows them: every sibling row holding a device key, in the rows' order, minus the retired; revoked when the fleet has it locked out.
    fn security_others(&self) -> Vec<Other> {
        let me = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let locked = self.locked_devices();
        let mut others: Vec<Other> = Vec::new();
        for c in self.contacts.iter().filter(|c| c.is_sibling) {
            let Some(key) = c.device_key() else {
                continue;
            };
            if Some(key) == me || self.fleet_retired.contains(&key) || others.iter().any(|o| o.key == key) {
                continue;
            }
            others.push(Other { key, revoked: c.locked_out || locked.contains(&key) });
        }
        others
    }

    /// The page's words at the top and its rows, from what this device knows now.
    pub(super) fn security_model(&self) -> (fgtw::text::Msg<'static>, Vec<Row>) {
        let others = self.security_others();
        let present = self.session.is_some();
        let me = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let facts = Facts {
            present,
            member: present && me.is_some_and(|k| !self.is_locked_device(&k)),
            // The RAM copy: the render makes no vault call.
            fleet_key: self.fleet_key_ram.lock().ok().is_some_and(|g| g.is_some()),
            leaving: self.depart.as_ref().is_some_and(|m| m.asking() || matches!(m.removal(), fgtw::flow::depart::Removal::Held { .. })),
            others: &others,
        };
        (fgtw::security::intro(&facts), fgtw::security::rows(&facts))
    }

    /// The device a row names, by its name in this fleet.
    pub(super) fn security_device_name(&self, act: Act) -> String {
        match (act.device(), self.session.as_ref()) {
            (Some(k), Some(s)) => super::machine_name(&k, &s.identity_seed, self.fleet_settings.as_ref()),
            _ => String::new(),
        }
    }

    /// The handle box is open: an unbind or a shred is asked.
    pub(super) fn security_box_open(&self) -> bool {
        self.settings_removeshred_armed || self.security_asked() == Some(Act::Shred)
    }

    /// The act waiting on its identity confirmation.
    pub(super) fn security_asked(&self) -> Option<Act> {
        match self.security_page.status() {
            Some(Status { act, phase: Phase::Asked }) => Some(act),
            _ => None,
        }
    }

    /// A press on the shared row at `i` of the rows drawn.
    pub(super) fn security_tap(&mut self, i: usize) {
        let (_, rows) = self.security_model();
        // WHY/PROOF: the rows are rebuilt from the facts each frame; a hit from the frame the person saw can outlive a device leaving the list.
        let Some(act) = rows.get(i).map(|r| r.act) else {
            return;
        };
        // A second press on the row asked puts it back: the box closes and nothing is sent.
        if self.security_asked() == Some(act) {
            self.security_ask_closed(act);
            return;
        }
        match self.security_page.press(&rows, act) {
            Do::Nothing => {}
            Do::Confirm(a) => self.security_confirm(a),
            Do::Carry(a) => self.security_carry(a),
        }
    }

    /// Collect the identity confirmation an act needs.
    fn security_confirm(&mut self, act: Act) {
        match act {
            Act::Revoke(k) | Act::Reinstate(k) => {
                let reinstate = matches!(act, Act::Reinstate(_));
                let name = self.security_device_name(act);
                let words = if reinstate {
                    tr(Msg::ConfirmReinstate(&name)).into_owned()
                } else {
                    // The last other device able to reinstate: the row's warning, said again where the handle is typed.
                    let live = self.security_others().iter().filter(|o| !o.revoked).count();
                    tr(Msg::ConfirmRevoke { name: &name, last_unlocker: live == 1 }).into_owned()
                };
                self.confirm_by_attest(k, name, reinstate, words);
            }
            Act::Shred => {
                self.security_verify = None;
                self.security_wrong = false;
                if let Some(tb) = self.release_confirm_tb.as_mut() {
                    tb.clear();
                    let id = tb.hit_id();
                    self.change_focus(Some(id));
                }
            }
            Act::Unbind => {
                self.release_arm();
                // The Depart machine refused it (the last device, no fleet known): nothing is asked.
                if !self.settings_removeshred_armed {
                    self.security_page.confirmed(act, false);
                }
            }
            Act::Lock | Act::Wipe => {}
        }
    }

    /// An asked act put back, or the page left with it asked: the handle box closes, nothing is sent, the row is as before.
    pub(super) fn security_ask_closed(&mut self, act: Act) {
        if act == Act::Unbind {
            self.settings_removeshred_armed = false;
            self.depart_removal_cancel();
        }
        self.security_verify = None;
        self.security_wrong = false;
        if let Some(tb) = self.release_confirm_tb.as_mut() {
            tb.clear();
        }
        self.change_focus(None);
        self.security_page.confirmed(act, false);
    }

    /// The pill under the handle box: the typed handle's proof starts, for the act asked.
    pub(super) fn security_box_submit(&mut self) {
        match self.security_asked() {
            Some(Act::Unbind) => self.release_verify_start(false),
            Some(Act::Shred) => self.security_verify_start(),
            _ => {}
        }
    }

    /// A shred's typed handle: its ~1 s memory-hard proof runs OFF the UI thread, as attest's does, so the box is never a cheap handle oracle.
    fn security_verify_start(&mut self) {
        if self.security_verify.is_some() {
            return;
        }
        let typed: String = self.release_confirm_tb.as_ref().map(|tb| tb.chars.iter().collect()).unwrap_or_default();
        if typed.is_empty() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            let proof = crate::types::Handle::username_to_handle_proof(&typed);
            crate::log_err!(tx.send(proof), "the typed handle's proof to the UI thread");
            #[cfg(not(target_os = "android"))]
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
            #[cfg(target_os = "android")]
            // Android wakes thru JNI, not the proxy: the clone has nothing to do here.
            drop(wake);
        });
        self.security_verify = Some(rx);
        self.security_wrong = false;
    }

    /// The typed handle's proof is in (tick): this identity's, the act asked is carried out; another's, the box empties and says so, nothing done. Returns whether anything changed.
    pub(super) fn security_verify_drain(&mut self) -> bool {
        let Some(proof) = self.security_verify.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.security_verify = None;
        let Some(act) = self.security_asked() else {
            return true;
        };
        if let Some(tb) = self.release_confirm_tb.as_mut() {
            tb.clear();
        }
        if !self.session.as_ref().is_some_and(|s| s.handle_proof == proof) {
            crate::log("SECURITY: the handle typed for a shred is not this identity's; nothing done");
            self.security_wrong = true;
            return true;
        }
        self.change_focus(None);
        if let Do::Carry(a) = self.security_page.confirmed(act, true) {
            self.security_carry(a);
        }
        true
    }

    /// The Depart machine took the unbind (sent, or held for its copies): the page's act is confirmed and carried.
    pub(super) fn security_unbind_taken(&mut self) {
        if self.security_page.confirmed(Act::Unbind, true) == Do::Carry(Act::Unbind) {
            self.security_page.finished(Act::Unbind, true);
        }
    }

    /// Carry an act out with Photon's hooks.
    fn security_carry(&mut self, act: Act) {
        match act {
            Act::Lock => {
                self.security_lock();
                self.security_page.finished(act, true);
            }
            Act::Shred => {
                // The shred as it stands until the vault salt lands (docs/settings-panel.md "Security"): the device vault discarded thru kete. Keys only, nothing deleted, once the salt is in.
                crate::log("SECURITY: shred, confirmed with the handle; this device's copy of the identity destroyed, and it stays in the fleet");
                self.clean_device_for_reuse();
            }
            // Before attest nothing is drawn, so nothing is pressed (owner, 2026-11-24: "if you cannot attest, there's ZERO reason my shit should get wiped"); the arm stays only while fgtw still names the act.
            Act::Wipe => {}
            Act::Revoke(k) => {
                self.revoke_device(k);
                self.security_page.finished(act, true);
            }
            Act::Reinstate(k) => {
                let name = self.security_device_name(act);
                self.reinstate_fleet_device(k, &name);
                self.security_page.finished(act, true);
            }
            // The Depart machine carries it; it was finished when the machine took it (security_unbind_taken).
            Act::Unbind => {}
        }
    }

    /// Lock: the identity's keys in RAM forgotten, the vault kept; typing the handle again opens it. Works on Android (the -1 broadcast drops Kotlin's sticky session).
    fn security_lock(&mut self) {
        tohu::clear_session();
        self.session = None;
        self.private_s = crate::crypto::blind::PrivateS::None;
        self.pending_broadcast_signal = -1;
        self.state = self.not_signed_in_screen();
        self.clear_handle_for_reproof();
        crate::log("SECURITY: locked — session cleared, vault kept; re-type handle to unlock");
    }

    /// Revoke or reinstate another device, confirmed on the attest screen (the Fleet page's Lock out and Unlock, the Security page's revoke and reinstate): this device de-attests now, and the act runs only inside the next attest whose handle is this identity's (launch.rs). The owner knows the handle and sails thru; a thief just signed themselves out of the one device they held.
    pub(super) fn confirm_by_attest(&mut self, pk: [u8; 32], name: String, reinstate: bool, words: String) {
        let Some(hp) = self.session.as_ref().map(|s| s.handle_proof) else {
            return;
        };
        if reinstate {
            self.pending_unlock = Some((pk, hp, name.clone()));
        } else {
            self.pending_lock = Some((pk, hp, name.clone()));
        }
        tohu::clear_session();
        self.session = None;
        self.private_s = crate::crypto::blind::PrivateS::None;
        self.pending_broadcast_signal = -1;
        self.state = AppState::Launch(LaunchState::Error(words));
        self.clear_handle_for_reproof();
        // The page is left with the session: nothing of it stands.
        self.security_page = fgtw::security::Page::new();
        crate::logf!("FLEET: {} of {} armed — de-attested, awaiting handle confirmation", if reinstate { "unlock" } else { "lock-out" }, name);
    }
}
