pub mod cloud;
pub mod contacts;
pub mod flat;
pub mod friendship;

pub use flat::{FlatStorage, StorageError};

/// Returns ~/.config/photon/ (or Android equivalent). All Photon files live here.
pub fn photon_config_dir() -> Result<std::path::PathBuf, std::io::Error> {
    #[cfg(target_os = "android")]
    {
        use crate::ui::avatar::get_android_data_dir;
        get_android_data_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Android data dir not set"))
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::config_dir()
            .map(|p| p.join("photon"))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "config dir not found"))
    }
}

// ============================================================================
// Unified Storage I/O ============================================================================

use std::fs;
use std::path::Path;

/// Controls error handling behavior for disk writes.
pub enum WritePolicy {
    /// Write MUST succeed or caller gets error back. Used for chain-critical paths (friendship chain saves before network send/ACK).
    MustSucceed,
    /// Write failure is logged but Ok(()) returned. Used for best-effort saves (avatars, UI message persistence, contact blobs, etc.).
    BestEffort,
}

/// Unified disk write: all storage writes go through this function.
///
/// - Ensures parent directory exists
/// - Writes to a temp file first, then atomically renames
/// - Calls fsync to ensure data reaches disk (critical for crash safety)
/// - Applies WritePolicy: MustSucceed returns errors, BestEffort logs and swallows them
pub fn write_file(
    path: &Path,
    data: &[u8],
    label: &str,
    policy: WritePolicy,
) -> Result<(), std::io::Error> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return handle_write_error(e, label, &policy);
        }
    }

    // Write to temp file first, then rename (atomic on most OS)
    let tmp_path = path.with_extension("tmp");

    match fs::write(&tmp_path, data) {
        Ok(()) => {
            // fsync the temp file before rename
            if let Ok(f) = fs::File::open(&tmp_path) {
                let _ = f.sync_all();
            }
            // Atomic rename
            if let Err(e) = fs::rename(&tmp_path, path) {
                let _ = fs::remove_file(&tmp_path);
                return handle_write_error(e, label, &policy);
            }

            // Read-back verification for critical writes
            if matches!(policy, WritePolicy::MustSucceed) {
                match fs::read(path) {
                    Ok(readback) if readback.len() == data.len() && readback == data => {}
                    Ok(readback) => {
                        crate::log(&format!(
                            "STORAGE CRITICAL: Write verification failed for {} (wrote {} bytes, read back {} bytes)",
                            label, data.len(), readback.len()
                        ));
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "write verification failed: data mismatch",
                        ));
                    }
                    Err(e) => {
                        crate::log(&format!(
                            "STORAGE CRITICAL: Write verification read-back failed for {}: {}",
                            label, e
                        ));
                        return Err(e);
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            handle_write_error(e, label, &policy)
        }
    }
}

/// Unified disk read: all storage reads go through this function.
///
/// Logs a contextual error message on failure and returns the io::Error.
pub fn read_file(path: &Path, label: &str) -> Result<Vec<u8>, std::io::Error> {
    fs::read(path).map_err(|e| {
        crate::log(&format!("STORAGE: Failed to read {}: {}", label, e));
        e
    })
}

/// Handle a write error according to the policy.
fn handle_write_error(
    e: std::io::Error,
    label: &str,
    policy: &WritePolicy,
) -> Result<(), std::io::Error> {
    match policy {
        WritePolicy::MustSucceed => {
            crate::log(&format!(
                "STORAGE CRITICAL: Failed to write {}: {}",
                label, e
            ));
            Err(e)
        }
        WritePolicy::BestEffort => {
            crate::log(&format!(
                "STORAGE: Failed to write {} (non-critical): {}",
                label, e
            ));
            Ok(())
        }
    }
}
