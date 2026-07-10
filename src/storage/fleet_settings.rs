//! The linked-settings layer (docs/global-vault.md "Settings: per-device maps + link-to-global").
//! Wraps the fgtw::fstate settings codec with photon's resolution + persistence: every setting is per-device with a link bit, born LINKED (the default is always "go with the fleet") — a linked key follows the fleet-global value and setting it writes the global; an unlinked key is set locally on this device.
//! This module owns the cached state, the effective-value resolution, and the vault persistence; the seal-and-push / pull-and-merge transport is photon_app's (riding the same fstate slot as the roster).

use crate::storage::{FlatStorage, StorageError};
use fgtw::fstate::{
    merge_device_settings, merge_global_settings, settings_from_bytes, settings_to_bytes,
    DeviceSetting, DeviceSettings, SettingEntry,
};

/// The cached settings state for this identity, plus which device WE are (the single-writer key for our own map).
#[derive(Debug, Clone)]
pub struct FleetSettings {
    pub global: Vec<SettingEntry>,
    pub devices: Vec<DeviceSettings>,
    pub our_device: [u8; 32],
}

impl FleetSettings {
    pub fn new(our_device: [u8; 32]) -> Self {
        Self { global: Vec::new(), devices: Vec::new(), our_device }
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
    pub fn effective(&self, key: &str) -> Option<&[u8]> {
        match self.our_entry(key) {
            Some(e) if !e.linked => Some(&e.value),
            own => self.global_entry(key).map(|g| g.value.as_slice()).or(own.map(|e| e.value.as_slice())),
        }
    }

    /// Set a key's value: writes the GLOBAL when linked (propagates to every linked device), our own map when unlinked. Returns true if anything changed (caller persists + pushes).
    pub fn set(&mut self, key: &str, value: Vec<u8>, now: i64) -> bool {
        if self.effective(key) == Some(value.as_slice()) {
            return false;
        }
        if self.linked(key) {
            self.global.retain(|e| e.key != key);
            self.global.push(SettingEntry { key: key.to_string(), value, updated: now, tombstone: false });
            self.global.sort_by(|a, b| a.key.cmp(&b.key));
        } else {
            self.upsert_own(key, |e| e.value = value.clone(), DeviceSetting { key: key.to_string(), value: value.clone(), updated: now, linked: false }, now);
        }
        true
    }

    /// Flip a key's link on THIS device. Unlinking snapshots the current effective value as the local one (the knob keeps its position, it just stops following); re-linking keeps the local value only as a fallback.
    pub fn set_link(&mut self, key: &str, linked: bool, now: i64) -> bool {
        if self.linked(key) == linked {
            return false;
        }
        let snapshot = self.effective(key).map(|v| v.to_vec()).unwrap_or_default();
        self.upsert_own(
            key,
            |e| e.linked = linked,
            DeviceSetting { key: key.to_string(), value: snapshot.clone(), updated: now, linked },
            now,
        );
        true
    }

    fn upsert_own(&mut self, key: &str, mutate: impl FnOnce(&mut DeviceSetting), insert: DeviceSetting, now: i64) {
        let our = self.our_device;
        let map = match self.devices.iter_mut().find(|d| d.device_pubkey == our) {
            Some(d) => d,
            None => {
                self.devices.push(DeviceSettings { device_pubkey: our, updated: now, entries: Vec::new() });
                self.devices.sort_by(|a, b| a.device_pubkey.cmp(&b.device_pubkey));
                self.devices.iter_mut().find(|d| d.device_pubkey == our).unwrap()
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

    /// Fold a pulled remote state in (global LWW + device newest-copy-wins). Returns true if our cached state changed (caller persists + re-applies live values).
    pub fn merge_from(&mut self, remote_global: Vec<SettingEntry>, remote_devices: Vec<DeviceSettings>) -> bool {
        let before = settings_to_bytes(&self.global, &self.devices);
        self.global = merge_global_settings(std::mem::take(&mut self.global), remote_global);
        self.devices = merge_device_settings(std::mem::take(&mut self.devices), remote_devices);
        settings_to_bytes(&self.global, &self.devices) != before
    }
}

/// Persist the settings state as one vault entry (the codec's own bytes; the vault layer AEADs them).
pub fn save_fleet_settings(fs: &FleetSettings, storage: &FlatStorage) -> Result<(), StorageError> {
    storage.write_addr(
        &crate::storage::vault_key("settings", storage.vault_seed()),
        &settings_to_bytes(&fs.global, &fs.devices),
    )
}

/// Load the settings state (empty on first run).
pub fn load_fleet_settings(storage: &FlatStorage, our_device: [u8; 32]) -> FleetSettings {
    let mut fs = FleetSettings::new(our_device);
    if let Ok(Some(bytes)) = storage.read_addr(&crate::storage::vault_key("settings", storage.vault_seed())) {
        match settings_from_bytes(&bytes) {
            Ok((g, d)) => {
                fs.global = g;
                fs.devices = d;
            }
            Err(e) => crate::log(&format!("SETTINGS: stored state unreadable ({e}) — starting empty")),
        }
    }
    fs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn born_linked_set_writes_global_and_unlink_goes_local() {
        let mut fs = FleetSettings::new([7; 32]);
        // Born linked: no entries anywhere, still linked.
        assert!(fs.linked("updates.auto"));
        assert_eq!(fs.effective("updates.auto"), None);
        // A linked set writes the GLOBAL layer.
        assert!(fs.set("updates.auto", vec![1], 100));
        assert_eq!(fs.effective("updates.auto"), Some(&[1u8][..]));
        assert_eq!(fs.global.len(), 1);
        assert!(fs.devices.is_empty());
        // Unlink: snapshots the effective value locally, global stops applying.
        assert!(fs.set_link("updates.auto", false, 200));
        assert!(!fs.linked("updates.auto"));
        assert!(fs.set("updates.auto", vec![0], 300));
        assert_eq!(fs.effective("updates.auto"), Some(&[0u8][..]));
        assert_eq!(fs.global[0].value, vec![1]); // global untouched by the local set
        // Re-link: follows the global again, local kept only as fallback.
        assert!(fs.set_link("updates.auto", true, 400));
        assert_eq!(fs.effective("updates.auto"), Some(&[1u8][..]));
        // No-op set returns false (nothing to persist or push).
        assert!(!fs.set("updates.auto", vec![1], 500));
    }

    #[test]
    fn linked_key_falls_back_to_local_until_global_arrives_and_merge_adopts_remote() {
        let mut fs = FleetSettings::new([7; 32]);
        // A linked key with only a local fallback (e.g. link flipped before any global write).
        fs.set_link("theme", false, 100);
        fs.set("theme", b"amber".to_vec(), 100);
        fs.set_link("theme", true, 150);
        assert_eq!(fs.effective("theme"), Some(&b"amber"[..])); // fallback: no global yet
        // A remote global arrives via merge — the linked key follows it.
        let remote = vec![SettingEntry { key: "theme".into(), value: b"green".to_vec(), updated: 200, tombstone: false }];
        assert!(fs.merge_from(remote, Vec::new()));
        assert_eq!(fs.effective("theme"), Some(&b"green"[..]));
        // Idempotent: merging the same state again changes nothing.
        assert!(!fs.merge_from(fs.global.clone(), fs.devices.clone()));
    }
}
