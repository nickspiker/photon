//! Generate the Ed25519 keypair that signs Photon Messenger binaries, manifests and official messages.
//!
//! The private key is written ONCE, to the one location the signing tools read (`KEY_DIR`), mode 0600, never over an existing file.
//! That directory is a local encrypted disk, not a cloud-synced folder: until 2026-11-03 this tool wrote into a MEGA-synced tree with the default umask (security audit NET-11), which put the one key that controls every client's update channel on a cloud account, world-readable on a shared host.
//! Keep the key cold: it is read only while a release is signed.
//!
//! Usage: photon-keygen

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::{fs, io::Write, path::PathBuf};

/// The one directory the release key lives in, shared with photon-signature-signer and photon-manifest.
pub const KEY_DIR: &str = "/mnt/Harbor/Code/keys";

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
