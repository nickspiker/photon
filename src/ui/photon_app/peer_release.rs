//! Releases travel peer to peer (network/peer_release.rs; the owner 2026-11-19: "use the communications network we built to communicate, not cloudflare").
//! This device holds the newest signed release manifest it has verified, from its own poll or a peer's hand-over, and hands it to each device whose pong says it holds less: our own siblings (fleet and alias) first, then our friends' devices.
//! A manifest a peer hands us is verified off this thread against the release key before anything is held, and one offering this build a newer version makes the update check due with it (settings.rs drive_auto_update).

use super::*;
use crate::network::peer_release::{HeldRelease, ReleaseClaim};

impl PhotonApp {
    /// Is `device` one we take a release manifest from and hand one to: a live device of ours, or a device of a friend's fold?
    fn release_peer(&self, device: &[u8; 32]) -> bool {
        self.contacts.iter().any(|c| c.knows_device(device) && !(c.is_sibling && c.locked_out))
    }

    /// A peer handed us a release manifest: from a device we know, not the copy we hold and not one already being checked, it is verified off this thread (every release egg, release rows, a stamp not ahead of now).
    pub(super) fn release_told(&mut self, manifest: Vec<u8>, sender: [u8; 32]) {
        if !self.release_peer(&sender) {
            crate::logf!("RELEASE: a manifest from {}, a device we do not know — dropped unread", crate::fp(&sender));
            return;
        }
        let hash = *blake3::hash(&manifest).as_bytes();
        if self.peer_release.holds_hash(&hash) {
            // The copy we hold: the giver holds it too, so it is never handed back.
            let stamp = self.peer_release.held().map(|h| h.stamp);
            if let Some(stamp) = stamp {
                self.peer_release.heard(sender, ReleaseClaim::Holds(stamp));
            }
            return;
        }
        if self.peer_release_verifying.contains(&hash) {
            return;
        }
        self.peer_release_verifying.push(hash);
        let tx = self.update_sender();
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            let result = crate::network::peer_release::verify(&manifest, vsf::eagle_time_oscillations());
            crate::log_err!(tx.send(UpdateEvent::ReleaseChecked { hash, from: Some(sender), result }), "hand a peer's release manifest check to the UI");
            if let Some(w) = wake.as_ref() {
                wake_ui(w);
            }
        });
    }

    /// A release manifest checked off this thread: a refused one is logged and dropped (never held, never passed on); a verified one is taken, and one newer than ours is held, claimed in our pongs, passed on, and may make the update check due.
    pub(super) fn take_release_checked(&mut self, hash: [u8; 32], from: Option<[u8; 32]>, result: Result<HeldRelease, String>) {
        self.peer_release_verifying.retain(|h| *h != hash);
        let rel = match result {
            Ok(rel) => rel,
            Err(e) => {
                crate::logf!("RELEASE: the manifest from {} was refused ({}) — not held, not passed on", from.map(|d| crate::fp(&d)).unwrap_or_default(), e);
                return;
            }
        };
        let stamp = rel.stamp;
        let version = crate::network::updates::our_row(&rel.rows).map(|r| r.version_string());
        let taken = self.peer_release.take(rel, from, crate::network::updates::our_version());
        if !taken.newer {
            return;
        }
        crate::network::status::set_own_release_stamp(stamp);
        crate::logf!(
            "RELEASE: holding the signed release manifest stamped {} ({} for this platform) from {}",
            stamp,
            version.as_deref().unwrap_or("no row"),
            match from {
                Some(d) => format!("device {}", crate::fp(&d)),
                None => "our own poll".to_string(),
            }
        );
        if let Some(due) = taken.check_due {
            self.peer_release_due = Some(due);
        }
        self.release_pass_on();
    }

    /// Our manifest just became newer: every online device that told us it holds less is handed it now, our own siblings first, then our friends' devices.
    fn release_pass_on(&mut self) {
        let mut candidates: Vec<[u8; 32]> = self.contacts.iter().filter(|c| c.is_sibling && !c.locked_out && c.is_online).filter_map(|c| c.device_key()).collect();
        for c in self.contacts.iter().filter(|c| !c.is_sibling) {
            for ep in c.device_endpoints.iter().filter(|ep| ep.online && c.knows_device(&ep.pubkey)) {
                if !candidates.contains(&ep.pubkey) {
                    candidates.push(ep.pubkey);
                }
            }
        }
        for dev in self.peer_release.owed(&candidates) {
            self.send_release_to(dev, "our manifest is newer");
        }
    }

    /// A device's pong said what release manifest it holds: one holding less than ours is handed it, once per link.
    pub(super) fn release_claimed(&mut self, device: [u8; 32], claim: ReleaseClaim) {
        if !self.release_peer(&device) {
            return;
        }
        if self.peer_release.heard(device, claim) {
            self.send_release_to(device, "its pong says it holds an older one");
        }
    }

    /// Hand our manifest to `device` in a `release_tell` frame: a sibling by its row's paths and relays, a friend's device at its own best addresses; with no path it stays owed for its next pong.
    fn send_release_to(&mut self, device: [u8; 32], why: &str) {
        let Some(frame) = self.release_frame() else {
            self.peer_release.untold(&device);
            return;
        };
        let stamp = self.peer_release.held().map(|h| h.stamp).unwrap_or_default();
        if self.contacts.iter().any(|c| c.is_sibling && !c.locked_out && c.device_key() == Some(device)) {
            crate::logf!("RELEASE: the manifest stamped {} to sibling {} ({})", stamp, crate::fp(&device), why);
            self.dispatch_frame_to_sibling(frame, Some(device));
            return;
        }
        let path = self.contacts.iter().filter(|c| !c.is_sibling && c.knows_device(&device)).find_map(|c| c.race_addrs_for(&device));
        match path {
            Some((addr, alt)) => {
                crate::logf!("RELEASE: the manifest stamped {} to a friend's device {} ({})", stamp, crate::fp(&device), why);
                self.send_frame_direct(frame, device, addr, alt);
            }
            None => {
                crate::logf!("RELEASE: no path to {} for the manifest yet — its next pong carries it", crate::fp(&device));
                self.peer_release.untold(&device);
            }
        }
    }

    /// The `release_tell` frame for the manifest held, built once per manifest and signed by this device.
    fn release_frame(&mut self) -> Option<Vec<u8>> {
        let held = self.peer_release.held()?;
        if let Some((stamp, frame)) = self.peer_release_frame.as_ref() {
            if *stamp == held.stamp {
                return Some(frame.clone());
            }
        }
        let kp = self.device_keypair.as_ref()?;
        match crate::network::fgtw::protocol::build_release_tell_vsf(&held.bytes, kp.public.as_bytes(), kp.secret.as_bytes()) {
            Ok(frame) => {
                self.peer_release_frame = Some((held.stamp, frame.clone()));
                Some(frame)
            }
            Err(e) => {
                crate::logf!("RELEASE: the hand-over frame did not build ({})", e);
                None
            }
        }
    }
}
