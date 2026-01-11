use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::fs;
use std::path::PathBuf;
use vsf::{VsfBuilder, VsfType};

fn main() -> std::io::Result<()> {
    let secret = SigningKey::generate(&mut OsRng);
    let public = secret.verifying_key();
    let path = PathBuf::from("/tmp/test_fgtw_device.key");

    // Build VSF file
    let section_fields = vec![
        (
            "secret".to_string(),
            VsfType::ke(secret.to_bytes().to_vec()),
        ),
        (
            "public".to_string(),
            VsfType::ke(public.to_bytes().to_vec()),
        ),
    ];

    let vsf_bytes = VsfBuilder::new()
        .add_section("fgtw_device_key", section_fields)
        .build()
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("VSF build error: {:?}", e),
            )
        })?;

    fs::write(&path, &vsf_bytes)?;

    println!("Generated keypair at: {}", path.display());
    println!("Public key: {}", hex::encode(public.as_bytes()));
    println!("File size: {} bytes", vsf_bytes.len());

    Ok(())
}
