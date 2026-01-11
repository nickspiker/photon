//! Device fingerprint and key derivation
//!
//! Keys are NEVER stored on disk - always derived deterministically from
//! platform-specific fingerprint ORACLES (not fixed IDs - oracles are harder to steal):
//! - Linux: /etc/machine-id (generated at install, unique per system)
//! - Windows: Registry MachineGuid (generated at install)
//! - macOS: IOPlatformUUID (hardware-burned, survives reinstalls)
//! - Android: Device fingerprint (passed via JNI - ANDROID_ID is an oracle)

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use std::io;
use std::path::PathBuf;

/// Ed25519 keypair for FGTW device/handle identity
///
/// NEVER persisted to disk - derived deterministically from device fingerprint.
/// On Android: BLAKE3(device_fingerprint) → Ed25519 seed
/// On Desktop: BLAKE3(machine-id) → Ed25519 seed
#[derive(Clone)]
pub struct Keypair {
    pub secret: SigningKey,
    pub public: VerifyingKey,
}

impl Keypair {
    /// Create keypair from a 32-byte seed (deterministic)
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let secret = SigningKey::from_bytes(seed);
        let public = secret.verifying_key();
        Self { secret, public }
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.secret.sign(message)
    }
}

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

/// Derive device keypair from machine fingerprint (deterministic, never stored)
///
/// Uses BLAKE3 to hash the fingerprint into a 32-byte Ed25519 seed.
/// The same fingerprint always produces the same keypair.
pub fn derive_device_keypair(fingerprint: &[u8]) -> Keypair {
    let hash = blake3::hash(fingerprint);
    let seed: [u8; 32] = *hash.as_bytes();
    Keypair::from_seed(&seed)
}

/// Get machine fingerprint for deterministic key derivation
///
/// Linux: /etc/machine-id (stable across reboots, unique per install)
/// Windows: MachineGuid from registry
/// macOS: IOPlatformUUID (hardware-burned, survives reinstalls)
/// Android: Handled separately via JNI with device fingerprint
#[cfg(target_os = "linux")]
pub fn get_machine_fingerprint() -> io::Result<Vec<u8>> {
    std::fs::read("/etc/machine-id")
}

#[cfg(target_os = "windows")]
pub fn get_machine_fingerprint() -> io::Result<Vec<u8>> {
    // Read MachineGuid from registry
    use std::process::Command;
    let output = Command::new("reg")
        .args([
            "query",
            "HKLM\\SOFTWARE\\Microsoft\\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()?;
    Ok(output.stdout)
}

#[cfg(target_os = "macos")]
pub fn get_machine_fingerprint() -> io::Result<Vec<u8>> {
    // Read IOPlatformUUID - hardware-burned UUID that survives OS reinstalls
    use std::process::Command;
    let output = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()?;
    // Output contains IOPlatformUUID among other fields
    // We hash the whole output - includes UUID and is deterministic
    Ok(output.stdout)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "windows",
    target_os = "macos",
    target_os = "android"
)))]
pub fn get_machine_fingerprint() -> io::Result<Vec<u8>> {
    // Fallback for other Unix-like systems (FreeBSD, etc.)
    // Try /etc/hostid first, then hostname
    if let Ok(hostid) = std::fs::read("/etc/hostid") {
        return Ok(hostid);
    }
    let hostname = std::fs::read("/etc/hostname").unwrap_or_else(|_| b"unknown".to_vec());
    Ok(hostname)
}
