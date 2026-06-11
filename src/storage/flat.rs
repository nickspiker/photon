//! Flat opaque storage — dual-ring `vsf_db` backend.
//!
//! Two equal vault files, each an append-only `vsf_db::Store`: ring 0 at the XDG config path (`~/.config/Photon/<derived>.vsf` on Linux) and ring 1 at the XDG data path (`~/.local/share/Photon/<derived>.vsf`). The filename is derived per (handle, device) via `passless-key` v0, so one directory holds many users' vaults under opaque names. Both files store the same logical state; neither is "primary".
//!
//! Crash model: power loss at ANY byte boundary is normal operation. Every record self-validates (per-record HMAC + length-redundant header), every write fsyncs, and open scans + silently truncates any torn tail. The dual-ring layer (`vsf_db::DualStore`) heals divergence between rings by anchor_seq comparison — a ring that missed writes (crash between ring writes, deleted file, torn tail) is rebuilt from the survivor on next open.
//!
//! Failure handling:
//! - On open, a missing/torn/corrupt ring is rebuilt from the other; hard repairs set `degraded` so the UI can show a persistent banner.
//! - On write, both rings are written. If one fails the survivor keeps the session alive and `degraded` flips.
//! - If both rings are unreadable, `new` errors — the vault is genuinely unrecoverable on this device.
//!
//! Public API (`new`, `read`, `write`, `delete`, `degraded`) unchanged from the ferros_vault-backed version — callers in `contacts.rs`, `friendship.rs`, etc. don't know the backend moved.

use std::path::PathBuf;
use std::sync::Mutex;

use vsf_db::DualStore;

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
    /// Vault-layer error from the vsf_db backend.
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

impl From<vsf_db::Error> for StorageError {
    fn from(e: vsf_db::Error) -> Self {
        StorageError::Vault(e.to_string())
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

// ============================================================================ FlatStorage ================================================================

/// All Photon disk I/O goes through this struct. Initialized once at auth with the handle + device_secret; the dual-ring vault is opened (or formatted) during construction. Callers only see logical keys; vault internals + per-key encryption are managed below.
pub struct FlatStorage {
    /// Frozen v0 handle_seed (`passless_key::handle_seed(handle)`). Used as the per-key encryption derivation root. Decoupled from `ihi::handle_to_hash` so future ihi changes (NFC fixes, Huffman tweaks) don't orphan local vaults.
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
        let anchor_key = passless_key::vault_anchor_key(PASSLESS_APP_ID, &handle_seed, &device_secret);

        let dual = DualStore::open_or_create(paths, &anchor_key)?;
        if dual.degraded() {
            crate::log("STORAGE: vault opened degraded — a ring needed repair");
        }

        Ok(Self {
            handle_seed,
            device_secret,
            dual: Mutex::new(dual),
        })
    }

    /// Write data under a logical key. Encrypts with per-key ChaCha20-Poly1305, appends a self-validating record to BOTH rings. At-least-one-ring durability semantics: succeeds as long as at least one ring landed the write; sets degraded if the other failed.
    pub fn write(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let enc_key = self.derive_enc_key(key);
        let ciphertext = encrypt_bytes(data, &enc_key).map_err(StorageError::Crypto)?;
        let entry_key = derive_entry_key(key);

        let mut dual = self
            .dual
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        dual.put(entry_key, &ciphertext, 0, None, unix_now())?;
        Ok(())
    }

    /// Read the value for a logical key, decrypting with the per-key ChaCha20-Poly1305 key. Returns `None` if the key isn't in the vault (logically "file not found"). Reads from the first healthy ring; both hold identical state under healthy conditions.
    pub fn read(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let entry_key = derive_entry_key(key);
        let mut dual = self
            .dual
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        let Some(ciphertext) = dual.get(&entry_key)? else {
            return Ok(None);
        };
        let enc_key = self.derive_enc_key(key);
        let plaintext = decrypt_bytes(&ciphertext, &enc_key).map_err(StorageError::Crypto)?;
        Ok(Some(plaintext))
    }

    /// Remove a logical key from the vault by writing a tombstone record to BOTH rings. The superseded bytes stay until compaction.
    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        let entry_key = derive_entry_key(key);
        let mut dual = self
            .dual
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        dual.delete(&entry_key, unix_now())?;
        Ok(())
    }

    /// True if any ring required repair on open this session or a write to one ring failed this session. UI reads this to render the persistent degraded banner.
    pub fn degraded(&self) -> bool {
        self.dual.lock().map(|d| d.degraded()).unwrap_or(true)
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

/// Map a logical key string to the fixed 32-byte entry address vsf_db wants. Pure function of the key — the vault file is already per-(handle, device, app) so no identity material needs mixing here.
fn derive_entry_key(key: &str) -> vsf_db::EntryKey {
    blake3::derive_key("photon.storage.entry.v0", key.as_bytes())
}

/// Caller-clock timestamp for record metadata (`created_at`). Unix seconds — vsf_db never interprets it; it exists for GC policy and debugging.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
