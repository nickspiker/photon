//! Generate Ed25519 keypair for signing Photon Messenger binaries
//!
//! This utility generates a signing keypair and stores it in the keys directory.
//! The private key is stored unencrypted (filesystem encryption assumed).
//!
//! Usage: photon-keygen

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Photon Messenger Key Generator");
    println!("==============================\n");

    // Keys directory
    let keys_dir = PathBuf::from("/mnt/Chiton/MEGA/Code/keys");

    // Ensure directory exists
    if !keys_dir.exists() {
        fs::create_dir_all(&keys_dir)?;
        println!("✓ Created keys directory: {}", keys_dir.display());
    }

    let private_key_path = keys_dir.join("photon-signing-key");
    let public_key_path = keys_dir.join("photon-signing-key.pub");

    // Check if keys already exist
    if private_key_path.exists() || public_key_path.exists() {
        eprintln!("ERROR: Keys already exist!");
        eprintln!("  Private: {}", private_key_path.display());
        eprintln!("  Public:  {}", public_key_path.display());
        eprintln!("\nDelete existing keys first if you want to regenerate.");
        std::process::exit(1);
    }

    println!("Generating Ed25519 keypair...");

    // Generate keypair
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    // Save private key (32 bytes)
    fs::write(&private_key_path, signing_key.to_bytes())?;
    println!("✓ Private key saved: {}", private_key_path.display());

    // Save public key (32 bytes)
    fs::write(&public_key_path, verifying_key.to_bytes())?;
    println!("✓ Public key saved:  {}", public_key_path.display());

    println!(
        "\nPublic key (hex): {}",
        hex::encode(verifying_key.to_bytes())
    );
    println!("\nKeep the private key secure!");
    println!("The public key will be embedded in installer scripts.");

    Ok(())
}
