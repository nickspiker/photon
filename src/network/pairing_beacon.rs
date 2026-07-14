//! Pairing v2 proximity beacon — the transport seam (docs/pairing-v2.md).
//!
//! One frame, one carrier, every platform: a single 128-bit BLE **service UUID** = `[ magic:4 ][ nonce:4 ][ keyed_tag:8 ]` (see [`fgtw::pair::beacon_uuid`]). Service UUID rather than manufacturer data because it's the only advertising payload Apple's CoreBluetooth lets an app emit — so the same beacon works on Linux, Android, Windows AND macOS with no per-platform frame fork.
//! The ceremony never learns which courier ran: the new device announces thru [`announce_guard`], the sponsor's scanner drops heard UUIDs into [`heard`], and the sponsor's matcher runs [`fgtw::pair::beacon_matches`] against its registry candidates to decide which are in proximity. Everything between is plumbing.
//! Per-platform couriers: Android advertises AND scans (the `PhotonBeacon` Kotlin bridge); Linux advertises AND scans via bluer (BlueZ over D-Bus); macOS/Windows land next (btleplug scan + native advertise); Redox waits on a ferros radio.

use std::sync::Mutex;
use std::time::Instant;

use fgtw::pair::BEACON_UUID_LEN;

/// A beacon UUID the scanner heard (already magic-filtered), deduped by exact UUID. The sponsor's matcher tests each against its registry candidates with [`fgtw::pair::beacon_matches`] — this module never needs the fleet key.
#[derive(Clone, Debug)]
pub struct HeardBeacon {
    pub uuid: [u8; BEACON_UUID_LEN],
    pub last_seen: Instant,
}

/// True while a scan session is live — couriers gate their ingest on it so a late callback after `stop_scan` is dropped.
static SCANNING: Mutex<bool> = Mutex::new(false);

/// Everything heard this scan session, deduped by UUID. Cleared on scan start.
static HEARD: Mutex<Vec<HeardBeacon>> = Mutex::new(Vec::new());

// ── Announce (new device) ──

/// Advertise this device's join beacon for as long as the returned guard lives — tie it to the join ceremony's thread scope so every exit path (bind, cancel, error) stops the radio. A fresh random nonce is minted here per ceremony so the same device is unlinkable across pairings.
pub fn announce_guard(handle_proof: &[u8; 32], device_pubkey: &[u8; 32]) -> AnnounceGuard {
    let nonce: [u8; 4] = rand::random();
    let uuid = fgtw::pair::beacon_uuid(handle_proof, device_pubkey, &nonce);
    imp::start_announce(uuid);
    AnnounceGuard(())
}

/// Stops the announce beacon on drop.
pub struct AnnounceGuard(());

impl Drop for AnnounceGuard {
    fn drop(&mut self) {
        imp::stop_announce();
    }
}

// ── Scan (old device / sponsor) ──

/// Start scanning for pairing beacons. The scanner is identity-agnostic — it collects every photon-magic service UUID it hears (foreign fleets included) and the sponsor's matcher rejects non-candidates via the keyed tag. Clears the heard list.
pub fn start_scan() {
    *SCANNING.lock().unwrap() = true;
    HEARD.lock().unwrap().clear();
    imp::start_scan();
}

/// Stop scanning. The heard list survives until the next start so a ceremony screen can still read what it saw.
pub fn stop_scan() {
    *SCANNING.lock().unwrap() = false;
    imp::stop_scan();
}

/// The beacon UUIDs heard this scan session (deduped, newest `last_seen`).
pub fn heard() -> Vec<HeardBeacon> {
    HEARD.lock().unwrap().clone()
}

/// Shared ingest for every courier: magic-filter, dedupe, store. Called by the bluer task (Linux) and `nativeOnBeaconHeard` (Android). Logs on FIRST sighting only — beacons repeat at scan rate and the log must not.
pub fn on_uuid_heard(uuid: [u8; BEACON_UUID_LEN]) {
    if !fgtw::pair::beacon_is_ours(&uuid) {
        return; // scanner noise (someone else's service UUID)
    }
    if !*SCANNING.lock().unwrap() {
        return; // scan already stopped; late callback
    }
    let mut heard = HEARD.lock().unwrap();
    match heard.iter_mut().find(|b| b.uuid == uuid) {
        Some(b) => b.last_seen = Instant::now(),
        None => {
            crate::log(&format!("BEACON: heard candidate {}", hex::encode(&uuid[..8])));
            heard.push(HeardBeacon { uuid, last_seen: Instant::now() });
        }
    }
}

// ── Linux courier: bluer (BlueZ) advertiser + scanner. ──

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// The running scan task's stop flag; replaced on every start (a stale task sees its own flag set and winds down).
    static SCAN_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
    /// The running advertise task's stop flag; dropping the held advertisement handle (when this flips) unregisters the beacon with BlueZ.
    static ADV_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

    pub(super) fn start_announce(uuid: [u8; BEACON_UUID_LEN]) {
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(old) = ADV_STOP.lock().unwrap().replace(stop.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        crate::network::http::runtime().spawn(async move {
            if let Err(e) = advertise(uuid, stop).await {
                crate::log(&format!("BEACON: bluer advertise failed: {e}"));
            }
        });
    }

    pub(super) fn stop_announce() {
        if let Some(stop) = ADV_STOP.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Advertise the beacon as a single 128-bit service UUID and hold it until stopped. 16 bytes + header fits the legacy 31-byte ADV with room to spare — no extended-advertising dependency, non-connectable.
    async fn advertise(uuid: [u8; BEACON_UUID_LEN], stop: Arc<AtomicBool>) -> bluer::Result<()> {
        let session = bluer::Session::new().await?;
        let adapter = session.default_adapter().await?;
        adapter.set_powered(true).await?;
        let mut service_uuids = std::collections::BTreeSet::new();
        service_uuids.insert(bluer::Uuid::from_bytes(uuid));
        let le_adv = bluer::adv::Advertisement {
            advertisement_type: bluer::adv::Type::Broadcast,
            service_uuids,
            discoverable: Some(true),
            min_interval: Some(std::time::Duration::from_millis(100)),
            max_interval: Some(std::time::Duration::from_millis(150)),
            ..Default::default()
        };
        // Held for the ceremony's lifetime; its Drop unregisters the advertisement with BlueZ.
        let _handle = adapter.advertise(le_adv).await?;
        crate::log("BEACON: bluer advertise started");
        while !stop.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        crate::log("BEACON: bluer advertise stopped");
        Ok(())
    }

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

    /// Feed every advertised service UUID a device is carrying into the shared ingest (which magic-filters). BlueZ populates `UUIDs` from the advertising data's service-UUID AD fields for non-connectable beacons, so this catches our advertisement without a connection.
    fn ingest_uuids(uuids: &std::collections::HashSet<bluer::Uuid>) {
        for u in uuids {
            super::on_uuid_heard(*u.as_bytes());
        }
    }

    async fn scan(stop: Arc<AtomicBool>) -> bluer::Result<()> {
        let session = bluer::Session::new().await?;
        let adapter = session.default_adapter().await?;
        adapter.set_powered(true).await?;
        // duplicate_data so a beacon that appears after we start still surfaces its UUID property.
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
                        if let Ok(Some(uuids)) = dev.uuids().await {
                            ingest_uuids(&uuids);
                        }
                        let stop_dev = stop.clone();
                        if let Ok(mut dev_events) = dev.events().await {
                            tokio::spawn(async move {
                                while let Some(ev) = dev_events.next().await {
                                    if stop_dev.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    if let bluer::DeviceEvent::PropertyChanged(
                                        bluer::DeviceProperty::Uuids(uuids),
                                    ) = ev
                                    {
                                        ingest_uuids(&uuids);
                                    }
                                }
                            });
                        }
                    }
                }
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
    use super::*;

    pub(super) fn start_announce(uuid: [u8; BEACON_UUID_LEN]) {
        crate::platform::jni_android::beacon_call_bytes("startAdvertise", &uuid);
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

// ── Everything else: stub couriers (macOS/Windows land next via btleplug scan + native advertise; Redox when ferros owns a radio). ──

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod imp {
    use super::*;

    pub(super) fn start_announce(_uuid: [u8; BEACON_UUID_LEN]) {
        crate::log("BEACON: no advertiser on this platform yet — words carry the ceremony");
    }

    pub(super) fn stop_announce() {}

    pub(super) fn start_scan() {
        crate::log("BEACON: no scanner on this platform yet — words carry the ceremony");
    }

    pub(super) fn stop_scan() {}
}
