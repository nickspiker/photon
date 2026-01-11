//! Sign photon-messenger binary with Ed25519 cryptographic signature
//!
//! This utility signs binaries for distribution.
//!
//! Usage: photon-signature-signer <binary-path>

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <binary-path>", args[0]);
        eprintln!("\nExample:");
        eprintln!("  {} ~/.cargo/bin/photon-messenger", args[0]);
        std::process::exit(1);
    }

    let binary_path = &args[1];

    println!("Signing binary: {}", binary_path);

    // Read the binary
    let mut binary_data = fs::read(binary_path)?;
    println!("  Binary size: {} bytes", binary_data.len());

    // Load private key - try multiple locations
    let key_locations = [
        "/mnt/Chiton/MEGA/Code/keys/photon-signing-key",
        "/home/nick/MEGA/code/keys/photon-signing-key",
    ];

    let private_key_path = key_locations
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists());

    let private_key_path = match private_key_path {
        Some(path) => path,
        None => {
            eprintln!("\nERROR: Private key not found!");
            eprintln!("  Searched:");
            for loc in &key_locations {
                eprintln!("    {}", loc);
            }
            eprintln!("\nRun 'photon-keygen' first to generate keys.");
            std::process::exit(1);
        }
    };

    let private_key_bytes = fs::read(&private_key_path)?;
    let signing_key = SigningKey::from_bytes(
        private_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid private key length")?,
    );
    let verifying_key = signing_key.verifying_key();

    // Check if already signed by attempting to verify the signature
    if binary_data.len() >= 64 {
        let signature_bytes = binary_data.split_off(binary_data.len() - 64);
        let signature =
            Signature::from_bytes(signature_bytes.as_slice().try_into().unwrap_or(&[0u8; 64]));

        let hash = blake3::hash(&binary_data);

        if verifying_key.verify(hash.as_bytes(), &signature).is_ok() {
            println!("\n⚠ Binary is already signed with your key!");
            println!("  Signature verification passed");
            println!("  Skipping to avoid double-signing");
            println!("  Rebuild the binary first if you need to re-sign");

            // Still output SHA256 for Windows binaries
            if binary_path.ends_with(".exe") {
                // Restore full binary for SHA256
                binary_data.extend_from_slice(&signature_bytes);
                let mut hasher = Sha256::new();
                hasher.update(&binary_data);
                let sha256_hash = hex::encode(hasher.finalize()).to_uppercase();

                let sha256_path = format!("{}.sha256", binary_path);
                fs::write(&sha256_path, &sha256_hash)?;
                println!("  SHA256 hash: {}", sha256_hash);
                println!("  Written to: {}", sha256_path);
            }
            return Ok(());
        }

        // Not signed with our key, restore the data
        binary_data.extend_from_slice(&signature_bytes);
    }

    // Hash it with BLAKE3
    let hash = blake3::hash(&binary_data);
    println!(
        "  BLAKE3 hash: {}",
        hex::encode(hash.as_bytes()).to_uppercase()
    );

    // Sign the hash
    let signature: Signature = signing_key.sign(hash.as_bytes());
    println!(
        "  Ed25519 signature: {}",
        hex::encode(signature.to_bytes()).to_uppercase()
    );

    // Append signature to binary (64 bytes)
    let mut signed_binary = binary_data;
    signed_binary.extend_from_slice(&signature.to_bytes());

    // Overwrite original file with signed version
    fs::write(binary_path, &signed_binary)?;

    println!("\n✓ Signature appended to binary!");
    println!(
        "  New size: {} bytes (+64 for signature)",
        signed_binary.len()
    );

    // For Windows binaries, also compute SHA256 and write to .sha256 file
    // (PowerShell uses SHA256 for verification since Defender blocks execution)
    if binary_path.ends_with(".exe") {
        let mut hasher = Sha256::new();
        hasher.update(&signed_binary);
        let sha256_hash = hex::encode(hasher.finalize()).to_uppercase();

        let sha256_path = format!("{}.sha256", binary_path);
        fs::write(&sha256_path, &sha256_hash)?;
        println!("  SHA256 hash: {}", sha256_hash);
        println!("  Written to: {}", sha256_path);
    }

    Ok(())
}
