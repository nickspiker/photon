#!/usr/bin/env cargo
//! Append BLAKE3 hash to a release binary for self-verification
//!
//! Usage: cargo run --bin hash-release <binary-path>

use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <binary-path>", args[0]);
        eprintln!("\nExample:");
        eprintln!("  {} target/release/photon-messenger", args[0]);
        std::process::exit(1);
    }

    let binary_path = &args[1];

    println!("Hashing binary: {}", binary_path);

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
    println!("  File: {}", binary_path);

    Ok(())
}
