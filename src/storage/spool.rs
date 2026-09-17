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
//! Custody: the spool holds wire-sealed bytes, so a spool on disk discloses exactly what the wire disclosed — nothing. Built for the bridge pigeon (ephemeral by construction: spool beside the landing directory, decrypt at finalize, shed both), but the primitive is transport-agnostic: any resumable sealed stream can spool this way.

use std::fs::File;
use std::io;
use std::path::Path;

/// The window width: 256 bits. Per-window false-present probability 2^-256; also one SIMD register, so the scan is a memory-bandwidth `all_zero` sweep.
pub const SPOOL_WINDOW: usize = 32;

/// Where each slot's sealed bytes live: tight-packed prefix sums, no padding — total is exactly the sealed sum, and every offset is derivable from the manifest alone on any device.
pub struct SpoolLayout {
    pub offsets: Vec<u64>,
    pub lens: Vec<u64>,
    pub total: u64,
}

/// Build the layout from the sealed slot lengths (the manifest's chunk count and the seal overhead give these on both ends). Every length must be ≥ [`SPOOL_WINDOW`]; an AEAD seal is ≥ 40 bytes, so this only trips on caller error.
pub fn layout(slot_lens: &[u64]) -> Option<SpoolLayout> {
    if slot_lens.iter().any(|&l| l < SPOOL_WINDOW as u64) {
        return None;
    }
    let mut offsets = Vec::with_capacity(slot_lens.len());
    let mut off = 0u64;
    for &l in slot_lens {
        offsets.push(off);
        off = off.checked_add(l)?;
    }
    Some(SpoolLayout { offsets, lens: slot_lens.to_vec(), total: off })
}

/// Create (or truncate) the spool: one `set_len` makes the whole extent a sparse zero hole — the "pre-zeroed" of the claim costs no write and no disk.
pub fn create(path: &Path, layout: &SpoolLayout) -> io::Result<File> {
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
    f.set_len(layout.total)?;
    Ok(f)
}

/// Re-open an existing spool for resume. The caller re-derives the layout from the manifest and asks [`missing`] — the file itself is the only state.
pub fn open(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new().read(true).write(true).open(path)
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

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("photon-spool-{}-{}", std::process::id(), name))
    }

    /// The theorem, end to end: write an arbitrary subset in arbitrary order, reopen cold, and the zero-scan reconstructs the subset EXACTLY — the file was the bitmap.
    #[test]
    fn the_file_is_its_own_bitmap() {
        let lens: Vec<u64> = vec![40, 262184, 63, 4096, 262184, 41, 1000];
        let lay = layout(&lens).expect("layout");
        let p = scratch("bitmap");
        let f = create(&p, &lay).expect("create");
        let held = [1usize, 4, 5];
        for &i in &held {
            write_slot(&f, &lay, i, &sealed_bytes(0x9E37 + i as u64, lens[i] as usize)).expect("write");
        }
        drop(f);
        // Cold resume: nothing but the file and the manifest-derived layout.
        let f = open(&p).expect("open");
        let miss = missing(&f, &lay).expect("scan");
        for i in 0..lens.len() {
            assert_eq!(miss[i], !held.contains(&i), "slot {i}: the scan must equal the write-set complement");
        }
        let _ = std::fs::remove_file(&p);
    }

    /// Crash model: a torn write lands a sector-grained prefix and the rest stays zero. The end-aligned tail window catches it, so a torn slot honestly reads missing and gets re-fetched whole — idempotent, since the same sealed bytes rewrite.
    #[test]
    fn a_torn_write_reads_missing() {
        let lens = vec![262184u64, 262184];
        let lay = layout(&lens).expect("layout");
        let p = scratch("torn");
        let f = create(&p, &lay).expect("create");
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

    /// THE COUNTEREXAMPLE the module header promises: plaintext breaks the scheme, because real files contain honest zero runs. An all-zero "chunk" — think executable padding — writes successfully and still reads missing, which in a plaintext design would be a fetch-forever livelock. This test exists so the seal is never "optimized" away: it is the bitmap.
    #[test]
    fn plaintext_zeros_would_lie() {
        let lens = vec![4096u64];
        let lay = layout(&lens).expect("layout");
        let p = scratch("plain");
        let f = create(&p, &lay).expect("create");
        write_slot(&f, &lay, 0, &vec![0u8; 4096]).expect("write zeros");
        assert!(missing(&f, &lay).expect("scan")[0], "written zeros are indistinguishable from absence — encryption is the load-bearing premise, not an add-on");
        let _ = std::fs::remove_file(&p);
    }

    /// Boundary honesty: slots pack tight with no padding, so total is the sealed sum, and a slot under one window is refused at layout (an AEAD seal is ≥ 40 bytes, so only a caller bug reaches this).
    #[test]
    fn layout_is_tight_and_guards_the_window() {
        let lay = layout(&[40, 100, 32]).expect("layout");
        assert_eq!(lay.total, 172);
        assert_eq!(lay.offsets, vec![0, 40, 140]);
        assert!(layout(&[31]).is_none(), "a slot narrower than the window could dodge the scan");
        assert!(layout(&[]).map(|l| l.total) == Some(0), "an empty layout is a zero-length spool, not an error");
    }
}
