//! The Octopus page (Nick 2026-11-10): typing exactly "Octopus" in the Diagnostics note opens it for this run of Photon, and nothing about it is saved.
//! It shows fgtw.org's buckets with a Clear each, Nuke all, the worker's activity log and this device's phonebook.
//! fgtw.org only counts, hands over its log and clears (`network::fgtw::octopus`); the page is drawn here, on the device.

use super::PhotonApp;
use crate::network::fgtw::octopus::{self as op, OctopusView};

/// An Octopus request's answer.
pub(super) enum OctopusEvent {
    View(Result<OctopusView, String>),
    /// What was cleared, and how many objects went.
    Cleared(String, Result<u64, String>),
}

/// The word that opens the page, typed exactly.
const OCTOPUS_WORD: &str = "Octopus";

impl PhotonApp {
    /// Open the Octopus page the moment the Diagnostics note reads exactly "Octopus", and clear the note so the word never rides a submitted log. Returns whether anything changed.
    pub(super) fn octopus_unlock_check(&mut self) -> bool {
        let typed = self.settings_note_textbox.as_ref().is_some_and(|tb| tb.chars.iter().copied().eq(OCTOPUS_WORD.chars()));
        if !typed {
            return false;
        }
        if let Some(tb) = self.settings_note_textbox.as_mut() {
            tb.clear();
        }
        if !self.octopus_open {
            self.octopus_open = true;
            crate::log("OCTOPUS: the page is open for this run of Photon");
        }
        self.scene_dirty = true;
        true
    }

    /// This device's key and identity, which sign every Octopus request; None (with the reason on the page) before attest.
    fn octopus_signer(&mut self) -> Option<(crate::network::fgtw::Keypair, [u8; 32])> {
        match (self.device_keypair.clone(), self.our_handle_proof()) {
            (Some(kp), Some(hp)) => Some((kp, hp)),
            _ => {
                self.octopus_status = Some("Needs an attested Photon".to_string());
                None
            }
        }
    }

    /// Copy this device's phonebook and ask fgtw.org for its buckets and log; one request at a time.
    pub(super) fn octopus_refresh(&mut self) {
        self.octopus_peers = self.peer_store.as_ref().map(|s| s.lock().unwrap().get_all_peers()).unwrap_or_default();
        self.scene_dirty = true;
        if self.octopus_rx.is_some() {
            return;
        }
        let Some((kp, hp)) = self.octopus_signer() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        self.octopus_rx = Some(rx);
        std::thread::spawn(move || {
            crate::log_err!(tx.send(OctopusEvent::View(op::view_blocking(&kp, &hp))), "the Octopus view to the UI thread");
        });
    }

    /// Empty one bucket (`Some`) or everything but fgtw.org's landing page (`None`), then refresh.
    pub(super) fn octopus_clear(&mut self, bucket: Option<String>) {
        if self.octopus_rx.is_some() {
            return;
        }
        let Some((kp, hp)) = self.octopus_signer() else { return };
        let label = bucket.clone().unwrap_or_else(|| "everything".to_string());
        crate::logf!("OCTOPUS: clearing {}", label);
        self.octopus_status = Some(format!("Clearing {label}…"));
        self.scene_dirty = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.octopus_rx = Some(rx);
        std::thread::spawn(move || {
            let result = op::clear_blocking(&kp, &hp, bucket.as_deref());
            crate::log_err!(tx.send(OctopusEvent::Cleared(label, result)), "an Octopus clear to the UI thread");
        });
    }

    /// Take an Octopus answer when it lands. Returns whether the page changed.
    pub(super) fn drain_octopus(&mut self) -> bool {
        let event = match self.octopus_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(event)) => event,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.octopus_rx = None;
                self.octopus_status = Some("The request ended without an answer".to_string());
                self.scene_dirty = true;
                return true;
            }
            _ => return false,
        };
        self.octopus_rx = None;
        match event {
            OctopusEvent::View(Ok(view)) => self.octopus_view = Some(view),
            OctopusEvent::View(Err(e)) => self.octopus_status = Some(format!("fgtw.org: {e}")),
            OctopusEvent::Cleared(label, Ok(n)) => {
                crate::logf!("OCTOPUS: cleared {} object(s) from {}", n, label);
                self.octopus_status = Some(format!("Cleared {n} object(s) from {label}"));
                self.octopus_refresh();
            }
            OctopusEvent::Cleared(label, Err(e)) => self.octopus_status = Some(format!("Clearing {label} failed: {e}")),
        }
        self.scene_dirty = true;
        true
    }
}
