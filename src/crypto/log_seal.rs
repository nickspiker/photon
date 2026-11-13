//! Submitted logs, sealed to the maintainer's log key alone (ferros docs/PHOTON.md M6, owner 2026-11-13: "a distinct log decrypt key").
//! A log is filed on fgtw.org under the submitter's handle proof, so typing a handle finds that person's logs; what is filed is sealed, so finding a log never opens it.
//! The seal is an ephemeral X25519 agreement with the log key's X25519 form, then XChaCha20-Poly1305 (kete's `encrypt_bytes`) under a key hashed from the agreement, the ephemeral key and the log key.
//! Before this the key came from the identity seed, so anyone who knew a person's handle, every friend among them, could open that person's logs.
//! The log key is an Ed25519 key kept in the maintainer's keys directory as VSF (`kge`, the type VSF has for a private key); its X25519 form is the standard birational map, so VSF needs no new key type.

use ed25519_dalek::{SigningKey, VerifyingKey};

/// The maintainer's log key (Ed25519 public), whose private half is `keys/photon-log-key.vsf` on the maintainer's machine only.
pub const LOG_KEY_PUBLIC: [u8; 32] = [0xfe, 0x8f, 0xbe, 0x7d, 0xac, 0x74, 0xb3, 0x76, 0x30, 0xe8, 0xd4, 0xe3, 0xb3, 0xbd, 0x55, 0x4f, 0xa1, 0xaf, 0x9e, 0x84, 0xb8, 0x33, 0x15, 0x2b, 0xf6, 0x94, 0xe6, 0xeb, 0xa0, 0x06, 0xab, 0x91];

/// Where the maintainer's tools read the log key's private half.
pub const LOG_KEY_PATH: &str = "/mnt/Harbor/Code/keys/photon-log-key.vsf";

/// The domain the seal key is hashed under.
const SEAL_CONTEXT: &str = "photon 2026-11-13 log seal to the maintainer";

/// A log as it leaves the device: the ephemeral public key, the sealed log, and the sealed note when one was typed.
pub struct SealedLog {
    pub ephemeral: [u8; 32],
    pub log: Vec<u8>,
    pub note: Option<Vec<u8>>,
}

fn seal_key(shared: &[u8; 32], ephemeral: &[u8; 32], recipient: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key(SEAL_CONTEXT);
    h.update(shared);
    h.update(ephemeral);
    h.update(recipient);
    *h.finalize().as_bytes()
}

/// Seal a log and its note to the log key whose Ed25519 public half is `recipient`.
pub fn seal(log: &[u8], note: &str, recipient: &[u8; 32]) -> Result<SealedLog, String> {
    let to = VerifyingKey::from_bytes(recipient).map_err(|e| format!("log key: {e}"))?.to_montgomery().to_bytes();
    let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let ephemeral = x25519_dalek::PublicKey::from(&secret).to_bytes();
    let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(to));
    let mut key = seal_key(shared.as_bytes(), &ephemeral, recipient);
    let sealed = (|| {
        let log = crate::storage::encrypt_bytes(log, &key)?;
        let note = if note.is_empty() { None } else { Some(crate::storage::encrypt_bytes(note.as_bytes(), &key)?) };
        Ok(SealedLog { ephemeral, log, note })
    })();
    zeroize::Zeroize::zeroize(&mut key);
    sealed
}

/// Open a sealed log (or note) with the log key's private half; `ephemeral` comes from the filed log, so it is checked as outside input.
pub fn open(sealed: &[u8], ephemeral: &[u8; 32], log_key: &SigningKey) -> Result<Vec<u8>, String> {
    let secret = x25519_dalek::StaticSecret::from(log_key.to_scalar_bytes());
    let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(*ephemeral));
    // A low-order ephemeral key makes the agreement all zeros, a key anyone can compute: such a log was never sealed to us.
    if !shared.was_contributory() {
        return Err("the log's ephemeral key is degenerate".to_string());
    }
    let mut key = seal_key(shared.as_bytes(), ephemeral, &log_key.verifying_key().to_bytes());
    let plain = crate::storage::decrypt_bytes(sealed, &key);
    zeroize::Zeroize::zeroize(&mut key);
    plain
}

/// The log key's VSF document: section `log_key`, field `seed` (`kge`).
pub fn log_key_document(seed: &[u8; 32]) -> Result<Vec<u8>, String> {
    vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .provenance_only()
        .add_section("log_key", vec![("seed".to_string(), vsf::VsfType::kge(seed.to_vec()))])
        .build()
        .map_err(|e| format!("log key document: {e}"))
}

/// Read the log key's private half from [`LOG_KEY_PATH`] (or `PHOTON_LOG_KEY`); it must be mode 0600, as the release key must.
pub fn log_key() -> Result<SigningKey, String> {
    let path = std::env::var("PHOTON_LOG_KEY").unwrap_or_else(|_| LOG_KEY_PATH.to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).map(|m| m.permissions().mode() & 0o777).map_err(|e| format!("{path}: {e} (make it with photon-keygen --log-key)"))?;
        if mode & 0o077 != 0 {
            return Err(format!("{path} is mode {mode:o}; the log key must be 0600"));
        }
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let sections = crate::storage::record::verified_sections(&bytes, None).map_err(|e| format!("{path}: {e}"))?;
    let seed = sections
        .iter()
        .filter(|s| s.name == "log_key")
        .find_map(|s| match s.get_field("seed").and_then(|f| f.values.first()) {
            Some(vsf::VsfType::kge(b)) if b.len() == 32 => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        })
        .ok_or_else(|| format!("{path}: no 32-byte log_key seed"))?;
    let key = SigningKey::from_bytes(&seed);
    // A key file that is not the compiled-in log key opens nothing a Photon sealed: say so, never print failures for every log.
    if key.verifying_key().to_bytes() != LOG_KEY_PUBLIC {
        return Err(format!("{path} is not the log key Photon seals to (LOG_KEY_PUBLIC)"));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_log_key_opens_a_sealed_log() {
        let maintainer = SigningKey::from_bytes(&[7; 32]);
        let sealed = seal(b"a log", "a note", &maintainer.verifying_key().to_bytes()).unwrap();
        assert_eq!(open(&sealed.log, &sealed.ephemeral, &maintainer).unwrap(), b"a log");
        assert_eq!(open(sealed.note.as_ref().unwrap(), &sealed.ephemeral, &maintainer).unwrap(), b"a note");
        let stranger = SigningKey::from_bytes(&[8; 32]);
        assert!(open(&sealed.log, &sealed.ephemeral, &stranger).is_err(), "another key cannot open it");
        assert!(open(&sealed.log, &[0; 32], &maintainer).is_err(), "a degenerate ephemeral key is refused");
        assert!(seal(b"x", "", &maintainer.verifying_key().to_bytes()).unwrap().note.is_none(), "no note typed, none sealed");
    }

    #[test]
    fn the_log_key_document_reads_back() {
        let doc = log_key_document(&[9; 32]).unwrap();
        let sections = crate::storage::record::verified_sections(&doc, None).unwrap();
        let s = sections.iter().find(|s| s.name == "log_key").unwrap();
        assert!(matches!(s.get_field("seed").and_then(|f| f.values.first()), Some(vsf::VsfType::kge(b)) if b == &[9u8; 32]));
    }
}
