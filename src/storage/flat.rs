//! Flat opaque storage — manifestus Vault backend (spine ring + plow tract + COW HAMT over mirrored files).
//!
//! Two mirror files per (handle, device), named via passless-key v0 so one directory holds many users' vaults under opaque 43-char names: ring 0 at the XDG config path, ring 1 at the XDG data path. Geometry lives in the vault's own spine entries — the filesystem is a witness, never the authority.
//!
//! Crash model: power loss at ANY byte boundary is normal operation. Every block self-validates; the committed generation defines exactly which writes exist; the rollback fence keeps the last 4 generations fully restorable. Open replicates divergent mirrors block-level (verified, never a file copy) before composing them.
//!
//! Public API (`new`, `read`, `write`, `delete`, `degraded`) unchanged thru three backends now — callers in `contacts.rs`, `friendship.rs`, etc. have never known.

use std::path::PathBuf;
use std::sync::Mutex;

use manifestus::{verified_replicate, FileDev, Mirror, Vault, HOST_RING_LOG2};

use crate::storage::{decrypt_bytes, encrypt_bytes};

/// Per-user vault dir under XDG config / data. Filename inside is derived per (handle, device) via passless-key so the same dir can hold multiple users' vaults without collision and without leaking which handles bind to this device — anyone listing the dir sees only opaque base64url names.
const VAULT_DIR: &str = "Photon";
/// Shadow-ring filename suffix used when XDG config_dir == data_dir (macOS): both rings live in the same directory but with distinct names. Keeps the dual-ring invariant intact across platforms.
const VAULT_SHADOW_SUFFIX: &str = ".shadow";
/// passless-key app identifier baked into every Photon build. Lumis et al. will use their own.
const PASSLESS_APP_ID: &str = "photon";
/// Initial tract size: 4096 blocks = 16MB per mirror file. Deliberately below the 64MB spec default — a messenger's vault starts small and growth is one fallocate + commit.
const PHOTON_TRACT_BLOCKS: u64 = 4096;

// ============================================================================ Error ======================================================================

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Crypto(String),
    Parse(String),
    /// Vault-layer error from the manifestus backend.
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

impl From<manifestus::Error> for StorageError {
    fn from(e: manifestus::Error) -> Self {
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

/// All Photon disk I/O goes thru this struct. Initialized once at auth with the handle + device_secret; the dual-ring vault is opened (or formatted) during construction. Callers only see logical keys; vault internals + per-key encryption are managed below.
pub struct FlatStorage {
    /// Frozen v0 handle_seed (`tohu::handle_seed(handle)`). Used as the per-key encryption derivation root. Decoupled from `ihi::handle_to_hash` so future ihi changes (NFC fixes, Huffman tweaks) don't orphan local vaults.
    handle_seed: [u8; 32],
    device_secret: [u8; 32],
    /// The manifestus engine. Mutex chosen over RefCell so future multi-threaded callers Just Work; cost is negligible in the single-threaded case.
    vault: Mutex<Vault<FileDev, FileDev>>,
    /// Mirrors diverged at open and were replicated back into agreement — surface to the UI banner alongside live degradation.
    healed_at_open: bool,
}

impl FlatStorage {
    /// Initialize storage. Called once at auth time. Derives the per-handle vault filename and anchor key via `passless-key` v0, opens the dual rings at `{config_dir,data_dir}/Photon/<base64url>.vsf`. Same `(handle, device)` always reproduces the same vault path and key.
    pub fn new(handle: &str, device_secret: [u8; 32]) -> Result<Self, StorageError> {
        let handle_seed = tohu::handle_seed(handle);
        let filename = tohu::vault_path_name(PASSLESS_APP_ID, &handle_seed, &device_secret);
        let paths = vault_paths(&filename)?;

        for p in &paths {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let blocks = (1u64 << HOST_RING_LOG2) + PHOTON_TRACT_BLOCKS;
        let mut a = FileDev::create(&paths[0], blocks)?;
        let mut b = FileDev::create(&paths[1], blocks)?;

        // Converge divergent mirrors (stale restore, missed writes, fresh second file) BEFORE composing them — block-level, verified, idempotent, never a file copy.
        let healed = verified_replicate(&mut a, &mut b, HOST_RING_LOG2)?;
        let healed_at_open = healed != manifestus::Replicated::default();
        if healed_at_open {
            crate::log(&format!(
                "STORAGE: mirrors diverged at open — replicated {} spine + {} tract blocks",
                healed.spine_copied, healed.tract_copied
            ));
        }

        let vault = Vault::open(Mirror::new(a, b), HOST_RING_LOG2, unix_now())?;
        Ok(Self {
            handle_seed,
            device_secret,
            vault: Mutex::new(vault),
            healed_at_open,
        })
    }

    /// Write data under a logical key. Encrypts with per-key ChaCha20-Poly1305; durable on return (a spine generation references the new state on at least one verified mirror).
    pub fn write(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let enc_key = self.derive_enc_key(key);
        let ciphertext = encrypt_bytes(data, &enc_key).map_err(StorageError::Crypto)?;
        let entry_key = derive_entry_key(key);

        let mut vault = self
            .vault
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        vault.put(&entry_key, &ciphertext, unix_now())?;
        Ok(())
    }

    /// Read the value for a logical key, decrypting with the per-key ChaCha20-Poly1305 key. Returns `None` if the key isn't in the vault (logically "file not found"). Every block on the path is hash-verified.
    pub fn read(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let entry_key = derive_entry_key(key);
        let mut vault = self
            .vault
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        let Some(ciphertext) = vault.get(&entry_key)? else {
            return Ok(None);
        };
        let enc_key = self.derive_enc_key(key);
        let plaintext = decrypt_bytes(&ciphertext, &enc_key).map_err(StorageError::Crypto)?;
        Ok(Some(plaintext))
    }

    /// Remove a logical key. Fast delete per the spec: the blocks are zeroed on both mirrors immediately; the plow reaps the slots.
    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        let entry_key = derive_entry_key(key);
        let mut vault = self
            .vault
            .lock()
            .map_err(|_| StorageError::Vault("FlatStorage mutex poisoned".to_string()))?;
        vault.delete(&entry_key, unix_now())?;
        Ok(())
    }

    /// True if the mirrors diverged at open (and were healed) or a mirror died mid-session. UI reads this to render the persistent degraded banner.
    pub fn degraded(&self) -> bool {
        self.healed_at_open
            || self
                .vault
                .lock()
                .map(|mut v| v.degraded())
                .unwrap_or(true)
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

/// Map a logical key string to the fixed 32-byte entry address manifestus wants. Pure function of the key — the vault file is already per-(handle, device, app) so no identity material needs mixing here.
fn derive_entry_key(key: &str) -> [u8; 32] {
    blake3::derive_key("photon.storage.entry.v0", key.as_bytes())
}

/// Caller-clock timestamp for record metadata (`created_at`). Unix seconds — manifestus never interprets it; it exists for GC policy and debugging.
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
