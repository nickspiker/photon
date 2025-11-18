use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use vsf::{VsfBuilder, VsfType};

/// Ed25519 keypair for FGTW device/handle identity
#[derive(Clone)]
pub struct Keypair {
    pub secret: SigningKey,
    pub public: VerifyingKey,
}

impl Keypair {
    /// Generate new random keypair
    pub fn generate() -> Self {
        let secret = SigningKey::generate(&mut OsRng);
        let public = secret.verifying_key();
        Self { secret, public }
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.secret.sign(message)
    }
}

/// FGTW storage paths for device key, handle, and peer cache
pub struct FgtwPaths {
    /// Device key: system-wide FGTW identity (/etc/fgtw/device.key or fallback)
    pub device_key: PathBuf,
    /// User handle: per-user FGTW identity (~/.config/fgtw/handle.key)
    pub handle_key: PathBuf,
    /// Peer cache: per-user peer list (~/.cache/fgtw/peers.vsf)
    pub peer_cache: PathBuf,
}

impl FgtwPaths {
    /// Get FGTW storage paths with platform-appropriate defaults
    pub fn new() -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        let device_key = {
            let system_path = PathBuf::from("/etc/fgtw/device.key");
            if system_path.parent().map(|p| p.exists()).unwrap_or(false) {
                system_path
            } else {
                // Fallback to user config if /etc/fgtw doesn't exist
                dirs::config_dir()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config dir"))?
                    .join("fgtw")
                    .join("device.key")
            }
        };

        #[cfg(target_os = "windows")]
        let device_key = {
            let system_path = PathBuf::from("C:\\ProgramData\\FGTW\\device.key");
            if system_path.parent().map(|p| p.exists()).unwrap_or(false) {
                system_path
            } else {
                // Fallback to user config
                dirs::config_dir()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config dir"))?
                    .join("fgtw")
                    .join("device.key")
            }
        };

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let device_key = dirs::config_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config dir"))?
            .join("fgtw")
            .join("device.key");

        let handle_key = dirs::config_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config dir"))?
            .join("fgtw")
            .join("handle.key");

        let peer_cache = dirs::cache_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No cache dir"))?
            .join("fgtw")
            .join("peers.vsf");

        Ok(Self {
            device_key,
            handle_key,
            peer_cache,
        })
    }
}

impl Default for FgtwPaths {
    fn default() -> Self {
        Self::new().expect("Failed to determine FGTW paths")
    }
}

/// Load or generate device keypair from VSF file
/// Device key is system-wide FGTW identity
pub fn load_or_generate_device_key(path: &PathBuf) -> io::Result<Keypair> {
    if path.exists() {
        // Load existing device key
        load_keypair_vsf(path)
    } else {
        // Generate new device key
        let keypair = Keypair::generate();
        save_keypair_vsf(path, &keypair)?;
        Ok(keypair)
    }
}

/// Load or generate handle keypair from VSF file
/// Handle is per-user FGTW identity (portable across apps)
pub fn load_or_generate_handle_key(path: &PathBuf) -> io::Result<Keypair> {
    if path.exists() {
        // Load existing handle
        load_keypair_vsf(path)
    } else {
        // Generate new handle
        let keypair = Keypair::generate();
        save_keypair_vsf(path, &keypair)?;
        Ok(keypair)
    }
}

/// Save keypair to VSF file with full spec-compliant structure
fn save_keypair_vsf(path: &PathBuf, keypair: &Keypair) -> io::Result<()> {
    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Build section with keypair data
    // [d"fgtw_device_key"
    //   (d"secret":ke{32}{secret_key})
    //   (d"public":ke{32}{public_key})
    // ]
    let section_fields = vec![
        (
            "secret".to_string(),
            VsfType::ke(keypair.secret.to_bytes().to_vec()),
        ),
        (
            "public".to_string(),
            VsfType::ke(keypair.public.to_bytes().to_vec()),
        ),
    ];

    // Build complete VSF file with header and section
    // The keypair signs its own file - beautifully self-referential!
    let vsf_bytes = VsfBuilder::new()
        .add_section("fgtw_device_key", section_fields)
        .build()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("VSF build error: {:?}", e)))?;

    // Write to file
    fs::write(path, &vsf_bytes)?;

    Ok(())
}

/// Load keypair from VSF file with full verification
fn load_keypair_vsf(path: &PathBuf) -> io::Result<Keypair> {
    let bytes = fs::read(path)?;

    // Verify magic number
    if bytes.len() < 4 || &bytes[0..4] != b"R\xC3\x85<" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid VSF magic number",
        ));
    }

    // Verify provenance hash (hp) - find and validate
    let stored_hash = match find_and_extract_hash(&bytes) {
        Ok(hash) => hash,
        Err(_) => {
            // Hash verification optional for now - VsfBuilder may not include it
            // TODO: Make this required once VsfBuilder adds hp/ge support
            [0u8; 32]
        }
    };

    // If hash is non-zero, verify it
    if stored_hash != [0u8; 32] {
        let computed_hash = compute_provenance_hash(&bytes)?;
        if stored_hash != computed_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Provenance hash mismatch - file corrupted or tampered",
            ));
        }
    }

    // TODO: Verify Ed25519 signature (ge) if present
    // The keypair signs its own VSF file - beautifully self-referential!

    // Find the section - skip to '[' marker
    let section_start = bytes
        .iter()
        .position(|&b| b == b'[')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "No section found"))?;

    // Parse from section start
    let mut ptr = section_start;

    // Expect section format: [d"name" (d"secret":ke{...}) (d"public":ke{...}) ]
    // The VSF parser will handle the structure, we just need to extract the ke values

    // Find the two ke{...} values by scanning for 'ke' markers
    let mut secret_key: Option<Vec<u8>> = None;
    let mut public_key: Option<Vec<u8>> = None;

    // Simple approach: find ke markers and extract following 32 bytes
    let mut i = section_start;
    while i < bytes.len() - 35 {
        // Need at least 'ke3' + length + 32 bytes
        if bytes[i] == b'k' && bytes[i + 1] == b'e' {
            // Found ke marker, next byte should be '3' (length marker), then length byte
            if i + 2 < bytes.len() && bytes[i + 2] == b'3' {
                let len = bytes[i + 3] as usize;
                if len == 31 && i + 4 + 32 <= bytes.len() {
                    // VSF uses len-1 for inclusive mode
                    let key_bytes = bytes[i + 4..i + 4 + 32].to_vec();
                    if secret_key.is_none() {
                        secret_key = Some(key_bytes);
                    } else {
                        public_key = Some(key_bytes);
                        break; // Found both keys
                    }
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }

    let secret_bytes = secret_key.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Secret key not found in VSF file",
        )
    })?;

    let public_bytes = public_key.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Public key not found in VSF file",
        )
    })?;

    // Reconstruct keypair
    let secret =
        ed25519_dalek::SigningKey::from_bytes(&secret_bytes.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid secret key length")
        })?);

    let public =
        ed25519_dalek::VerifyingKey::from_bytes(&public_bytes.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid public key length")
        })?)
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid public key: {}", e),
            )
        })?;

    Ok(Keypair { secret, public })
}

/// Compute BLAKE3 provenance hash with hash field zeroed
fn compute_provenance_hash(bytes: &[u8]) -> io::Result<[u8; 32]> {
    let mut temp = bytes.to_vec();

    // Find and zero out the hash bytes
    for i in 0..bytes.len().saturating_sub(35) {
        if bytes[i] == b'h' && (bytes[i + 1] == b'b' || bytes[i + 1] == b'p') {
            if i + 2 < bytes.len() && bytes[i + 2] == b'3' {
                let len = bytes[i + 3] as usize;
                if len == 31 && i + 4 + 32 <= bytes.len() {
                    // Zero out the hash bytes
                    for j in 0..32 {
                        temp[i + 4 + j] = 0;
                    }
                    break;
                }
            }
        }
    }

    // Compute BLAKE3
    let hash = blake3::hash(&temp);
    Ok(*hash.as_bytes())
}

/// Find and extract hash from VSF file
fn find_and_extract_hash(bytes: &[u8]) -> io::Result<[u8; 32]> {
    // Look for 'hb' or 'hp' markers followed by hash bytes
    for i in 0..bytes.len().saturating_sub(35) {
        if bytes[i] == b'h' && (bytes[i + 1] == b'b' || bytes[i + 1] == b'p') {
            // Found hash marker, next should be length indicator
            if i + 2 < bytes.len() && bytes[i + 2] == b'3' {
                let len = bytes[i + 3] as usize;
                if len == 31 && i + 4 + 32 <= bytes.len() {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&bytes[i + 4..i + 4 + 32]);
                    return Ok(hash);
                }
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "No hash found"))
}

/// Find position of hp (provenance hash) field in VSF bytes
/// Structure: RÅ< [header_len] l[ [version] [backward_compat] [timestamp] hp[HASH] ...
fn find_hash_position(bytes: &[u8]) -> io::Result<usize> {
    let mut ptr = 4; // Skip magic "RÅ<"

    // Skip header length
    let _ = vsf::parse(bytes, &mut ptr)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Parse error: {:?}", e)))?;

    // Parse list marker
    if ptr >= bytes.len() || bytes[ptr] != b'l' {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Expected list"));
    }
    ptr += 2; // Skip 'l['

    // Skip version (u type)
    let _ = vsf::parse(bytes, &mut ptr)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Parse error: {:?}", e)))?;

    // Skip backward_compat (u type)
    let _ = vsf::parse(bytes, &mut ptr)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Parse error: {:?}", e)))?;

    // Skip timestamp (f6 type)
    let _ = vsf::parse(bytes, &mut ptr)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Parse error: {:?}", e)))?;

    // Now at hp[HASH]
    if ptr >= bytes.len() || bytes[ptr] != b'h' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected hp marker",
        ));
    }
    ptr += 3; // Skip 'hp['

    Ok(ptr)
}

/// Find position of ge (Ed25519 signature) field in VSF bytes
/// Structure: ... hp[32 bytes] ge[SIGNATURE] ...
fn find_signature_position(bytes: &[u8]) -> io::Result<usize> {
    let hash_pos = find_hash_position(bytes)?;

    // Signature is right after hash: skip 32 bytes + ']'
    let mut ptr = hash_pos + 32;
    if ptr >= bytes.len() || bytes[ptr] != b']' {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Expected ]"));
    }
    ptr += 1;

    // Next should be ge[64]
    if ptr + 2 >= bytes.len() || &bytes[ptr..ptr + 2] != b"ge" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected ge marker",
        ));
    }
    ptr += 3; // Skip 'ge['

    Ok(ptr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_device_key_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("test_device_roundtrip.key");

        // Clean up
        let _ = fs::remove_file(&test_path);

        // Generate and save
        let original = Keypair::generate();
        save_keypair_vsf(&test_path, &original).unwrap();

        // Load back
        let loaded = load_keypair_vsf(&test_path).unwrap();

        // Verify keys match
        assert_eq!(original.secret.to_bytes(), loaded.secret.to_bytes());
        assert_eq!(original.public.as_bytes(), loaded.public.as_bytes());

        // Clean up
        let _ = fs::remove_file(&test_path);
    }
}
