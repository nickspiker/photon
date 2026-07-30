pub mod cloud;
pub mod contacts;
pub mod device_binding;
pub mod fleet_settings;
pub mod friendship;
pub mod settings;

// The storage adapter (was `flat.rs`) now lives in the shared `kete` crate. Re-export its surface so existing call sites — `crate::storage::FlatStorage`, `StorageError`, `encrypt_bytes`/`decrypt_bytes` (used by cloud.rs) — keep resolving unchanged.
pub use kete::{decrypt_bytes, encrypt_bytes, App, FlatStorage, StorageError};

/// Photon's app namespace for kete. `id`/`dir` reproduce the original baked-in `"photon"` / `"Photon"` constants exactly, so every existing vault's filename and KDF contexts are unchanged.
pub const APP: kete::App<'static> = kete::App {
    id: "photon",
    dir: "Photon",
};

#[cfg(target_os = "android")]
pub use kete::{android_vault_dirs, set_android_vault_dirs};

/// The canonical vault address for a logical entry: `blake3_kdf("photon.storage.entry.v0", domain || scope)`.
///
/// `domain` is a plain English word naming *what kind* of entry this is ("avatar", "settings", "state", "chains", ...). `scope` is the 32-byte identity that the entry is *about*: our own vault seed for self/global entries, a peer's identity seed for per-peer entries, or a `friendship_id` for per-conversation entries. The vault file is already one-per-handle, so the address never needs to encode *whose vault* it is — only what the entry is and whom it concerns.
///
/// This replaces the old file-tree key strings (`contacts/{hex8}/state`, base64 avatar filenames). Nothing here is ever text-encoded: the 32-byte scope goes straight into the hash, and the result goes straight to `FlatStorage::{read,write,delete}_addr`. The matching KDF context is kete's own entry context, so these addresses share the app's one namespace.
pub fn vault_key(domain: &str, scope: &[u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(domain.len() + 32);
    input.extend_from_slice(domain.as_bytes());
    input.extend_from_slice(scope);
    blake3::derive_key(&format!("{}.storage.entry.v0", APP.id), &input)
}

/// Returns ~/.config/photon/ (or Android equivalent). All Photon files live here.
pub fn photon_config_dir() -> Result<std::path::PathBuf, std::io::Error> {
    #[cfg(target_os = "android")]
    {
        use crate::ui::avatar::get_android_data_dir;
        get_android_data_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "Android data dir not set")
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        // Dev-only override: PHOTON_DATA_DIR points a whole instance (vault + log + lock) at a separate dir, so a second instance can run isolated for two-party testing (pair with PHOTON_FINGERPRINT for a distinct device identity). Compiled out of release so production has no escape hatch from the single-instance lock.
        #[cfg(feature = "development")]
        if let Ok(custom) = std::env::var("PHOTON_DATA_DIR") {
            if !custom.is_empty() {
                return Ok(std::path::PathBuf::from(custom));
            }
        }
        dirs::config_dir().map(|p| p.join("photon")).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "config dir not found")
        })
    }
}

/// The attachment blob directory: `<config>/blobs/`. Blobs live OUTSIDE the vault deliberately — the dual-ring mirror doubles every vault write and a multi-MB value triggers a vault-grow fsync that freezes the UI (the same phenomenon that made clutch keypairs memory-only). Each blob is one sealed file, content-addressed by the PLAINTEXT blake3.
pub fn blob_dir() -> Result<std::path::PathBuf, std::io::Error> {
    let dir = photon_config_dir()?.join("blobs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Per-device blob seal key. Derived from the identity seed — blobs never leave this device as FILES (the wire carries its own seal under history/fleet keys), so the at-rest key needs no cross-device agreement; identity-seed derivation just means a restored session can still open its old blobs.
pub fn blob_seal_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(&format!("{}.blob.seal.v0", APP.id), identity_seed)
}

/// Store an attachment blob: seal plaintext under the device blob key, write `<blobs>/<hash-hex>.bin`. Idempotent — an existing file is left alone (content-addressed, same hash = same bytes).
pub fn blob_store(identity_seed: &[u8; 32], content_hash: &[u8; 32], plaintext: &[u8]) -> Result<(), String> {
    let dir = blob_dir().map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.bin", hex::encode(content_hash)));
    if path.exists() {
        return Ok(());
    }
    let sealed = kete::encrypt_bytes(plaintext, &blob_seal_key(identity_seed))?;
    std::fs::write(&path, &sealed).map_err(|e| e.to_string())
}

/// Load + open an attachment blob; verifies the content hash after decrypt. None = not held locally.
pub fn blob_load(identity_seed: &[u8; 32], content_hash: &[u8; 32]) -> Option<Vec<u8>> {
    let dir = blob_dir().ok()?;
    let sealed = std::fs::read(dir.join(format!("{}.bin", hex::encode(content_hash)))).ok()?;
    let plain = kete::decrypt_bytes(&sealed, &blob_seal_key(identity_seed)).ok()?;
    if blake3::hash(&plain).as_bytes() != content_hash {
        crate::log("blob: content hash mismatch on load — corrupt blob file dropped");
        return None;
    }
    Some(plain)
}

/// Whether the blob for `content_hash` is held locally (no decrypt — presence only).
pub fn blob_present(content_hash: &[u8; 32]) -> bool {
    blob_dir().map(|d| d.join(format!("{}.bin", hex::encode(content_hash))).exists()).unwrap_or(false)
}

/// Delete a blob file (attachment tombstone follow-through — blobs CAN truly shred; only row content is braid-bound).
pub fn blob_delete(content_hash: &[u8; 32]) {
    if let Ok(dir) = blob_dir() {
        let _ = std::fs::remove_file(dir.join(format!("{}.bin", hex::encode(content_hash))));
    }
}

/// Holds the single-instance lock for the whole process; dropping it (or process exit/crash) releases it.
/// Unix-only: this is the `flock`-backed variant. Non-unix desktops (Windows) use the socket-backed `InstanceLock` defined below, and Android is single-instance by construction so neither is compiled there.
#[cfg(all(unix, not(target_os = "android")))]
pub struct InstanceLock {
    _file: std::fs::File,
}

/// Single-instance guard, keyed to the data dir: two instances on the SAME dir would race the vault and corrupt the log (the trim is read-truncate-rewrite), so the second must not start.
/// An advisory exclusive `flock` on `<data_dir>/photon.lock` — exact-keyed (no port hashing/collision, no interference from other apps), and the kernel releases it when the holding process dies, so a crash leaves no stale lock.
/// Returns the guard to keep alive for the whole process, or `None` if another instance already holds this dir. (Non-unix desktops fall back to a localhost socket; Android is single-instance by construction so this isn't compiled there.)
#[cfg(all(unix, not(target_os = "android")))]
pub fn acquire_single_instance(data_dir: &std::path::Path) -> Option<InstanceLock> {
    use std::os::unix::io::AsRawFd;
    let _ = std::fs::create_dir_all(data_dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(data_dir.join("photon.lock"))
        .ok()?;
    // LOCK_EX | LOCK_NB: take it now or fail immediately if another live process holds it.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (rc == 0).then_some(InstanceLock { _file: file })
}

/// Non-unix fallback: a localhost-only socket on a dir-derived port (advisory file locking varies on Windows).
#[cfg(all(not(unix), not(target_os = "android")))]
pub struct InstanceLock {
    _socket: std::net::TcpListener,
}

#[cfg(all(not(unix), not(target_os = "android")))]
impl InstanceLock {
    /// A clone of the lock socket for the second-launch control channel — the listener already exists and is dir-keyed, so it doubles as the handoff endpoint (see `platform::control`).
    pub fn control_listener(&self) -> Option<std::net::TcpListener> {
        self._socket.try_clone().ok()
    }
}
#[cfg(all(not(unix), not(target_os = "android")))]
pub fn acquire_single_instance(data_dir: &std::path::Path) -> Option<InstanceLock> {
    let h = blake3::hash(data_dir.to_string_lossy().as_bytes());
    let port = 20000 + (u16::from_le_bytes([h.as_bytes()[0], h.as_bytes()[1]]) % 20000);
    std::net::TcpListener::bind(("127.0.0.1", port))
        .ok()
        .map(|s| InstanceLock { _socket: s })
}

// ============================================================================ Unified Storage I/O ============================================================================

use std::fs;
use std::path::Path;

// The shared ChaCha20-Poly1305 (`encrypt_bytes`/`decrypt_bytes`) moved to the `kete` crate and is re-exported above; cloud.rs and FlatStorage use it there.

/// Unified disk write: all storage writes go thru this function. Every write is read-back-verified before returning success — if the bytes on disk don't match the bytes we asked to write, the call returns an error and the caller treats that as a hard failure. No "best effort" path; silent corruption is forbidden, and the cost of a `fs::read` per write is cheap against the cost of discovering on next launch that a contact's messages didn't actually persist.
///
/// - Ensures parent directory exists
/// - Writes to a fresh-random-named sibling first, then atomically renames into place
/// - Calls fsync to ensure data reaches disk (critical for crash safety)
/// - Reads back the file and compares byte-for-byte against the data we asked to write
///
/// The pre-rename file uses a random base64url name (not a `.tmp` extension) so in-flight writes are indistinguishable in shape from finished files — `~/.config/photon/` stays FAF (flat as fuck), no metadata leak about which file was being written when a crash happened.
pub fn write_file(path: &Path, data: &[u8], label: &str) -> Result<(), std::io::Error> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            crate::logf!("STORAGE: Failed to create dir for {}: {}", label, e);
            return Err(e);
        }
    }

    // Fresh random sibling — looks like any other opaque file on disk. 24 random bytes → 32-char base64url, matching the filename-shape FlatStorage already uses for everything else.
    let tmp_path = {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut rand_bytes = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut rand_bytes);
        let rand_name = URL_SAFE_NO_PAD.encode(rand_bytes);
        parent.join(rand_name)
    };

    if let Err(e) = fs::write(&tmp_path, data) {
        let _ = fs::remove_file(&tmp_path);
        crate::logf!("STORAGE: Failed to write {}: {}", label, e);
        return Err(e);
    }

    // fsync the temp file before rename so the renamed inode points at durable bytes.
    if let Ok(f) = fs::File::open(&tmp_path) {
        let _ = f.sync_all();
    }
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        crate::logf!("STORAGE: Failed to rename {}: {}", label, e);
        return Err(e);
    }

    // Read-back verify: every write, no exceptions. If the bytes on disk don't match what we sent, fail loudly — silent persistence corruption is the worst failure mode for a personal-data store.
    match fs::read(path) {
        Ok(readback) if readback.len() == data.len() && readback == data => Ok(()),
        Ok(readback) => {
            crate::logf!("STORAGE: Write verification failed for {} (wrote {} bytes, read back {} bytes)", label, data.len(), readback.len());
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "write verification failed: data mismatch",
            ))
        }
        Err(e) => {
            crate::logf!("STORAGE: Write verification read-back failed for {}: {}", label, e);
            Err(e)
        }
    }
}

/// Unified disk read: all storage reads go thru this function.
///
/// Logs a contextual error message on failure and returns the io::Error.
pub fn read_file(path: &Path, label: &str) -> Result<Vec<u8>, std::io::Error> {
    fs::read(path).map_err(|e| {
        crate::logf!("STORAGE: Failed to read {}: {}", label, e);
        e
    })
}
