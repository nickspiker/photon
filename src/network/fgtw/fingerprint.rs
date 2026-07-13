//! Device fingerprint and key derivation
//!
//! Keys are NEVER stored on disk - always derived deterministically from platform-specific fingerprint ORACLES (not fixed IDs - oracles are harder to steal):
//! - Linux: /etc/machine-id (generated at install, unique per system)
//! - Windows: Registry MachineGuid (generated at install)
//! - macOS: IOPlatformUUID (hardware-burned, survives reinstalls)
//! - Android: Device fingerprint (passed via JNI - ANDROID_ID is an oracle)

use std::io;
use std::path::PathBuf;

// The device keypair now lives in the `fgtw` crate (`fgtw::keys::Keypair`) so every TOKEN app + the FGTW worker share one identity primitive. Re-exported here so photon's `crate::network::fgtw::Keypair` call sites are unchanged.
pub use fgtw::keys::Keypair;

/// FGTW storage paths (peer cache only - NO KEY STORAGE)
pub struct FgtwPaths {
    /// Peer cache: per-user peer list (~/.cache/fgtw/peers.vsf)
    pub peer_cache: PathBuf,
}

impl FgtwPaths {
    /// Get FGTW storage paths with platform-appropriate defaults
    pub fn new() -> io::Result<Self> {
        // Android: use the data dir set at init time
        #[cfg(target_os = "android")]
        let peer_cache = {
            let base_dir = crate::avatar::get_android_data_dir().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Android data dir not set")
            })?;
            base_dir.join("peers.vsf")
        };

        #[cfg(not(target_os = "android"))]
        let peer_cache = dirs::cache_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No cache dir"))?
            .join("fgtw")
            .join("peers.vsf");

        Ok(Self { peer_cache })
    }
}

impl Default for FgtwPaths {
    fn default() -> Self {
        Self::new().expect("Failed to determine FGTW paths")
    }
}

// Fingerprint → keypair derivation now lives in the `fgtw` crate too (same crate as `Keypair`), so every TOKEN app derives the same device identity from the same oracle bytes. Re-exported for unchanged call sites.
pub use fgtw::keys::derive_device_keypair;

/// Machine fingerprint for deterministic key derivation — delegates to tohu's per-platform device oracle so the read logic lives once in the shared crate, not duplicated across every stack app. Desktop only: on Android the keypair is derived from the JNI-fetched oracle (today pushed via `NetworkContext`; `tohu::device` owns the in-Rust fetch once it's device-verified). Source per platform: Linux `/etc/machine-id` · Windows `MachineGuid` · macOS `IOPlatformUUID` · other `/etc/hostid`→`/etc/hostname`.
#[cfg(not(target_os = "android"))]
pub fn get_machine_fingerprint() -> io::Result<Vec<u8>> {
    // Dev-only override: PHOTON_FINGERPRINT forces a distinct device identity, so a second instance (own PHOTON_DATA_DIR) is a genuinely different device for two-party / device-ADD testing on one machine. Compiled out of release.
    #[cfg(feature = "development")]
    if let Ok(fp) = std::env::var("PHOTON_FINGERPRINT") {
        if !fp.is_empty() {
            return Ok(fp.into_bytes());
        }
    }
    tohu::device::machine_fingerprint()
}
