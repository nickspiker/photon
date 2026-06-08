//! Smoke test for the ferros_vault-backed FlatStorage.
//!
//! Exercises every FlatStorage public method against a real `~/.config/photon.vsf` file using hard-coded test keying material. Useful for verifying the on-disk vault works end-to-end before a real attestation has fired in Photon (which is what would normally trigger `FlatStorage::new` at runtime).
//!
//! Usage:
//!   cargo run --bin photon-vault-smoke
//!   ls -la ~/.config/photon.vsf       # should be ~2 MiB
//!   file ~/.config/photon.vsf         # should say "VSF data"
//!   cargo run --bin photon-vault-smoke # again, should find existing data + add more
//!
//! Cleanup: just `rm ~/.config/photon.vsf` between runs.
//!
//! Keying material is hard-coded constants — NOT real photon identity. The vault file written by this smoke test is intentionally compatible-but-separate from any real vault Photon would create; once a real attestation lands and Photon calls `FlatStorage::new` with actual `(identity_seed, device_secret)`, the smoke-test vault becomes inaccessible (different anchor key) and should be deleted.

use photon_messenger::storage::FlatStorage;

// Hard-coded test seeds — same on every run so the vault round-trips deterministically. NOT secret; do not reuse for anything real.
const TEST_IDENTITY_SEED: [u8; 32] = [
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00,
];
const TEST_DEVICE_SECRET: [u8; 32] = [
    0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
    0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
];

fn main() {
    println!("=== photon-vault-smoke ===");
    println!("Initializing FlatStorage at ~/.config/photon.vsf …");

    let storage = match FlatStorage::new(TEST_IDENTITY_SEED, TEST_DEVICE_SECRET) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FATAL: FlatStorage::new failed: {}", e);
            std::process::exit(1);
        }
    };
    println!("  ✓ open / format succeeded");

    // Write three logical keys with distinct content.
    println!("\nWriting three logical keys …");
    let payloads: &[(&str, &[u8])] = &[
        ("contacts/index", b"alice,bob,carol"),
        ("contacts/aabbccdd/state", b"trust=verified,added=2026-06-08"),
        (
            "contacts/aabbccdd/messages",
            b"[{from:alice,text:hi,time:1717873617}]",
        ),
    ];
    for (key, data) in payloads {
        match storage.write(key, data) {
            Ok(()) => println!("  ✓ write {} ({} bytes)", key, data.len()),
            Err(e) => {
                eprintln!("FATAL: write {} failed: {}", key, e);
                std::process::exit(1);
            }
        }
    }

    // Read them back.
    println!("\nReading them back …");
    for (key, expected) in payloads {
        match storage.read(key) {
            Ok(Some(bytes)) if bytes.as_slice() == *expected => {
                println!("  ✓ read {} ({} bytes, matches)", key, bytes.len())
            }
            Ok(Some(bytes)) => {
                eprintln!(
                    "FATAL: read {} mismatch — expected {} bytes, got {}",
                    key,
                    expected.len(),
                    bytes.len()
                );
                std::process::exit(1);
            }
            Ok(None) => {
                eprintln!("FATAL: read {} returned None", key);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("FATAL: read {} failed: {}", key, e);
                std::process::exit(1);
            }
        }
    }

    // Read an unknown key, expect None.
    println!("\nReading unknown key …");
    match storage.read("nonexistent/key") {
        Ok(None) => println!("  ✓ read nonexistent → None (correct)"),
        Ok(Some(_)) => {
            eprintln!("FATAL: read nonexistent returned Some — should be None");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("FATAL: read nonexistent errored: {}", e);
            std::process::exit(1);
        }
    }

    // Delete one and verify it's gone.
    println!("\nDeleting one key and verifying it's gone …");
    let delete_key = payloads[2].0;
    if let Err(e) = storage.delete(delete_key) {
        eprintln!("FATAL: delete {} failed: {}", delete_key, e);
        std::process::exit(1);
    }
    println!("  ✓ delete {}", delete_key);
    match storage.read(delete_key) {
        Ok(None) => println!("  ✓ read {} after delete → None", delete_key),
        Ok(Some(_)) => {
            eprintln!("FATAL: deleted key still readable");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("FATAL: read after delete errored: {}", e);
            std::process::exit(1);
        }
    }

    // Verify the remaining two are still there.
    println!("\nVerifying remaining keys survive the delete …");
    for (key, expected) in &payloads[..2] {
        match storage.read(key) {
            Ok(Some(bytes)) if bytes.as_slice() == *expected => {
                println!("  ✓ {} still readable", key)
            }
            other => {
                eprintln!("FATAL: {} unexpectedly affected by delete: {:?}", key, other);
                std::process::exit(1);
            }
        }
    }

    // Report file size + path.
    println!("\n=== Disk state ===");
    if let Some(p) = dirs::config_dir().map(|p| p.join("photon.vsf")) {
        match std::fs::metadata(&p) {
            Ok(m) => println!("  {} — {} bytes", p.display(), m.len()),
            Err(e) => println!("  (couldn't stat {}: {})", p.display(), e),
        }
    }

    println!("\n✓ all smoke checks passed");
}
