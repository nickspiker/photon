//! Flat opaque storage — all Photon disk I/O goes through here, but the storage backend is now `ferros_vault`'s `host-file` layer. One file on disk: `~/.config/photon/photon.vsf`. Everything Photon persists lives inside it: contacts, messages, friendship chains, blob refs.
//!
//! Public API (`new`, `read`, `write`, `delete`) is the same as the previous FAF-per-file implementation. Callers in `contacts.rs`, `friendship.rs`, etc. don't know the backend changed.
//!
//! Internals:
//!   anchor_key = `ferros_vault::host_file::derive_anchor_key(identity_seed, device_secret)`
//!   enc_key(logical_key) = `blake3_kdf("photon.storage.encryption.v0", logical_key || identity_seed || device_secret)`
//!   value bytes are ChaCha20-Poly1305-encrypted with the per-key enc_key BEFORE going into the vault as content-addressed objects. So objects on disk are ciphertext; the root_commit dict maps logical_key → hash-of-ciphertext.
//!
//! Concurrency: FileStore lives behind a `std::sync::Mutex` so the public `&self` methods can mutate it. Photon is single-threaded today but the Mutex costs nothing and future-proofs against concurrent access.

use std::sync::Mutex;

use ferros_vault::host_file::{
    derive_anchor_key, FileDevice, FileStore, DEFAULT_PAYLOAD_CAPACITY, DEFAULT_RING_SIZE,
};
use ferros_vault::object::{Object, VsfType};
use ferros_vault::store::ObjectStore;

use crate::storage::{decrypt_bytes, encrypt_bytes};

const VAULT_FILENAME: &str = "photon.vsf";

// ============================================================================
// Error ============================================================================

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

// ============================================================================
// FlatStorage ============================================================================

/// All Photon disk I/O goes through this struct. Initialized once at auth with identity_seed + device_secret. Callers only see logical keys; vault internals + per-key encryption are managed below.
pub struct FlatStorage {
    /// Photon's two persistent roots — kept for per-key encryption derivation. The vault's anchor_key is derived from these but stored separately on the FileStore.
    identity_seed: [u8; 32],
    device_secret: [u8; 32],
    /// The ferros_vault FileStore behind a Mutex for interior mutability. Mutex chosen over RefCell so future multi-threaded callers Just Work; cost is negligible in the single-threaded case.
    store: Mutex<FileStore>,
}

impl FlatStorage {
    /// Initialize storage. Called once at auth time. Opens `~/.config/photon.vsf` if it exists; formats a fresh vault if not. Treats `identity_seed + device_secret` as the keying material — same auth flow reproduces the same vault key.
    ///
    /// Note: the vault file lives directly under `~/.config/`, NOT inside a `~/.config/photon/` subdirectory. The whole point of the vault is that one file holds all Photon state; spawning a subdirectory just to hold it would leak the same metadata the vault is designed to hide. Avatars + the Windows log still live in a `~/.config/photon/` subdirectory for now (Phase 1F migrates them into the vault).
    pub fn new(identity_seed: [u8; 32], device_secret: [u8; 32]) -> Result<Self, StorageError> {
        let config_dir = dirs::config_dir().ok_or_else(|| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "config directory not found",
            ))
        })?;
        std::fs::create_dir_all(&config_dir)?;
        let vault_path = config_dir.join(VAULT_FILENAME);

        let anchor_key = derive_anchor_key(&identity_seed, &device_secret);

        let device_id = device_id_from_secret(&device_secret);

        let store = if vault_path.exists() {
            // Open existing vault.
            let device = FileDevice::open(&vault_path, device_id).map_err(|e| {
                StorageError::Vault(format!("FileDevice::open failed: {:?}", e))
            })?;
            FileStore::open(device, anchor_key).map_err(|e| {
                StorageError::Vault(format!("FileStore::open failed: {:?}", e))
            })?
        } else {
            // Format a fresh vault. Default capacity is 1 MiB which holds hundreds of contacts comfortably; growth happens via compact (Phase 2).
            let device = FileDevice::create(&vault_path, device_id, DEFAULT_PAYLOAD_CAPACITY * 2)
                .map_err(|e| {
                    StorageError::Vault(format!("FileDevice::create failed: {:?}", e))
                })?;
            FileStore::format(
                device,
                anchor_key,
                DEFAULT_PAYLOAD_CAPACITY,
                DEFAULT_RING_SIZE,
            )
            .map_err(|e| StorageError::Vault(format!("FileStore::format failed: {:?}", e)))?
        };

        Ok(Self {
            identity_seed,
            device_secret,
            store: Mutex::new(store),
        })
    }

    /// Write data under a logical key. Encrypts with per-key ChaCha20-Poly1305, stores as a content-addressed object, updates the root_commit dict to map `key → content_hash`, commits a new anchor. Atomic at the anchor-write boundary: either the new state is fully durable or the prior state remains intact.
    pub fn write(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let enc_key = self.derive_enc_key(key);
        let ciphertext = encrypt_bytes(data, &enc_key).map_err(StorageError::Crypto)?;

        let mut store = self.store.lock().map_err(|_| {
            StorageError::Vault("FlatStorage mutex poisoned".to_string())
        })?;

        let obj = build_blob_object(&ciphertext);
        let hash = store
            .put(obj)
            .map_err(|e| StorageError::Vault(format!("FileStore::put: {:?}", e)))?;

        let mut rc = store
            .load_root_commit()
            .map_err(|e| StorageError::Vault(format!("load_root_commit: {:?}", e)))?;
        rc.insert(key.to_string(), hash);
        store
            .commit_root(&rc)
            .map_err(|e| StorageError::Vault(format!("commit_root: {:?}", e)))?;

        Ok(())
    }

    /// Read the value for a logical key, decrypting with the per-key ChaCha20-Poly1305 key. Returns `None` if the key isn't in the vault's root commit (logically "file not found").
    pub fn read(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let store = self.store.lock().map_err(|_| {
            StorageError::Vault("FlatStorage mutex poisoned".to_string())
        })?;
        let rc = store
            .load_root_commit()
            .map_err(|e| StorageError::Vault(format!("load_root_commit: {:?}", e)))?;
        let hash = match rc.get(key) {
            Some(h) => *h,
            None => return Ok(None),
        };
        let obj = store
            .get(&hash)
            .map_err(|e| StorageError::Vault(format!("FileStore::get: {:?}", e)))?;
        let enc_key = self.derive_enc_key(key);
        let plaintext = decrypt_bytes(&obj.content, &enc_key).map_err(StorageError::Crypto)?;
        Ok(Some(plaintext))
    }

    /// Remove a logical key from the vault. The underlying object stays until compact (Phase 2 GC); only the root_commit dict entry goes. Subsequent reads return None.
    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        let mut store = self.store.lock().map_err(|_| {
            StorageError::Vault("FlatStorage mutex poisoned".to_string())
        })?;
        let mut rc = store
            .load_root_commit()
            .map_err(|e| StorageError::Vault(format!("load_root_commit: {:?}", e)))?;
        if rc.remove(key).is_some() {
            store
                .commit_root(&rc)
                .map_err(|e| StorageError::Vault(format!("commit_root: {:?}", e)))?;
        }
        Ok(())
    }

    // ========================================================================
    // Internal key derivation ========================================================================

    fn derive_enc_key(&self, key: &str) -> [u8; 32] {
        // Same KDF context + input shape as the pre-vault FAF storage layer used. Re-using the formula keeps the per-key encryption keys derivation stable across the migration; the vault layer is purely a backend swap, not a wire-format key change.
        let context = [
            key.as_bytes(),
            self.identity_seed.as_slice(),
            self.device_secret.as_slice(),
        ]
        .concat();
        blake3::derive_key("photon.storage.encryption.v0", &context)
    }
}

/// Build a DeviceId from the device_secret. The DeviceId is informational (it goes into the vault anchor as part of the MeshMember identity but the SingleDeviceMeshEngine doesn't compare against anything). Derived deterministically so reopening the vault gets the same ID.
fn device_id_from_secret(device_secret: &[u8; 32]) -> ferros_vault::device::DeviceId {
    let h = blake3::derive_key("photon.vault.device_id.v0", device_secret);
    let mut id = [0u8; 16];
    id.copy_from_slice(&h[..16]);
    ferros_vault::device::DeviceId(id)
}

/// Build a Blob-typed Object whose content is the ciphertext. The vault expects the meta.hash field to equal `blake3(content)` (raw, no salt/name/domain mixing) — FileStore verifies this on put and uses the hash as the object's address. Construct the Object directly rather than going through ObjectBuilder (which mixes in salt + name + domain + permission level for the hash).
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
