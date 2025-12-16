///! Binary self-verification using Ed25519 cryptographic signatures
///!
///! **SIGNED BINARIES ONLY**: All official Photon Messenger binaries are cryptographically signed by Nick Spiker fractaldecoder@proton.me and verified on every startup. This prevents tampering and ensures authenticity of distributed binaries.
///!
///! ## For End Users
///!
///! **Use the official installer** - don't build from source unless you know what you're doing:
///!
///! - **Linux/macOS**: `curl -sSfL https://holdmyoscilloscope.com/photon/install.sh | sh`
///! - **Windows**: `iwr -useb https://holdmyoscilloscope.com/photon/install.ps1 | iex`
///!
///! These download pre-built, pre-signed binaries that will verify correctly.
///!
///! ## For Contributors & Source Builds
///!
///! **WARNING**: `cargo install photon-messenger` from crates.io will NOT work out of the box!
///! Crates.io doesn't include signing scripts or keys, so you must:
///!
///! 0. **Clone the full repository** (includes signing scripts):
///!    ```bash
///!    git clone https://github.com/nickspiker/photon
///!    cd photon
///!    ```
///!
///! 1. **Generate your own signing keys**:
///!    ```bash
///!    mkdir -p ~/.photon-keys  # Or wherever you want
///!    cargo run --bin photon-keygen  # Will fail - keys path is hardcoded
///!    # Edit src/bin/photon-keygen.rs to point to ~/.photon-keys
///!    cargo run --bin photon-keygen  # Now it works
///!    ```
///!
///! 2. **Replace the public key** in this file:
///!    - Update `PUBLIC_KEY_BYTES` constant below with your generated public key
///!    - Your binaries will now verify with YOUR signature (not Nick's)
///!
///! 3. **Build and sign**:
///!    ```bash
///!    cargo build && ./sign-after-build.sh debug
///!    cargo build --release && ./sign-after-build.sh release
///!    ```
///!
///! **Why so complicated?** This is intentional. If you're building from source, you should
///! understand what you're signing and why. Official binaries are signed by Nick Spiker only.
///!
///! Official binaries are signed by Nick Spiker <fractaldecoder@proton.me>
///! Public key: dff3af0c127c0bebe539c421da37993a517bfd78d2f5ee491d52fbf616444747
///!
///! This is a software commitment—binaries bearing this signature are guaranteed to be
///! built and released by the original author. No exceptions.
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Embedded public key for signature verification and system messages.
/// This is Nick Spiker's (fractaldecoder) signing key - used for:
/// - Binary signature verification
/// - System messages (updates, security notices, etc.)
///
/// Messages signed by this key are official Photon communications.
/// Public key (hex): dff3af0c127c0bebe539c421da37993a517bfd78d2f5ee491d52fbf616444747
pub const AUTHOR_PUBKEY: [u8; 32] = [
    0xdf, 0xf3, 0xaf, 0x0c, 0x12, 0x7c, 0x0b, 0xeb, 0xe5, 0x39, 0xc4, 0x21, 0xda, 0x37, 0x99, 0x3a,
    0x51, 0x7b, 0xfd, 0x78, 0xd2, 0xf5, 0xee, 0x49, 0x1d, 0x52, 0xfb, 0xf6, 0x16, 0x44, 0x47, 0x47,
];

/// List of trusted system pubkeys for official messages.
/// Currently just the author - future: democratic governance for adding keys.
pub const SYSTEM_PUBKEYS: &[[u8; 32]] = &[AUTHOR_PUBKEY];

/// Check if a pubkey is a trusted system pubkey.
/// Messages signed by system pubkeys are official Photon communications.
pub fn is_system_pubkey(pubkey: &[u8; 32]) -> bool {
    SYSTEM_PUBKEYS.iter().any(|k| k == pubkey)
}

/// Verify that this binary has a valid Ed25519 signature
///
/// Returns Ok(signature_hex) ONLY if signature is present and valid
/// Returns Err for any other condition (missing signature, tampered binary, invalid signature)
pub fn verify_binary_hash() -> Result<String, String> {
    // Read our own executable
    let exe_path =
        std::env::current_exe().map_err(|e| format!("Failed to get executable path: {}", e))?;

    let mut exe_data =
        std::fs::read(&exe_path).map_err(|e| format!("Failed to read executable: {}", e))?;

    // Check if binary has signature appended (last 64 bytes)
    if exe_data.len() < 64 {
        return Err("Binary too small - signature verification failed!".to_string());
    }

    // Extract appended signature (last 64 bytes)
    let signature_bytes = exe_data.split_off(exe_data.len() - 64);

    // Check if it looks like zeros (signature was stripped or never added)
    if signature_bytes.iter().all(|&b| b == 0) {
        return Err("Binary signature missing - executable must be signed!".to_string());
    }

    // Parse signature
    let signature = Signature::from_bytes(
        signature_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid signature format".to_string())?,
    );

    // Hash the binary (without the appended signature)
    let hash = blake3::hash(&exe_data);

    // Load public key
    let verifying_key = VerifyingKey::from_bytes(&AUTHOR_PUBKEY)
        .map_err(|e| format!("Invalid public key: {}", e))?;

    // Verify signature
    verifying_key
        .verify(hash.as_bytes(), &signature)
        .map_err(|_| "Signature verification failed - binary corrupted or modified".to_string())?;

    Ok(hex::encode(signature.to_bytes()).to_uppercase())
}
