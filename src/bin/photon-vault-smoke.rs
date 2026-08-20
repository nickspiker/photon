//! Smoke test for the ferros_vault-backed FlatStorage.
//!
//! Exercises every FlatStorage public method against a vault file using a hard-coded test handle. Useful for verifying the on-disk vault works end-to-end before a real attestation has fired in Photon (which is what would normally trigger `FlatStorage::new` at runtime).
//!
//! DEFAULT: writes under the system tempdir (`/tmp/photon-vault-smoke/`) — a smoke run must never salt the developer's REAL config dir with plausible-looking 17MB vault rings (the "eight vaults, recent timestamps" field mystery, 2026-08-20). Pass `--real-dirs` to exercise the true XDG paths deliberately; clean up after with `rm -rf ~/.config/photon/ ~/.local/share/photon/`.
//!
//! Hard-coded test handle + test device_secret — NOT real photon identity.

use photon_messenger::storage::FlatStorage;

const TEST_VAULT_SEED: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
];
const TEST_DEVICE_SECRET: [u8; 32] = [
    0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
    0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
];

fn main() {
    println!("=== photon-vault-smoke ===");
    if !std::env::args().any(|a| a == "--real-dirs") {
        let root = std::env::temp_dir().join("photon-vault-smoke");
        kete::set_vault_dirs_override(
            root.join("cfg").to_string_lossy().into_owned(),
            root.join("data").to_string_lossy().into_owned(),
        );
        println!("(tempdir mode: {} — pass --real-dirs for the XDG paths)", root.display());
    }
    println!("Initializing FlatStorage …");

    let storage = match FlatStorage::new(
        photon_messenger::storage::APP,
        TEST_VAULT_SEED,
        TEST_DEVICE_SECRET,
    ) {
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
        (
            "contacts/aabbccdd/state",
            b"trust=verified,added=2026-06-08",
        ),
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
                eprintln!(
                    "FATAL: {} unexpectedly affected by delete: {:?}",
                    key, other
                );
                std::process::exit(1);
            }
        }
    }

    // Large-value probe (EWE: values are arbitrary size in principle — a blob is ONE vault value, never at-rest chunks). `--big` adds 500MB to the default 25MB probe.
    println!("\n=== Large-value probe ===");
    let mut sizes: Vec<usize> = vec![25 * 1024 * 1024];
    if std::env::args().any(|a| a == "--big") {
        sizes.push(500 * 1024 * 1024);
    }
    for size in sizes {
        let mut value = vec![0u8; size];
        // Incompressible-ish, deterministic fill so dedup/zero-page effects can't flatter the numbers.
        for (i, b) in value.iter_mut().enumerate() {
            *b = (i as u64).wrapping_mul(0x9E3779B97F4A7C15).to_le_bytes()[0];
        }
        let addr = *blake3::hash(&size.to_le_bytes()).as_bytes();
        let t = std::time::Instant::now();
        match storage.write_addr(&addr, &value) {
            Ok(()) => {
                let w_ms = t.elapsed().as_millis();
                let t2 = std::time::Instant::now();
                let back = storage.read_addr(&addr).ok().flatten();
                let r_ms = t2.elapsed().as_millis();
                let ok = back.as_deref() == Some(value.as_slice());
                println!(
                    "  {:>4} MB: write = {} ms  read = {} ms  verify = {}",
                    size / (1024 * 1024),
                    w_ms,
                    r_ms,
                    if ok { "OK" } else { "MISMATCH" }
                );
                if !ok {
                    std::process::exit(1);
                }
                let _ = storage.delete_addr(&addr);
            }
            Err(e) => {
                eprintln!("FATAL: {} MB write failed: {}", size / (1024 * 1024), e);
                std::process::exit(1);
            }
        }
    }

    // Report file size + path.
    println!("\n=== Disk state ===");
    if let Some(dir) = dirs::config_dir().map(|p| p.join(photon_messenger::storage::APP.dir)) {
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if let Ok(m) = entry.metadata() {
                        println!("  {} — {} bytes", entry.path().display(), m.len());
                    }
                }
            }
            Err(e) => println!("  (couldn't list {}: {})", dir.display(), e),
        }
    }

    println!("\n✓ all smoke checks passed");
}
