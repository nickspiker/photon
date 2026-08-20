pub mod cloud;
pub mod contacts;
pub mod device_binding;
pub mod fanout_pairs;
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

/// THE vault open — every runtime site (attest worker, launch, resume) comes thru here and nowhere else.
///
/// Opens the ONE device vault (named from the device secret alone, exists from first launch), installs the session's vault seed as its identity (idempotent; a different identity is refused — one identity per device), then runs the in-place legacy migration exactly once. The shared registry underneath means concurrent callers (resume path + attest worker) receive the SAME engine — a second independent engine racing the live one is the corruption class that bricked a field vault ("seal verification failed" on every subsequent open).
pub fn open_session_vault(
    vault_seed: [u8; 32],
    device_secret: [u8; 32],
) -> Result<std::sync::Arc<FlatStorage>, StorageError> {
    let vault = FlatStorage::open_device_shared(APP, device_secret)?;
    vault.set_identity(vault_seed)?;
    migrate_legacy_vault(&vault, &vault_seed, &device_secret);
    Ok(vault)
}

/// In-place migration from the per-identity vault era: every entry of the legacy vault is raw-copied (stored bytes verbatim, same addresses — the identity-scope key derivation is unchanged, so ciphertexts decrypt identically) into the device vault, then the legacy rings are renamed to `.legacy` backups — kept, not deleted, as missed-domain insurance until a later version reaps them. Marker-gated in the device scope, single-flight, idempotent; every row of history — including the first message ever sent over FGTW — survives.
fn migrate_legacy_vault(vault: &FlatStorage, vault_seed: &[u8; 32], device_secret: &[u8; 32]) {
    // Single-flight: the resume path and the attest worker can race into open_session_vault; only one runs the walk (the other would only redo identical idempotent writes, but the log should say it happened once).
    static MIGRATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _flight = match MIGRATION.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    const MARKER: &str = "migration/legacy-vault-v1";
    let already = matches!(vault.read_device(MARKER), Ok(Some(_)));
    let legacy_paths = match kete::vault_ring_paths(APP, vault_seed, device_secret) {
        Ok(p) => p,
        Err(e) => {
            crate::logf!("STORAGE: legacy vault path resolution failed (migration skipped): {}", e);
            return;
        }
    };
    if !already {
        if legacy_paths.iter().any(|p| p.exists()) {
            match FlatStorage::open_shared(APP, *vault_seed, *device_secret) {
                Ok(legacy) => match vault.adopt_all_entries_from(&legacy) {
                    Ok(count) => {
                        // Marker AFTER the copy is durable — a crash mid-walk re-runs the whole (idempotent) walk next launch. Value = entry count, binary at rest.
                        if let Err(e) = vault.write_device(MARKER, &(count as u64).to_le_bytes()) {
                            crate::logf!("STORAGE: migration marker write failed (walk will re-run next launch): {}", e);
                            return;
                        }
                        crate::logf!("STORAGE: legacy vault migrated in place — {} entries carried into the device vault", count);
                    }
                    Err(e) => {
                        crate::logf!("STORAGE: legacy vault migration FAILED (will retry next launch, legacy vault untouched): {}", e);
                        return;
                    }
                },
                Err(e) => {
                    crate::logf!("STORAGE: legacy vault open failed (migration deferred): {}", e);
                    return;
                }
            }
        } else {
            // Fresh install — nothing to carry; the marker records that the question was settled.
            let _ = vault.write_device(MARKER, &0u64.to_le_bytes());
        }
    }
    // Backup-rename OUTSIDE the marker gate: retried every launch until the rings are actually out of the census (Windows can refuse a rename while the just-dropped legacy engine's handle lingers; Linux never does).
    for p in legacy_paths.iter().filter(|p| p.exists()) {
        let backup = p.with_extension("vsf.legacy");
        match std::fs::rename(p, &backup) {
            Ok(()) => crate::logf!("STORAGE: legacy vault ring parked as backup: {}", backup.display()),
            Err(e) => crate::logf!("STORAGE: legacy ring rename deferred ({}): {}", p.display(), e),
        }
    }
}

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
        // Dev-only override: PHOTON_DATA_DIR points a whole instance (vault + log + lock) at a separate dir, so a second instance can run isolated for two-party testing (pair with PHOTON_FINGERPRINT for a distinct device identity). Compiled out of release so production has no escape hatch from the single-instance lock. Also alive under cfg(test): the test-isolation helper routes EVERY disk write thru it so `cargo test` can never touch a real config dir again (the "eight vaults" mystery, 2026-08-20).
        #[cfg(any(feature = "development", test))]
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

/// TEST ISOLATION — every disk-touching test calls this FIRST. Routes photon's config dir (blobs, settings, binding, markers) AND kete's vault rings into the system tempdir; a test that forgets this writes REAL 17MB vault rings + strays into the developer's live home (the "eight vaults, recent timestamps" field mystery). Process-global within a run; the root is per-PID with a once-per-process sweep so a PREVIOUS run's vault files never leak into this run's fresh-state assumptions.
#[cfg(test)]
pub fn isolate_test_storage() {
    let root = std::env::temp_dir().join(format!("photon-tests-{}", std::process::id()));
    static CLEAN: std::sync::Once = std::sync::Once::new();
    CLEAN.call_once(|| {
        let _ = std::fs::remove_dir_all(&root); // pid-reuse insurance — before any test writes; same-run siblings only ever see the fresh root
    });
    std::env::set_var("PHOTON_DATA_DIR", root.join("cfg"));
    kete::set_vault_dirs_override(
        root.join("vault-cfg").to_string_lossy().into_owned(),
        root.join("vault-data").to_string_lossy().into_owned(),
    );
}

/// The attachment blob directory: `<config>/blobs/`. Blobs live OUTSIDE the vault deliberately — the dual-ring mirror doubles every vault write and a multi-MB value triggers a vault-grow fsync that freezes the UI (the same phenomenon that made clutch keypairs memory-only). Each blob is one sealed file; the FILENAME is a keyed hash, never the plaintext hash (see BLOB_NAME_KEY).
pub fn blob_dir() -> Result<std::path::PathBuf, std::io::Error> {
    let dir = photon_config_dir()?.join("blobs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Per-device blob seal key. Derived from the identity seed — blobs never leave this device as FILES (the wire carries its own seal under history/fleet keys), so the at-rest key needs no cross-device agreement; identity-seed derivation just means a restored session can still open its old blobs.
pub fn blob_seal_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(&format!("{}.blob.seal.v0", APP.id), identity_seed)
}

// FILENAMES ARE A FORENSIC SURFACE (Nick's threat model, found 2026-08-20): the v0 scheme named each file by the PLAINTEXT blake3 — the content was sealed, but anyone holding a CANDIDATE file could hash it and prove possession by filename alone, zero keys needed (a known-plaintext existence oracle in the filesystem). Names are now blake3 KEYED by a seed-derived name key — meaningless without the identity seed. The name key lives in a process global because presence checks run on the render path where no seed is in scope; it is set the moment a session's seed exists (blob_init_names) and cleared with the session.
static BLOB_NAME_KEY: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

fn blob_name_key_of(identity_seed: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(&format!("{}.blob.name.v1", APP.id), identity_seed)
}

/// The sealed file's name for a content hash: keyed blake3 under the session's name key. None = no session yet (presence reads false, deletes no-op — the same "vault not open" posture as everything else).
fn blob_file_name(content_hash: &[u8; 32]) -> Option<String> {
    let k = (*BLOB_NAME_KEY.lock().unwrap())?;
    Some(format!(
        "{}.bin",
        hex::encode(blake3::keyed_hash(&k, content_hash).as_bytes())
    ))
}

/// Install the blob name key for this session AND run the one-time v0→v1 rename walk: every 64-hex-named file is a v0 plaintext-hash name — re-key it. Marker-gated (`.names-v1`) so keyed names are never re-keyed; a fresh install writes the marker over an empty dir. Call whenever a session's identity seed becomes available.
pub fn blob_init_names(identity_seed: &[u8; 32]) {
    let key = blob_name_key_of(identity_seed);
    *BLOB_NAME_KEY.lock().unwrap() = Some(key);
    let Ok(dir) = blob_dir() else { return };
    let marker = dir.join(".names-v1");
    if marker.exists() {
        return;
    }
    let mut renamed = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".bin")) else {
                continue;
            };
            let Ok(bytes) = hex::decode(stem) else { continue };
            let Ok(content_hash) = <[u8; 32]>::try_from(bytes.as_slice()) else {
                continue;
            };
            let new = format!(
                "{}.bin",
                hex::encode(blake3::keyed_hash(&key, &content_hash).as_bytes())
            );
            if std::fs::rename(e.path(), dir.join(new)).is_ok() {
                renamed += 1;
            }
        }
    }
    let _ = std::fs::write(&marker, b"1");
    if renamed > 0 {
        crate::logf!(
            "blob: re-keyed {} v0 plaintext-hash filename(s) — the possession oracle is closed",
            renamed
        );
    }
}

/// Store an attachment blob: seal plaintext under the device blob key, write it under the KEYED name. Idempotent — an existing file is left alone (content-addressed, same hash = same bytes).
pub fn blob_store(
    identity_seed: &[u8; 32],
    content_hash: &[u8; 32],
    plaintext: &[u8],
) -> Result<(), String> {
    if BLOB_NAME_KEY.lock().unwrap().is_none() {
        blob_init_names(identity_seed);
    }
    let dir = blob_dir().map_err(|e| e.to_string())?;
    let name = blob_file_name(content_hash).ok_or("blob: no session name key")?;
    let path = dir.join(name);
    if path.exists() {
        return Ok(());
    }
    let sealed = kete::encrypt_bytes(plaintext, &blob_seal_key(identity_seed))?;
    std::fs::write(&path, &sealed).map_err(|e| e.to_string())
}

/// Load + open an attachment blob; verifies the content hash after decrypt. None = not held locally.
pub fn blob_load(identity_seed: &[u8; 32], content_hash: &[u8; 32]) -> Option<Vec<u8>> {
    if BLOB_NAME_KEY.lock().unwrap().is_none() {
        blob_init_names(identity_seed);
    }
    let dir = blob_dir().ok()?;
    let sealed = std::fs::read(dir.join(blob_file_name(content_hash)?)).ok()?;
    let plain = kete::decrypt_bytes(&sealed, &blob_seal_key(identity_seed)).ok()?;
    if blake3::hash(&plain).as_bytes() != content_hash {
        crate::log("blob: content hash mismatch on load — corrupt blob file dropped");
        return None;
    }
    Some(plain)
}

/// Whether the blob for `content_hash` is held locally (no decrypt — presence only). False before a session exists (no name key = no way to look, same as no vault).
pub fn blob_present(content_hash: &[u8; 32]) -> bool {
    match (blob_dir(), blob_file_name(content_hash)) {
        (Ok(d), Some(name)) => d.join(name).exists(),
        _ => false,
    }
}

/// Delete a blob file (attachment tombstone follow-through — blobs CAN truly shred; only row content is braid-bound).
pub fn blob_delete(content_hash: &[u8; 32]) {
    if let (Ok(dir), Some(name)) = (blob_dir(), blob_file_name(content_hash)) {
        let _ = std::fs::remove_file(dir.join(name));
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
            crate::logf!(
                "STORAGE: Write verification failed for {} (wrote {} bytes, read back {} bytes)",
                label,
                data.len(),
                readback.len()
            );
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "write verification failed: data mismatch",
            ))
        }
        Err(e) => {
            crate::logf!(
                "STORAGE: Write verification read-back failed for {}: {}",
                label,
                e
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// THE no-data-loss contract: a legacy per-identity vault's entries — string keys and raw addresses alike — survive verbatim into the device vault on the first open_session_vault, the legacy rings park as `.legacy` backups, and a second open changes nothing.
    #[test]
    fn open_session_vault_migrates_legacy_in_place() {
        isolate_test_storage();
        let vault_seed = [0x4Au8; 32];
        let device_secret = [0x4Bu8; 32];
        {
            let legacy = FlatStorage::new(APP, vault_seed, device_secret).unwrap();
            legacy.write("contacts/index", b"Emma,Nick").unwrap();
            let first_message_addr = vault_key("rows", &[0x11u8; 32]);
            legacy.write_addr(&first_message_addr, b"the first message ever sent over FGTW").unwrap();
        }
        let vault = open_session_vault(vault_seed, device_secret).unwrap();
        assert_eq!(vault.read("contacts/index").unwrap(), Some(b"Emma,Nick".to_vec()));
        let first_message_addr = vault_key("rows", &[0x11u8; 32]);
        assert_eq!(
            vault.read_addr(&first_message_addr).unwrap(),
            Some(b"the first message ever sent over FGTW".to_vec())
        );
        // The legacy rings are OUT of the census — parked as backups, never deleted.
        let legacy_paths = kete::vault_ring_paths(APP, &vault_seed, &device_secret).unwrap();
        for p in &legacy_paths {
            assert!(!p.exists(), "legacy ring still in the census: {}", p.display());
            assert!(p.with_extension("vsf.legacy").exists(), "backup missing for {}", p.display());
        }
        // Idempotent: a re-open (same session, next launch) is a no-op that still reads everything.
        drop(vault);
        let again = open_session_vault(vault_seed, device_secret).unwrap();
        assert_eq!(again.read("contacts/index").unwrap(), Some(b"Emma,Nick".to_vec()));
    }
}
