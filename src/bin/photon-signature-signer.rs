//! Sign a photon-messenger binary with the release key, every egg (2026-11-10, "no single eggs").
//!
//! The binary gets `egg document ‖ length field ‖ legacy tail` appended (crypto::self_verify::sign_binary): a VSF document signed by the release key with Ed25519, Falcon-512 and SPHINCS+, naming the binary's BLAKE3, then the 64-byte Ed25519 tail older Photons check until the pin date.
//!
//! Usage: photon-signature-signer <binary-path>

use photon_messenger::crypto::self_verify::{release_signing_key, sign_binary, verify_signed_binary};
use sha2::{Digest, Sha256};
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <binary-path>", args[0]);
        std::process::exit(1);
    }
    let binary_path = &args[1];
    println!("Signing binary: {}", binary_path);
    let binary = fs::read(binary_path)?;
    println!("  Binary size: {} bytes", binary.len());

    // An already-signed binary is left alone: signing it again would sign the old signature into the new one.
    let signed = match verify_signed_binary(&binary) {
        Ok(hash) => {
            println!("\n⚠ Binary is already signed with every release egg (BLAKE3 {hash}); skipping. Rebuild it to re-sign.");
            binary
        }
        Err(_) => {
            let seed = release_signing_key().map_err(|e| format!("release key: {e}"))?;
            let signed = sign_binary(&binary, &seed)?;
            // The round trip is the gate: what was written must pass the exact check every Photon runs.
            let hash = verify_signed_binary(&signed).map_err(|e| format!("the signed binary does not verify: {e}"))?;
            fs::write(binary_path, &signed)?;
            println!("\n✓ Signed with every release egg (BLAKE3 {hash})");
            println!("  New size: {} bytes (+{} for the signature)", signed.len(), signed.len() - binary.len());
            signed
        }
    };

    // Windows installers check SHA-256 (PowerShell's hash; Defender blocks executing to self-verify), so the signed file's SHA-256 is written beside it.
    if binary_path.ends_with(".exe") {
        let sha256_hash = hex::encode(Sha256::digest(&signed)).to_uppercase();
        let sha256_path = format!("{}.sha256", binary_path);
        fs::write(&sha256_path, &sha256_hash)?;
        println!("  SHA256 hash: {}", sha256_hash);
        println!("  Written to: {}", sha256_path);
    }
    Ok(())
}
