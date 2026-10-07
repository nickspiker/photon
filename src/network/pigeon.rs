//! Bridge-pigeon receive: sealed chunks land in a zero-bitmap spool, and a completed spool decrypts once and lands in the host's shell directory (docs/PT.md "Pigeons, v2"; the wire is `pigeon_chunk` / `pigeon_want` / `pigeon_ack`, distinct from the attachment frames).
//!
//! PT moves the packets — every chunk its own stream, lettered a..z inside the send window — `storage::spool` is custody and resume, the prelude is the manifest, and verification is one whole-file hash at finalize with per-chunk integrity from the AEAD tags.
//!
//! NO BITMAP (Nick 2026-10-07: "no bitmap is needed since we store it encrypted"): a written slot is ciphertext and an unwritten one is zeros, so the spool file is the only record of what has landed. The receiver keeps a COUNT (re-derivable from the file at any moment, so nothing can drift) and computes a repair ask from the zero-scan when it asks, never storing the list.
//!
//! v2 (2026-10-07, the LAN drop whose chunks beat their announcement): every chunk SELF-DESCRIBES (size + sealed name), so the first one to arrive opens the spool and order stops mattering; the host REPAIRS what is missing by asking (`pigeon_want`) — on resume, and when a pigeon stops moving — and the sender keeps its copy until the host reports every slot held.

use crate::storage::spool::{self, SpoolDesc, SpoolLayout};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One inbound pigeon mid-flight: the spool file and everything needed to finalize it. Keyed in [`PigeonReceiver`] by the whole-file plaintext hash. Detachable (`take_complete`) so the finalize decrypt-walk runs on a worker.
pub struct Inflight {
    desc: SpoolDesc,
    layout: SpoolLayout,
    file: File,
    path: PathBuf,
    /// The wire seal key for this sender (the fleet key for a sibling bridge). Held so finalize can open each chunk.
    wire_key: [u8; 32],
    /// Which device the slots come from — the trust gate re-checks each chunk's signer against this, and repair asks go to it.
    from_device: [u8; 32],
    /// Slots held: counted ONCE from the spool's zero-scan at open, then +1 when a frame fills a slot that read empty. Never a list — the file is the list.
    held: u32,
    /// The last time a slot filled (or the spool opened) — a pigeon that stops moving is asked to repair.
    last_progress: Instant,
    /// Repair asks sent since the last progress.
    asks: u8,
}

impl Inflight {
    /// The device this pigeon came from.
    pub fn from_device(&self) -> [u8; 32] {
        self.from_device
    }
}

/// What a completed pigeon produced: the file landed at this path in the host's shell directory. The receiver has already shed the spool and forgotten the transfer.
#[derive(Debug, PartialEq)]
pub struct Landed {
    pub name: String,
    pub path: String,
    pub from_device: [u8; 32],
}

/// Host-side pigeon receiver. Owns the in-flight spools; the caller feeds it announcements and chunks off the wire, asks it what to repair, and asks it to finalize into a directory once a spool is whole.
#[derive(Default)]
pub struct PigeonReceiver {
    inflight: HashMap<[u8; 32], Inflight>,
}

/// The seal overhead every chunk carries — kete's XChaCha20-Poly1305 nonce (24) + tag (16). Stored in the spool prelude so the primitive stays transport-agnostic, but the pigeon path knows it directly.
pub const PIGEON_SEAL_OVERHEAD: u32 = 24 + 16;

/// A pigeon with no slot filled for this long is asked to repair; after [`MAX_REPAIR_ASKS`] unanswered asks it is let go (the spool stays on disk — the next session resumes it).
pub const REPAIR_AFTER: Duration = Duration::from_secs(20);
pub const MAX_REPAIR_ASKS: u8 = 8;

/// The spool file for a transfer, under the receiver's directory.
fn spool_path(dir: &std::path::Path, hash: &[u8; 32]) -> PathBuf {
    dir.join(format!("pigeon-{}.spool", hex::encode(&hash[..8])))
}

impl PigeonReceiver {
    /// Open (or resume) the spool for a pigeon — from its announcement row OR from the first self-describing chunk, whichever arrives first. Idempotent: a pigeon already in flight keeps its spool, and only gains a name it did not have. Returns (held, total, resumed) — `resumed` = the spool came off disk holding slots already, so the caller asks for the rest.
    pub fn open(&mut self, dir: &std::path::Path, hash: [u8; 32], name: String, size: u64, chunk_size: u32, wire_key: [u8; 32], from_device: [u8; 32]) -> std::io::Result<(u32, u32, bool)> {
        if let Some(inf) = self.inflight.get_mut(&hash) {
            if inf.desc.name.is_empty() && !name.is_empty() {
                inf.desc.name = name;
            }
            return Ok((inf.held, inf.layout.lens.len() as u32, false));
        }
        let _ = std::fs::create_dir_all(dir);
        let path = spool_path(dir, &hash);
        let desc = SpoolDesc { name, hash, size, chunk_size, seal_overhead: PIGEON_SEAL_OVERHEAD, peer: Some(from_device) };
        // Resume if a prior session left a spool for this exact transfer; a mismatched or torn one is replaced.
        let (file, layout, desc) = match spool::resume(&path)? {
            Some((d, lay, f)) if d.hash == hash && d.size == size => {
                let name = if d.name.is_empty() { desc.name.clone() } else { d.name.clone() };
                (f, lay, SpoolDesc { name, peer: Some(from_device), ..d })
            }
            _ => {
                let (f, lay) = spool::create(&path, &desc)?;
                (f, lay, desc)
            }
        };
        Ok(self.insert(hash, desc, layout, file, path, wire_key, from_device))
    }

    fn insert(&mut self, hash: [u8; 32], desc: SpoolDesc, layout: SpoolLayout, file: File, path: PathBuf, wire_key: [u8; 32], from_device: [u8; 32]) -> (u32, u32, bool) {
        let held = spool::missing(&file, &layout).map(|m| m.iter().filter(|x| !**x).count() as u32).unwrap_or(0);
        let total = layout.lens.len() as u32;
        self.inflight.insert(hash, Inflight { desc, layout, file, path, wire_key, from_device, held, last_progress: Instant::now(), asks: 0 });
        (held, total, held > 0)
    }

    /// RESTART: re-open every spool a previous session left in `dir` that names its sending device, and report each incomplete one with what it lacks — the caller asks that device to repair. A spool with no device on record (written before v2) cannot ask anyone; it waits for its announcement or chunks.
    pub fn rediscover(&mut self, dir: &std::path::Path, wire_key: [u8; 32]) -> Vec<([u8; 32], [u8; 32])> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            if !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("pigeon-") && n.ends_with(".spool")) {
                continue;
            }
            let Ok(Some((desc, layout, file))) = spool::resume(&path) else {
                continue;
            };
            let (hash, Some(peer)) = (desc.hash, desc.peer) else {
                continue;
            };
            if self.inflight.contains_key(&hash) {
                continue;
            }
            let (held, total, _) = self.insert(hash, desc, layout, file, path, wire_key, peer);
            if held < total {
                out.push((hash, peer));
            }
        }
        out
    }

    /// True once every chunk of this pigeon is present: the count says so AND the spool's own zero-scan agrees (one full read, at the end).
    pub fn is_complete(&self, hash: &[u8; 32]) -> std::io::Result<bool> {
        match self.inflight.get(hash) {
            Some(inf) if inf.held as usize >= inf.layout.lens.len() => Ok(spool::missing(&inf.file, &inf.layout)?.iter().all(|m| !m)),
            _ => Ok(false),
        }
    }

    /// How far along one pigeon is: (chunks landed, chunks in all). None for a pigeon this receiver does not hold.
    pub fn progress(&self, hash: &[u8; 32]) -> Option<(u32, u32)> {
        let inf = self.inflight.get(hash)?;
        Some((inf.held, inf.layout.lens.len() as u32))
    }

    /// The name a pigeon will land under — empty until its announcement or a named chunk supplied one.
    pub fn name(&self, hash: &[u8; 32]) -> Option<&str> {
        self.inflight.get(hash).map(|i| i.desc.name.as_str())
    }

    /// True when this receiver holds the pigeon.
    pub fn holds(&self, hash: &[u8; 32]) -> bool {
        self.inflight.contains_key(hash)
    }

    /// Land one sealed chunk. The signer must match the sending device (the caller has already verified the frame's signature; this rejects a valid frame from the wrong device). A chunk for an unknown or out-of-range slot is refused.
    /// The ONE slot is checked before the write: a slot that already reads present is left alone (a duplicate moves nothing and is never counted twice).
    /// Returns the pigeon's progress after this chunk — (landed, total) — when the chunk was accepted, None when it was refused.
    pub fn chunk(&mut self, hash: &[u8; 32], idx: u32, sealed: &[u8], signer: &[u8; 32]) -> std::io::Result<Option<(u32, u32)>> {
        let Some(inf) = self.inflight.get_mut(hash) else {
            return Ok(None);
        };
        if signer != &inf.from_device || (idx as usize) >= inf.layout.lens.len() {
            return Ok(None);
        }
        if spool::slot_missing(&inf.file, &inf.layout, idx as usize)? {
            spool::write_slot(&inf.file, &inf.layout, idx as usize, sealed)?;
            inf.held += 1;
            inf.last_progress = Instant::now();
            inf.asks = 0;
        }
        Ok(Some((inf.held, inf.layout.lens.len() as u32)))
    }

    /// THE REPAIR ASK: the slots this pigeon still lacks, read off the spool's zero-scan right now, and the device to ask. Never stored.
    pub fn want(&self, hash: &[u8; 32]) -> Option<(Vec<u32>, [u8; 32])> {
        let inf = self.inflight.get(hash)?;
        let missing = spool::missing(&inf.file, &inf.layout).ok()?;
        let want: Vec<u32> = missing.iter().enumerate().filter(|(_, m)| **m).map(|(i, _)| i as u32).collect();
        (!want.is_empty()).then_some((want, inf.from_device))
    }

    /// Pigeons that stopped moving: incomplete, nothing landed for [`REPAIR_AFTER`]. Each due one counts an ask and is returned to be asked; one past [`MAX_REPAIR_ASKS`] is returned in the second list, let go from RAM (its spool stays on disk for the next session).
    pub fn stalled(&mut self, now: Instant) -> (Vec<[u8; 32]>, Vec<([u8; 32], String, [u8; 32])>) {
        let (mut ask, mut gone) = (Vec::new(), Vec::new());
        for (h, inf) in self.inflight.iter_mut() {
            if (inf.held as usize) >= inf.layout.lens.len() || now.duration_since(inf.last_progress) < REPAIR_AFTER {
                continue;
            }
            inf.last_progress = now;
            inf.asks += 1;
            if inf.asks > MAX_REPAIR_ASKS {
                gone.push((*h, inf.desc.name.clone(), inf.from_device));
            } else {
                ask.push(*h);
            }
        }
        for (h, _, _) in &gone {
            self.inflight.remove(h);
        }
        (ask, gone)
    }

    /// Detach a COMPLETE, NAMED pigeon so `finalize_inflight` can run it on a worker thread. None if unknown, not yet whole, or still nameless (the announcement row has not arrived — the landing waits for it).
    pub fn take_complete(&mut self, hash: &[u8; 32]) -> Option<Inflight> {
        if !self.is_complete(hash).ok()? || self.inflight.get(hash).is_some_and(|i| i.desc.name.is_empty()) {
            return None;
        }
        self.inflight.remove(hash)
    }
}

/// The decrypt-walk + whole-file hash + landing + shed for one detached pigeon, safe on any thread (it owns the spool handle). Err returns the pigeon for a chunk that would not open (resumable); a hash mismatch or a store failure sheds the spool and returns Ok(None) — a spool that completed to the wrong bytes is poison, not a resumable state.
pub fn finalize_inflight(inf: Inflight, seed: &[u8; 32], dir: &std::path::Path) -> Result<Option<Landed>, Inflight> {
    let hash = inf.desc.hash;
    let mut whole = Vec::with_capacity(inf.desc.size as usize);
    for i in 0..inf.layout.lens.len() {
        let Ok(sealed) = spool::read_slot(&inf.file, &inf.layout, i) else {
            return Err(inf);
        };
        match crate::storage::decrypt_bytes(&sealed, &inf.wire_key) {
            Ok(plain) => whole.extend_from_slice(&plain),
            Err(e) => {
                crate::logf!("PIGEON: chunk {} failed to open — spool kept for re-fetch: {}", i, e);
                return Err(inf);
            }
        }
    }
    if *blake3::hash(&whole).as_bytes() != hash {
        crate::logf!("PIGEON: reassembled bytes do not match the announced hash — shedding the poisoned spool");
        let _ = std::fs::remove_file(&inf.path);
        return Ok(None);
    }
    // The bytes are whole and verified. Store them once so land_blob can stream from the vault, then land into the shell directory and shed everything.
    let name = inf.desc.name.clone();
    if let Err(e) = crate::storage::blob_store(seed, &hash, &whole) {
        crate::logf!("PIGEON: could not store the landed bytes: {}", e);
        let _ = std::fs::remove_file(&inf.path);
        return Ok(None);
    }
    let landed = crate::storage::land_blob(seed, &hash, dir, &name);
    // The pigeon is ephemeral: shed the spool and the vault copy whatever the landing did.
    let _ = std::fs::remove_file(&inf.path);
    crate::storage::blob_delete(&hash);
    Ok(landed.map(|path| Landed { name, path, from_device: inf.from_device }))
}

impl PigeonReceiver {

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
        rx.open(&spool_dir, hash, "dropped.bin".into(), plain.len() as u64, chunk_size, wire_key, from).expect("open");

        // Out of order, and a duplicate, and a wrong-signer frame that must be ignored.
        for &i in &[2usize, 0, 3] {
            rx.chunk(&hash, i as u32, &chunks[i], &from).expect("chunk");
        }
        assert!(!rx.is_complete(&hash).unwrap(), "not complete — chunk 1 is missing");
        assert_eq!(rx.progress(&hash), Some((3, 4)), "three of four landed");
        assert_eq!(rx.chunk(&hash, 1, &chunks[1], &[0xEE; 32]).expect("wrong signer"), None, "a wrong-signer chunk is refused");
        assert!(!rx.is_complete(&hash).unwrap(), "a wrong-signer chunk must not count");
        assert_eq!(rx.chunk(&hash, 2, &chunks[2], &from).expect("dup"), Some((3, 4)), "a duplicate is accepted and moves nothing");
        assert_eq!(rx.chunk(&hash, 1, &chunks[1], &from).expect("chunk"), Some((4, 4)));
        assert!(rx.is_complete(&hash).unwrap(), "all four present now");

        let inf = rx.take_complete(&hash).expect("complete and named");
        let landed = finalize_inflight(inf, &seed, &land_dir).ok().flatten().expect("finalize lands");
        assert_eq!(landed.name, "dropped.bin");
        assert!(landed.path.ends_with("dropped.bin"));
        assert_eq!(std::fs::read(&landed.path).unwrap(), plain, "the landed file is the sender's exact bytes");
        // Ephemeral: spool gone, vault copy gone, transfer forgotten.
        assert!(!spool_dir.join(format!("pigeon-{}.spool", hex::encode(&hash[..8]))).exists(), "spool shed");
        assert!(!crate::storage::blob_present(&hash), "vault copy shed");
        assert!(rx.take_complete(&hash).is_none() && !rx.holds(&hash), "a finalized pigeon is forgotten");
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
        rx.open(&spool_dir, announced, "x.bin".into(), plain.len() as u64, chunk_size, wire_key, from).expect("open");
        for (i, c) in chunks.iter().enumerate() {
            rx.chunk(&announced, i as u32, c, &from).expect("chunk");
        }
        assert!(rx.is_complete(&announced).unwrap(), "all slots non-zero");
        let inf = rx.take_complete(&announced).expect("complete");
        assert!(finalize_inflight(inf, &seed, &scratch("poisonland")).ok().flatten().is_none(), "a hash mismatch refuses to land");
        assert!(!spool_dir.join(format!("pigeon-{}.spool", hex::encode(&announced[..8]))).exists(), "the poisoned spool is shed, never resumed");
        let _ = std::fs::remove_dir_all(&spool_dir);
    }

    /// THE FIELD FAILURE (2026-10-07): chunks that beat their announcement. A self-describing chunk opens a NAMELESS spool, every chunk lands, the whole spool still waits for its name (no landing under an empty name), and the announcement supplies it.
    #[test]
    fn chunks_before_the_announcement_open_the_spool_and_wait_for_the_name() {
        crate::storage::isolate_test_storage();
        let wire_key = [0x55u8; 32];
        let from = [0xB2u8; 32];
        let chunk_size = crate::storage::BLOB_CHUNK_SIZE as u32;
        let plain: Vec<u8> = (0..(chunk_size as usize + 99)).map(|i| (i * 3 % 241) as u8).collect();
        let hash = *blake3::hash(&plain).as_bytes();
        let chunks: Vec<Vec<u8>> = plain.chunks(chunk_size as usize).map(|c| crate::storage::encrypt_bytes(c, &wire_key).expect("seal")).collect();
        let dir = scratch("early");
        let _ = std::fs::remove_dir_all(&dir);
        let mut rx = PigeonReceiver::default();
        // The first chunk opens the spool from the frame alone — no name yet.
        rx.open(&dir, hash, String::new(), plain.len() as u64, chunk_size, wire_key, from).expect("open from a chunk");
        assert_eq!(rx.chunk(&hash, 1, &chunks[1], &from).unwrap(), Some((1, 2)));
        assert_eq!(rx.want(&hash).map(|(w, d)| (w, d)), Some((vec![0], from)), "the repair ask names exactly the slot still empty, and whom to ask");
        assert_eq!(rx.chunk(&hash, 0, &chunks[0], &from).unwrap(), Some((2, 2)));
        assert!(rx.is_complete(&hash).unwrap());
        assert!(rx.want(&hash).is_none(), "nothing to ask for once whole");
        assert!(rx.take_complete(&hash).is_none(), "a nameless spool never lands");
        // The announcement arrives late: same spool, now named.
        assert_eq!(rx.open(&dir, hash, "late.bin".into(), plain.len() as u64, chunk_size, wire_key, from).unwrap(), (2, 2, false));
        assert_eq!(rx.name(&hash), Some("late.bin"));
        assert!(rx.take_complete(&hash).is_some(), "named and whole — it lands");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RESTART: a half-filled spool survives the process; a fresh receiver rediscovers it from the directory alone, knows whom to ask (the device rides the prelude), and asks for exactly what is missing — the count comes back from the file, there is no list to lose.
    #[test]
    fn a_restarted_host_rediscovers_and_asks_for_the_rest() {
        crate::storage::isolate_test_storage();
        let wire_key = [0x66u8; 32];
        let from = [0xC3u8; 32];
        let chunk_size = crate::storage::BLOB_CHUNK_SIZE as u32;
        let plain: Vec<u8> = (0..(chunk_size as usize * 3 + 5)).map(|i| (i * 11 % 239) as u8).collect();
        let hash = *blake3::hash(&plain).as_bytes();
        let chunks: Vec<Vec<u8>> = plain.chunks(chunk_size as usize).map(|c| crate::storage::encrypt_bytes(c, &wire_key).expect("seal")).collect();
        let dir = scratch("restart");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut rx = PigeonReceiver::default();
            rx.open(&dir, hash, "kept.bin".into(), plain.len() as u64, chunk_size, wire_key, from).unwrap();
            rx.chunk(&hash, 1, &chunks[1], &from).unwrap();
            rx.chunk(&hash, 3, &chunks[3], &from).unwrap();
        } // the process dies
        let mut rx = PigeonReceiver::default();
        let asks = rx.rediscover(&dir, wire_key);
        assert_eq!(asks, vec![(hash, from)], "the spool is found, with its device");
        assert_eq!(rx.progress(&hash), Some((2, 4)), "the count comes back from the file");
        assert_eq!(rx.want(&hash).map(|(w, _)| w), Some(vec![0, 2]));
        assert_eq!(rx.name(&hash), Some("kept.bin"));
        // A duplicate of a held slot moves nothing.
        assert_eq!(rx.chunk(&hash, 1, &chunks[1], &from).unwrap(), Some((2, 4)));
        let _ = std::fs::remove_dir_all(&dir);
    }

}
