//! THE WRAPPED ORIGINAL (docs/attachments.md "Target shape"; Nick 2026-10-17): an attachment's original bytes travel and rest inside a small VSF document, so the file carries its own identity instead of leaning on a hash stored somewhere else.
//!
//! The identity is the document's header `hb` — BLAKE3 over the whole wrapped file with only the hb slot zeroed, the hash VSF computes last — so one value addresses the blob, names it on the wire, keys the vault and the device-removal have-set, and verifies a fetched copy by itself.
//! The wrapper is DETERMINISTIC: unsigned, no thumbnails (they live in the head), and the header time is the CONTENT's own time when known (a photo's capture, a recording's start) — never the moment of wrapping, never a filesystem mtime — so the same content always wraps to the same bytes and the same hb, wherever and whoever wraps it.
//! Inside: `rh` (hb) = BLAKE3 of the raw bytes, the identity rows carried before the wrapper (and keep carrying until v134), and `data` = `v(b'b', raw)`, VSF's standard binary blob (length in bits, vsf_type.rs).

use vsf::file_format::VsfSection;
use vsf::VsfType;

/// The VSF section a wrapped original lives in.
const SECTION: &str = "attach_orig";

/// A wrapped original: the document bytes and their header hb (its identity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrapped {
    pub bytes: Vec<u8>,
    pub hb: [u8; 32],
}

/// Wrap `raw` byte-exact. `content_time` (oscillations) = when the content was made, if known — optional, strongly recommended.
pub fn wrap(raw: &[u8], content_time: Option<i64>) -> Result<Wrapped, String> {
    let mut sec = VsfSection::new(SECTION);
    sec.add_field("rh", VsfType::hb(blake3::hash(raw).as_bytes().to_vec()));
    sec.add_field("data", VsfType::v(b'b', raw.to_vec()));
    let mut b = vsf::VsfBuilder::new();
    if let Some(t) = content_time {
        b = b.creation_time_oscillations(t);
    }
    let bytes = b.add_section_direct(sec).build().map_err(|e| format!("wrap: {e}"))?;
    let hb = hb_of(&verified_header(&bytes)?)?;
    Ok(Wrapped { bytes, hb })
}

fn hb_of(header: &vsf::file_format::VsfHeader) -> Result<[u8; 32], String> {
    match &header.rolling_hash {
        Some(VsfType::hb(h)) => <[u8; 32]>::try_from(h.as_slice()).map_err(|_| "wrapped: hb is not 32 bytes".to_string()),
        _ => Err("wrapped: no hb in the header".to_string()),
    }
}

/// A whole wrapped document's header, read thru `read_verified`: provenance and hb both hash-checked before any field is believed.
fn verified_header(doc: &[u8]) -> Result<vsf::file_format::VsfHeader, String> {
    vsf::verification::read_verified(doc, None).map(|(h, _)| h).map_err(|e| format!("wrapped: {e}"))
}

/// The hb a header CLAIMS, plus where the header ends — compared, never trusted: only [`StreamVerifier`] reads it, from a first chunk that is not a whole document (so `read_verified` cannot apply), and its `finish` hashes every byte before anything acts on the content.
fn claimed_hb(chunk: &[u8]) -> Result<([u8; 32], usize), String> {
    let h = vsf::verification::parse_full_header(chunk).map_err(|e| format!("wrapped: header: {e}"))?;
    match h.rolling_hash {
        Some(VsfType::hb(b)) => Ok((<[u8; 32]>::try_from(b.as_slice()).map_err(|_| "wrapped: hb is not 32 bytes".to_string())?, h.header_end)),
        _ => Err("wrapped: no hb in the header".to_string()),
    }
}

/// Verify a whole wrapped document: its bytes hash to the hb its header claims, and that hb is the one asked for.
pub fn verify(doc: &[u8], expect_hb: &[u8; 32]) -> Result<(), String> {
    verified_and_named(doc, expect_hb).map(|_| ())
}

fn verified_and_named(doc: &[u8], expect_hb: &[u8; 32]) -> Result<vsf::file_format::VsfHeader, String> {
    let header = verified_header(doc)?;
    if hb_of(&header)? != *expect_hb {
        return Err("wrapped: header hb is not the hb asked for".to_string());
    }
    Ok(header)
}

/// The raw original out of a VERIFIED wrapper, plus its content time: checks the document against `expect_hb`, then the payload against its own `rh`.
pub fn unwrap(doc: &[u8], expect_hb: &[u8; 32]) -> Result<(Vec<u8>, Option<i64>), String> {
    let header = verified_and_named(doc, expect_hb)?;
    let (raw, rh) = payload(doc)?;
    if *blake3::hash(&raw).as_bytes() != rh {
        return Err("wrapped: payload does not match its rh".to_string());
    }
    let time = match &header.creation_time {
        Some(VsfType::e(vsf::types::EtType::e6(osc))) => Some(*osc),
        _ => None,
    };
    Ok((raw, time))
}

/// The raw hash a wrapper names (its `rh`) — what a pre-wrapper row or an old build's request knows the content by. Schema-validated, not hash-verified.
pub fn raw_hash(doc: &[u8]) -> Result<[u8; 32], String> {
    payload(doc).map(|(_, rh)| rh)
}

fn payload(doc: &[u8]) -> Result<(Vec<u8>, [u8; 32]), String> {
    use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
    let schema = SectionSchema::new(SECTION).field("rh", TypeConstraint::Any).field("data", TypeConstraint::Any);
    let sec = SectionBuilder::parse_document(schema, doc, None).map_err(|e| format!("wrapped: {e:?}"))?;
    let first = |n: &str| sec.get_fields(n).first().and_then(|f| f.values.first()).cloned();
    let rh = match first("rh") {
        Some(VsfType::hb(h)) => <[u8; 32]>::try_from(h.as_slice()).map_err(|_| "wrapped: rh is not 32 bytes".to_string())?,
        _ => return Err("wrapped: no rh".to_string()),
    };
    let raw = match first("data") {
        Some(VsfType::v(b'b', d)) => d,
        _ => return Err("wrapped: no vb data".to_string()),
    };
    Ok((raw, rh))
}

/// Streaming hb check for a wrapper too big to hold whole (a chunked blob): feed the chunks in order, the FIRST holding the whole header (any chunk size ≥ a header — the store's 256 KB always is), then `finish`.
/// The hb slot is found from the header the first chunk carries and zeroed in a copy of that chunk only; everything after streams thru the hasher as-is.
pub struct StreamVerifier {
    hasher: blake3::Hasher,
    expect: [u8; 32],
    started: bool,
}

impl StreamVerifier {
    pub fn new(expect_hb: [u8; 32]) -> Self {
        Self { hasher: blake3::Hasher::new(), expect: expect_hb, started: false }
    }

    pub fn update(&mut self, chunk: &[u8]) -> Result<(), String> {
        if self.started {
            self.hasher.update(chunk);
            return Ok(());
        }
        self.started = true;
        let (claimed, header_end) = claimed_hb(chunk)?;
        if claimed != self.expect {
            return Err("wrapped: header hb is not the hb asked for".to_string());
        }
        // The slot is the one place in the header holding the claimed hb's bytes (32 random bytes repeat nowhere else in a header by chance).
        let at = chunk[..header_end.min(chunk.len())]
            .windows(32)
            .position(|w| w == claimed)
            .ok_or("wrapped: hb slot not found in the first chunk")?;
        let mut first = chunk.to_vec();
        first[at..at + 32].fill(0);
        self.hasher.update(&first);
        Ok(())
    }

    pub fn finish(self) -> Result<(), String> {
        if *self.hasher.finalize().as_bytes() == self.expect {
            Ok(())
        } else {
            Err("wrapped: bytes do not hash to the hb".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same content, same wrapper, same hb — wherever and whenever it is wrapped (no wrap time, no signer, no thumbnails).
    #[test]
    fn wrapping_is_deterministic() {
        let raw = b"a photograph's bytes, byte-exact".to_vec();
        let a = wrap(&raw, Some(2_565_000_000_000_000_000)).unwrap();
        let b = wrap(&raw, Some(2_565_000_000_000_000_000)).unwrap();
        assert_eq!(a, b);
        assert_ne!(a.hb, wrap(&raw, None).unwrap().hb, "the content time is part of the document");
        assert_ne!(a.hb, wrap(b"other bytes", Some(2_565_000_000_000_000_000)).unwrap().hb);
    }

    /// The round trip is byte-exact and carries the content time; the wrapper names its raw content.
    #[test]
    fn unwrap_returns_the_original_and_its_time() {
        let raw: Vec<u8> = (0..70_000u32).map(|i| (i * 31 % 251) as u8).collect();
        let w = wrap(&raw, Some(2_565_123_456_789_000_000)).unwrap();
        let (back, t) = unwrap(&w.bytes, &w.hb).unwrap();
        assert_eq!(back, raw);
        assert_eq!(t, Some(2_565_123_456_789_000_000));
        assert_eq!(raw_hash(&w.bytes).unwrap(), *blake3::hash(&raw).as_bytes());
        assert_eq!(unwrap(&wrap(&raw, None).unwrap().bytes, &wrap(&raw, None).unwrap().hb).unwrap().1, None);
    }

    /// One flipped byte anywhere — header or payload — fails; so does asking for a different hb.
    #[test]
    fn tamper_and_wrong_hb_are_refused() {
        let raw = vec![7u8; 5000];
        let w = wrap(&raw, None).unwrap();
        let mut bad = w.bytes.clone();
        let n = bad.len();
        bad[n - 10] ^= 1;
        assert!(unwrap(&bad, &w.hb).is_err());
        assert!(verify(&w.bytes, &[9u8; 32]).is_err());
    }

    /// The streaming check over chunks agrees with the whole-document check, and catches a flipped byte in a later chunk.
    #[test]
    fn streaming_verify_matches_whole() {
        let raw: Vec<u8> = (0..600_000u32).map(|i| (i % 253) as u8).collect();
        let w = wrap(&raw, None).unwrap();
        let mut v = StreamVerifier::new(w.hb);
        for c in w.bytes.chunks(256 * 1024) {
            v.update(c).unwrap();
        }
        v.finish().unwrap();
        let mut bad = w.bytes.clone();
        let n = bad.len();
        bad[n - 1] ^= 0x80;
        let mut v = StreamVerifier::new(w.hb);
        for c in bad.chunks(256 * 1024) {
            v.update(c).unwrap();
        }
        assert!(v.finish().is_err());
    }
}
