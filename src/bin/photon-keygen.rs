//! Generate the Ed25519 keypair that signs Photon Messenger binaries, manifests and official messages.
//!
//! The private key is written ONCE, to the one location the signing tools read (`KEY_DIR`), mode 0600, never over an existing file.
//! That directory is a local encrypted disk, not a cloud-synced folder: until 2026-11-03 this tool wrote into a MEGA-synced tree with the default umask (security audit NET-11), which put the one key that controls every client's update channel on a cloud account, world-readable on a shared host.
//! Keep the key cold: it is read only while a release is signed.
//!
//! Usage: photon-keygen (the release key), or photon-keygen --log-key (the maintainer's log key, ferros docs/PHOTON.md M6: submitted logs are sealed to it alone).

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::{fs, io::Write, path::PathBuf};

/// The one directory the release key lives in, shared with photon-signature-signer and photon-manifest.
pub const KEY_DIR: &str = "/mnt/Harbor/Code/keys";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--log-key") {
        return log_key();
    }
    println!("Photon Messenger Key Generator");
    println!("==============================\n");

    let keys_dir = PathBuf::from(KEY_DIR);
    if !keys_dir.exists() {
        fs::create_dir_all(&keys_dir)?;
        println!("✓ Created keys directory: {}", keys_dir.display());
    }

    let private_key_path = keys_dir.join("photon-signing-key");
    let public_key_path = keys_dir.join("photon-signing-key.pub");

    println!("Generating Ed25519 keypair...");
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    // create_new refuses an existing file, so a second run can never overwrite the live key; 0600 from the first byte, never a chmod after a world-readable write.
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut private = match opts.open(&private_key_path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            eprintln!("ERROR: a key already exists at {} — move it aside by hand if you really mean to replace the release key.", private_key_path.display());
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    private.write_all(&signing_key.to_bytes())?;
    private.sync_all()?;
    println!("✓ Private key saved (0600): {}", private_key_path.display());

    fs::write(&public_key_path, verifying_key.to_bytes())?;
    println!("✓ Public key saved:  {}", public_key_path.display());

    println!("\nPublic key (hex): {}", hex::encode(verifying_key.to_bytes()));
    println!("\nThe public key is compiled into the client (crypto/self_verify.rs AUTHOR_PUBKEY) and inlined into the installers.");
    println!("Keep the private key cold: this directory only, never a synced folder, never a copy.");

    Ok(())
}

/// The maintainer's log key: an Ed25519 key, written once as VSF (`kge`) to `photon_messenger::crypto::log_seal::LOG_KEY_PATH`, mode 0600, never over an existing file.
/// Only the public half is printed; it goes into `LOG_KEY_PUBLIC`.
fn log_key() -> Result<(), Box<dyn std::error::Error>> {
    use photon_messenger::crypto::log_seal::{log_key_document, LOG_KEY_PATH};
    let signing_key = SigningKey::generate(&mut OsRng);
    let doc = log_key_document(&signing_key.to_bytes())?;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = match opts.open(LOG_KEY_PATH) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            eprintln!("ERROR: a log key already exists at {LOG_KEY_PATH}; move it aside by hand if you really mean to replace it (logs sealed to it become unreadable).");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    file.write_all(&doc)?;
    file.sync_all()?;
    println!("✓ Log key saved (0600): {LOG_KEY_PATH}");
    println!("Public key (hex): {}", hex::encode(signing_key.verifying_key().to_bytes()));
    println!("It goes into crypto/log_seal.rs LOG_KEY_PUBLIC.");
    Ok(())
}
