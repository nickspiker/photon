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

        #[cfg(target_os = "ferros")]
        let peer_cache: std::path::PathBuf = return Err(io::Error::new(io::ErrorKind::Unsupported, "no peer cache on ferros yet: it belongs in the vault"));
        #[cfg(not(any(target_os = "android", target_os = "ferros")))]
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

/// Name this process's device root before anything derives from it (Linux and Windows); call once the instance lock is won, so a lifeline that waited hours for the lock still sees what the instance before it left on disk.
/// A strict priority, never a choice (Nick 2026-11-08: "don't think the user should be choosing"): the papers when this machine's TPM answers, the machine identity when it has none.
/// A vault under the machine identity alone runs as the legacy device and leaves the move owed for the session ([`crate::ui::photon_app::root_conversion`]); vaults under both run on the papers and leave the cleanup owed.
/// A TPM that does not answer on a device whose vault is under its papers stops the start: falling to the machine identity would make it a different device and census its vault away (Android's reinstall trap, 2026-11-04).
/// Logs name the choice, never a root.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn select_desktop_root() {
    use crate::ui::photon_app::root_conversion::{note_owed, Owed};
    use tohu::device::{desktop_select_root, DesktopRoot};
    // The development override is a whole device identity of its own, which never reaches tohu's root.
    #[cfg(feature = "development")]
    if std::env::var_os("PHOTON_FINGERPRINT").is_some_and(|v| !v.is_empty()) {
        return;
    }
    let legacy = match tohu::device::machine_identity() {
        Ok(id) => id,
        Err(e) => {
            crate::logf!("ROOT: the machine identity does not read ({}); the device stays on it and fails where it did before", e);
            desktop_select_root(DesktopRoot::Legacy);
            return;
        }
    };
    // A chip that has no TPM, or a user who may not open it, answers the same every time; anything else is asked again, a few times, before it counts.
    let mut answer = tohu::device::papers_root();
    for _ in 1..(1 << 2) {
        match &answer {
            Err(e) if !matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied) => answer = tohu::device::papers_root(),
            _ => break,
        }
    }
    let papers = match answer {
        Ok(p) => p,
        Err(e) => {
            if crate::storage::other_ring_present(&tohu::device::device_secret_from(&legacy)) {
                let why = format!("ROOT: the TPM did not answer ({}) and this device's vault is under its papers; Photon stops rather than start as a different device and lose that vault", e);
                crate::write_crash_sidecar(&why);
                crate::log(&why);
                crate::flush_log_buffer();
                std::process::exit(1);
            }
            crate::logf!("ROOT: no papers on this machine ({}); the device stays on the machine identity, added by proximity only", e);
            desktop_select_root(DesktopRoot::Legacy);
            return;
        }
    };
    let vault_under = |root: &[u8]| kete::device_vault_present(crate::storage::APP, &tohu::device::device_secret_from(root));
    let (on_papers, on_legacy) = (vault_under(&papers), vault_under(&legacy));
    let root = if on_legacy && !on_papers {
        crate::log("ROOT: this device's vault is under the machine identity — running as that device until the session moves it onto the papers");
        note_owed(Owed::Convert { legacy, new: papers });
        DesktopRoot::Legacy
    } else {
        if on_legacy {
            crate::log("ROOT: vaults under both roots — running on the papers; the old device's departure and vault are finished once the session is live");
            note_owed(Owed::Cleanup { legacy, new: papers });
        } else {
            crate::log("ROOT: running on the papers");
        }
        DesktopRoot::Papers
    };
    if !desktop_select_root(root) {
        crate::log("ROOT: a root was already named in this process before the selection; the device keeps that one");
    }
}

/// No selection on other platforms: Android selects at service start (jni_android.rs), macOS has no papers yet.
#[cfg(not(any(target_os = "android", target_os = "linux", target_os = "windows")))]
pub fn select_desktop_root() {}
