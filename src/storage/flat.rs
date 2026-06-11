//! Flat opaque storage — dual-ring `ferros_vault` host_file backend.
//!
//! Two equal vault files, each a complete VSF-wrapped vault: ring 0 at the XDG config path (`~/.config/photon.vsf` on Linux) and ring 1 at the XDG data path (`~/.local/share/photon.vsf` on Linux). Both files store the same logical state; neither is "primary" — write order is randomized per call so wear distributes evenly and a damaged-always-first-ring failure mode can't sneak in.
//!
//! Failure handling:
//! - On open, if one file is missing or corrupt, it's silently rebuilt from the other
//!   and `degraded` is set so the UI can show a persistent banner.
//! - If both files exist but disagree on `anchor_seq` (crashed mid-dual-write), the
//!   higher-seq file overwrites the lower. This is normal self-heal, not degraded.
//! - On write, both rings are written. If one fails the survivor is kept and the
//!   failed ring is dropped from the session; `degraded` flags it for the user.
//!
//! Public API (`new`, `read`, `write`, `delete`) signatures unchanged from the single-file version — callers in `contacts.rs`, `friendship.rs`, etc. don't know the backend went dual.

use std::path::PathBuf;
use std::sync::Mutex;

use ferros_vault::anchor::AnchorKey;
use ferros_vault::device::DeviceId;
use ferros_vault::host_file::{
    derive_anchor_key, FileDevice, FileStore, DEFAULT_PAYLOAD_CAPACITY, DEFAULT_RING_SIZE,
};
use ferros_vault::object::{Object, VsfType};
use ferros_vault::store::ObjectStore;
use rand::Rng;

use crate::storage::{decrypt_bytes, encrypt_bytes};

/// Per-user vault dir under XDG config / data. Filename inside is derived per (handle, device) via passless-key so the same dir can hold multiple users' vaults without collision and without leaking which handles bind to this device — anyone listing the dir sees only opaque base64url names.
const VAULT_DIR: &str = "Photon";
/// Shadow-ring filename suffix used when XDG config_dir == data_dir (macOS): both rings live in the same directory but with distinct names. Keeps the dual-ring invariant intact across platforms.
const VAULT_SHADOW_SUFFIX: &str = ".shadow";
/// passless-key app identifier baked into every Photon build. Lumis et al. will use their own.
const PASSLESS_APP_ID: &str = "photon";

// ============================================================================ Error ======================================================================

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Crypto(String),
    Parse(String),
    /// Vault-layer error from ferros_vault's host_file backend.
    Vault(String),
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}

impl From<String> for StorageError {
    fn from(s: String) -> Self {
        StorageError::Vault(s)
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "IO: {}", e),
            StorageError::Crypto(s) => write!(f, "Crypto: {}", s),
            StorageError::Parse(s) => write!(f, "Parse: {}", s),
            StorageError::Vault(s) => write!(f, "Vault: {}", s),
        }
    }
}

// ============================================================================ DualStore ==================================================================

/// Two FileStore instances backing the same logical vault. Both files store identical state under healthy conditions. A ring is `None` only when it failed mid-session; next process start re-opens both via `DualStore::open_or_create` which repairs.
struct DualStore {
    rings: [Option<FileStore>; 2],
    /// Set when one ring needed repair on open OR a write failed to one ring this session. Sticky for the session — clears on next process restart after both files are healthy.
    degraded: bool,
}

impl DualStore {
    /// Open or create both rings. Robust to any of: missing files, files that exist but can't be opened (permission denied, corrupt envelope, bad HMAC, etc.).
    ///
    /// Cases handled, in order:
    /// 1. Neither file exists → format ring 0, copy to ring 1. If the copy fails, ring 1 stays None + degraded.
    /// 2. Both opened cleanly → if anchor_seqs differ, silent self-heal (lower copies from higher). Healed silently; no degraded flag.
    /// 3. One opens, the other doesn't (missing, perm-denied, or corrupt) → attempt repair by copying the survivor's file. Repair failures don't error — the surviving ring keeps running, degraded flag flips on.
    /// 4. Neither opens → hard error (vault is unrecoverable).
    fn open_or_create(
        paths: [PathBuf; 2],
        anchor_key: AnchorKey,
        device_id: DeviceId,
    ) -> Result<Self, StorageError> {
        for p in &paths {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        let exists = [paths[0].exists(), paths[1].exists()];

        // Fresh setup.
        if !exists[0] && !exists[1] {
            let s0 = format_fresh(&paths[0], anchor_key, device_id)?;
            drop(s0);
            let r0 = Some(open_existing(&paths[0], anchor_key, device_id)?);
            let r1 = match std::fs::copy(&paths[0], &paths[1]) {
                Ok(_) => open_existing(&paths[1], anchor_key, device_id).ok(),
                Err(e) => {
                    crate::log(&format!(
                        "STORAGE: ring 1 seed copy to {:?} failed: {}",
                        paths[1], e
                    ));
                    None
                }
            };
            let degraded = r1.is_none();
            return Ok(Self {
                rings: [r0, r1],
                degraded,
            });
        }

        // At least one file present. Try to open both — failure here is "unreadable for any reason" (missing, perm-denied, corrupt VSF wrapper, bad HMAC, etc.).
        let mut r0 = open_existing(&paths[0], anchor_key, device_id).ok();
        let mut r1 = open_existing(&paths[1], anchor_key, device_id).ok();

        if r0.is_none() && r1.is_none() {
            return Err(StorageError::Vault(format!(
                "both rings unreadable: ring0={:?}; ring1={:?}",
                paths[0], paths[1]
            )));
        }

        let mut degraded = false;

        // Asymmetric repair: copy from the survivor to the broken ring.
        if r0.is_some() && r1.is_none() {
            crate::log(&format!(
                "STORAGE: ring 1 ({:?}) unreadable — attempting repair from ring 0",
                paths[1]
            ));
            r1 = repair_ring(&paths[0], &paths[1], anchor_key, device_id);
            degraded = true;
        } else if r0.is_none() && r1.is_some() {
            crate::log(&format!(
                "STORAGE: ring 0 ({:?}) unreadable — attempting repair from ring 1",
                paths[0]
            ));
            r0 = repair_ring(&paths[1], &paths[0], anchor_key, device_id);
            degraded = true;
        }

        // Both open: check seq parity. Mismatched seqs are normal crash-during- dual-write recovery, silent self-heal (no degraded flag).
        if let (Some(a), Some(b)) = (&r0, &r1) {
            let seq0 = a.anchor().anchor_seq;
            let seq1 = b.anchor().anchor_seq;
            if seq0 != seq1 {
                let (src_idx, dst_idx) = if seq0 > seq1 { (0, 1) } else { (1, 0) };
                crate::log(&format!(
                    "STORAGE: silent self-heal — ring {} (seq {}) → ring {} (seq {})",
                    src_idx,
                    if src_idx == 0 { seq0 } else { seq1 },
                    dst_idx,
                    if dst_idx == 0 { seq0 } else { seq1 },
                ));
                // Drop the loser handle before overwriting its file.
                if dst_idx == 0 {
                    r0 = None;
                } else {
                    r1 = None;
                }
                let healed = repair_ring(&paths[src_idx], &paths[dst_idx], anchor_key, device_id);
                if dst_idx == 0 {
                    r0 = healed;
                } else {
                    r1 = healed;
                }
                // If self-heal failed, the surviving ring still has the truth; flag degraded.
                if r0.is_none() || r1.is_none() {
                    degraded = true;
                }
            }
        }

        Ok(Self {
            rings: [r0, r1],
            degraded,
        })
    }

    /// Read root_commit dict from the first available ring; both contain identical state under healthy conditions.
    fn load_root_commit(&self) -> Result<ferros_vault::host_file::RootCommit, StorageError> {
        for r in &self.rings {
            if let Some(store) = r {
                return store
                    .load_root_commit()
                    .map_err(|e| StorageError::Vault(format!("load_root_commit: {:?}", e)));
            }
        }
        Err(StorageError::Vault("no readable ring".into()))
    }

    fn get(&self, hash: &ferros_vault::hash::ObjectHash) -> Result<Object, StorageError> {
        for r in &self.rings {
            if let Some(store) = r {
                return store
                    .get(hash)
                    .map_err(|e| StorageError::Vault(format!("get: {:?}", e)));
            }
        }
        Err(StorageError::Vault("no readable ring".into()))
    }
}

/// Format a fresh vault at `path` and return the open FileStore.
fn format_fresh(
    path: &std::path::Path,
    anchor_key: AnchorKey,
    device_id: DeviceId,
) -> Result<FileStore, StorageError> {
    let device = FileDevice::create(path, device_id, DEFAULT_PAYLOAD_CAPACITY)
        .map_err(|e| StorageError::Vault(format!("FileDevice::create {:?}: {:?}", path, e)))?;
    FileStore::format(device, anchor_key, DEFAULT_PAYLOAD_CAPACITY, DEFAULT_RING_SIZE)
        .map_err(|e| StorageError::Vault(format!("FileStore::format {:?}: {:?}", path, e)))
}

/// Open an existing vault file and return the FileStore. Wraps device + store errors into a single string for the caller's error path (we don't need to discriminate further — corrupt is corrupt, the dual-ring layer just rebuilds from the other).
fn open_existing(
    path: &std::path::Path,
    anchor_key: AnchorKey,
    device_id: DeviceId,
) -> Result<FileStore, String> {
    let device = FileDevice::open(path, device_id)
        .map_err(|e| format!("FileDevice::open {:?}: {:?}", path, e))?;
    FileStore::open(device, anchor_key)
        .map_err(|e| format!("FileStore::open {:?}: {:?}", path, e))
}

/// Copy `src` over `dst` and try to open the result. Returns None if either step fails (most commonly: dst path is unwritable, or src is unreadable mid-copy). Failure logged at warn level — caller decides whether to flip degraded.
fn repair_ring(
    src: &std::path::Path,
    dst: &std::path::Path,
    anchor_key: AnchorKey,
    device_id: DeviceId,
) -> Option<FileStore> {
    match std::fs::copy(src, dst) {
        Ok(_) => match open_existing(dst, anchor_key, device_id) {
            Ok(s) => Some(s),
            Err(e) => {
                crate::log(&format!("STORAGE: repair reopen {:?} failed: {}", dst, e));
                None
            }
        },
        Err(e) => {
            crate::log(&format!(
                "STORAGE: repair copy {:?} → {:?} failed: {}",
                src, dst, e
            ));
            None
        }
    }
}

// ============================================================================ FlatStorage ================================================================

/// All Photon disk I/O goes through this struct. Initialized once at auth with identity_seed + device_secret; the dual-ring vault is opened (or formatted) during construction. Callers only see logical keys; vault internals + per-key encryption are managed below.
pub struct FlatStorage {
    /// Frozen v0 handle_seed (`passless_key::handle_seed(handle)`). Used as the per-key encryption derivation root and as the identity input to `derive_anchor_key`. Decoupled from `ihi::handle_to_hash` so future ihi changes (NFC fixes, Huffman tweaks) don't orphan local vaults.
    handle_seed: [u8; 32],
    device_secret: [u8; 32],
    /// Dual-ring backing store. Mutex chosen over RefCell so future multi-threaded callers Just Work; cost is negligible in the single-threaded case.
    dual: Mutex<DualStore>,
}

impl FlatStorage {
    /// Initialize storage. Called once at auth time. Derives the per-handle vault filename and anchor key via `passless-key` v0, opens the dual rings at `{config_dir,data_dir}/Photon/<base64url>.vsf`. Same `(handle, device)` always reproduces the same vault path and key.
    pub fn new(handle: &str, device_secret: [u8; 32]) -> Result<Self, StorageError> {
        let handle_seed = passless_key::handle_seed(handle);
        let filename = passless_key::vault_path_name(PASSLESS_APP_ID, &handle_seed, &device_secret);
        let paths = vault_paths(&filename)?;
        let anchor_key = derive_anchor_key(&handle_seed, &device_secret);
        let device_id = device_id_from_secret(&device_secret);

        let dual = DualStore::open_or_create(paths, anchor_key, device_id)?;

        Ok(Self {
            handle_seed,
            device_secret,
            dual: Mutex::new(dual),
        })
    }

    /// Write data under a logical key. Encrypts with per-key ChaCha20-Poly1305, stores as a content-addressed object in BOTH rings, updates each ring's root_commit dict. At-least-one-ring durability semantics: succeeds as long as at least one ring landed the write; sets degraded if the other failed.
    pub fn write(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let enc_key = self.derive_enc_key(key);
        let ciphertext = encrypt_bytes(data, &enc_key).map_err(StorageError::Crypto)?;
        let obj = build_blob_object(&ciphertext);
        let hash = obj.meta.hash;

        let mut dual = self
            .dual
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;

        // Put the object into each healthy ring before committing the root. We do the put + commit per-ring because each FileStore tracks its own index + anchor_seq; sharing object payloads between two FileStores would require ferros_vault changes we're not making in v1.
        let first: usize = rand::thread_rng().gen_range(0..2);
        let order = [first, 1 - first];
        let mut any_ok = false;
        for &i in &order {
            let Some(store) = dual.rings[i].as_mut() else {
                continue;
            };
            match write_one(store, key, obj.clone(), hash) {
                Ok(()) => any_ok = true,
                Err(e) => {
                    crate::log(&format!("STORAGE: write to ring {} failed: {}", i, e));
                    dual.rings[i] = None;
                    dual.degraded = true;
                }
            }
        }

        if any_ok {
            Ok(())
        } else {
            Err(StorageError::Vault(
                "both rings failed to accept write".into(),
            ))
        }
    }

    /// Read the value for a logical key, decrypting with the per-key ChaCha20-Poly1305 key. Returns `None` if the key isn't in the vault's root commit (logically "file not found"). Reads from the first healthy ring; both hold identical state under healthy conditions.
    pub fn read(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let dual = self
            .dual
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        let rc = dual.load_root_commit()?;
        let hash = match rc.get(key) {
            Some(h) => *h,
            None => return Ok(None),
        };
        let obj = dual.get(&hash)?;
        let enc_key = self.derive_enc_key(key);
        let plaintext = decrypt_bytes(&obj.content, &enc_key).map_err(StorageError::Crypto)?;
        Ok(Some(plaintext))
    }

    /// Remove a logical key from the vault. The underlying object stays until compact (Phase 2 GC); only the root_commit dict entry goes. Removed from BOTH rings under healthy conditions.
    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        let mut dual = self
            .dual
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;

        let first: usize = rand::thread_rng().gen_range(0..2);
        let order = [first, 1 - first];
        let mut any_ok = false;
        for &i in &order {
            let Some(store) = dual.rings[i].as_mut() else {
                continue;
            };
            match delete_one(store, key) {
                Ok(()) => any_ok = true,
                Err(e) => {
                    crate::log(&format!("STORAGE: delete on ring {} failed: {}", i, e));
                    dual.rings[i] = None;
                    dual.degraded = true;
                }
            }
        }

        if any_ok {
            Ok(())
        } else {
            Err(StorageError::Vault(
                "both rings failed to accept delete".into(),
            ))
        }
    }

    /// True if any ring required repair on open this session or a write to one ring failed this session. UI reads this to render the persistent degraded banner.
    pub fn degraded(&self) -> bool {
        self.dual.lock().map(|d| d.degraded).unwrap_or(true)
    }

    // ======================================================================== Internal key derivation ================================================

    fn derive_enc_key(&self, key: &str) -> [u8; 32] {
        let context = [
            key.as_bytes(),
            self.handle_seed.as_slice(),
            self.device_secret.as_slice(),
        ]
        .concat();
        blake3::derive_key("photon.storage.encryption.v0", &context)
    }
}

/// Android vault dir override — populated once at startup from [`crate::platform::jni_android::NetworkContext::new`] via [`set_android_vault_dirs`]. `dirs::config_dir()` doesn't resolve on Android (no XDG), and even when it did the right scope is the app-private dirs Java side hands us. Tuple is `(primary, shadow)` — primary points at `context.filesDir`, shadow at `context.getExternalFilesDir(null)` (empty when external storage unavailable, in which case [`vault_paths`] falls back to a shadow-suffix file inside primary).
#[cfg(target_os = "android")]
static ANDROID_VAULT_DIRS: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Inject the Android dual-ring vault directories. Must be called before any storage operation (`new`, `read`, `write`, `delete`) — the JNI shim wires it from `nativeNetworkInit` so the dirs are set before HandleQuery's storage init fires post-attestation.
#[cfg(target_os = "android")]
pub fn set_android_vault_dirs(primary: String, shadow: String) {
    if let Ok(mut g) = ANDROID_VAULT_DIRS.lock() {
        *g = Some((primary, shadow));
    }
}

/// Resolve the two ring paths for the given per-handle filename. Files live under `<config_dir>/Photon/<filename>.vsf` and `<data_dir>/Photon/<filename>.vsf`. On Linux + Windows the XDG split gives meaningfully different directories. On macOS `config_dir()` and `data_dir()` collide; in that case the shadow shares the directory with `<filename>.shadow.vsf` — worse for accidental-dir-deletion resistance but keeps the dual-write invariant. On Android the dirs come from the JNI shim ([`set_android_vault_dirs`]) — `filesDir` for primary, `getExternalFilesDir(null)` for shadow; the latter empties to the shadow-suffix fallback if external storage wasn't available at startup.
fn vault_paths(filename: &str) -> Result<[PathBuf; 2], StorageError> {
    let primary_name = format!("{}.vsf", filename);
    let shadow_name = format!("{}{}.vsf", filename, VAULT_SHADOW_SUFFIX);

    #[cfg(target_os = "android")]
    {
        let dirs = ANDROID_VAULT_DIRS
            .lock()
            .map_err(|e| StorageError::Io(std::io::Error::other(format!("vault-dir lock: {}", e))))?
            .clone()
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Android vault dirs not set — JNI shim must call set_android_vault_dirs",
                ))
            })?;
        let primary_dir = PathBuf::from(&dirs.0).join(VAULT_DIR);
        let shadow_dir = if dirs.1.is_empty() {
            primary_dir.clone()
        } else {
            PathBuf::from(&dirs.1).join(VAULT_DIR)
        };
        let primary = primary_dir.join(&primary_name);
        let shadow = if primary_dir == shadow_dir {
            shadow_dir.join(&shadow_name)
        } else {
            shadow_dir.join(&primary_name)
        };
        return Ok([primary, shadow]);
    }

    #[cfg(not(target_os = "android"))]
    {
        let primary_dir = dirs::config_dir()
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "config directory not found",
                ))
            })?
            .join(VAULT_DIR);
        let shadow_dir = dirs::data_dir()
            .unwrap_or_else(|| primary_dir.clone())
            .join(VAULT_DIR);

        let primary = primary_dir.join(&primary_name);
        let shadow = if primary_dir == shadow_dir {
            shadow_dir.join(&shadow_name)
        } else {
            shadow_dir.join(&primary_name)
        };

        Ok([primary, shadow])
    }
}

/// Build a DeviceId from the device_secret. Same derivation as before — both rings share the same DeviceId since they're logically the same device.
fn device_id_from_secret(device_secret: &[u8; 32]) -> DeviceId {
    let h = blake3::derive_key("photon.vault.device_id.v0", device_secret);
    let mut id = [0u8; 16];
    id.copy_from_slice(&h[..16]);
    DeviceId(id)
}

/// Put + insert + commit_root on a single store. Errors stringified for the dual-ring layer's per-ring tracking.
fn write_one(
    store: &mut FileStore,
    key: &str,
    obj: Object,
    hash: ferros_vault::hash::ObjectHash,
) -> Result<(), String> {
    store
        .put(obj)
        .map_err(|e| format!("put: {:?}", e))?;
    let mut rc = store
        .load_root_commit()
        .map_err(|e| format!("load_root_commit: {:?}", e))?;
    rc.insert(key.to_string(), hash);
    store
        .commit_root(&rc)
        .map_err(|e| format!("commit_root: {:?}", e))?;
    Ok(())
}

/// Remove + commit_root on a single store. No-op if the key wasn't present.
fn delete_one(store: &mut FileStore, key: &str) -> Result<(), String> {
    let mut rc = store
        .load_root_commit()
        .map_err(|e| format!("load_root_commit: {:?}", e))?;
    if rc.remove(key).is_some() {
        store
            .commit_root(&rc)
            .map_err(|e| format!("commit_root: {:?}", e))?;
    }
    Ok(())
}

/// Build a Blob-typed Object whose content is the ciphertext. The vault expects the meta.hash field to equal `blake3(content)` (raw, no salt/name/domain mixing) — FileStore verifies this on put and uses the hash as the object's address.
fn build_blob_object(ciphertext: &[u8]) -> Object {
    use ferros_vault::object::ObjectMeta;
    let hash = ferros_vault::hash::ObjectHash(*blake3::hash(ciphertext).as_bytes());
    Object {
        meta: ObjectMeta {
            hash,
            vsf_type: VsfType::Blob,
            name: Vec::new(),
            domain: Vec::new(),
            content_len: ciphertext.len() as u64,
            generation: 0,
            parent: None,
        },
        content: ciphertext.to_vec(),
    }
}
