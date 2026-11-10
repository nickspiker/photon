//! Binary self-verification using Ed25519 cryptographic signatures.
//!
//! SIGNED BINARIES ONLY: all official Photon Messenger binaries are cryptographically signed by Nick Spiker <fractaldecoder@proton.me> and verified on every startup, which prevents tampering and proves the authenticity of distributed binaries.
//!
//! End users: use the official installer rather than building from source.
//! - Linux/macOS: `curl -sSfL https://holdmyoscilloscope.com/photon/install.sh | sh`
//! - Windows: `iwr -useb https://holdmyoscilloscope.com/photon/install.ps1 | iex`
//! These download pre-built, pre-signed binaries that verify correctly.
//!
//! Source builds: `cargo install photon-messenger` from crates.io will NOT work out of the box, because crates.io ships no signing scripts or keys. To build from source you must clone the full repo, generate your own signing keys (`cargo run --bin photon-keygen`), replace `AUTHOR_PUBKEY` below with your generated public key, then build and sign (`cargo build && ./sign-after-build.sh debug`). This friction is intentional: if you build from source you should understand what you're signing and why.
//!
//! Official binaries are signed by Nick Spiker <fractaldecoder@proton.me>, public key dff3af0c127c0bebe539c421da37993a517bfd78d2f5ee491d52fbf616444747 — a software commitment that binaries bearing this signature were built and released by the original author.
use vsf::VsfType;

/// Embedded public key for signature verification and system messages. This is Nick Spiker's (fractaldecoder) signing key - used for:
/// - Binary signature verification
/// - System messages (updates, security notices, etc.)
///
/// Messages signed by this key are official Photon communications. Public key (hex): dff3af0c127c0bebe539c421da37993a517bfd78d2f5ee491d52fbf616444747
pub const AUTHOR_PUBKEY: [u8; 32] = [
    0xdf, 0xf3, 0xaf, 0x0c, 0x12, 0x7c, 0x0b, 0xeb, 0xe5, 0x39, 0xc4, 0x21, 0xda, 0x37, 0x99, 0x3a,
    0x51, 0x7b, 0xfd, 0x78, 0xd2, 0xf5, 0xee, 0x49, 0x1d, 0x52, 0xfb, 0xf6, 0x16, 0x44, 0x47, 0x47,
];

/// List of trusted system pubkeys for official messages. Currently just the author - future: democratic governance for adding keys.
pub const SYSTEM_PUBKEYS: &[[u8; 32]] = &[AUTHOR_PUBKEY];

/// Check if a pubkey is a trusted system pubkey. Messages signed by system pubkeys are official Photon communications.
pub fn is_system_pubkey(pubkey: &[u8; 32]) -> bool {
    SYSTEM_PUBKEYS.iter().any(|k| k == pubkey)
}

/// Verify that this binary has a valid Ed25519 signature
///
/// Returns Ok(signature_hex) ONLY if signature is present and valid Returns Err for any other condition (missing signature, tampered binary, invalid signature)
pub fn verify_binary_hash() -> Result<String, String> {
    // Read our own executable
    let exe_path =
        std::env::current_exe().map_err(|e| format!("Failed to get executable path: {}", e))?;
    let result = verify_file(&exe_path);
    // DEVELOPMENT builds waive the SELF-check: an outside builder (clone + `cargo build --features development`) has no signing key, and hard-exiting on the missing signature made from-source photon unrunnable without editing this file (field: first outside builder, 2026-08-30). The waiver is self-only and dev-only — `verify_file` stays strict for everyone (it gates update installs), and release builds (which the update channel ships) never carry the development feature, so the distribution trust story is unchanged. A dev build that IS signed still reports its real signature.
    #[cfg(feature = "development")]
    if result.is_err() {
        return Ok("(unsigned development build — self-verify waived)".to_string());
    }
    result
}

/// Verify an arbitrary ON-DISK binary's release signature WITHOUT executing it — the update path's gate (docs/updates.md: verify before exec; running a downloaded file to "self-verify" would run unverified code).
pub fn verify_file(exe_path: &std::path::Path) -> Result<String, String> {
    let data = std::fs::read(exe_path).map_err(|e| format!("Failed to read executable: {}", e))?;
    verify_signed_binary(&data)
}

/// The fixed-width field that ends a signed binary's egg document and says how long the document is, so a reader finds it from the end (the `gH` trailer's shape).
fn length_field(doc_len: u64) -> Vec<u8> {
    VsfType::u6(doc_len).flatten()
}

/// The 64-byte Ed25519 tail older Photons check: a signature by the release key over BLAKE3 of everything before it.
/// MIGRATION-EXPIRES: 2027-02-01
const LEGACY_TAIL: usize = 64;

/// Sign a release binary (2026-11-10, "no single eggs"): `binary ‖ egg document ‖ length field ‖ legacy tail`.
/// The egg document is a complete VSF file signed by the release key with every scheme (fgtw::release), naming the binary's BLAKE3 in its `binary` section; the length field locates it from the end; the legacy tail keeps older Photons updating until the pin date.
pub fn sign_binary(binary: &[u8], release_seed: &[u8; 32]) -> Result<Vec<u8>, String> {
    use ed25519_dalek::Signer;
    let key = fgtw::pq::SigningBundle::release(release_seed);
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only_eggs(VsfType::ke(ed25519_dalek::SigningKey::from_bytes(release_seed).verifying_key().to_bytes().to_vec()), &fgtw::release::release_slots())
        .add_section("binary", vec![("hash".to_string(), VsfType::hb(blake3::hash(binary).as_bytes().to_vec()))])
        .build()
        .map_err(|e| format!("binary signature build: {e}"))?;
    let doc = fgtw::release::sign_release(unsigned, &key)?;
    let mut out = binary.to_vec();
    out.extend_from_slice(&doc);
    out.extend_from_slice(&length_field(doc.len() as u64));
    let legacy = ed25519_dalek::SigningKey::from_bytes(release_seed).sign(blake3::hash(&out).as_bytes());
    out.extend_from_slice(&legacy.to_bytes());
    Ok(out)
}

/// Check a signed release binary: the egg document at its end is signed by the release key with every egg, and names exactly the bytes before it. Returns the binary's BLAKE3 as hex.
/// The legacy tail is stripped and never relied on: the eggs are the check.
pub fn verify_signed_binary(data: &[u8]) -> Result<String, String> {
    let field_len = length_field(0).len();
    // WHY/PROOF: `data` is a file read from disk or a download, of any length; every split below is checked, because a short or truncated file would otherwise slice out of bounds and panic the self-check.
    let body = data.len().checked_sub(LEGACY_TAIL).map(|n| &data[..n]).ok_or("binary too short for its signature")?;
    let doc_end = body.len().checked_sub(field_len).ok_or("binary too short for its signature")?;
    let mut ptr = 0usize;
    let doc_len = match vsf::decoding::parse::parse(&body[doc_end..], &mut ptr) {
        Ok(VsfType::u6(n)) => usize::try_from(n).map_err(|_| "signature length out of range")?,
        _ => return Err("binary signature missing - executable must be signed with the release eggs".to_string()),
    };
    let doc_start = doc_end.checked_sub(doc_len).ok_or("signature length runs past the start of the file")?;
    let (binary, doc) = (&body[..doc_start], &body[doc_start..doc_end]);
    fgtw::release::verify_release(doc)?;
    let (header, header_end) = vsf::verification::read_verified(doc, Some(fgtw::release::RELEASE_ED25519)).map_err(|e| format!("binary signature header: {e}"))?;
    let named = header
        .sections(doc, header_end)
        .map_err(|e| format!("binary signature body: {e}"))?
        .into_iter()
        .find(|s| s.name == "binary")
        .and_then(|s| s.get_field("hash").and_then(|f| f.values.first().cloned()));
    let hash = blake3::hash(binary);
    match named {
        Some(VsfType::hb(h)) if h.as_slice() == hash.as_bytes() => Ok(hex::encode(hash.as_bytes()).to_uppercase()),
        _ => Err("Signature verification failed - binary corrupted or modified".to_string()),
    }
}

/// The release signing key, the secret half of [`AUTHOR_PUBKEY`]: `$PHOTON_SIGNING_KEY`, else the one location photon-keygen writes to.
/// Refused unless it is exactly 32 bytes and readable by its owner alone (a key another user on the host can read is not a secret).
/// The maintainer's tools sign with it: the update manifest, the release notice and an Idea's status, which fgtw.org checks against [`AUTHOR_PUBKEY`] (no token, no secret on the server).
pub fn release_signing_key() -> Result<[u8; 32], String> {
    let key_path = std::env::var("PHOTON_SIGNING_KEY").unwrap_or_else(|_| "/mnt/Harbor/Code/keys/photon-signing-key".to_string());
    if !std::path::Path::new(&key_path).exists() {
        return Err(format!("release signing key not found at {key_path} — set PHOTON_SIGNING_KEY or run photon-keygen"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&key_path).map(|m| m.permissions().mode() & 0o777).map_err(|e| format!("key metadata: {e}"))?;
        if mode & 0o077 != 0 {
            return Err(format!("{key_path} is mode {mode:o}; the release key must be 0600"));
        }
    }
    let key_bytes = std::fs::read(&key_path).map_err(|e| format!("key read: {e}"))?;
    key_bytes.as_slice().try_into().map_err(|_| "signing key file must be exactly 32 bytes".to_string())
}

#[cfg(test)]
mod release_binary_tests {
    use super::*;

    /// The legacy tail checks the way an older Photon checks it: Ed25519 by the signing key over BLAKE3 of everything before the last 64 bytes.
    #[test]
    fn the_legacy_tail_verifies_like_an_older_photon() {
        use ed25519_dalek::Verifier;
        let seed = [9u8; 32];
        let signed = sign_binary(b"a binary", &seed).unwrap();
        let (rest, tail) = signed.split_at(signed.len() - LEGACY_TAIL);
        let sig = ed25519_dalek::Signature::from_bytes(tail.try_into().unwrap());
        let vk = ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key();
        assert!(vk.verify(blake3::hash(rest).as_bytes(), &sig).is_ok());
        assert!(rest.starts_with(b"a binary"));
    }

    /// Signed by any key but the release key, the eggs are refused; short and unsigned files are refused without a panic.
    #[test]
    fn only_the_release_eggs_pass() {
        let signed = sign_binary(b"a binary", &[9u8; 32]).unwrap();
        assert!(verify_signed_binary(&signed).unwrap_err().contains("release key"));
        assert!(verify_signed_binary(&[]).is_err());
        assert!(verify_signed_binary(&[0u8; 70]).is_err());
        assert!(verify_signed_binary(b"an unsigned binary with no signature at its end, long enough to split").is_err());
        let mut cut = signed.clone();
        cut.truncate(signed.len() / 2);
        assert!(verify_signed_binary(&cut).is_err());
    }
}
