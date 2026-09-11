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

/// DURABLE SPOOL REGISTER (record-by-default, Nick 2026-09-08): the per-call spool key used to live ONLY in RAM — "crash = delete", honest under keep/delete, a LIE under record-by-default (a dead battery at 1h59m of a 2h wave threw the recording away). The register persists {key ‖ peer ‖ offer_osc} in the device vault at call start; launch-time recovery finds orphaned spool files, reads their registers, and finishes the keep the crash interrupted. Deleting a recording later is the vault's UnlinkOnly space-reclaim — the security boundary is the boot-gated vault key, not erasure (docs/vault-delete.md).
const REG_LEN: usize = 32 + 32 + 8;

fn reg_key(call_id8: &[u8; 8]) -> String {
    format!("call.spool.{}", hex::encode(call_id8))
}

/// Persist the spool register — called at call start, the moment the ticket exists.
pub fn persist_register(call_id8: &[u8; 8], ticket: &SpoolTicket, peer: &[u8; 32], offer_osc: i64) {
    let Some(v) = crate::storage::device_vault() else {
        return;
    };
    let mut reg = Vec::with_capacity(REG_LEN);
    reg.extend_from_slice(&ticket.key);
    reg.extend_from_slice(peer);
    reg.extend_from_slice(&offer_osc.to_le_bytes());
    match v.write(&reg_key(call_id8), &reg) {
        Ok(()) => crate::logf!("CALL: spool register persisted ({})", hex::encode(call_id8)),
        Err(e) => crate::logf!("CALL: spool register persist FAILED ({}) — a crash mid-wave loses this recording", e),
    }
}

/// Drop the register — the keep completed (blob stored) or the recording was deliberately discarded. Ordering law: the caller deletes the register BEFORE the spool file is removed, so every crash window resolves at recovery (file+register → re-finish the keep, idempotent by content hash; file-without-register → stray, deleted).
pub fn drop_register(call_id8: &[u8; 8]) {
    if let Some(v) = crate::storage::device_vault() {
        let _ = v.delete(&reg_key(call_id8));
    }
}

/// Launch-time recovery: every orphaned spool file whose register survives becomes a (ticket, peer, offer_osc, call_id8) ready for the normal keep-transcode; a file with no register is a stray (pre-durability, or its keep completed thru the crash window) and is deleted.
pub fn recover_orphans() -> Vec<(SpoolTicket, [u8; 32], i64, [u8; 8])> {
    let mut out = Vec::new();
    let Some(v) = crate::storage::device_vault() else {
        return out;
    };
    let dir = crate::storage::runtime_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(hexid) = name.strip_prefix("callspool-").and_then(|n| n.strip_suffix(".tmp")) else {
            continue;
        };
        let Some(id8) = hex::decode(hexid).ok().and_then(|b| <[u8; 8]>::try_from(b).ok()) else {
            continue;
        };
        match v.read(&reg_key(&id8)) {
            Ok(Some(reg)) if reg.len() == REG_LEN => {
                let key: [u8; 32] = reg[..32].try_into().unwrap();
                let peer: [u8; 32] = reg[32..64].try_into().unwrap();
                let offer_osc = i64::from_le_bytes(reg[64..72].try_into().unwrap());
                out.push((SpoolTicket { key, path: e.path() }, peer, offer_osc, id8));
            }
            _ => {
                let _ = std::fs::remove_file(e.path());
                crate::logf!("CALL: stray spool file removed ({name})");
            }
        }
    }
    out
}

/// Set on the record's channel byte when the frame is RAW little-endian i16 PCM (the plaid rung) rather than an Opus packet; the low bits stay the channel index.
pub const RAW_FLAG: u8 = 0x80;
/// Set when the record carries its WINDOW IDENTITY — `[seq u32 LE][slot u8]` between the stamp and the frame (recording fills, 2026-09-10): the wire window seq the frame travelled in and its slot inside that window. Both ends name a frame the same way, so a peer can serve exactly the windows this side lost.
pub const SEQ_FLAG: u8 = 0x40;
/// Set on a FILL: a remote frame that did NOT arrive live but was served by the peer afterwards (live re-request or the post-hangup drain). Its stamp is the time it landed, meaningless for placement — the transcode slots it by seq beside the windows that did arrive (record.rs).
pub const FILL_FLAG: u8 = 0x20;
/// Set on a RAW MIC record (2026-09-10, "mic RAW, per channel"): the frame as captured, before the canceller, the gain and the gate, followed by the verdict the live path applied — `[gain_q8 u16 LE][verdict u8]` (0 full, 1 ducked, 2 gated) between the window identity and the samples. The wire copy of the same frame rides as its own record without this flag; a keep prefers PROC records for the local channel and falls back to the wire copy for spools that predate them.
pub const PROC_FLAG: u8 = 0x10;
/// The channel index under the flag bits.
pub const CHAN_MASK: u8 = 0x0F;

/// One decrypted spool record: channel byte (flags + index), eagle stamp, the window identity when the record carries one, and the frame bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub chan: u8,
    pub osc: i64,
    pub seq: Option<(u32, u8)>,
    /// `(gain in 8.8 fixed point, verdict)` on a PROC_FLAG record — what the live path did to this raw frame.
    pub proc: Option<(u16, u8)>,
    pub bytes: Vec<u8>,
}

impl Record {
    pub fn index(&self) -> usize {
        (self.chan & CHAN_MASK) as usize
    }
    pub fn is_raw(&self) -> bool {
        self.chan & RAW_FLAG != 0
    }
    pub fn is_fill(&self) -> bool {
        self.chan & FILL_FLAG != 0
    }
    pub fn is_raw_mic(&self) -> bool {
        self.chan & PROC_FLAG != 0
    }
}

/// Where one record sits in the spool file — enough to read it back on its own (the fill server's index: window seq → the frames it sent).
#[derive(Clone, Copy, Debug)]
pub struct RecordAt {
    pub offset: u64,
    pub counter: u64,
}

fn parse_plain(plain: &[u8]) -> Option<Record> {
    if plain.len() < 9 {
        return None;
    }
    let chan = plain[0];
    let osc = i64::from_le_bytes(plain[1..9].try_into().unwrap());
    let mut at = 9usize;
    let seq = if chan & SEQ_FLAG != 0 {
        if plain.len() < at + 5 {
            return None;
        }
        let s = u32::from_le_bytes(plain[at..at + 4].try_into().unwrap());
        let slot = plain[at + 4];
        at += 5;
        Some((s, slot))
    } else {
        None
    };
    let proc = if chan & PROC_FLAG != 0 {
        if plain.len() < at + 3 {
            return None;
        }
        let gain = u16::from_le_bytes(plain[at..at + 2].try_into().unwrap());
        let verdict = plain[at + 2];
        at += 3;
        Some((gain, verdict))
    } else {
        None
    };
    Some(Record { chan, osc, seq, proc, bytes: plain[at..].to_vec() })
}

/// The engine-side writer. Appends sealed records; closing is just dropping (the ticket owns the fate).
pub struct SpoolWriter {
    cipher: XChaCha20Poly1305,
    file: std::fs::File,
    counter: u64,
    offset: u64,
}

impl SpoolWriter {
    pub fn create(key: &[u8; 32], path: &std::path::Path) -> Option<SpoolWriter> {
        let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
        // Name the failure (field 2026-09-09: Emma's phone logged "no spool" on every wave, v87 and v88, while the register minted fine — the reason was swallowed here, so the log could not say whether it was the path, the permission, or the disk).
        let file = match std::fs::File::create(path) {
            Ok(f) => f,
            Err(e) => {
                crate::logf!("CALL: spool file create FAILED at {} — {}", path.display(), e);
                return None;
            }
        };
        Some(SpoolWriter {
            cipher,
            file,
            counter: 0,
            offset: 0,
        })
    }

    /// One encoded frame: dir 0 = local mic, 1 = remote. Failures are logged-not-fatal — a full disk must not kill the call.
    pub fn append(&mut self, dir: u8, osc: i64, opus: &[u8]) {
        self.append_seq(dir & !SEQ_FLAG, osc, None, opus);
    }

    /// A frame with its window identity (`SEQ_FLAG` is set for the caller when `seq` is given). Returns where the record landed, for the fill server's index.
    pub fn append_seq(&mut self, chan: u8, osc: i64, seq: Option<(u32, u8)>, frame: &[u8]) -> Option<RecordAt> {
        self.append_seq_proc(chan & !PROC_FLAG, osc, seq, None, frame)
    }

    /// A frame with its window identity and, when `proc` is given, the live path's verdict on it (`PROC_FLAG` set for the caller).
    pub fn append_seq_proc(&mut self, chan: u8, osc: i64, seq: Option<(u32, u8)>, proc: Option<(u16, u8)>, frame: &[u8]) -> Option<RecordAt> {
        let mut plain = Vec::with_capacity(1 + 8 + 5 + 3 + frame.len());
        let mut c = chan;
        if seq.is_some() { c |= SEQ_FLAG; }
        if proc.is_some() { c |= PROC_FLAG; } else { c &= !PROC_FLAG; }
        plain.push(c);
        plain.extend_from_slice(&osc.to_le_bytes());
        if let Some((s, slot)) = seq {
            plain.extend_from_slice(&s.to_le_bytes());
            plain.push(slot);
        }
        if let Some((gain, verdict)) = proc {
            plain.extend_from_slice(&gain.to_le_bytes());
            plain.push(verdict);
        }
        plain.extend_from_slice(frame);
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&self.counter.to_le_bytes());
        let at = RecordAt { offset: self.offset, counter: self.counter };
        self.counter += 1;
        let Ok(sealed) = self.cipher.encrypt(&nonce.into(), plain.as_slice()) else {
            return None;
        };
        let _ = self.file.write_all(&(sealed.len() as u16).to_le_bytes());
        let _ = self.file.write_all(&sealed);
        self.offset += 2 + sealed.len() as u64;
        Some(at)
    }
}

/// A read-only view of a spool the engine is still writing: opens the records the writer's index names. The fill server reads the windows the peer asks for straight off disk (the page cache makes a just-written record free), so a whole call's worth of sent audio never has to sit in RAM.
pub struct SpoolReader {
    cipher: XChaCha20Poly1305,
    file: std::fs::File,
}

impl SpoolReader {
    pub fn open(key: &[u8; 32], path: &std::path::Path) -> Option<SpoolReader> {
        Some(SpoolReader { cipher: XChaCha20Poly1305::new_from_slice(key).ok()?, file: std::fs::File::open(path).ok()? })
    }

    pub fn read_at(&mut self, at: RecordAt) -> Option<Record> {
        use std::io::{Read, Seek, SeekFrom};
        self.file.seek(SeekFrom::Start(at.offset)).ok()?;
        let mut len = [0u8; 2];
        self.file.read_exact(&mut len).ok()?;
        let mut sealed = vec![0u8; u16::from_le_bytes(len) as usize];
        self.file.read_exact(&mut sealed).ok()?;
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&at.counter.to_le_bytes());
        let plain = self.cipher.decrypt(&nonce.into(), sealed.as_slice()).ok()?;
        parse_plain(&plain)
    }
}

/// Decrypt the spool into its raw records `[(channel, osc, opus)…]` in write order (`channel` is the old `dir` byte — 0=local mic, 1=remote, generalizing to a per-participant index). A truncated/corrupt tail record (engine mid-write at the stop edge) ends the read — at most one lost frame, never an error. Shared by [`finalize`] (the PHCALL1 packer) and the N-channel transcode in [`crate::call::record`], so the decrypt/nonce discipline lives in exactly one place.
pub(crate) fn drain_records(ticket: &SpoolTicket) -> Option<Vec<Record>> {
    let cipher = XChaCha20Poly1305::new_from_slice(&ticket.key).ok()?;
    let bytes = std::fs::read(&ticket.path).ok()?;
    let mut out = Vec::new();
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
        if let Some(r) = parse_plain(&plain) {
            out.push(r);
        }
    }
    Some(out)
}

/// KEEP into the flat PHCALL1 container (one segment, records `[dir][osc][len][opus]`), stored as a content-addressed blob. Returns (content_hash, size); consumes the ticket; the spool file is removed after a successful store. This is the raw (untranscoded) keep — [`crate::call::record::finalize_nchannel`] is the current keep path (a true N-channel audio file); `finalize` stays for the KAT / any consumer that wants the bare spool packed 1:1.
pub fn finalize(ticket: SpoolTicket, identity_seed: &[u8; 32]) -> Option<([u8; 32], u64)> {
    let records = drain_records(&ticket)?;
    let mut container = Vec::with_capacity(CONTAINER_MAGIC.len() + records.len() * 32);
    container.extend_from_slice(CONTAINER_MAGIC);
    for r in &records {
        if r.is_raw_mic() {
            continue; // the flat legacy container carries the wire copies only
        }
        container.push(r.chan & (CHAN_MASK | RAW_FLAG));
        container.extend_from_slice(&r.osc.to_le_bytes());
        container.extend_from_slice(&(r.bytes.len() as u16).to_le_bytes());
        container.extend_from_slice(&r.bytes);
    }
    if container.len() <= CONTAINER_MAGIC.len() {
        shred(ticket);
        return None; // nothing recorded — treat keep as delete
    }
    let hash = *blake3::hash(&container).as_bytes();
    let size = container.len() as u64;
    crate::storage::blob_store_any(identity_seed, &hash, &container).ok()?;
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
        // Decrypt thru the finalize parser (without blob_store — replicate its loop here against the file).
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

    #[test]
    fn seq_records_read_back_by_index_and_thru_the_drain() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("photon-spool-seq-test-{}.tmp", std::process::id()));
        let key: [u8; 32] = [9u8; 32];
        let mut w = SpoolWriter::create(&key, &path).unwrap();
        let a = w.append_seq(0, 5000, Some((17, 0)), &[1, 2, 3]).unwrap();
        w.append(1, 5001, &[4, 4]);
        let b = w.append_seq(1 | FILL_FLAG | RAW_FLAG, 5002, Some((40, 3)), &[7; 8]).unwrap();
        drop(w);
        let mut r = SpoolReader::open(&key, &path).unwrap();
        let ra = r.read_at(a).unwrap();
        assert_eq!(ra, Record { chan: SEQ_FLAG, osc: 5000, seq: Some((17, 0)), proc: None, bytes: vec![1, 2, 3] });
        let rb = r.read_at(b).unwrap();
        assert_eq!(rb.seq, Some((40, 3)));
        assert!(rb.is_fill() && rb.is_raw() && rb.index() == 1);
        let ticket = SpoolTicket { key, path: path.clone() };
        let all = drain_records(&ticket).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[1], Record { chan: 1, osc: 5001, seq: None, proc: None, bytes: vec![4, 4] });
        shred(ticket);
    }

    #[test]
    fn raw_mic_records_carry_their_verdict() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("photon-spool-proc-test-{}.tmp", std::process::id()));
        let key: [u8; 32] = [11u8; 32];
        let mut w = SpoolWriter::create(&key, &path).unwrap();
        let at = w.append_seq_proc(RAW_FLAG | PROC_FLAG, 7000, Some((3, 1)), Some((0x0180, 2)), &[9, 9, 9, 9]).unwrap();
        w.append_seq(0, 7000, Some((3, 1)), &[5; 12]);
        drop(w);
        let mut r = SpoolReader::open(&key, &path).unwrap();
        let rec = r.read_at(at).unwrap();
        assert!(rec.is_raw_mic() && rec.is_raw() && rec.index() == 0);
        assert_eq!(rec.proc, Some((0x0180, 2)));
        assert_eq!(rec.seq, Some((3, 1)));
        assert_eq!(rec.bytes, vec![9, 9, 9, 9]);
        let ticket = SpoolTicket { key, path: path.clone() };
        let all = drain_records(&ticket).unwrap();
        assert_eq!(all.len(), 2);
        assert!(!all[1].is_raw_mic());
        shred(ticket);
    }
}
