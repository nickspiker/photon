//! Test device key derivation
//!
//! Computes pubkey from machine-id to debug key derivation issues.

use photon_messenger::network::fgtw::{derive_device_keypair, get_machine_fingerprint};

fn main() {
    println!("Device Key Derivation Test");
    println!("==========================\n");

    let fp = match get_machine_fingerprint() {
        Ok(fp) => fp,
        Err(e) => {
            eprintln!("Failed to get machine fingerprint: {}", e);
            std::process::exit(1);
        }
    };

    println!("Fingerprint ({} bytes):", fp.len());
    println!("  Raw: {:?}", String::from_utf8_lossy(&fp));
    println!("  Hex: {}", hex::encode(&fp));

    let hash = blake3::hash(&fp);
    println!("\nBLAKE3 hash (seed): {}", hex::encode(hash.as_bytes()));

    let kp = derive_device_keypair(&fp);
    println!("\nDerived pubkey: {}", hex::encode(kp.public.as_bytes()));

    println!("\nExpected (registered with FGTW): b204d906...");
    println!("Current (from FGTW error):       90e571bf...");
}
