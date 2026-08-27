//! The linked-settings layer (docs/global-vault.md "Settings: per-device maps + link-to-global").
//! Wraps the fgtw::fstate settings codec with photon's resolution + persistence: every setting is per-device with a link bit, born LINKED (the default is always "go with the fleet") — a linked key follows the fleet-global value and setting it writes the global; an unlinked key is set locally on this device.
//! This module owns the cached state, the effective-value resolution, and the vault persistence; the seal-and-push / pull-and-merge transport is photon_app's (riding the same fstate slot as the roster).

use crate::storage::{FlatStorage, StorageError};
use fgtw::fstate::{
    merge_device_settings, merge_global_settings, settings_from_bytes, settings_to_bytes,
    DeviceSetting, DeviceSettings, SettingEntry,
};
use vsf::VsfType;

// ============================================================================ Typed-value readers =======================================================
// Every value is natively typed (v7); the v6 raw-wrapper fallbacks died with the flag-day clean start (fresh vaults never carry one).

/// f5 float.
pub fn as_f32(v: &VsfType) -> Option<f32> {
    match v {
        VsfType::f5(f) => Some(*f),
        _ => None,
    }
}

/// u0 bool (or any unsigned width ≠ 0).
pub fn as_bool(v: &VsfType) -> Option<bool> {
    match v {
        VsfType::u0(b) => Some(*b),
        _ => v.as_u64().map(|n| n != 0),
    }
}

/// Eagle-time oscillations (e6).
pub fn as_osc(v: &VsfType) -> Option<i64> {
    match v {
        VsfType::e(vsf::types::EtType::e6(o)) => Some(*o),
        _ => None,
    }
}

/// UTF-8 text (x, or ascii a).
pub fn as_text(v: &VsfType) -> Option<String> {
    match v {
        VsfType::x(s) | VsfType::a(s) => Some(s.clone()),
        _ => None,
    }
}

/// A 32-byte key (ke).
pub fn as_key32(v: &VsfType) -> Option<[u8; 32]> {
    match v {
        VsfType::ke(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
        _ => None,
    }
}

/// Signed 32 thru the width-agnostic accessor — the key names the semantics, the encoder picks the width; an exact-variant match never fires on parsed data.
pub fn as_i32(v: &VsfType) -> Option<i32> {
    v.as_i64().and_then(|n| i32::try_from(n).ok())
}

/// Unsigned 32 thru the width-agnostic accessor.
pub fn as_u32(v: &VsfType) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// A typed i32 pair (v_i5 of exactly two) — the window-position shape.
pub fn as_i32_pair(v: &VsfType) -> Option<(i32, i32)> {
    match v {
        VsfType::v_i5(p) if p.data.len() == 2 => Some((p.data[0], p.data[1])),
        _ => None,
    }
}

/// A typed u32 pair (v_u5 of exactly two) — the window-size shape.
pub fn as_u32_pair(v: &VsfType) -> Option<(u32, u32)> {
    match v {
        VsfType::v_u5(p) if p.data.len() == 2 => Some((p.data[0], p.data[1])),
        _ => None,
    }
}

/// Opaque application bytes (hR).
pub fn as_bytes(v: &VsfType) -> Option<Vec<u8>> {
    match v {
        VsfType::hR(b) => Some(b.clone()),
        _ => None,
    }
}

/// The cached settings state for this identity, plus which device WE are (the single-writer key for our own map).
#[derive(Debug, Clone)]
pub struct FleetSettings {
    pub global: Vec<SettingEntry>,
    pub devices: Vec<DeviceSettings>,
    pub our_device: [u8; 32],
}

impl FleetSettings {
    pub fn new(our_device: [u8; 32]) -> Self {
        Self {
            global: Vec::new(),
            devices: Vec::new(),
            our_device,
        }
    }

    fn our_entry(&self, key: &str) -> Option<&DeviceSetting> {
        self.devices
            .iter()
            .find(|d| d.device_pubkey == self.our_device)
            .and_then(|d| d.entries.iter().find(|e| e.key == key))
    }

    fn global_entry(&self, key: &str) -> Option<&SettingEntry> {
        self.global.iter().find(|e| e.key == key && !e.tombstone)
    }

    /// Is this key linked on THIS device? Born linked: no device entry = linked.
    pub fn linked(&self, key: &str) -> bool {
        self.our_entry(key).map_or(true, |e| e.linked)
    }

    /// The value this device should act on: an UNLINKED local entry wins; otherwise the fleet-global; otherwise the local entry as a fallback (a linked key whose global hasn't arrived yet).
    pub fn effective(&self, key: &str) -> Option<&VsfType> {
        match self.our_entry(key) {
            Some(e) if !e.linked => Some(&e.value),
            own => self
                .global_entry(key)
                .map(|g| &g.value)
                .or(own.map(|e| &e.value)),
        }
    }

    /// This DEVICE's own value for a key, ignoring the fleet global entirely — `None` if this device has never set one.
    /// For settings that are ergonomics rather than preferences: zoom is tied to the physical monitor in front of you, so inheriting another device's value is always wrong. `effective` deliberately falls back to the global for a born-linked key, which is right for "how do my devices behave" and wrong for "how big is this screen". Reading zoom through `effective` is what let a fresh device adopt a 4K desktop's zoom seconds after launch.
    pub fn device_local(&self, key: &str) -> Option<&VsfType> {
        self.our_entry(key).map(|e| &e.value)
    }

    /// Set a key's value: writes the GLOBAL when linked (propagates to every linked device), our own map when unlinked. Returns true if anything changed (caller persists + pushes).
    pub fn set(&mut self, key: &str, value: VsfType, now: i64) -> bool {
        if self.effective(key) == Some(&value) {
            return false;
        }
        if self.linked(key) {
            self.global.retain(|e| e.key != key);
            self.global.push(SettingEntry {
                key: key.to_string(),
                value,
                updated: now,
                tombstone: false,
            });
            self.global.sort_by(|a, b| a.key.cmp(&b.key));
        } else {
            self.upsert_own(
                key,
                |e| e.value = value.clone(),
                DeviceSetting {
                    key: key.to_string(),
                    value: value.clone(),
                    updated: now,
                    linked: false,
                },
                now,
            );
        }
        true
    }

    /// Flip a key's link on THIS device. Unlinking snapshots the current effective value as the local one (the knob keeps its position, it just stops following); re-linking keeps the local value only as a fallback.
    pub fn set_link(&mut self, key: &str, linked: bool, now: i64) -> bool {
        if self.linked(key) == linked {
            return false;
        }
        let snapshot = self
            .effective(key)
            .cloned()
            .unwrap_or(VsfType::hR(Vec::new()));
        self.upsert_own(
            key,
            |e| e.linked = linked,
            DeviceSetting {
                key: key.to_string(),
                value: snapshot.clone(),
                updated: now,
                linked,
            },
            now,
        );
        true
    }

    fn upsert_own(
        &mut self,
        key: &str,
        mutate: impl FnOnce(&mut DeviceSetting),
        insert: DeviceSetting,
        now: i64,
    ) {
        let our = self.our_device;
        let map = match self.devices.iter_mut().find(|d| d.device_pubkey == our) {
            Some(d) => d,
            None => {
                self.devices.push(DeviceSettings {
                    device_pubkey: our,
                    updated: now,
                    entries: Vec::new(),
                });
                self.devices
                    .sort_by(|a, b| a.device_pubkey.cmp(&b.device_pubkey));
                self.devices
                    .iter_mut()
                    .find(|d| d.device_pubkey == our)
                    .unwrap()
            }
        };
        match map.entries.iter_mut().find(|e| e.key == key) {
            Some(e) => {
                mutate(e);
                e.updated = now;
            }
            None => map.entries.push(insert),
        }
        map.updated = now;
    }

    /// A fleet-wide GROW-ONLY pubkey set read as the union of its per-key entries (`<prefix><hex pubkey>`, value = typed ke).
    /// Per-key because the settings layer is LWW per key: two devices growing ONE blob concurrently each wrote old+own and LWW dropped one — for `fleet.locked` that meant a stolen device stayed trusted on part of the fleet until someone re-locked (the B4 union-merge race). One key per member makes concurrent adds COMMUTE: `merge_global_settings` unions distinct keys by construction, so no write order can lose an entry.
    /// Grow-only holds by convention: per-key entries are never tombstoned by a merge (an unlock is a deliberate typed u0(false), which never parses as a key).
    pub fn pubkey_set_union(&self, prefix: &str) -> Vec<[u8; 32]> {
        let mut out: Vec<[u8; 32]> = Vec::new();
        for e in self
            .global
            .iter()
            .filter(|e| !e.tombstone && e.key.starts_with(prefix))
        {
            if let Some(k) = as_key32(&e.value) {
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }

    /// Fold a pulled remote state in (global LWW + device newest-copy-wins). Returns true if our cached state changed (caller persists + re-applies live values).
    pub fn merge_from(
        &mut self,
        remote_global: Vec<SettingEntry>,
        remote_devices: Vec<DeviceSettings>,
    ) -> bool {
        // Compare STATE, not serialized bytes: the document codec stamps a creation time, so identical state never re-encodes to identical bytes — a byte compare here reported every merge as a change (= a spurious re-push per pull).
        let before = (self.global.clone(), self.devices.clone());
        self.global = merge_global_settings(std::mem::take(&mut self.global), remote_global);
        self.devices = merge_device_settings(std::mem::take(&mut self.devices), remote_devices);
        (&self.global, &self.devices) != (&before.0, &before.1)
    }
}

/// Persist the settings state as one vault entry (the codec's own bytes; the vault layer AEADs them).
pub fn save_fleet_settings(fs: &FleetSettings, storage: &FlatStorage) -> Result<(), StorageError> {
    let bytes = settings_to_bytes(&fs.global, &fs.devices);
    // GROWTH CONFESSION (field 2026-08-28): a single 6.4MB live value was 93% of the Mac's vault, re-put in pairs every few seconds — the librarian queue behind it was the settings-open beachball and 5s waits for 53-byte puts. When this blob is fat, name WHICH key family holds the bytes (prefix before the first '.'), because the blob is opaque at every other layer.
    if bytes.len() > 512 * 1024 {
        let mut fams: std::collections::HashMap<&str, (usize, usize)> = Default::default();
        for e in &fs.global {
            let fam = e.key.split('.').next().unwrap_or("");
            let f = fams.entry(fam).or_default();
            f.0 += 1;
            f.1 += e.value.flatten().len() + e.key.len();
        }
        let mut top: Vec<_> = fams.into_iter().collect();
        top.sort_by_key(|(_, (_, b))| std::cmp::Reverse(*b));
        let summary: Vec<String> = top
            .iter()
            .take(4)
            .map(|(f, (n, b))| format!("{f}×{n}≈{b}B"))
            .collect();
        crate::logf!(
            "SETTINGS: blob {} bytes ({} global keys, {} device sets) — top families: {}",
            bytes.len(),
            fs.global.len(),
            fs.devices.len(),
            summary.join(", ")
        );
    }
    storage.write_addr(
        &crate::storage::vault_key("settings", &storage.vault_seed()),
        &bytes,
    )
}

/// Load the settings state (empty on first run).
pub fn load_fleet_settings(storage: &FlatStorage, our_device: [u8; 32]) -> FleetSettings {
    let mut fs = FleetSettings::new(our_device);
    if let Ok(Some(bytes)) =
        storage.read_addr(&crate::storage::vault_key("settings", &storage.vault_seed()))
    {
        match settings_from_bytes(&bytes) {
            Ok((g, d)) => {
                fs.global = g;
                fs.devices = d;
            }
            Err(e) => crate::logf!("SETTINGS: stored state unreadable ({}) — starting empty", e),
        }
    }
    fs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sibling's device map must NEVER become our `device_local` value.
    ///
    /// This is the "half size a few moments after the contacts show up" bug. `apply_settings_to_ui` re-arms the one-shot zoom restore from `device_local("display.zoom")` on EVERY fleet merge that changed anything, and the fleet poll runs every ~15s. So if a merge can ever make `device_local` return another device's value, the window silently re-zooms to a foreign monitor's ergonomics on a timer -- which is exactly what a phone's zoom doing to a 3360x2100 Mac looks like.
    #[test]
    fn a_siblings_device_map_never_leaks_into_our_device_local() {
        const US: [u8; 32] = [1; 32];
        const SIBLING: [u8; 32] = [2; 32];

        let mut fs = FleetSettings::new(US);
        // We have never set a zoom: device_local must be None, so nothing is restored.
        assert_eq!(
            fs.device_local("display.zoom"),
            None,
            "no zoom of our own to start"
        );

        // A sibling (a phone) pushes ITS zoom. This arrives through the normal fleet pull.
        let sibling_zoom = VsfType::f5(0.5);
        let remote = vec![DeviceSettings {
            device_pubkey: SIBLING,
            updated: 500,
            entries: vec![DeviceSetting {
                key: "display.zoom".to_string(),
                value: sibling_zoom.clone(),
                updated: 500,
                linked: false,
            }],
        }];
        fs.merge_from(Vec::new(), remote);

        // The sibling's map is now in our cached state -- that is correct, we mirror the fleet.
        assert!(
            fs.devices.iter().any(|d| d.device_pubkey == SIBLING),
            "sibling map is cached"
        );
        // But it must NOT be OUR value. If this fails, every fleet poll re-zooms this window.
        assert_eq!(
            fs.device_local("display.zoom"),
            None,
            "a sibling's zoom must not be readable as ours -- this is the half-size regression"
        );

        // Our own zoom, once set, is the only thing device_local returns.
        fs.set_link("display.zoom", false, 600);
        fs.set("display.zoom", VsfType::f5(1.0), 600);
        assert_eq!(
            fs.device_local("display.zoom").and_then(as_f32),
            Some(1.0),
            "our own zoom wins for us"
        );
        // And the sibling's own value is untouched by ours -- the fleet map still carries both.
        let sib = fs
            .devices
            .iter()
            .find(|d| d.device_pubkey == SIBLING)
            .expect("sibling still present");
        assert_eq!(
            sib.entries[0].value, sibling_zoom,
            "we did not overwrite the sibling's ergonomics"
        );
    }

    #[test]
    fn born_linked_set_writes_global_and_unlink_goes_local() {
        let mut fs = FleetSettings::new([7; 32]);
        // Born linked: no entries anywhere, still linked.
        assert!(fs.linked("updates.auto"));
        assert_eq!(fs.effective("updates.auto"), None);
        // A linked set writes the GLOBAL layer.
        assert!(fs.set("updates.auto", VsfType::u0(true), 100));
        assert_eq!(fs.effective("updates.auto"), Some(&VsfType::u0(true)));
        assert_eq!(fs.global.len(), 1);
        assert!(fs.devices.is_empty());
        // Unlink: snapshots the effective value locally, global stops applying.
        assert!(fs.set_link("updates.auto", false, 200));
        assert!(!fs.linked("updates.auto"));
        assert!(fs.set("updates.auto", VsfType::u0(false), 300));
        assert_eq!(fs.effective("updates.auto"), Some(&VsfType::u0(false)));
        assert_eq!(fs.global[0].value, VsfType::u0(true)); // global untouched by the local set
                                                 // Re-link: follows the global again, local kept only as fallback.
        assert!(fs.set_link("updates.auto", true, 400));
        assert_eq!(fs.effective("updates.auto"), Some(&VsfType::u0(true)));
        // No-op set returns false (nothing to persist or push).
        assert!(!fs.set("updates.auto", VsfType::u0(true), 500));
    }

    /// THE B4 LOCKOUT RACE, closed: two devices locking DIFFERENT pubkeys at the SAME instant each write their own per-key entry, and the merge unions distinct keys — no write order can drop a lock. The old one-blob shape lost exactly this race (each side wrote old+own into one LWW key; one lock vanished and a stolen device stayed trusted on part of the fleet).
    #[test]
    fn concurrent_locks_of_different_devices_both_survive_the_merge() {
        const STOLEN_A: [u8; 32] = [0xAA; 32];
        const STOLEN_B: [u8; 32] = [0xBB; 32];

        let mut dev1 = FleetSettings::new([1; 32]);
        let mut dev2 = dev1.clone();
        dev2.our_device = [2; 32];

        // The race: both lock a different device at the SAME stamp, before either sees the other's write.
        dev1.set(
            &format!("fleet.locked.{}", hex::encode(STOLEN_A)),
            VsfType::ke(STOLEN_A.to_vec()),
            500,
        );
        dev2.set(
            &format!("fleet.locked.{}", hex::encode(STOLEN_B)),
            VsfType::ke(STOLEN_B.to_vec()),
            500,
        );

        // Each side pulls the other's state (either order).
        dev1.merge_from(dev2.global.clone(), dev2.devices.clone());
        dev2.merge_from(dev1.global.clone(), dev1.devices.clone());

        for fs in [&dev1, &dev2] {
            let locked = fs.pubkey_set_union("fleet.locked.");
            assert!(
                locked.contains(&STOLEN_A),
                "device A's lock must survive the race"
            );
            assert!(
                locked.contains(&STOLEN_B),
                "device B's lock must survive the race"
            );
            assert_eq!(locked.len(), 2, "union, no duplicates");
        }
        // A malformed value never parses as a pubkey, and an unlock tombstone (u0(false)) never reads as a lock.
        let mut fs = FleetSettings::new([3; 32]);
        fs.set("fleet.locked.deadbeef", VsfType::hR(vec![1, 2, 3]), 100);
        fs.set(
            &format!("fleet.locked.{}", hex::encode([0xDDu8; 32])),
            VsfType::u0(false),
            100,
        );
        assert!(fs.pubkey_set_union("fleet.locked.").is_empty());
    }

    #[test]
    fn empty_value_unlock_clears_the_lock_syncs_and_loses_to_a_newer_relock() {
        const STOLEN: [u8; 32] = [0xCC; 32];
        let key = format!("fleet.locked.{}", hex::encode(STOLEN));
        let mut a = FleetSettings::new([1; 32]);
        let mut b = FleetSettings::new([2; 32]);
        a.set(&key, VsfType::ke(STOLEN.to_vec()), 100);
        b.merge_from(a.global.clone(), a.devices.clone());
        assert!(b
            .pubkey_set_union("fleet.locked.")
            .contains(&STOLEN));
        // The owner's reversal: the typed u0(false) tombstone unlock_fleet_device writes. It never parses as a key, so it drops out of the union locally and syncs the reversal fleet-wide.
        a.set(&key, VsfType::u0(false), 200);
        assert!(a
            .pubkey_set_union("fleet.locked.")
            .is_empty());
        b.merge_from(a.global.clone(), a.devices.clone());
        assert!(
            b.pubkey_set_union("fleet.locked.")
                .is_empty(),
            "the unlock must sync"
        );
        // A later RE-LOCK wins over the tombstone by LWW — unlock is a reversal, not an immunity.
        b.set(&key, VsfType::ke(STOLEN.to_vec()), 300);
        a.merge_from(b.global.clone(), b.devices.clone());
        assert!(a
            .pubkey_set_union("fleet.locked.")
            .contains(&STOLEN));
    }

    #[test]
    fn linked_key_falls_back_to_local_until_global_arrives_and_merge_adopts_remote() {
        let mut fs = FleetSettings::new([7; 32]);
        // A linked key with only a local fallback (e.g. link flipped before any global write).
        fs.set_link("theme", false, 100);
        fs.set("theme", VsfType::x("amber".into()), 100);
        fs.set_link("theme", true, 150);
        assert_eq!(fs.effective("theme"), Some(&VsfType::x("amber".into()))); // fallback: no global yet
                                                                // A remote global arrives via merge — the linked key follows it.
        let remote = vec![SettingEntry {
            key: "theme".into(),
            value: VsfType::x("green".into()),
            updated: 200,
            tombstone: false,
        }];
        assert!(fs.merge_from(remote, Vec::new()));
        assert_eq!(fs.effective("theme"), Some(&VsfType::x("green".into())));
        // Idempotent: merging the same state again changes nothing.
        assert!(!fs.merge_from(fs.global.clone(), fs.devices.clone()));
    }
}
