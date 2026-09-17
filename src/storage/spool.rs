//! The ZERO-BITMAP SPOOL — a pre-zeroed file that is its own receipt bitmap (Nick 2026-09-17: "any file stored, as long as it's encrypted, whilst receiving a stream, if you zero'd it first, functions as its own bitmap, within 256-bit blocks, with zero tracking overhead").
//!
//! THE CLAIM, proven. Pre-zero a file, then write SEALED chunks into it at their slot offsets as they arrive off the wire, in any order, across any number of restarts. Partition each slot's extent into 256-bit windows. Then at every moment: a window is all-zero if and only if no write has covered it — so scanning for zero windows reconstructs exactly which chunks are held, and the file needs no companion state at all.
//!
//! (⇐) is deterministic: writes are the only mutation, so a never-covered window keeps its initial zeros. Probability one.
//! (⇒) is where the encryption is load-bearing: a covered window holds 32 bytes of XChaCha20-Poly1305 output — nonce, keystream-XOR body, Poly1305 tag, all computationally indistinguishable from uniform random bits — so it is all-zero with probability 2^-256. Union-bound over every window of a 256 MB spool: 2^23 windows × 2^-256 = 2^-233. The content-addressing layer this rides on already treats "blake3 digests equal" as "bytes equal", accepting 2^-128 birthday risk; this claim is weaker than that one by a factor of 2^105. If a zero window ever lied, it would be far stranger than every blake3 collision the whole design already ignores.
//!
//! THE CONTRAPOSITIVE is why the premise is stated: for PLAINTEXT the scheme is unsound, not merely weaker. Real files contain honest 32-byte zero runs — executable padding, sparse media, zeroed headers — so "zero = missing" would re-fetch a present chunk forever: fetch, write zeros, read "missing", fetch again. A livelock, not a slow path. The encryption is not a confidentiality feature the bitmap happens to sit beside; it is the fact that BANISHES ZEROS FROM THE VALUE SPACE, which is the bitmap property itself. Confidentiality pays for the accounting. (The `plaintext_zeros_would_lie` test below pins the counterexample so nobody ever "optimizes" the seal away.)
//!
//! ZERO TRACKING OVERHEAD is three overheads dying at once:
//! - No bitmap persisted: the data is the bitmap.
//! - No ordering discipline between data and bookkeeping: they are the same write, atomic BY IDENTITY. The classic torn state of every sidecar-bitmap design — "bitmap says held, data missing" after a crash — is not merely avoided; it is unrepresentable.
//! - No disk for the missing part: `set_len` makes a sparse hole, so unwritten slots occupy nothing, and a filesystem with SEEK_DATA can enumerate arrivals without reading a byte of zeros. (The scan below reads, for portability; resumes are rare and it runs at memory bandwidth.)
//!
//! CRASH SOUNDNESS. Storage tears at sector granularity (≥ 512 bytes). A zero run of length ≥ 63 bytes must fully cover at least one slot-relative 32-byte window whatever its phase, so any torn or reordered page within a slot leaves a detectable zero window and the slot honestly reads missing — a re-fetch rewrites the same sealed bytes, idempotent by content addressing. The windows are SLOT-relative (slots pack tight, no alignment padding), and the tail window is end-aligned so a torn tail is caught too. The one length that could dodge the tail check is a slot under 32 bytes, which cannot occur: an AEAD seal is at least nonce + tag = 40 bytes.
//!
//! SELF-DESCRIPTION (Nick 2026-09-17, the follow-through: "the header for an encrypted VSF is not zeros and never will be — we should be able to self describe, receive and check as we go"): the spool opens with a PRELUDE — a complete VSF document carrying the transfer's own manifest (name, whole-file hash, sealed slot lengths, plaintext chunk hashes) — and the body slots follow it. So the file at the spool path is the ONLY state a resume needs: no vault entry, no sidecar, no in-RAM obligation across restarts.
//! And the prelude needs no zero-heuristic at all, which is the sharper half of the observation: a VSF document SELF-VERIFIES — `read_verified` either parses it and confirms the provenance hash, or it does not. Parse success IS the presence test, and a torn prelude fails closed into "fresh spool" rather than into a wrong layout. The zero-window rule governs the body, where every byte is AEAD ciphertext and the guarantee is airtight; the prelude, which is plaintext VSF and COULD contain an honest 32-byte zero run (a zeroed hash field, a padded value), is governed by its own hash instead. Each region is checked by the mechanism that is actually sound for it.
//! The prelude rides behind an 8-byte little-endian length — LOCAL file framing, never wire (the transport rule "complete VSF files only" is about what travels; this is a file layout, and the length is what lets the exact document slice reach `read_verified` without guessing).
//!
//! Custody: the spool holds wire-sealed bytes, so a spool on disk discloses exactly what the wire disclosed — nothing (the prelude adds the name and hashes, which the announcement already disclosed to this device). Built for the bridge pigeon (ephemeral by construction: spool beside the landing directory, decrypt at finalize, shed both), but the primitive is transport-agnostic: any resumable sealed stream can spool this way.

use std::fs::File;
use std::io;
use std::path::Path;
use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::VsfType;

/// The window width: 256 bits. Per-window false-present probability 2^-256; also one SIMD register, so the scan is a memory-bandwidth `all_zero` sweep.
pub const SPOOL_WINDOW: usize = 32;

/// Everything the transfer IS, carried by the spool itself: the landing name, the whole-file plaintext hash, the sealed slot lengths (the body layout), and the per-chunk plaintext hashes so finalize verifies chunk-by-chunk as it decrypts — no whole-file second pass, no other record.
#[derive(Clone, Debug, PartialEq)]
pub struct SpoolDesc {
    pub name: String,
    pub hash: [u8; 32],
    pub size: u64,
    pub sealed_lens: Vec<u64>,
    pub chunk_hashes: Vec<[u8; 32]>,
}

/// Where each slot's sealed bytes live: `base` is the prelude region's end, then tight-packed prefix sums — total is base plus the sealed sum, and everything is derivable from the prelude alone.
pub struct SpoolLayout {
    pub base: u64,
    pub offsets: Vec<u64>,
    pub lens: Vec<u64>,
    pub total: u64,
}

/// Build the body layout atop the prelude region. Every length must be ≥ [`SPOOL_WINDOW`]; an AEAD seal is ≥ 40 bytes, so this only trips on caller error.
pub fn layout(base: u64, slot_lens: &[u64]) -> Option<SpoolLayout> {
    if slot_lens.iter().any(|&l| l < SPOOL_WINDOW as u64) {
        return None;
    }
    let mut offsets = Vec::with_capacity(slot_lens.len());
    let mut off = base;
    for &l in slot_lens {
        offsets.push(off);
        off = off.checked_add(l)?;
    }
    Some(SpoolLayout { base, offsets, lens: slot_lens.to_vec(), total: off })
}

const SPOOL_SECTION: &str = "spool";

fn spool_schema() -> SectionSchema {
    SectionSchema::new(SPOOL_SECTION)
        .field("version", TypeConstraint::AnyUnsigned)
        .field("name", TypeConstraint::Utf8Text)
        .field("hash", TypeConstraint::AnyHash)
        .field("size", TypeConstraint::AnyUnsigned)
        .field("slen", TypeConstraint::AnyUnsigned) // one per slot: the SEALED length (layout)
        .field("chash", TypeConstraint::AnyHash) // one per slot: the PLAINTEXT chunk hash (finalize verifies as it decrypts)
}

/// The prelude document: a COMPLETE VSF file (header, TOC, provenance hash), which is what lets it verify itself.
fn prelude_doc(desc: &SpoolDesc) -> Result<Vec<u8>, String> {
    let mut b = spool_schema()
        .build()
        .set("version", 1u8)
        .map_err(|e| e.to_string())?
        .set("name", VsfType::x(desc.name.clone()))
        .map_err(|e| e.to_string())?
        .set("hash", VsfType::hb(desc.hash.to_vec()))
        .map_err(|e| e.to_string())?
        .set("size", VsfType::u(desc.size as usize, false))
        .map_err(|e| e.to_string())?;
    for &l in &desc.sealed_lens {
        b = b.append_multi("slen", vec![VsfType::u(l as usize, false)]).map_err(|e| e.to_string())?;
    }
    for h in &desc.chunk_hashes {
        b = b.append_multi("chash", vec![VsfType::hb(h.to_vec())]).map_err(|e| e.to_string())?;
    }
    let section = b.encode().map_err(|e| e.to_string())?;
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(SPOOL_SECTION, section)
        .build()
        .map_err(|e| e.to_string())
}

/// Create the spool at `path`: prelude written eagerly (it is the only part that must survive to make the rest resumable), body a sparse zero hole. Returns the open file and the body layout.
pub fn create(path: &Path, desc: &SpoolDesc) -> io::Result<(File, SpoolLayout)> {
    let doc = prelude_doc(desc).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let base = 8 + doc.len() as u64;
    let lay = layout(base, &desc.sealed_lens).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "spool slot under one window"))?;
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
    f.set_len(lay.total)?;
    pwrite_all(&f, &(doc.len() as u64).to_le_bytes(), 0)?;
    pwrite_all(&f, &doc, 8)?;
    Ok((f, lay))
}

/// Resume from the file ALONE. `Ok(None)` means the prelude does not verify — a torn or foreign file — and the caller starts fresh; it never means a wrong layout, because the prelude's own provenance hash stands between a torn header and a parse.
pub fn resume(path: &Path) -> io::Result<Option<(SpoolDesc, SpoolLayout, File)>> {
    let f = match std::fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let file_len = f.metadata()?.len();
    let mut lenb = [0u8; 8];
    if file_len < 8 {
        return Ok(None);
    }
    pread_exact(&f, &mut lenb, 0)?;
    let doc_len = u64::from_le_bytes(lenb);
    // A sane prelude is small; a wild length is a torn or foreign file, not an error to propagate.
    if doc_len == 0 || doc_len > 1 << 20 || 8 + doc_len > file_len {
        return Ok(None);
    }
    let mut doc = vec![0u8; doc_len as usize];
    pread_exact(&f, &mut doc, 8)?;
    // THE presence test: the document verifies itself or there is no prelude. No zero-heuristic here — plaintext VSF may contain honest zero runs, so it is judged by its hash, the mechanism that is sound for it.
    if vsf::verification::read_verified(&doc, None).is_err() {
        return Ok(None);
    }
    let Ok(section) = SectionBuilder::parse_document(spool_schema(), &doc, None) else {
        return Ok(None);
    };
    let name = match section.get_fields("name").first().and_then(|f| f.values.first()) {
        Some(VsfType::x(t)) => t.clone(),
        _ => return Ok(None),
    };
    let Ok(hash) = section.get_value::<[u8; 32]>("hash") else {
        return Ok(None);
    };
    let size = section.get_fields("size").first().and_then(|f| f.values.first()).and_then(|v| v.as_u64()).unwrap_or(0);
    let sealed_lens: Vec<u64> = section.get_fields("slen").iter().filter_map(|f| f.values.first()).filter_map(|v| v.as_u64()).collect();
    let chunk_hashes: Vec<[u8; 32]> = section
        .get_fields("chash")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        })
        .collect();
    if sealed_lens.is_empty() || sealed_lens.len() != chunk_hashes.len() {
        return Ok(None);
    }
    let desc = SpoolDesc { name, hash, size, sealed_lens, chunk_hashes };
    let base = 8 + doc_len;
    let Some(lay) = layout(base, &desc.sealed_lens) else {
        return Ok(None);
    };
    if lay.total != file_len {
        // A truncated body cannot lie its way to "held" (missing zeros still read missing), but a WRONG-LENGTH file is a different transfer at the same path — start fresh.
        return Ok(None);
    }
    Ok(Some((desc, lay, f)))
}

// The muts feed the WINDOWS arm's seek_write loop; unix's write_all_at never rebinds them, hence the allow.
#[allow(unused_mut)]
fn pwrite_all(f: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.write_all_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let n = f.seek_write(buf, off)?;
            buf = &buf[n..];
            off += n as u64;
        }
        Ok(())
    }
}

fn pread_exact(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let (mut done, mut at) = (0usize, off);
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], at)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "spool short read"));
            }
            done += n;
            at += n as u64;
        }
        Ok(())
    }
}

/// Land one sealed chunk. The length must be exactly the layout's — a short write would leave zeros that honestly read missing, but a LONG one would smear into the neighbour, so it is refused outright.
pub fn write_slot(f: &File, layout: &SpoolLayout, idx: usize, sealed: &[u8]) -> io::Result<()> {
    if layout.lens.get(idx).copied() != Some(sealed.len() as u64) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "spool slot length mismatch"));
    }
    pwrite_all(f, sealed, layout.offsets[idx])
}

/// Read one slot back (the finalize pass decrypts these in order and sheds the spool).
pub fn read_slot(f: &File, layout: &SpoolLayout, idx: usize) -> io::Result<Vec<u8>> {
    let len = layout.lens[idx] as usize;
    let mut buf = vec![0u8; len];
    pread_exact(f, &mut buf, layout.offsets[idx])?;
    Ok(buf)
}

/// One slot's verdict from its bytes alone: present iff NO slot-relative 256-bit window is all-zero, with the tail window end-aligned. This is the (⇒) direction of the proof applied per slot.
fn slot_present(bytes: &[u8]) -> bool {
    let n = bytes.len();
    let mut i = 0;
    while i + SPOOL_WINDOW <= n {
        if bytes[i..i + SPOOL_WINDOW].iter().all(|b| *b == 0) {
            return false;
        }
        i += SPOOL_WINDOW;
    }
    if n % SPOOL_WINDOW != 0 {
        let s = n.saturating_sub(SPOOL_WINDOW);
        if bytes[s..].iter().all(|b| *b == 0) {
            return false;
        }
    }
    true
}

/// THE BITMAP READ: which slots are still wanted, reconstructed from the file alone. This is the whole point — after any crash, restart, or handoff, the spool plus the manifest is a complete resume, and there was never a side file to fsync in the right order.
pub fn missing(f: &File, layout: &SpoolLayout) -> io::Result<Vec<bool>> {
    let mut out = Vec::with_capacity(layout.lens.len());
    let mut buf: Vec<u8> = Vec::new();
    for i in 0..layout.lens.len() {
        let len = layout.lens[i] as usize;
        buf.resize(len, 0);
        pread_exact(f, &mut buf[..len], layout.offsets[i])?;
        out.push(!slot_present(&buf[..len]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic "ciphertext": a seeded xorshift filling bytes. Statistically it only needs to avoid 32-byte zero runs, which it does astronomically harder than the real seal does.
    fn sealed_bytes(seed: u64, len: usize) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    fn desc_of(lens: &[u64]) -> SpoolDesc {
        SpoolDesc {
            name: "pigeon.bin".into(),
            hash: [0xAB; 32],
            size: lens.iter().sum::<u64>().saturating_sub(40 * lens.len() as u64),
            sealed_lens: lens.to_vec(),
            chunk_hashes: (0..lens.len()).map(|i| [i as u8 + 1; 32]).collect(),
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("photon-spool-{}-{}", std::process::id(), name))
    }

    /// The theorem, end to end: write an arbitrary subset in arbitrary order, come back COLD through resume — nothing but the file — and the prelude self-verifies, the desc round-trips, and the zero-scan reconstructs the subset EXACTLY. The file was the state.
    #[test]
    fn the_file_is_its_own_bitmap_and_its_own_manifest() {
        let lens: Vec<u64> = vec![40, 262184, 63, 4096, 262184, 41, 1000];
        let desc = desc_of(&lens);
        let p = scratch("bitmap");
        let (f, lay) = create(&p, &desc).expect("create");
        let held = [1usize, 4, 5];
        for &i in &held {
            write_slot(&f, &lay, i, &sealed_bytes(0x9E37 + i as u64, lens[i] as usize)).expect("write");
        }
        drop(f);
        let (back, lay2, f) = resume(&p).expect("io").expect("prelude verifies");
        assert_eq!(back, desc, "the spool carried its own manifest");
        assert_eq!(lay2.offsets, lay.offsets, "same layout from the file alone");
        let miss = missing(&f, &lay2).expect("scan");
        for i in 0..lens.len() {
            assert_eq!(miss[i], !held.contains(&i), "slot {i}: the scan must equal the write-set complement");
        }
        let _ = std::fs::remove_file(&p);
    }

    /// The prelude is judged by ITS OWN HASH, not the zero rule (plaintext VSF may hold honest zero runs): a torn prelude fails `read_verified` and resume says fresh — never a wrong layout.
    #[test]
    fn a_torn_prelude_fails_closed_to_fresh() {
        let lens = vec![4096u64, 4096];
        let p = scratch("tornprelude");
        let (f, lay) = create(&p, &desc_of(&lens)).expect("create");
        write_slot(&f, &lay, 0, &sealed_bytes(3, 4096)).expect("write");
        drop(f);
        // Corrupt one byte INSIDE the prelude document: parse or provenance now fails.
        let mut bytes = std::fs::read(&p).expect("read");
        bytes[12] ^= 0x5A;
        std::fs::write(&p, &bytes).expect("write back");
        assert!(resume(&p).expect("io").is_none(), "a torn prelude must read as no spool, not as a guessed layout");
        assert!(resume(&scratch("never-existed")).expect("io").is_none(), "absent file is fresh, not an error");
        let _ = std::fs::remove_file(&p);
    }

    /// Crash model: a torn write lands a sector-grained prefix and the rest stays zero. The end-aligned tail window catches it, so a torn slot honestly reads missing and gets re-fetched whole — idempotent, since the same sealed bytes rewrite.
    #[test]
    fn a_torn_write_reads_missing() {
        let lens = vec![262184u64, 262184];
        let p = scratch("torn");
        let (f, lay) = create(&p, &desc_of(&lens)).expect("create");
        let whole = sealed_bytes(7, lens[0] as usize);
        pwrite_all(&f, &whole[..131072], lay.offsets[0]).expect("torn prefix");
        // Out-of-order writeback model: a full slot lands but one interior page never did.
        write_slot(&f, &lay, 1, &sealed_bytes(9, lens[1] as usize)).expect("write");
        pwrite_all(&f, &[0u8; 4096], lay.offsets[1] + 32768).expect("lost page");
        let miss = missing(&f, &lay).expect("scan");
        assert!(miss[0], "a torn prefix must read missing");
        assert!(miss[1], "a lost interior page must read missing");
        let _ = std::fs::remove_file(&p);
    }

    /// THE COUNTEREXAMPLE the module header promises: plaintext breaks the zero rule, because real files contain honest zero runs. An all-zero "chunk" — think executable padding — writes successfully and still reads missing, which in a plaintext design would be a fetch-forever livelock. This test exists so the seal is never "optimized" away: it is the bitmap.
    #[test]
    fn plaintext_zeros_would_lie() {
        let lens = vec![4096u64];
        let p = scratch("plain");
        let (f, lay) = create(&p, &desc_of(&lens)).expect("create");
        write_slot(&f, &lay, 0, &vec![0u8; 4096]).expect("write zeros");
        assert!(missing(&f, &lay).expect("scan")[0], "written zeros are indistinguishable from absence — encryption is the load-bearing premise, not an add-on");
        let _ = std::fs::remove_file(&p);
    }

    /// Boundary honesty: slots pack tight after the prelude, so total is base plus the sealed sum, and a slot under one window is refused at layout (an AEAD seal is ≥ 40 bytes, so only a caller bug reaches this).
    #[test]
    fn layout_is_tight_and_guards_the_window() {
        let lay = layout(100, &[40, 100, 32]).expect("layout");
        assert_eq!(lay.total, 272);
        assert_eq!(lay.offsets, vec![100, 140, 240]);
        assert!(layout(0, &[31]).is_none(), "a slot narrower than the window could dodge the scan");
    }
}
