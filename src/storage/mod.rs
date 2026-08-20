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

/// The pre-unification namespace (`Photon/`, capital P) — used ONLY to locate and open legacy per-identity vaults during the in-place migration. Same `id`, so every KDF context (and therefore every migrated ciphertext) is identical.
const LEGACY_APP: kete::App<'static> = kete::App {
    id: "photon",
    dir: "Photon",
};

#[cfg(target_os = "android")]
pub use kete::{android_vault_dirs, set_android_vault_dirs};

/// THE vault open — every runtime site (attest worker, launch, resume) comes thru here and nowhere else.
///
/// Opens the ONE device vault (named from the device secret alone, exists from first launch), installs the session's vault seed as its identity (idempotent; a different identity is refused — one identity per device), then runs the in-place legacy migration exactly once. The shared registry underneath means concurrent callers (resume path + attest worker) receive the SAME engine — a second independent engine racing the live one is the corruption class that bricked a field vault ("seal verification failed" on every subsequent open).
pub fn open_session_vault(
    identity_seed: [u8; 32],
    vault_seed: [u8; 32],
    device_secret: [u8; 32],
) -> Result<std::sync::Arc<FlatStorage>, StorageError> {
    install_device_secret(device_secret);
    let vault = device_vault().ok_or_else(|| StorageError::Vault("device vault unavailable".to_string()))?;
    vault.set_identity(vault_seed)?;
    migrate_legacy_vault(&vault, &vault_seed, &device_secret);
    // Blob addressing keys off the IDENTITY seed; with the identity scope live, the file-era blobs/ dir folds in.
    blob_init_names(&identity_seed);
    absorb_blob_files(&identity_seed);
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

/// THE process-wide device vault, pre-identity: open from the device secret alone, from first launch, before any handle is typed. This is where the pre-attest state lives (D2 binding, opt-in flags, reboot capsule) — Nick's key model: entries are hash(thing|device) here and hash(thing|device|person) once attested (the identity scope on the same vault, unlocked by open_session_vault). First open absorbs the loose-file sprawl.
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
            absorb_loose_files(&v, &secret);
            *g = Some((secret, v.clone()));
            Some(v)
        }
        Err(e) => {
            crate::logf!("STORAGE: device vault open failed: {}", e);
            None
        }
    }
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

/// Fold the loose-file sprawl into the device vault — each artifact is move-then-delete, so the walk is self-terminating (absent file = already folded) and crash-safe (a crash re-folds only what's left). No marker needed. Runs once per process at first device-vault open.
fn absorb_loose_files(vault: &FlatStorage, device_secret: &[u8; 32]) {
    let Ok(dir) = photon_config_dir() else { return };
    // D2 binding marker: sealed under its own device-derived key in the file era — decrypt with that key, store the party id as a device-scope entry (the vault seals it from here on).
    let binding = dir.join("device_binding.vsf");
    if let Ok(bytes) = std::fs::read(&binding) {
        let key = blake3::derive_key("photon.device_binding.v0", device_secret);
        if let Ok(plain) = kete::decrypt_bytes(&bytes, &key) {
            if vault.write_device("binding/party", &plain).is_ok() {
                let _ = std::fs::remove_file(&binding);
                crate::log("STORAGE: device binding folded into the vault");
            }
        } else {
            // Unreadable under this device's key = not ours/corrupt — the worker index backstops; the stray still leaves the census.
            let _ = std::fs::remove_file(&binding);
        }
    }
    // Opt-in / veto markers: file existence becomes a flag entry.
    for (file, key) in [
        ("unattended_reboot", "flags/unattended_reboot"),
        ("remote_terminal", "flags/remote_terminal"),
        ("background_optout", "flags/background_optout"),
    ] {
        let p = dir.join(file);
        if p.exists() && vault.write_device(key, &[1u8]).is_ok() {
            let _ = std::fs::remove_file(&p);
            crate::logf!("STORAGE: {} marker folded into the vault", file);
        }
    }
    // Reboot capsule: already-sealed bytes move verbatim — tohu opens them from wherever they live.
    let capsule = dir.join("reboot_capsule");
    if let Ok(bytes) = std::fs::read(&capsule) {
        if vault.write_device("capsule/reboot", &bytes).is_ok() {
            let _ = std::fs::remove_file(&capsule);
            crate::log("STORAGE: reboot capsule folded into the vault");
        }
    }
    // THE CENSUS SWEEP (Nick's rule, 2026-08-20): anything in the config dir that isn't the log or a `<token>.vsf` ring gets AUTO-NUKED. The keep-set: the device ring pair (primary + macOS same-dir shadow), the log + its crash sidecar (they land here only when temp is unwritable), and `blobs/` — spared solely because the session-open fold owns it (decrypt-or-delete, then the dir itself goes). Everything else — settings.vsf, lock/socket relics, orphan strays from any era — is deleted by name-independent sweep, so the next stray CLASS never needs its own line here.
    // Desktop only: on Android `photon_config_dir` is the app-private files ROOT, which the OS and the JNI layer also write into — sweeping unknown names there would eat platform files. Android's sandbox already isolates it; the fold above still runs.
    #[cfg(not(target_os = "android"))]
    {
        let ring = tohu::device_vault_path_name(APP.id, device_secret);
        let keep = [
            format!("{}.vsf", ring),
            format!("{}{}.vsf", ring, ".shadow"),
            "photon.log.vsf".to_string(),
            "photon.crash.txt".to_string(),
            "blobs".to_string(),
        ];
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if keep.iter().any(|k| *k == name) {
                    continue;
                }
                let p = entry.path();
                let removed = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                match removed {
                    Ok(()) => crate::logf!("STORAGE: census sweep — stray {} auto-nuked (config dir = the log + the vault)", name),
                    Err(e) => crate::logf!("STORAGE: census sweep — could not remove {}: {}", name, e),
                }
            }
        }
    }
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
    let legacy_paths = match kete::vault_ring_paths(LEGACY_APP, vault_seed, device_secret) {
        Ok(p) => p,
        Err(e) => {
            crate::logf!("STORAGE: legacy vault path resolution failed (migration skipped): {}", e);
            return;
        }
    };
    if !already {
        if legacy_paths.iter().any(|p| p.exists()) {
            match FlatStorage::open_shared(LEGACY_APP, *vault_seed, *device_secret) {
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

// BLOBS LIVE IN THE VAULT (2026-08-20, "ONE VAULT WITH A MIRROR FOR FUCKUPS AND HEALING"): an attachment/recording is one vault value at an identity-keyed address — arbitrary size in principle (EWE), sealed by the vault's own per-address keys, mirrored by the dual rings like everything else. The loose blobs/ dir is absorbed at session open.
// ADDRESSES ARE A FORENSIC SURFACE (the v0 filename lesson): trie keys sit in plaintext in the vault leaves, so an unkeyed content-hash address would let anyone holding the rings + a CANDIDATE file prove possession with zero keys. The address is blake3 KEYED by a seed-derived name key — meaningless without the identity seed. The name key lives in a process global because presence checks run on the render path where no seed is in scope; it is set the moment a session's seed exists (blob_init_names) and cleared with the session.
static BLOB_NAME_KEY: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);

fn blob_name_key_of(identity_seed: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(&format!("{}.blob.name.v1", APP.id), identity_seed)
}

/// Legacy file-era blob seal key — still needed to OPEN old loose blob files during the absorb walk (the vault seals with its own keys from here on).
fn blob_seal_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(&format!("{}.blob.seal.v0", APP.id), identity_seed)
}

/// The vault address for a content hash: keyed blake3 under the session's name key. None = no session yet (presence reads false, deletes no-op — the same "vault not open" posture as everything else).
fn blob_addr(content_hash: &[u8; 32]) -> Option<[u8; 32]> {
    let k = (*BLOB_NAME_KEY.lock().unwrap())?;
    Some(*blake3::keyed_hash(&k, content_hash).as_bytes())
}

/// Install the blob name key for this session. Call whenever a session's identity seed becomes available. (The file-era v0→v1 rename walk is gone — the whole blobs/ dir folds into the vault in `absorb_blob_files`.)
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

/// Fold the file-era blobs/ dir into the vault: decrypt each loose sealed file with the legacy blob key, re-derive its content hash from the plaintext (filenames are keyed hashes — unreversible by design), store it as a vault value, delete the file. Self-terminating (dir absent = done), crash-safe (a crash re-folds only what is left). Un-openable strays (crashed spool tmp, corrupt files) are dead ciphertext — deleted.
fn absorb_blob_files(identity_seed: &[u8; 32]) {
    let Ok(cfg) = photon_config_dir() else { return };
    let dir = cfg.join("blobs");
    if !dir.exists() {
        return;
    }
    let seal = blob_seal_key(identity_seed);
    let mut folded = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            let opened = std::fs::read(&path)
                .ok()
                .and_then(|sealed| kete::decrypt_bytes(&sealed, &seal).ok());
            if let Some(plain) = opened {
                let hash = *blake3::hash(&plain).as_bytes();
                if blob_store(identity_seed, &hash, &plain).is_ok() {
                    let _ = std::fs::remove_file(&path);
                    folded += 1;
                    continue;
                }
                continue; // vault refused — keep the file, retry next launch
            }
            // Marker, crashed spool tmp, or corrupt seal — nothing recoverable lives here.
            let _ = std::fs::remove_file(&path);
        }
    }
    let _ = std::fs::remove_dir(&dir); // only falls when empty — a kept-for-retry file holds it
    if folded > 0 {
        crate::logf!("STORAGE: {} loose blob(s) folded into the vault", folded);
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

    /// THE census rule: anything in the config dir that isn't the log or a vault ring is auto-nuked at first vault open — strays don't need to be known by name to die (the []x-left-blobs field find, 2026-08-20).
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
        std::fs::write(dir.join("blobs/awaiting-fold.bin"), b"x").unwrap();
        install_device_secret(secret);
        let _v = device_vault().unwrap();
        assert!(!dir.join("junkdir").exists(), "stray dir survived the sweep");
        assert!(!dir.join("mystery.dat").exists(), "stray file survived the sweep");
        assert!(dir.join("photon.log.vsf").exists(), "the log must survive the sweep");
        assert!(dir.join("blobs").exists(), "blobs are the session-open fold's to consume, not the sweep's");
    }

    /// Blobs are vault values at identity-keyed addresses; the file-era blobs/ dir folds in at session open and leaves the census.
    #[test]
    fn blobs_live_in_the_vault_and_loose_files_fold() {
        let _g = serial();
        isolate_test_storage();
        let identity = [0x6Au8; 32];
        let vault_seed = [0x6Bu8; 32];
        let secret = [0x6Cu8; 32];
        // Plant a file-era sealed blob under a keyed (unreversible) name — the fold must recover the content hash from the plaintext.
        let dir = photon_config_dir().unwrap().join("blobs");
        std::fs::create_dir_all(&dir).unwrap();
        let plain = b"the kept recording".to_vec();
        let hash = *blake3::hash(&plain).as_bytes();
        let sealed = kete::encrypt_bytes(&plain, &blob_seal_key(&identity)).unwrap();
        std::fs::write(dir.join("deadbeef00.bin"), sealed).unwrap();
        std::fs::write(dir.join(".names-v1"), b"1").unwrap();
        let _vault = open_session_vault(identity, vault_seed, secret).unwrap();
        assert!(blob_present(&hash), "folded blob not present in the vault");
        assert_eq!(blob_load(&identity, &hash), Some(plain));
        assert!(!dir.exists(), "blobs dir still in the census");
        // Fresh store / presence / delete round trip, all vault-side.
        let p2 = vec![0xAB; 300_000];
        let h2 = *blake3::hash(&p2).as_bytes();
        blob_store(&identity, &h2, &p2).unwrap();
        assert!(blob_present(&h2));
        assert_eq!(blob_load(&identity, &h2), Some(p2));
        blob_delete(&h2);
        assert!(!blob_present(&h2));
    }

    /// The file-era sprawl folds into the device vault at first open: binding marker (re-sealed), opt-in markers (→ flags), dead settings.vsf (deleted) — and the loose files leave the census.
    #[test]
    fn loose_files_fold_into_the_device_vault() {
        let _g = serial();
        isolate_test_storage();
        let secret = [0x5Cu8; 32];
        let dir = photon_config_dir().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let key = blake3::derive_key("photon.device_binding.v0", &secret);
        std::fs::write(
            dir.join("device_binding.vsf"),
            kete::encrypt_bytes(&[0x77u8; 32], &key).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("unattended_reboot"), b"operator opt-in").unwrap();
        std::fs::write(dir.join("settings.vsf"), b"dead-knobs").unwrap();
        install_device_secret(secret);
        // Asserts go thru the held Arc, not the global accessors — a parallel test may rebind the process-wide secret mid-flight; the vault itself is immutable truth.
        let v = device_vault().unwrap();
        assert_eq!(v.read_device("binding/party").unwrap(), Some(vec![0x77u8; 32]));
        assert!(matches!(v.read_device("flags/unattended_reboot"), Ok(Some(_))));
        assert!(!dir.join("device_binding.vsf").exists(), "binding file still in the census");
        assert!(!dir.join("unattended_reboot").exists(), "marker file still in the census");
        assert!(!dir.join("settings.vsf").exists(), "dead settings file still in the census");
    }

    /// THE census contract: after a session opens over a full file-era sprawl (legacy vault + strays + blobs), the config tree holds EXACTLY the device-vault ring per dir — everything else absorbed or parked as a `.legacy` backup in the old parking lot.
    #[test]
    fn end_state_census_is_the_vault_alone() {
        let _g = serial();
        isolate_test_storage();
        let identity = [0x7Au8; 32];
        let vault_seed = [0x7Bu8; 32];
        let secret = [0x7Cu8; 32];
        // Full sprawl: legacy vault with history, loose binding + marker + a sealed blob file.
        {
            let legacy = FlatStorage::new(LEGACY_APP, vault_seed, secret).unwrap();
            legacy.write("rows/genesis", b"the first message ever sent over FGTW").unwrap();
        }
        let cfg = photon_config_dir().unwrap();
        std::fs::create_dir_all(cfg.join("blobs")).unwrap();
        let plain = b"attached bytes".to_vec();
        let hash = *blake3::hash(&plain).as_bytes();
        std::fs::write(
            cfg.join("blobs/aa.bin"),
            kete::encrypt_bytes(&plain, &blob_seal_key(&identity)).unwrap(),
        )
        .unwrap();
        std::fs::write(cfg.join("unattended_reboot"), b"on").unwrap();
        std::fs::write(cfg.join("settings.vsf"), b"dead").unwrap();

        let vault = open_session_vault(identity, vault_seed, secret).unwrap();
        assert_eq!(vault.read("rows/genesis").unwrap(), Some(b"the first message ever sent over FGTW".to_vec()));
        assert_eq!(blob_load(&identity, &hash), Some(plain));

        // Census: the strays and the blobs dir are gone from the config dir.
        let leftovers: Vec<String> = std::fs::read_dir(&cfg)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "photon.log.vsf" && n != "photon.crash.txt")
            .collect();
        assert!(leftovers.is_empty(), "config-dir strays survived: {:?}", leftovers);
        // The device-vault rings exist at both mirror paths. (Per-dir "exactly one file" isn't assertable here — the test root is shared by the whole parallel suite, so sibling tests' vaults coexist; the field census is the two rings.)
        let device_name = tohu::device_vault_path_name(APP.id, &secret);
        let ring_paths = kete::vault_ring_paths(APP, &vault_seed, &secret).unwrap();
        for p in &ring_paths {
            let ring = p.parent().unwrap().join(format!("{}.vsf", device_name));
            assert!(ring.exists(), "device ring missing: {}", ring.display());
        }
        let legacy_paths = kete::vault_ring_paths(LEGACY_APP, &vault_seed, &secret).unwrap();
        for p in &legacy_paths {
            assert!(!p.exists());
            assert!(p.with_extension("vsf.legacy").exists());
        }
    }

    /// THE no-data-loss contract: a legacy per-identity vault's entries — string keys and raw addresses alike — survive verbatim into the device vault on the first open_session_vault, the legacy rings park as `.legacy` backups, and a second open changes nothing.
    #[test]
    fn open_session_vault_migrates_legacy_in_place() {
        let _g = serial();
        isolate_test_storage();
        let vault_seed = [0x4Au8; 32];
        let device_secret = [0x4Bu8; 32];
        {
            let legacy = FlatStorage::new(LEGACY_APP, vault_seed, device_secret).unwrap();
            legacy.write("contacts/index", b"Emma,Nick").unwrap();
            let first_message_addr = vault_key("rows", &[0x11u8; 32]);
            legacy.write_addr(&first_message_addr, b"the first message ever sent over FGTW").unwrap();
        }
        let vault = open_session_vault([0x49u8; 32], vault_seed, device_secret).unwrap();
        assert_eq!(vault.read("contacts/index").unwrap(), Some(b"Emma,Nick".to_vec()));
        let first_message_addr = vault_key("rows", &[0x11u8; 32]);
        assert_eq!(
            vault.read_addr(&first_message_addr).unwrap(),
            Some(b"the first message ever sent over FGTW".to_vec())
        );
        // The legacy rings are OUT of the census — parked as backups, never deleted.
        let legacy_paths = kete::vault_ring_paths(LEGACY_APP, &vault_seed, &device_secret).unwrap();
        for p in &legacy_paths {
            assert!(!p.exists(), "legacy ring still in the census: {}", p.display());
            assert!(p.with_extension("vsf.legacy").exists(), "backup missing for {}", p.display());
        }
        // Idempotent: a re-open (same session, next launch) is a no-op that still reads everything.
        drop(vault);
        let again = open_session_vault([0x49u8; 32], vault_seed, device_secret).unwrap();
        assert_eq!(again.read("contacts/index").unwrap(), Some(b"Emma,Nick".to_vec()));
    }
}
