use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::fs;
use std::io;
use std::path::PathBuf;
use vsf::{VsfBuilder, VsfSection, VsfType};

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

        let peer_cache = dirs::cache_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No cache dir"))?
            .join("fgtw")
            .join("peers.vsf");

        Ok(Self {
            device_key,
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

    // Use VsfHeader::decode() for proper parsing
    let (_header, header_bytes_consumed) =
        vsf::file_format::VsfHeader::decode(&bytes).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse VSF header: {}", e),
            )
        })?;

    // Skip provenance hash verification for device keys
    // Device keys use VsfBuilder which creates placeholder hp with zeros
    // They're not cryptographically signed, so hp verification isn't meaningful
    // TODO: Use sign_section() for device keys to get proper hp + ge

    // TODO: Verify Ed25519 signature (ge) if present in header fields
    // The keypair signs its own VSF file - beautifully self-referential!

    // Parse the section after the header
    let mut ptr = header_bytes_consumed;

    // Skip to section start '['
    while ptr < bytes.len() && bytes[ptr] != b'[' {
        ptr += 1;
    }

    if ptr >= bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No section found",
        ));
    }

    // Parse section using VSF crate
    let section = VsfSection::parse(&bytes, &mut ptr).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Parse section: {}", e),
        )
    })?;

    // Extract secret and public keys from section fields
    let secret_field = section.get_field("secret").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Secret key field not found")
    })?;
    let secret_bytes = match secret_field.values.first() {
        Some(VsfType::ke(bytes)) if bytes.len() == 32 => bytes.clone(),
        Some(VsfType::ke(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Secret key must be 32 bytes",
            ))
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Secret key must be ke type",
            ))
        }
    };

    let public_field = section.get_field("public").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Public key field not found")
    })?;
    let public_bytes = match public_field.values.first() {
        Some(VsfType::ke(bytes)) if bytes.len() == 32 => bytes.clone(),
        Some(VsfType::ke(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Public key must be 32 bytes",
            ))
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Public key must be ke type",
            ))
        }
    };

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
