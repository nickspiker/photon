//! Sign photon-messenger binary with Ed25519 cryptographic signature
//!
//! This utility is installed alongside photon-messenger via cargo install.
//! After signing the main binary, it deletes itself.
//!
//! Usage: photon-signature-signer <binary-path>

use ed25519_dalek::{Signature, Signer, SigningKey};
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
    let binary_data = fs::read(binary_path)?;
    println!("  Binary size: {} bytes", binary_data.len());

    // Hash it with BLAKE3
    let hash = blake3::hash(&binary_data);
    println!("  BLAKE3 hash: {}", hex::encode(hash.as_bytes()));

    // Load private key
    let keys_dir = PathBuf::from("/mnt/Chiton/MEGA/Code/keys");
    let private_key_path = keys_dir.join("photon-signing-key");

    if !private_key_path.exists() {
        eprintln!("\nERROR: Private key not found!");
        eprintln!("  Expected: {}", private_key_path.display());
        eprintln!("\nRun 'photon-keygen' first to generate keys.");
        std::process::exit(1);
    }

    let private_key_bytes = fs::read(&private_key_path)?;
    let signing_key = SigningKey::from_bytes(
        private_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid private key length")?,
    );

    // Sign the hash
    let signature: Signature = signing_key.sign(hash.as_bytes());
    println!("  Ed25519 signature: {}", hex::encode(signature.to_bytes()));

    // Append signature to binary (64 bytes)
    let mut signed_binary = binary_data;
    signed_binary.extend_from_slice(&signature.to_bytes());

    // Overwrite original file with signed version
    fs::write(binary_path, &signed_binary)?;

    println!("\n✓ Signature appended to binary!");
    println!("  New size: {} bytes (+64 for signature)", signed_binary.len());

    // Self-delete after successful signing
    let self_path = env::current_exe()?;
    println!("\nCleaning up...");

    // On Windows, we can't delete ourselves while running, so we need a workaround
    #[cfg(target_os = "windows")]
    {
        // Create a batch file that waits and deletes us
        let batch_path = self_path.with_extension("bat");
        let batch_content = format!(
            "@echo off\n\
             :wait\n\
             timeout /t 1 /nobreak >nul\n\
             del /f /q \"{}\" 2>nul\n\
             if exist \"{}\" goto wait\n\
             del /f /q \"%~f0\"\n",
            self_path.display(),
            self_path.display()
        );
        fs::write(&batch_path, batch_content)?;

        // Launch the batch file detached
        std::process::Command::new("cmd")
            .args(&["/C", "start", "/B", batch_path.to_str().unwrap()])
            .spawn()?;

        println!("✓ Self-deletion scheduled");
    }

    // On Unix, we can delete ourselves directly
    #[cfg(not(target_os = "windows"))]
    {
        fs::remove_file(&self_path)?;
        println!("✓ Signer removed");
    }

    Ok(())
}
