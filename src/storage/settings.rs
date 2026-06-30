//! User-adjustable app settings, persisted as a plain (unencrypted) VSF file at `photon_config_dir()/settings.vsf`. Settings are non-secret operational knobs (not identity or conversation data), so they live in the config dir, NOT the encrypted vault.
//!
//! Today the only knobs are the diagnostic-log hex elision lengths (`hex_head` / `hex_tail`): how many head/tail bytes of a large binary VSF field the inspector prints before eliding the middle.
//! The defaults keep whole-session logs readable instead of dumping kilobytes of hex per packet.
//!
//! Resolution order (highest priority first):
//!   1. `VSF_HEX_HEAD` / `VSF_HEX_TAIL` environment variables (quick per-run override; read by vsf)
//!   2. `settings.vsf` on disk (persisted, adjustable)
//!   3. Built-in defaults (and the file is created with them on first run, so there's always something to edit)
//!
//! The env override is handled inside vsf's `hex_elision()`; here we only push the file/default values via `set_hex_elision`, and vsf's OnceLock means the env var still wins if set.

use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::VsfType;

/// Default head/tail bytes shown before eliding a large binary field in diagnostic logs.
/// 32 is enough that two distinct payloads never look alike (head+tail fingerprint) while staying roughly one line each. Mirrors vsf's own built-in default.
const HEX_HEAD_DEFAULT: usize = 32;
const HEX_TAIL_DEFAULT: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Bytes shown at the head of a large binary field in logs before elision.
    pub hex_head: usize,
    /// Bytes shown at the tail of a large binary field in logs before elision.
    pub hex_tail: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hex_head: HEX_HEAD_DEFAULT,
            hex_tail: HEX_TAIL_DEFAULT,
        }
    }
}

fn settings_schema() -> SectionSchema {
    SectionSchema::new("settings")
        .field("hex_head", TypeConstraint::AnyUnsigned)
        .field("hex_tail", TypeConstraint::AnyUnsigned)
}

fn settings_path() -> Option<std::path::PathBuf> {
    crate::storage::photon_config_dir()
        .ok()
        .map(|d| d.join("settings.vsf"))
}

impl Settings {
    /// Serialize to a VSF document (one `settings` section with the knobs as inline fields).
    /// Lengths are stored as u3 (a single byte), so they're clamped to 255 — far beyond any useful head/tail for log readability, and it keeps the file one byte per knob.
    fn encode(&self) -> Result<Vec<u8>, String> {
        let head = self.hex_head.min(255) as u8;
        let tail = self.hex_tail.min(255) as u8;
        settings_schema()
            .build()
            .append_multi("hex_head", vec![VsfType::u3(head)])
            .map_err(|e| e.to_string())?
            .append_multi("hex_tail", vec![VsfType::u3(tail)])
            .map_err(|e| e.to_string())?
            .encode()
            .map_err(|e| e.to_string())
    }

    /// Parse from a VSF document, falling back to defaults for any missing/unreadable field.
    fn decode(bytes: &[u8]) -> Self {
        let mut s = Settings::default();
        if let Ok(builder) = SectionBuilder::parse(settings_schema(), bytes) {
            // Read each knob by its field name; missing/unreadable → keep the default.
            let read = |name: &str| {
                builder
                    .get_fields(name)
                    .first()
                    .and_then(|f| f.values.first())
                    .and_then(|v| v.as_usize())
            };
            if let Some(v) = read("hex_head") {
                s.hex_head = v;
            }
            if let Some(v) = read("hex_tail") {
                s.hex_tail = v;
            }
        }
        s
    }

    /// Load settings from disk, creating `settings.vsf` with defaults if it doesn't exist yet (so there's always a file to hand-edit). Any I/O or parse failure falls back to defaults — a bad settings file must never stop the app from launching.
    pub fn load_or_create() -> Self {
        let Some(path) = settings_path() else {
            return Settings::default();
        };

        // Read quietly (std::fs, not the error-logging read_file) — a missing file on first run is expected, not an error worth a log line.
        match std::fs::read(&path) {
            Ok(bytes) => Settings::decode(&bytes),
            Err(_) => {
                // First run (or unreadable): write defaults so the file exists for editing.
                let defaults = Settings::default();
                if let Ok(bytes) = defaults.encode() {
                    let _ = crate::storage::write_file(&path, &bytes, "settings");
                }
                defaults
            }
        }
    }

    /// No-op: vsf removed the runtime `set_hex_elision` API; hex elision is now a compile-time constant in vsf's inspect module. Settings are still persisted to disk for when/if vsf adds the runtime API back.
    pub fn apply(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let s = Settings { hex_head: 48, hex_tail: 8 };
        let bytes = s.encode().expect("encode");
        let back = Settings::decode(&bytes);
        assert_eq!(back.hex_head, 48);
        assert_eq!(back.hex_tail, 8);
    }

    #[test]
    fn decode_garbage_falls_back_to_defaults() {
        let d = Settings::decode(b"not a vsf doc");
        assert_eq!(d.hex_head, HEX_HEAD_DEFAULT);
        assert_eq!(d.hex_tail, HEX_TAIL_DEFAULT);
    }
}
