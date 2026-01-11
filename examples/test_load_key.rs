use ed25519_dalek::{SigningKey, VerifyingKey};
use std::fs;
use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let path = PathBuf::from("/tmp/test_fgtw_device.key");
    let bytes = fs::read(&path)?;

    println!("Loading VSF key file: {}", path.display());
    println!("File size: {} bytes", bytes.len());

    // Verify magic number
    if bytes.len() < 4 || &bytes[0..4] != b"R\xC3\x85<" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid VSF magic number",
        ));
    }
    println!("✓ Magic number verified: RÅ<");

    // Find section
    let section_start = bytes
        .iter()
        .position(|&b| b == b'[')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "No section found"))?;
    println!("✓ Section found at position {}", section_start);

    // Find ke values
    let mut secret_key: Option<Vec<u8>> = None;
    let mut public_key: Option<Vec<u8>> = None;

    let mut i = section_start;
    while i < bytes.len() - 35 {
        if bytes[i] == b'k' && bytes[i + 1] == b'e' && i + 2 < bytes.len() && bytes[i + 2] == b'3' {
            let len = bytes[i + 3] as usize;
            if len == 31 && i + 4 + 32 <= bytes.len() {
                let key_bytes = bytes[i + 4..i + 4 + 32].to_vec();
                if secret_key.is_none() {
                    secret_key = Some(key_bytes);
                    println!("✓ Found secret key at position {}", i);
                } else {
                    public_key = Some(key_bytes);
                    println!("✓ Found public key at position {}", i);
                    break;
                }
            }
        }
        i += 1;
    }

    let secret_bytes = secret_key
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Secret key not found"))?;

    let public_bytes = public_key
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Public key not found"))?;

    // Reconstruct keypair
    let secret =
        SigningKey::from_bytes(&secret_bytes.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid secret key length")
        })?);

    let public =
        VerifyingKey::from_bytes(&public_bytes.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid public key length")
        })?)
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid public key: {}", e),
            )
        })?;

    println!("✓ Keypair loaded successfully!");
    println!("  Secret: {}", hex::encode(secret.to_bytes()));
    println!("  Public: {}", hex::encode(public.as_bytes()));

    // Verify the secret derives the public key
    let derived_public = secret.verifying_key();
    if derived_public.as_bytes() == public.as_bytes() {
        println!("✓ Public key matches derived key from secret!");
    } else {
        println!("✗ WARNING: Public key mismatch!");
    }

    Ok(())
}
