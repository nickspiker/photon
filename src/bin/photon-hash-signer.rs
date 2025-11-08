//! Append BLAKE3 hash to photon-messenger binary for self-verification
//!
//! This utility is installed alongside photon-messenger via cargo install.
//! After signing the main binary, it deletes itself.
//!
//! Usage: photon-hash-signer <binary-path>

use std::{env, fs};

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

    // Append hash to binary
    let mut hashed_binary = binary_data;
    hashed_binary.extend_from_slice(hash.as_bytes());

    // Overwrite original file with hashed version
    fs::write(binary_path, &hashed_binary)?;

    println!("\n✓ Hash appended to binary!");
    println!("  New size: {} bytes (+32 for hash)", hashed_binary.len());

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
