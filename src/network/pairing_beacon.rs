//! Pairing v2 proximity beacon — the transport seam (docs/pairing-v2.md).
//!
//! One frame, TWO carriers, every platform: the 16 beacon bytes = `keyed_hash(handle_key, device_pubkey ‖ eagle_time)` (see [`fgtw::pair::beacon_id`]) — the whole 16 bytes are the hash, no magic, no nonce. Advertised as a 128-bit BLE **service UUID** where the OS allows it (Apple's CoreBluetooth permits ONLY service UUIDs; Linux/Android too), and as **manufacturer data** under company id 0xFFFF on Windows (whose publisher REFUSES service UUIDs — 0x80070057, the OS owns every AD section except manufacturer data). Same bytes either way; every scanner ingests BOTH carriers, so any platform pair works with no frame fork.
//! `eagle_time` is the new device's PUBLISHED binding-offer stamp, so the beacon is a function of real, signed, published registry state — not invented entropy. It rolls per repost (unlinkable) and is indistinguishable from any random service UUID at rest.
//! The ceremony never learns which courier ran: the new device announces thru [`announce_guard`] (re-emitting via [`reannounce`] on each repost), the sponsor's scanner drops every heard UUID into [`heard`], and the sponsor's matcher recomputes [`fgtw::pair::beacon_id`] for each public-list candidate and intersects — the hash-match IS the filter. Everything between is plumbing.
//! Per-platform couriers: Android advertises AND scans (the `PhotonBeacon` Kotlin bridge); Linux advertises AND scans via bluer (BlueZ over D-Bus); macOS/Windows land next (btleplug scan + native advertise); Redox waits on a ferros radio.

use std::sync::Mutex;
use std::time::Instant;

use fgtw::pair::BEACON_UUID_LEN;

/// Manufacturer-data company id for the Windows-advertised carrier: 0xFFFF is the Bluetooth SIG internal-use/testing id. Scanners match (this id, 16-byte payload) as an alternate spelling of the service-UUID carrier.
pub const BEACON_COMPANY_ID: u16 = 0xFFFF;

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

/// Advertise this device's join beacon for as long as the returned guard lives — tie it to the join ceremony's thread scope so every exit path (bind, cancel, error) stops the radio. `eagle_time` is this device's PUBLISHED binding-offer stamp (`BindRequest::t`) — the beacon is derived entirely from published offer state, so post the offer first, then announce with the stamp it returned.
pub fn announce_guard(handle_proof: &[u8; 32], device_pubkey: &[u8; 32], eagle_time: i64) -> AnnounceGuard {
    announce(handle_proof, device_pubkey, eagle_time);
    AnnounceGuard(())
}

/// Re-emit the beacon with the offer's CURRENT published stamp — called on each request repost so the aired id tracks the `eagle_time` the sponsor is now matching against. Replaces the live advertisement in place; the ceremony's single [`AnnounceGuard`] still owns teardown.
pub fn reannounce(handle_proof: &[u8; 32], device_pubkey: &[u8; 32], eagle_time: i64) {
    announce(handle_proof, device_pubkey, eagle_time);
}

fn announce(handle_proof: &[u8; 32], device_pubkey: &[u8; 32], eagle_time: i64) {
    imp::start_announce(fgtw::pair::beacon_id(handle_proof, device_pubkey, eagle_time));
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

/// Shared ingest for every courier: dedupe + store every advertised service UUID heard while scanning. There is no magic prefix to pre-filter on — the beacon is indistinguishable from any random service UUID by design — so the sponsor's matcher does the selection by recomputing each public-list entry's [`fgtw::pair::beacon_id`] and intersecting. Room noise that never matches is simply never looked up. No per-UUID log (without a magic, every fitness tracker in the room would spam it).
pub fn on_uuid_heard(uuid: [u8; BEACON_UUID_LEN]) {
    if !*SCANNING.lock().unwrap() {
        return; // scan already stopped; late callback
    }
    let mut heard = HEARD.lock().unwrap();
    match heard.iter_mut().find(|b| b.uuid == uuid) {
        Some(b) => b.last_seen = Instant::now(),
        None => heard.push(HeardBeacon { uuid, last_seen: Instant::now() }),
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
                crate::logf!("BEACON: bluer advertise failed: {}", e);
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
        // Peripheral (not Broadcast) + no custom intervals: BlueZ's advertising manager rejects our beacon under Broadcast/with intervals ("Failed to parse advertisement"). A connectable beacon is harmless here — there's no GATT server, so the sponsor reads the advertised service UUID without ever connecting. This is the canonical, universally-accepted bluer advertisement shape.
        let le_adv = bluer::adv::Advertisement {
            advertisement_type: bluer::adv::Type::Peripheral,
            service_uuids,
            discoverable: Some(true),
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
                crate::logf!("BEACON: bluer scan failed: {}", e);
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

    /// The Windows-advertised carrier: the same 16 beacon bytes ride manufacturer data under [`super::BEACON_COMPANY_ID`] (Windows publishers can't emit service UUIDs).
    fn ingest_manufacturer(md: &std::collections::HashMap<u16, Vec<u8>>) {
        if let Some(data) = md.get(&super::BEACON_COMPANY_ID) {
            if data.len() == BEACON_UUID_LEN {
                let mut b = [0u8; BEACON_UUID_LEN];
                b.copy_from_slice(data);
                super::on_uuid_heard(b);
            }
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
                        if let Ok(Some(md)) = dev.manufacturer_data().await {
                            ingest_manufacturer(&md);
                        }
                        let stop_dev = stop.clone();
                        if let Ok(mut dev_events) = dev.events().await {
                            tokio::spawn(async move {
                                while let Some(ev) = dev_events.next().await {
                                    if stop_dev.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    match ev {
                                        bluer::DeviceEvent::PropertyChanged(
                                            bluer::DeviceProperty::Uuids(uuids),
                                        ) => ingest_uuids(&uuids),
                                        bluer::DeviceEvent::PropertyChanged(
                                            bluer::DeviceProperty::ManufacturerData(md),
                                        ) => ingest_manufacturer(&md),
                                        _ => {}
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

// ── macOS + Windows courier: btleplug central (CoreBluetooth / WinRT) scanner. The new-device advertiser is native per-OS (below). ──

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use super::*;
    use btleplug::api::{Central, CentralEvent, Manager as _, ScanFilter};
    use btleplug::platform::Manager;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    static SCAN_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

    // btleplug is central-only, so the new-device advertiser is native per-OS. Windows: WinRT BluetoothLEAdvertisementPublisher (win_adv). macOS: CoreBluetooth CBPeripheralManager via the ObjC shim (macos/photon_ble.m) — the unified service-UUID format is what makes it possible there at all, it's the one payload CoreBluetooth will emit.
    #[cfg(target_os = "macos")]
    extern "C" {
        fn photon_ble_adv_start(bytes: *const u8, len: usize);
        fn photon_ble_adv_stop();
    }

    pub(super) fn start_announce(uuid: [u8; BEACON_UUID_LEN]) {
        #[cfg(target_os = "windows")]
        win_adv::start(uuid);
        #[cfg(target_os = "macos")]
        // SAFETY: the shim copies the bytes into an NSData before returning; `uuid` need not outlive the call.
        unsafe {
            photon_ble_adv_start(uuid.as_ptr(), uuid.len());
        }
    }

    pub(super) fn stop_announce() {
        #[cfg(target_os = "windows")]
        win_adv::stop();
        #[cfg(target_os = "macos")]
        unsafe {
            photon_ble_adv_stop();
        }
    }

    pub(super) fn start_scan() {
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(old) = SCAN_STOP.lock().unwrap().replace(stop.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        crate::network::http::runtime().spawn(async move {
            if let Err(e) = scan(stop).await {
                crate::logf!("BEACON: btleplug scan failed: {}", e);
            }
        });
    }

    pub(super) fn stop_scan() {
        if let Some(stop) = SCAN_STOP.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    async fn scan(stop: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = Manager::new().await?;
        let central =
            manager.adapters().await?.into_iter().next().ok_or("no BLE adapter")?;
        let mut events = central.events().await?;
        // Scan-all: our beacon UUID carries a per-ceremony nonce, so it can't be pre-listed in a ScanFilter — the magic-prefix check in on_uuid_heard does the selection. Foreground scan-all is permitted on macOS/Windows.
        central.start_scan(ScanFilter::default()).await?;
        crate::log("BEACON: btleplug scan started");
        while !stop.load(Ordering::Relaxed) {
            tokio::select! {
                ev = events.next() => {
                    let Some(ev) = ev else { break };
                    match ev {
                        CentralEvent::ServicesAdvertisement { services, .. } => {
                            for u in services {
                                super::on_uuid_heard(*u.as_bytes());
                            }
                        }
                        // The Windows-advertised carrier: same 16 bytes as manufacturer data under the beacon company id.
                        CentralEvent::ManufacturerDataAdvertisement { manufacturer_data, .. } => {
                            for (company, data) in manufacturer_data {
                                if company == super::BEACON_COMPANY_ID && data.len() == BEACON_UUID_LEN {
                                    let mut b = [0u8; BEACON_UUID_LEN];
                                    b.copy_from_slice(&data);
                                    super::on_uuid_heard(b);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
        }
        let _ = central.stop_scan().await;
        crate::log("BEACON: btleplug scan stopped");
        Ok(())
    }

    // ── Windows advertiser: WinRT BluetoothLEAdvertisementPublisher. ──
    // Runs on a dedicated thread that owns a COM apartment (WinRT activation requires one) and holds the publisher alive until stopped — dropping the publisher stops the advertisement, so the thread parks on it rather than returning.
    #[cfg(target_os = "windows")]
    mod win_adv {
        use super::*;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use windows::Devices::Bluetooth::Advertisement::{
            BluetoothLEAdvertisementPublisher, BluetoothLEManufacturerData,
        };
        use windows::Storage::Streams::DataWriter;
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

        static ADV_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

        pub(in crate::network::pairing_beacon) fn start(uuid: [u8; BEACON_UUID_LEN]) {
            let stop = Arc::new(AtomicBool::new(false));
            if let Some(old) = ADV_STOP.lock().unwrap().replace(stop.clone()) {
                old.store(true, Ordering::Relaxed);
            }
            std::thread::spawn(move || {
                if let Err(e) = run(uuid, stop) {
                    crate::logf!("BEACON: WinRT advertise failed: {}", e);
                }
            });
        }

        pub(in crate::network::pairing_beacon) fn stop() {
            if let Some(stop) = ADV_STOP.lock().unwrap().take() {
                stop.store(true, Ordering::Relaxed);
            }
        }

        fn run(uuid: [u8; BEACON_UUID_LEN], stop: Arc<AtomicBool>) -> windows::core::Result<()> {
            // MTA: no message pump needed while the thread parks holding the publisher.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let result = (|| -> windows::core::Result<()> {
                let publisher = BluetoothLEAdvertisementPublisher::new()?;
                // Windows REFUSES service UUIDs from app publishers (ServiceUuids().Append → Start = 0x80070057 E_INVALIDARG, seen in the field): the OS owns every AD section except MANUFACTURER DATA. So this one courier carries the same 16 beacon bytes as manufacturer data under company id 0xFFFF (the Bluetooth SIG internal-use id) — every scanner matches BOTH carriers (UUID from mac/Linux/Android advertisers, this from Windows).
                let md = BluetoothLEManufacturerData::new()?;
                md.SetCompanyId(super::super::BEACON_COMPANY_ID)?;
                let writer = DataWriter::new()?;
                writer.WriteBytes(&uuid)?;
                md.SetData(&writer.DetachBuffer()?)?;
                publisher.Advertisement()?.ManufacturerData()?.Append(&md)?;
                publisher.Start()?;
                crate::log("BEACON: WinRT advertise started");
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                publisher.Stop()?;
                crate::log("BEACON: WinRT advertise stopped");
                Ok(())
            })();
            unsafe {
                CoUninitialize();
            }
            result
        }
    }
}

// ── Everything else: stub couriers (Redox when ferros owns a radio). ──

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "windows"
)))]
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
