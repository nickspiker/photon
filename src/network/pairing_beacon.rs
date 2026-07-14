//! Pairing v2 proximity beacon — the transport seam (docs/pairing-v2.md, milestone A: SHADOW MODE).
//!
//! One frame format (the `fgtw::pair` beacon codec), per-platform couriers: Android advertises AND scans via the `PhotonBeacon` Kotlin bridge; Linux scans via bluer (BlueZ over D-Bus); everything else stubs with a log line (Linux/Windows/macOS advertisers land in milestone B).
//! The ceremony never learns which courier ran — frames go out via [`announce_guard`], frames come in thru [`on_frame_heard`], and everything between is plumbing.
//! Shadow mode means: the v1 words ceremony keeps doing the real binding while this module proves the radio path — heard beacons are logged + stored, and nothing else happens.
//!
//! Wire detail: the frame is split across TWO manufacturer-data entries (company ids 0xFFFF then 0xFFFE) because both Android's ScanRecord and BlueZ merge ADV+SCAN_RSP into one map keyed by company id — one id per chunk, concatenated on receive. 0xFFFF/0xFFFE are the Bluetooth SIG's reserved/internal-use ids, correct for a non-shipping protocol.

use std::sync::Mutex;
use std::time::Instant;

/// Max payload bytes in the primary advertising packet's manufacturer-data entry: 31 bytes legacy ADV budget − 3 flags − 4 AD header/company-id. The remainder of a frame rides the scan response (own 31-byte budget, no flags, 27 usable).
pub const ADV_CHUNK: usize = 24;

/// A candidate heard by the scanner, deduped by device pubkey. `proof` fills in when the candidate's beacon upgrades from announce to proof (milestone B consumes this; shadow mode only logs it).
#[derive(Clone, Debug)]
pub struct HeardCandidate {
    pub device_pubkey: [u8; 32],
    pub proof: Option<[u8; fgtw::pair::WORD_MAC_LEN]>,
    pub last_seen: Instant,
}

/// The scan filter: `Some(prefix)` while scanning (frames for other identities are dropped at ingest), `None` when idle.
static SCAN_PREFIX: Mutex<Option<[u8; 4]>> = Mutex::new(None);

/// Everything heard this scan session, deduped by pubkey. Cleared on scan start.
static HEARD: Mutex<Vec<HeardCandidate>> = Mutex::new(Vec::new());

// ── Announce (new device) ──

/// Advertise `{hp_prefix, device_pubkey}` for as long as the returned guard lives — tie it to the join ceremony's thread scope so every exit path (bind, cancel, error) stops the radio. Shadow mode: v1 keeps binding via the relay while this proves the airwaves.
pub fn announce_guard(handle_proof: &[u8; 32], device_pubkey: &[u8; 32]) -> AnnounceGuard {
    let frame = fgtw::pair::beacon_announce(handle_proof, device_pubkey);
    start_announce_frame(&frame);
    AnnounceGuard(())
}

/// Stops the announce beacon on drop.
pub struct AnnounceGuard(());

impl Drop for AnnounceGuard {
    fn drop(&mut self) {
        stop_announce();
    }
}

fn start_announce_frame(frame: &[u8]) {
    let (adv, rsp) = frame.split_at(frame.len().min(ADV_CHUNK));
    imp::start_announce(adv, rsp);
}

fn stop_announce() {
    imp::stop_announce();
}

// ── Scan (old device) ──

/// Start scanning for pairing beacons carrying `own_prefix` (this identity's `hp_prefix` — other fleets pairing in the same room are dropped at ingest). Clears the heard list.
pub fn start_scan(own_prefix: [u8; 4]) {
    *SCAN_PREFIX.lock().unwrap() = Some(own_prefix);
    HEARD.lock().unwrap().clear();
    imp::start_scan();
}

/// Stop scanning. The heard list survives until the next start so a ceremony screen can still read what it saw.
pub fn stop_scan() {
    *SCAN_PREFIX.lock().unwrap() = None;
    imp::stop_scan();
}

/// The candidates heard this scan session (deduped by pubkey, newest state per device).
pub fn heard() -> Vec<HeardCandidate> {
    HEARD.lock().unwrap().clone()
}

/// Shared ingest for every courier: parse, filter on our hp prefix, dedupe-log, store. Called by the bluer task (Linux) and `nativeOnBeaconHeard` (Android). Logs on NEW information only (first sighting, announce→proof upgrade) — beacons repeat at ~10 Hz and the log must not.
pub fn on_frame_heard(bytes: &[u8]) {
    let Some(beacon) = fgtw::pair::parse_beacon(bytes) else {
        return; // scanner noise (someone else's manufacturer data)
    };
    let Some(want) = *SCAN_PREFIX.lock().unwrap() else {
        return; // scan already stopped; late callback
    };
    let (prefix, pubkey, proof) = match beacon {
        fgtw::pair::Beacon::Announce { hp_prefix, device_pubkey } => (hp_prefix, device_pubkey, None),
        fgtw::pair::Beacon::Proof { hp_prefix, device_pubkey, word_mac } => {
            (hp_prefix, device_pubkey, Some(word_mac))
        }
    };
    if prefix != want {
        return; // someone else's fleet
    }
    let mut heard = HEARD.lock().unwrap();
    match heard.iter_mut().find(|c| c.device_pubkey == pubkey) {
        Some(c) => {
            let upgraded = proof.is_some() && c.proof.is_none();
            c.last_seen = Instant::now();
            if proof.is_some() {
                c.proof = proof;
            }
            if upgraded {
                crate::log(&format!(
                    "BEACON: candidate {} upgraded announce→proof",
                    hex::encode(&pubkey[..8])
                ));
            }
        }
        None => {
            crate::log(&format!(
                "BEACON: heard {} candidate pk = {}",
                if proof.is_some() { "proof" } else { "announce" },
                hex::encode(&pubkey[..8])
            ));
            heard.push(HeardCandidate { device_pubkey: pubkey, proof, last_seen: Instant::now() });
        }
    }
}

// ── Linux courier: bluer (BlueZ) scanner. Advertiser lands in milestone B. ──

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// The running scan task's stop flag; replaced on every start (a stale task sees its own flag set and winds down).
    static SCAN_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

    pub(super) fn start_announce(_adv: &[u8], _rsp: &[u8]) {
        // BlueZ CAN advertise (bluer Advertisement) — deferred to milestone B with the rest of the desktop-as-new-device path.
        crate::log("BEACON: announce requested — Linux advertiser lands in milestone B, words still carry this ceremony");
    }

    pub(super) fn stop_announce() {}

    pub(super) fn start_scan() {
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(old) = SCAN_STOP.lock().unwrap().replace(stop.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        crate::network::http::runtime().spawn(async move {
            if let Err(e) = scan(stop).await {
                crate::log(&format!("BEACON: bluer scan failed: {e}"));
            }
        });
    }

    pub(super) fn stop_scan() {
        if let Some(stop) = SCAN_STOP.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn ingest_map(md: &std::collections::HashMap<u16, Vec<u8>>) {
        // Reassemble the two-chunk split: 0xFFFF is the ADV half, 0xFFFE the scan-response half.
        let Some(a) = md.get(&0xFFFF) else { return };
        let mut frame = a.clone();
        if let Some(b) = md.get(&0xFFFE) {
            frame.extend_from_slice(b);
        }
        super::on_frame_heard(&frame);
    }

    async fn scan(stop: Arc<AtomicBool>) -> bluer::Result<()> {
        let session = bluer::Session::new().await?;
        let adapter = session.default_adapter().await?;
        adapter.set_powered(true).await?;
        // duplicate_data: beacons REPEAT (and upgrade announce→proof mid-session) — without it BlueZ caches the first sighting and the proof upgrade never arrives.
        let filter = bluer::DiscoveryFilter {
            transport: bluer::DiscoveryTransport::Le,
            duplicate_data: true,
            ..Default::default()
        };
        adapter.set_discovery_filter(filter).await?;
        let mut events = adapter.discover_devices().await?;
        crate::log("BEACON: bluer scan started");
        while !stop.load(Ordering::Relaxed) {
            tokio::select! {
                ev = events.next() => {
                    let Some(ev) = ev else { break };
                    if let bluer::AdapterEvent::DeviceAdded(addr) = ev {
                        let Ok(dev) = adapter.device(addr) else { continue };
                        // Whatever BlueZ already cached for this device, then live property changes (the announce→proof swap arrives as a ManufacturerData change).
                        if let Ok(Some(md)) = dev.manufacturer_data().await {
                            ingest_map(&md);
                        }
                        let stop_dev = stop.clone();
                        if let Ok(mut dev_events) = dev.events().await {
                            tokio::spawn(async move {
                                while let Some(ev) = dev_events.next().await {
                                    if stop_dev.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    if let bluer::DeviceEvent::PropertyChanged(
                                        bluer::DeviceProperty::ManufacturerData(md),
                                    ) = ev
                                    {
                                        ingest_map(&md);
                                    }
                                }
                            });
                        }
                    }
                }
                // Periodic stop-flag check so a quiet room still winds the task down promptly.
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
        }
        crate::log("BEACON: bluer scan stopped");
        Ok(())
    }
}

// ── Android courier: JNI bridge to the PhotonBeacon Kotlin object (advertiser + scanner). ──

#[cfg(target_os = "android")]
mod imp {
    pub(super) fn start_announce(adv: &[u8], rsp: &[u8]) {
        crate::platform::jni_android::beacon_call_bytes("startAdvertise", adv, rsp);
    }

    pub(super) fn stop_announce() {
        crate::platform::jni_android::beacon_call("stopAdvertise");
    }

    pub(super) fn start_scan() {
        crate::platform::jni_android::beacon_call("startScan");
    }

    pub(super) fn stop_scan() {
        crate::platform::jni_android::beacon_call("stopScan");
    }
}

// ── Everything else: stub couriers (Windows/macOS scanners land in milestone B via btleplug; Redox when ferros owns a radio). ──

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod imp {
    pub(super) fn start_announce(_adv: &[u8], _rsp: &[u8]) {
        crate::log("BEACON: no advertiser on this platform yet — words carry the ceremony");
    }

    pub(super) fn stop_announce() {}

    pub(super) fn start_scan() {
        crate::log("BEACON: no scanner on this platform yet — words carry the ceremony");
    }

    pub(super) fn stop_scan() {}
}
