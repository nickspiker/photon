//! The recording spool (docs/calls.md) — recording by default, ENDPOINT MEMORY, not wire retention.
//!
//! The wire story is untouched: media step-keys still zeroize as the call runs; the spool is the endpoint keeping audio it already legitimately had — the already-ENCODED frames, so an hour of call costs ~25MB and "record" is just not-discarding what Opus produced. Each record is individually sealed under a random per-call **spool key** held in RAM (a [`SpoolTicket`]): at hangup, *delete* = drop the ticket (key zeroizes, the file is ciphertext-garbage — instant true shred), *keep* = decrypt once into the call container and store it as an ordinary content-addressed blob. An app crash before the decision is equivalent to delete — the key lived nowhere else, which is the honest default.
//!
//! Container (one SEGMENT — this device's view; multi-segment reassembly rides the handoff work): `"PHCALL1\0"` then records of `[dir u8 (0=local mic, 1=remote)] [osc i64 LE] [len u16 LE] [opus frame]`. Every frame eagle-stamped so notes/transcription later are annotation layers on the same timeline.
//!
//! A KEPT call is fleet-internal: the blob + an attachment-style row that is inserted locally and pushed to OUR siblings only — never chain-transmitted. The friend's fleet keeps (or deletes) its own recording; neither side is the other's archive.

use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305};
use std::io::Write;
use zeroize::Zeroize;

pub const CONTAINER_MAGIC: &[u8; 8] = b"PHCALL1\0";

/// The keep/delete decision material — outlives the engine thread. Dropping it without `finalize` IS the shred (the key zeroizes; the spool file becomes garbage).
pub struct SpoolTicket {
    key: [u8; 32],
    pub path: std::path::PathBuf,
}

impl Drop for SpoolTicket {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Mint a fresh spool (key + path) for a call. The engine writes it; the ticket decides its fate.
pub fn mint(call_id8: &[u8; 8]) -> Option<([u8; 32], std::path::PathBuf, SpoolTicket)> {
    let dir = crate::storage::runtime_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("callspool-{}.tmp", hex::encode(call_id8)));
    let key: [u8; 32] = rand::random();
    Some((
        key,
        path.clone(),
        SpoolTicket { key, path },
    ))
}

/// The engine-side writer. Appends sealed records; closing is just dropping (the ticket owns the fate).
pub struct SpoolWriter {
    cipher: XChaCha20Poly1305,
    file: std::fs::File,
    counter: u64,
}

impl SpoolWriter {
    pub fn create(key: &[u8; 32], path: &std::path::Path) -> Option<SpoolWriter> {
        let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
        let file = std::fs::File::create(path).ok()?;
        Some(SpoolWriter {
            cipher,
            file,
            counter: 0,
        })
    }

    /// One encoded frame: dir 0 = local mic, 1 = remote. Failures are logged-not-fatal — a full disk must not kill the call.
    pub fn append(&mut self, dir: u8, osc: i64, opus: &[u8]) {
        let mut plain = Vec::with_capacity(1 + 8 + opus.len());
        plain.push(dir);
        plain.extend_from_slice(&osc.to_le_bytes());
        plain.extend_from_slice(opus);
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&self.counter.to_le_bytes());
        self.counter += 1;
        let Ok(sealed) = self.cipher.encrypt(&nonce.into(), plain.as_slice()) else {
            return;
        };
        let _ = self.file.write_all(&(sealed.len() as u16).to_le_bytes());
        let _ = self.file.write_all(&sealed);
    }
}

/// KEEP: decrypt the spool into the container and store it as a content-addressed blob (sealed at rest under the device blob key like any attachment). Returns (content_hash, size). Consumes the ticket; the spool file is removed after a successful store. A truncated tail record (engine mid-write at the stop edge) ends the read — one lost frame, never an error.
pub fn finalize(ticket: SpoolTicket, identity_seed: &[u8; 32]) -> Option<([u8; 32], u64)> {
    let cipher = XChaCha20Poly1305::new_from_slice(&ticket.key).ok()?;
    let bytes = std::fs::read(&ticket.path).ok()?;
    let mut container = Vec::with_capacity(bytes.len() + 8);
    container.extend_from_slice(CONTAINER_MAGIC);
    let mut off = 0usize;
    let mut counter = 0u64;
    while off + 2 <= bytes.len() {
        let len = u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap()) as usize;
        off += 2;
        if off + len > bytes.len() {
            break; // truncated tail — the stop-edge race, at most one frame
        }
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&counter.to_le_bytes());
        counter += 1;
        let Ok(plain) = cipher.decrypt(&nonce.into(), &bytes[off..off + len]) else {
            break; // corruption past here — keep what decrypted
        };
        off += len;
        if plain.len() < 9 {
            continue;
        }
        container.extend_from_slice(&plain[..1]); // dir
        container.extend_from_slice(&plain[1..9]); // osc
        container.extend_from_slice(&((plain.len() - 9) as u16).to_le_bytes());
        container.extend_from_slice(&plain[9..]);
    }
    if container.len() <= CONTAINER_MAGIC.len() {
        shred(ticket);
        return None; // nothing recorded — treat keep as delete
    }
    let hash = *blake3::hash(&container).as_bytes();
    let size = container.len() as u64;
    crate::storage::blob_store(identity_seed, &hash, &container).ok()?;
    let _ = std::fs::remove_file(&ticket.path);
    Some((hash, size))
}

/// DELETE: remove the file; the key dies with the ticket drop. (The removal is defence-in-depth — the shred already happened when the key ceased to exist.)
pub fn shred(ticket: SpoolTicket) {
    let _ = std::fs::remove_file(&ticket.path);
    drop(ticket);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spool_round_trips_and_shreds() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("photon-spool-test-{}.tmp", std::process::id()));
        let key: [u8; 32] = [7u8; 32];
        {
            let mut w = SpoolWriter::create(&key, &path).unwrap();
            w.append(0, 1000, &[0xAA; 40]);
            w.append(1, 1010, &[0xBB; 42]);
        }
        // Decrypt through the finalize parser (without blob_store — replicate its loop here against the file).
        let ticket = SpoolTicket {
            key,
            path: path.clone(),
        };
        let cipher = XChaCha20Poly1305::new_from_slice(&ticket.key).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let len = u16::from_le_bytes(bytes[..2].try_into().unwrap()) as usize;
        let nonce = [0u8; 24];
        let plain = cipher.decrypt(&nonce.into(), &bytes[2..2 + len]).unwrap();
        assert_eq!(plain[0], 0);
        assert_eq!(i64::from_le_bytes(plain[1..9].try_into().unwrap()), 1000);
        assert_eq!(&plain[9..], &[0xAA; 40]);
        // Wrong key = garbage (the shred story): a fresh key cannot open record 0.
        let other = XChaCha20Poly1305::new_from_slice(&[8u8; 32]).unwrap();
        assert!(other.decrypt(&nonce.into(), &bytes[2..2 + len]).is_err());
        shred(ticket);
        assert!(!path.exists());
    }
}
