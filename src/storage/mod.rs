pub mod cloud;
pub mod contacts;
pub mod device_binding;
pub mod fanout_pairs;
pub mod fleet_settings;
pub mod friendship;

// The storage adapter (was `flat.rs`) now lives in the shared `kete` crate. Re-export its surface so existing call sites — `crate::storage::FlatStorage`, `StorageError`, `encrypt_bytes`/`decrypt_bytes` (used by cloud.rs) — keep resolving unchanged.
pub use kete::{decrypt_bytes, encrypt_bytes, App, FlatStorage, StorageError};

/// Photon's app namespace for kete. ONE directory, ONE casing (2026-08-20): everything lives under `photon/` — the vault ring joins the config dir the strays used to sprawl in, and the census goal is that ring being the only file there. `id` keeps the KDF contexts unchanged.
pub const APP: kete::App<'static> = kete::App {
    id: "photon",
    dir: "photon",
};

#[cfg(target_os = "android")]
pub use kete::{android_vault_dirs, set_android_vault_dirs};

/// THE vault open — every runtime site (attest worker, launch, resume) comes thru here and nowhere else.
///
/// Opens the ONE device vault (named from the device secret alone, exists from first launch) and installs the session's vault seed as its identity (idempotent; a different identity is refused — one identity per device). No local import/migration layer: THE FLEET IS THE BACKUP — a fresh vault fills from chain replication + history sync, the same way any rejoining device does. The shared registry underneath means concurrent callers (resume path + attest worker) receive the SAME engine — a second independent engine racing the live one is the corruption class that bricked a field vault ("seal verification failed" on every subsequent open).
pub fn open_session_vault(
    identity_seed: [u8; 32],
    vault_seed: [u8; 32],
    device_secret: [u8; 32],
) -> Result<std::sync::Arc<FlatStorage>, StorageError> {
    install_device_secret(device_secret);
    let vault = device_vault().ok_or_else(|| StorageError::Vault("device vault unavailable".to_string()))?;
    vault.set_identity(vault_seed)?;
    // Blob addressing keys off the IDENTITY seed.
    blob_init_names(&identity_seed);
    Ok(vault)
}

/// The resolved device secret — installed by driver init / open_session_vault / the Android JNI wiring (all hand in the same fingerprint-deterministic bytes) and self-derived on desktop when a pre-init caller (main's CLI toggles, early widget state) needs the device vault first. A Mutex, not a OnceLock, so tests can rebind — at runtime every writer carries identical bytes.
static DEVICE_SECRET: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

/// Install the device secret (from the resolved device keypair).
pub fn install_device_secret(secret: [u8; 32]) {
    if let Ok(mut g) = DEVICE_SECRET.lock() {
        *g = Some(secret);
    }
}

fn resolved_device_secret() -> Option<[u8; 32]> {
    if let Ok(g) = DEVICE_SECRET.lock() {
        if let Some(s) = *g {
            return Some(s);
        }
    }
    // Desktop self-derivation; Android has no in-Rust fingerprint oracle (Build.FINGERPRINT lives Java-side), so a pre-install caller there gets None and the feature reads its default.
    #[cfg(not(target_os = "android"))]
    {
        let fp = crate::network::fgtw::get_machine_fingerprint().ok()?;
        let kp = crate::network::fgtw::derive_device_keypair(&fp);
        let s = *kp.secret.as_bytes();
        install_device_secret(s);
        return Some(s);
    }
    #[cfg(target_os = "android")]
    None
}

/// THE process-wide device vault, pre-identity: open from the device secret alone, from first launch, before any handle is typed. This is where the pre-attest state lives (D2 binding, opt-in flags, reboot capsule) — Nick's key model: entries are hash(thing|device) here and hash(thing|device|person) once attested (the identity scope on the same vault, unlocked by open_session_vault). First open runs the census sweep.
/// The cache is SECRET-KEYED, not first-open-wins: a rebound secret (tests; never runtime) re-resolves instead of silently serving the old vault. kete's shared-engine registry does the real dedup underneath.
pub fn device_vault() -> Option<std::sync::Arc<FlatStorage>> {
    static DEVICE_VAULT: std::sync::Mutex<Option<([u8; 32], std::sync::Arc<FlatStorage>)>> =
        std::sync::Mutex::new(None);
    let secret = resolved_device_secret()?;
    let mut g = DEVICE_VAULT.lock().ok()?;
    if let Some((s, v)) = g.as_ref() {
        if *s == secret {
            return Some(v.clone());
        }
    }
    match FlatStorage::open_device_shared(APP, secret) {
        Ok(v) => {
            // A repair-open that pruned values booted the vault but LOST data — the banner must say so even though the open "succeeded".
            if v.repaired_lost_values() > 0 {
                crate::logf!("STORAGE: device vault opened via repair — {} value(s) lost to pruned dangling pointers", v.repaired_lost_values());
                flag_vault_sick();
            }
            census_sweep(&secret);
            *g = Some((secret, v.clone()));
            Some(v)
        }
        Err(e) => {
            crate::logf!("STORAGE: device vault open failed: {}", e);
            flag_vault_sick();
            None
        }
    }
}

/// Cross-thread storage-failure latch: writer threads and open paths set it, the UI tick mirrors it into `vault_degraded` (the amber banner). Born of the 2026-08-24 zombie boots — a vault-open death and 1,276 fence errors ran for hours as log lines while the screen claimed all was well. Sticky for the session by design: a vault that failed once is a vault to distrust until relaunch.
static VAULT_SICK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn flag_vault_sick() {
    VAULT_SICK.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn vault_sick() -> bool {
    VAULT_SICK.load(std::sync::atomic::Ordering::Relaxed)
}

/// A device-scope boolean flag (opt-ins/vetoes that used to be marker FILES). Present = true, absent = false; the caller owns the default polarity.
pub fn device_flag(key: &str) -> bool {
    device_vault().map_or(false, |v| matches!(v.read_device(key), Ok(Some(_))))
}

/// Write (`true`) or delete (`false`) a device-scope flag entry.
pub fn set_device_flag(key: &str, on: bool) {
    let Some(v) = device_vault() else { return };
    let r = if on { v.write_device(key, &[1u8]) } else { v.delete_device(key) };
    if let Err(e) = r {
        crate::logf!("STORAGE: device flag {} write failed: {}", key, e);
    }
}

/// THE CENSUS (Nick's rule, sharpened 2026-08-20): a file in the primary or secondary photon dir either matches `<device token>.vsf` or the log, or it is DELETED — no backups, no folding, no conversion, no exceptions. File-era artifacts (device_binding.vsf, opt-in markers, reboot capsule, blobs/, settings.vsf) die here with everything else: flags and bindings re-arm thru their live vault paths; sealed file content nothing can re-key is dead. Name-independent sweep, so the next stray CLASS never needs its own line. Runs once per process at first device-vault open.
/// Desktop only: on Android `photon_config_dir` is the app-private files ROOT, which the OS and the JNI layer also write into — sweeping unknown names there would eat platform files; the sandbox already isolates it.
fn census_sweep(device_secret: &[u8; 32]) {
    #[cfg(target_os = "android")]
    let _ = device_secret;
    #[cfg(not(target_os = "android"))]
    {
        let ring = tohu::device_vault_path_name(APP.id, device_secret);
        let keep = [
            format!("{}.vsf", ring),
            format!("{}.shadow.vsf", ring), // the mirror when primary and secondary collide into one dir (macOS)
            "photon.log.vsf".to_string(),
        ];
        let mut targets: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(d) = photon_config_dir() {
            targets.push(d);
        }
        if let Some(d) = photon_data_dir() {
            if !targets.contains(&d) {
                targets.push(d);
            }
        }
        for dir in targets {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for entry in rd.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if keep.iter().any(|k| *k == name) {
                        continue;
                    }
                    let p = entry.path();
                    let removed = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                    match removed {
                        Ok(()) => crate::logf!("STORAGE: census — {} deleted (the census is the vault ring + the log, nothing else)", name),
                        Err(e) => crate::logf!("STORAGE: census — could not remove {}: {}", name, e),
                    }
                }
            }
            // The pre-unification `Photon/` sibling dir is a stray WHOLESALE — the fleet is the backup, so per-identity ring archaeology has no home here. Guard: on a case-insensitive filesystem (macOS) "Photon" resolves to the LIVE dir — canonical-compare and skip.
            let Some(parent) = dir.parent() else { continue };
            let legacy = parent.join("Photon");
            let is_alias = matches!((legacy.canonicalize(), dir.canonicalize()), (Ok(a), Ok(b)) if a == b);
            if legacy.exists() && !is_alias {
                match std::fs::remove_dir_all(&legacy) {
                    Ok(()) => crate::logf!("STORAGE: census — legacy dir {} deleted (the fleet is the backup)", legacy.display()),
                    Err(e) => crate::logf!("STORAGE: census — could not remove {}: {}", legacy.display(), e),
                }
            }
        }
    }
}

/// The secondary (data-dir) photon dir — the vault mirror's home where the XDG split gives two roots. Honors the same dev/test override as [`photon_config_dir`] (the override points the whole instance at ONE dir; the census dedupes).
#[cfg(not(target_os = "android"))]
fn photon_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(any(feature = "development", test))]
    if let Ok(custom) = std::env::var("PHOTON_DATA_DIR") {
        if !custom.is_empty() {
            return Some(std::path::PathBuf::from(custom));
        }
    }
    dirs::data_dir().map(|p| p.join(APP.dir))
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
    let key = blake3::derive_key(&format!("{}.storage.entry.v0", APP.id), &input);
    // NAMETAG AT THE MINT (dev builds, field 2026-08-28): an 8MB value at addr 920ee297 grew all day while BOTH per-blob confessions (settings, chains) stayed silent — the monster belongs to a third domain, and kete's SLOW-put line can only name the address. Log each (address, domain) pair ONCE so any addr in any log maps straight to its domain. Addresses aren't secrets (they appear in every SLOW line already); the DOMAIN string is the only new information.
    #[cfg(feature = "development")]
    {
        static SEEN: std::sync::Mutex<Option<std::collections::HashSet<[u8; 4]>>> =
            std::sync::Mutex::new(None);
        let prefix: [u8; 4] = key[..4].try_into().unwrap();
        if let Ok(mut g) = SEEN.lock() {
            let set = g.get_or_insert_with(Default::default);
            if set.insert(prefix) {
                crate::logf!("VAULTKEY: {} = domain '{}'", hex::encode(prefix), domain);
            }
        }
    }
    key
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

// BLOBS LIVE IN THE VAULT (2026-08-20, "ONE VAULT WITH A MIRROR FOR FUCKUPS AND HEALING"): an attachment/recording is one vault value at an identity-keyed address — arbitrary size in principle (EWE), sealed by the vault's own per-address keys, mirrored by the dual rings like everything else. A file-era blobs/ dir is a census stray — deleted, never imported.
// ADDRESSES ARE A FORENSIC SURFACE (the v0 filename lesson): trie keys sit in plaintext in the vault leaves, so an unkeyed content-hash address would let anyone holding the rings + a CANDIDATE file prove possession with zero keys. The address is blake3 KEYED by a seed-derived name key — meaningless without the identity seed. The name key lives in a process global because presence checks run on the render path where no seed is in scope; it is set the moment a session's seed exists (blob_init_names) and cleared with the session.
static BLOB_NAME_KEY: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

fn blob_name_key_of(identity_seed: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(&format!("{}.blob.name.v1", APP.id), identity_seed)
}

/// The vault address for a content hash: keyed blake3 under the session's name key. None = no session yet (presence reads false, deletes no-op — the same "vault not open" posture as everything else).
fn blob_addr(content_hash: &[u8; 32]) -> Option<[u8; 32]> {
    let k = (*BLOB_NAME_KEY.lock().unwrap())?;
    Some(*blake3::keyed_hash(&k, content_hash).as_bytes())
}

/// Install the blob name key for this session. Call whenever a session's identity seed becomes available. (The file-era blobs/ dir is NOT imported — the census deletes it like every other stray.)
pub fn blob_init_names(identity_seed: &[u8; 32]) {
    *BLOB_NAME_KEY.lock().unwrap() = Some(blob_name_key_of(identity_seed));
}

/// Store an attachment blob as a vault value at its identity-keyed address. Idempotent — content-addressed, same hash = same bytes, and the vault write is an overwrite-with-itself.
pub fn blob_store(
    identity_seed: &[u8; 32],
    content_hash: &[u8; 32],
    plaintext: &[u8],
) -> Result<(), String> {
    if BLOB_NAME_KEY.lock().unwrap().is_none() {
        blob_init_names(identity_seed);
    }
    let addr = blob_addr(content_hash).ok_or("blob: no session name key")?;
    let vault = device_vault().ok_or("blob: vault unavailable")?;
    vault.write_addr(&addr, plaintext).map_err(|e| e.to_string())
}

/// Load an attachment blob from the vault; verifies the content hash after the vault's own AEAD. None = not held locally.
pub fn blob_load(identity_seed: &[u8; 32], content_hash: &[u8; 32]) -> Option<Vec<u8>> {
    if BLOB_NAME_KEY.lock().unwrap().is_none() {
        blob_init_names(identity_seed);
    }
    let addr = blob_addr(content_hash)?;
    let plain = device_vault()?.read_addr(&addr).ok()??;
    if blake3::hash(&plain).as_bytes() != content_hash {
        crate::log("blob: content hash mismatch on load — corrupt blob value dropped");
        return None;
    }
    Some(plain)
}

/// Whether the blob for `content_hash` is held locally — stored-bytes presence, NO decrypt (render-path cheap). False before a session exists (no name key = no way to look, same as no vault).
pub fn blob_present(content_hash: &[u8; 32]) -> bool {
    match (device_vault(), blob_addr(content_hash)) {
        (Some(v), Some(addr)) => matches!(v.read_stored(&addr), Ok(Some(_))),
        _ => false,
    }
}

/// Delete a blob value (attachment tombstone follow-through — blobs CAN truly shred; only row content is braid-bound).
pub fn blob_delete(content_hash: &[u8; 32]) {
    if let (Some(v), Some(addr)) = (device_vault(), blob_addr(content_hash)) {
        let _ = v.delete_addr(&addr);
    }
}

/// Process-lifetime artifacts (single-instance lock, control socket) live in the RUNTIME dir, not config — they are not state, and the config-dir census is exactly two files (log + vault). tmpfs where available, so a crash leaves nothing behind past reboot.
pub fn runtime_dir() -> std::path::PathBuf {
    #[cfg(unix)]
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        if !d.is_empty() {
            return std::path::PathBuf::from(d).join("photon");
        }
    }
    std::env::temp_dir().join("photon")
}

/// A runtime artifact path keyed to the DATA dir it guards: two instances on different data dirs (the PHOTON_DATA_DIR two-party dev setup) must never share a lock or a control socket.
pub fn runtime_artifact(data_dir: &std::path::Path, ext: &str) -> std::path::PathBuf {
    let h = blake3::hash(data_dir.to_string_lossy().as_bytes());
    runtime_dir().join(format!("{}.{}", hex::encode(&h.as_bytes()[..8]), ext))
}

/// Holds the single-instance lock for the whole process; dropping it (or process exit/crash) releases it.
/// Unix-only: this is the `flock`-backed variant. Non-unix desktops (Windows) use the socket-backed `InstanceLock` defined below, and Android is single-instance by construction so neither is compiled there.
#[cfg(all(unix, not(target_os = "android")))]
pub struct InstanceLock {
    _file: std::fs::File,
}

/// Single-instance guard, keyed to the data dir: two instances on the SAME dir would race the vault and corrupt the log (the trim is read-truncate-rewrite), so the second must not start.
/// An advisory exclusive `flock` on the dir-keyed runtime lock (see `runtime_artifact`) — the kernel releases it when the holding process dies, so a crash leaves no stale lock, and tmpfs means no artifact survives a reboot either.
/// Returns the guard to keep alive for the whole process, or `None` if another instance already holds this dir. (Non-unix desktops fall back to a localhost socket; Android is single-instance by construction so this isn't compiled there.)
#[cfg(all(unix, not(target_os = "android")))]
pub fn acquire_single_instance(data_dir: &std::path::Path) -> Option<InstanceLock> {
    use std::os::unix::io::AsRawFd;
    let lock_path = runtime_artifact(data_dir, "lock");
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
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

    /// Tests that touch the process-wide device secret / name-key globals run SERIALLY — a parallel rebind mid-test reads the wrong vault and flakes.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        GATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// THE census rule (Nick, verbatim, 2026-08-20): a file in the primary or secondary dir that doesn't match `<device token>.vsf` or the log "goes bye bye. no backey uppey, no convertey, no touchey." Strays don't need to be known by name to die, file-era artifacts included — nothing is imported into the vault.
    #[test]
    fn census_sweep_auto_nukes_strays() {
        let _g = serial();
        isolate_test_storage();
        let secret = [0x9Du8; 32];
        let dir = photon_config_dir().unwrap();
        std::fs::create_dir_all(dir.join("junkdir")).unwrap();
        std::fs::write(dir.join("junkdir/orphan.bin"), b"x").unwrap();
        std::fs::write(dir.join("mystery.dat"), b"x").unwrap();
        std::fs::write(dir.join("photon.log.vsf"), b"log").unwrap();
        std::fs::create_dir_all(dir.join("blobs")).unwrap();
        std::fs::write(dir.join("blobs/orphan.bin"), vec![0xEEu8; 64]).unwrap();
        let key = blake3::derive_key("photon.device_binding.v0", &secret);
        std::fs::write(dir.join("device_binding.vsf"), kete::encrypt_bytes(&[0x77u8; 32], &key).unwrap()).unwrap();
        std::fs::write(dir.join("unattended_reboot"), b"operator opt-in").unwrap();
        std::fs::write(dir.join("settings.vsf"), b"dead-knobs").unwrap();
        install_device_secret(secret);
        let v = device_vault().unwrap();
        assert!(!dir.join("junkdir").exists(), "stray dir survived the census");
        assert!(!dir.join("mystery.dat").exists(), "stray file survived the census");
        assert!(!dir.join("blobs").exists(), "blobs dir survived the census");
        assert!(!dir.join("device_binding.vsf").exists(), "binding file survived the census");
        assert!(!dir.join("unattended_reboot").exists(), "marker file survived the census");
        assert!(!dir.join("settings.vsf").exists(), "settings file survived the census");
        assert!(dir.join("photon.log.vsf").exists(), "the log must survive the census");
        // No conversion: deleted files leave NOTHING behind in the vault.
        assert_eq!(v.read_device("binding/party").unwrap(), None);
        assert_eq!(v.read_device("flags/unattended_reboot").unwrap(), None);
    }

    /// Blobs are vault values at identity-keyed addresses — store/presence/load/delete all vault-side; a planted file-era blobs/ dir is deleted, not imported.
    #[test]
    fn blobs_live_in_the_vault() {
        let _g = serial();
        isolate_test_storage();
        let identity = [0x6Au8; 32];
        let vault_seed = [0x6Bu8; 32];
        let secret = [0x6Cu8; 32];
        let dir = photon_config_dir().unwrap().join("blobs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("deadbeef00.bin"), vec![0xCDu8; 128]).unwrap();
        let _vault = open_session_vault(identity, vault_seed, secret).unwrap();
        assert!(!dir.exists(), "blobs dir survived the census");
        // Fresh store / presence / load / delete round trip, all vault-side.
        let p2 = vec![0xAB; 300_000];
        let h2 = *blake3::hash(&p2).as_bytes();
        blob_store(&identity, &h2, &p2).unwrap();
        assert!(blob_present(&h2));
        assert_eq!(blob_load(&identity, &h2), Some(p2));
        blob_delete(&h2);
        assert!(!blob_present(&h2));
    }

    /// THE census contract: after a session opens over a full file-era sprawl (strays + blobs + a legacy `Photon/` sibling dir), the config tree holds EXACTLY the ring + log — every stray deleted, the legacy dir gone wholesale.
    #[test]
    fn end_state_census_is_the_vault_alone() {
        let _g = serial();
        isolate_test_storage();
        let identity = [0x7Au8; 32];
        let vault_seed = [0x7Bu8; 32];
        let secret = [0x7Cu8; 32];
        let cfg = photon_config_dir().unwrap();
        std::fs::create_dir_all(cfg.join("blobs")).unwrap();
        std::fs::write(cfg.join("blobs/aa.bin"), vec![0xAAu8; 96]).unwrap();
        std::fs::write(cfg.join("unattended_reboot"), b"on").unwrap();
        std::fs::write(cfg.join("settings.vsf"), b"dead").unwrap();
        // The pre-unification parking lot: a capital-P sibling dir with an old per-identity ring in it.
        let legacy_dir = cfg.parent().unwrap().join("Photon");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("oldring.vsf"), vec![0x11u8; 256]).unwrap();

        let _vault = open_session_vault(identity, vault_seed, secret).unwrap();

        // Census: the strays and the blobs dir are gone from the config dir, and the legacy dir is gone wholesale.
        let leftovers: Vec<String> = std::fs::read_dir(&cfg)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "photon.log.vsf")
            .collect();
        assert!(leftovers.is_empty(), "config-dir strays survived: {:?}", leftovers);
        assert!(!legacy_dir.exists(), "legacy Photon/ dir survived the census");
        // The device-vault rings exist at both mirror paths. (Per-dir "exactly one file" isn't assertable here — the test root is shared by the whole parallel suite, so sibling tests' vaults coexist; the field census is the two rings.)
        let device_name = tohu::device_vault_path_name(APP.id, &secret);
        let ring_paths = kete::vault_ring_paths(APP, &vault_seed, &secret).unwrap();
        for p in &ring_paths {
            let ring = p.parent().unwrap().join(format!("{}.vsf", device_name));
            assert!(ring.exists(), "device ring missing: {}", ring.display());
        }
    }

    /// THE born-empty contract: a fresh vault holds ZERO entries — no imports, no parked ciphertext, nothing. History reaches a new device from the FLEET (chain replication + history sync), never from local file archaeology. A re-open writes nothing either.
    #[test]
    fn fresh_vault_is_born_empty() {
        let _g = serial();
        isolate_test_storage();
        let vault_seed = [0x4Au8; 32];
        let device_secret = [0x4Bu8; 32];
        let vault = open_session_vault([0x49u8; 32], vault_seed, device_secret).unwrap();
        assert!(vault.live_addrs().unwrap().is_empty(), "fresh vault is not empty");
        drop(vault);
        let again = open_session_vault([0x49u8; 32], vault_seed, device_secret).unwrap();
        assert!(again.live_addrs().unwrap().is_empty(), "a bare re-open wrote entries");
        // And the identity scope round-trips.
        again.write("state/probe", b"x").unwrap();
        assert_eq!(again.read("state/probe").unwrap(), Some(b"x".to_vec()));
    }
}
