//! Bridge-pigeon receive: sealed chunks land in a zero-bitmap spool, and a completed spool decrypts once and lands in the host's shell directory (docs/PT.md "Spooled receive"; the wire is `pigeon_chunk`, distinct from `attach_chunk` so a drop never touches the vault-install path).
//!
//! This is the consolidation's first live tenant: PT moves the packets, `storage::spool` is custody and resume, the five-scalar prelude is the manifest, and verification is one whole-file hash at finalize with per-chunk AEAD tags falling out of the decrypt. A pigeon is ephemeral by construction — the spool lives in the runtime dir, decrypts into the shell's cwd on completion, and both the spool and its bytes are shed. Nothing here persists into the vault, because a dropped file is a one-device transfer, not fleet content to replicate.
//!
//! Ordering is arbitrary and restart is free: the spool is the only state, so a chunk that arrives twice is an idempotent rewrite and a chunk that never arrives reads missing from the file itself. The receiver holds only the open spool handle plus what the announcement told it, all of which `spool::resume` can rebuild from disk.

use crate::storage::spool::{self, SpoolDesc, SpoolLayout};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

/// One inbound pigeon mid-flight: the spool file and everything needed to finalize it. Keyed in [`PigeonReceiver`] by the whole-file plaintext hash the announcement named.
struct Inflight {
    desc: SpoolDesc,
    layout: SpoolLayout,
    file: File,
    path: PathBuf,
    /// The wire seal key for this sender (the fleet key for a sibling bridge). Held so finalize can open each chunk.
    wire_key: [u8; 32],
    /// Which sender device announced it — the trust gate re-checks each chunk's signer against this.
    from_device: [u8; 32],
}

/// What a completed pigeon produced: the file landed at this path in the host's shell directory. The receiver has already shed the spool and forgotten the transfer.
#[derive(Debug, PartialEq)]
pub struct Landed {
    pub name: String,
    pub path: String,
    pub from_device: [u8; 32],
}

/// Host-side pigeon receiver. Owns the in-flight spools; the caller feeds it announcements and chunks off the wire and asks it to finalize into a directory once a spool is whole.
#[derive(Default)]
pub struct PigeonReceiver {
    inflight: HashMap<[u8; 32], Inflight>,
}

/// The seal overhead every chunk carries — kete's XChaCha20-Poly1305 nonce (24) + tag (16). Stored in the spool prelude so the primitive stays transport-agnostic, but the pigeon path knows it directly.
pub const PIGEON_SEAL_OVERHEAD: u32 = 24 + 16;

impl PigeonReceiver {
    /// A drop was announced: open (or resume) the spool for it. `chunk_size` is the sender's plaintext chunk granularity (`storage::BLOB_CHUNK_SIZE` today). Idempotent — a re-announced pigeon already in flight keeps its spool and its arrived chunks.
    pub fn announce(&mut self, dir: &std::path::Path, hash: [u8; 32], name: String, size: u64, chunk_size: u32, wire_key: [u8; 32], from_device: [u8; 32]) -> std::io::Result<()> {
        if self.inflight.contains_key(&hash) {
            return Ok(());
        }
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(format!("pigeon-{}.spool", hex::encode(&hash[..8])));
        let desc = SpoolDesc { name, hash, size, chunk_size, seal_overhead: PIGEON_SEAL_OVERHEAD };
        // Resume if a prior session left a spool for this exact transfer; a mismatched or torn one is replaced.
        let (file, layout) = match spool::resume(&path)? {
            Some((d, lay, f)) if d.hash == hash && d.size == size => (f, lay),
            _ => spool::create(&path, &desc)?,
        };
        self.inflight.insert(hash, Inflight { desc, layout, file, path, wire_key, from_device });
        Ok(())
    }

    /// True once every chunk of this pigeon is present (the zero-scan says nothing is missing).
    pub fn is_complete(&self, hash: &[u8; 32]) -> std::io::Result<bool> {
        match self.inflight.get(hash) {
            Some(inf) => Ok(spool::missing(&inf.file, &inf.layout)?.iter().all(|m| !m)),
            None => Ok(false),
        }
    }

    /// Land one sealed chunk. The signer must match the announcer (the caller has already verified the frame's signature; this rejects a valid frame from the wrong device). A chunk for an unknown or out-of-range slot is dropped, not an error — a late frame after finalize is a natural no-op.
    pub fn chunk(&mut self, hash: &[u8; 32], idx: u32, sealed: &[u8], signer: &[u8; 32]) -> std::io::Result<()> {
        let Some(inf) = self.inflight.get(hash) else {
            return Ok(());
        };
        if signer != &inf.from_device {
            return Ok(());
        }
        if (idx as usize) >= inf.layout.lens.len() {
            return Ok(());
        }
        spool::write_slot(&inf.file, &inf.layout, idx as usize, sealed)
    }

    /// Finalize a whole spool into `dir`: decrypt each slot in order, verify the reassembled plaintext against the announced hash, land it under a non-overwriting name, and shed the spool. Returns None if the pigeon is unknown, incomplete, a chunk fails to open (a torn slot the zero-scan should have caught, so this is defence in depth), or the whole-file hash mismatches. On any failure the spool is kept for a re-fetch, EXCEPT a hash mismatch, which sheds it — a spool that completed to the wrong bytes is poison, not a resumable state.
    pub fn finalize(&mut self, hash: &[u8; 32], seed: &[u8; 32], dir: &std::path::Path) -> Option<Landed> {
        if !self.is_complete(hash).ok()? {
            return None;
        }
        let inf = self.inflight.get(hash)?;
        let mut whole = Vec::with_capacity(inf.desc.size as usize);
        for i in 0..inf.layout.lens.len() {
            let sealed = spool::read_slot(&inf.file, &inf.layout, i).ok()?;
            match crate::storage::decrypt_bytes(&sealed, &inf.wire_key) {
                Ok(plain) => whole.extend_from_slice(&plain),
                Err(e) => {
                    crate::logf!("PIGEON: chunk {} failed to open — spool kept for re-fetch: {}", i, e);
                    return None;
                }
            }
        }
        if *blake3::hash(&whole).as_bytes() != *hash {
            crate::logf!("PIGEON: reassembled bytes do not match the announced hash — shedding the poisoned spool");
            let inf = self.inflight.remove(hash).unwrap();
            let _ = std::fs::remove_file(&inf.path);
            return None;
        }
        // The bytes are whole and verified. Store them once so land_blob can stream from the vault, then land into the shell directory and shed everything.
        let inf = self.inflight.remove(hash)?;
        let name = inf.desc.name.clone();
        if let Err(e) = crate::storage::blob_store(seed, hash, &whole) {
            crate::logf!("PIGEON: could not store the landed bytes: {}", e);
            let _ = std::fs::remove_file(&inf.path);
            return None;
        }
        let landed = crate::storage::land_blob(seed, hash, dir, &name);
        // The pigeon is ephemeral: shed the spool and the vault copy whatever the landing did.
        let _ = std::fs::remove_file(&inf.path);
        crate::storage::blob_delete(hash);
        let path = landed?;
        Some(Landed { name, path, from_device: inf.from_device })
    }

    /// Forget an in-flight pigeon and shed its spool (a bridge session reset, or the operator withdrawing a drop). A no-op if it already finalized.
    pub fn drop_pigeon(&mut self, hash: &[u8; 32]) {
        if let Some(inf) = self.inflight.remove(hash) {
            let _ = std::fs::remove_file(&inf.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("photon-pigeonrecv-{}-{}", std::process::id(), name))
    }

    /// The whole receive arc against in-memory sealed chunks: announce, feed chunks OUT OF ORDER, complete only when the last lands, then finalize — the bytes match, the file lands under its name, and both the spool and the vault copy are shed. Everything but the wire is exercised.
    #[test]
    fn out_of_order_chunks_reassemble_and_land() {
        crate::storage::isolate_test_storage();
        let seed = [0x70u8; 32];
        let _v = crate::storage::open_session_vault(seed, [0x72; 32], [0x74; 32]).expect("vault");
        let wire_key = [0x99u8; 32];
        let from = [0xD1u8; 32];

        // A three-and-a-bit-chunk file.
        let chunk_size = crate::storage::BLOB_CHUNK_SIZE as u32;
        let plain: Vec<u8> = (0..(chunk_size as usize * 3 + 777)).map(|i| (i * 7 % 251) as u8).collect();
        let hash = *blake3::hash(&plain).as_bytes();
        let chunks: Vec<Vec<u8>> = plain.chunks(chunk_size as usize).map(|c| crate::storage::encrypt_bytes(c, &wire_key).expect("seal")).collect();

        let spool_dir = scratch("spool");
        let land_dir = scratch("land");
        let _ = std::fs::remove_dir_all(&spool_dir);
        let _ = std::fs::remove_dir_all(&land_dir);

        let mut rx = PigeonReceiver::default();
        rx.announce(&spool_dir, hash, "dropped.bin".into(), plain.len() as u64, chunk_size, wire_key, from).expect("announce");

        // Out of order, and a duplicate, and a wrong-signer frame that must be ignored.
        for &i in &[2usize, 0, 3] {
            rx.chunk(&hash, i as u32, &chunks[i], &from).expect("chunk");
        }
        assert!(!rx.is_complete(&hash).unwrap(), "not complete — chunk 1 is missing");
        rx.chunk(&hash, 1, &chunks[1], &[0xEE; 32]).expect("wrong signer");
        assert!(!rx.is_complete(&hash).unwrap(), "a wrong-signer chunk must not count");
        rx.chunk(&hash, 2, &chunks[2], &from).expect("dup"); // idempotent
        rx.chunk(&hash, 1, &chunks[1], &from).expect("chunk");
        assert!(rx.is_complete(&hash).unwrap(), "all four present now");

        let landed = rx.finalize(&hash, &seed, &land_dir).expect("finalize lands");
        assert_eq!(landed.name, "dropped.bin");
        assert!(landed.path.ends_with("dropped.bin"));
        assert_eq!(std::fs::read(&landed.path).unwrap(), plain, "the landed file is the sender's exact bytes");
        // Ephemeral: spool gone, vault copy gone, transfer forgotten.
        assert!(!spool_dir.join(format!("pigeon-{}.spool", hex::encode(&hash[..8]))).exists(), "spool shed");
        assert!(!crate::storage::blob_present(&hash), "vault copy shed");
        assert!(rx.finalize(&hash, &seed, &land_dir).is_none(), "a finalized pigeon is forgotten");
        let _ = std::fs::remove_dir_all(&spool_dir);
        let _ = std::fs::remove_dir_all(&land_dir);
    }

    /// Poisoned completion: every chunk present and individually openable, but one carries the wrong bytes, so the whole-file hash fails. The spool is SHED, not kept — a spool that completed to wrong bytes would resume to the same wrong bytes forever.
    #[test]
    fn a_hash_mismatch_sheds_rather_than_resumes() {
        crate::storage::isolate_test_storage();
        let seed = [0x70u8; 32];
        let _v = crate::storage::open_session_vault(seed, [0x72; 32], [0x74; 32]).expect("vault");
        let wire_key = [0x88u8; 32];
        let from = [0xC1u8; 32];
        let chunk_size = crate::storage::BLOB_CHUNK_SIZE as u32;
        let plain: Vec<u8> = (0..(chunk_size as usize + 10)).map(|i| (i % 251) as u8).collect();
        let announced = *blake3::hash(&plain).as_bytes();
        let mut chunks: Vec<Vec<u8>> = plain.chunks(chunk_size as usize).map(|c| crate::storage::encrypt_bytes(c, &wire_key).expect("seal")).collect();
        // Reseal the tail from a SAME-LENGTH but different plaintext: the slot length still matches the layout, the AEAD tag still opens, but the reassembled whole hashes wrong — the one way to reach the poison path rather than the torn-chunk path.
        let tail_len = plain.len() - chunk_size as usize;
        chunks[1] = crate::storage::encrypt_bytes(&vec![0xFFu8; tail_len], &wire_key).expect("seal");

        let spool_dir = scratch("poison");
        let _ = std::fs::remove_dir_all(&spool_dir);
        let mut rx = PigeonReceiver::default();
        rx.announce(&spool_dir, announced, "x.bin".into(), plain.len() as u64, chunk_size, wire_key, from).expect("announce");
        for (i, c) in chunks.iter().enumerate() {
            rx.chunk(&announced, i as u32, c, &from).expect("chunk");
        }
        assert!(rx.is_complete(&announced).unwrap(), "all slots non-zero");
        assert!(rx.finalize(&announced, &seed, &scratch("poisonland")).is_none(), "a hash mismatch refuses to land");
        assert!(!spool_dir.join(format!("pigeon-{}.spool", hex::encode(&announced[..8]))).exists(), "the poisoned spool is shed, never resumed");
        let _ = std::fs::remove_dir_all(&spool_dir);
    }
}
