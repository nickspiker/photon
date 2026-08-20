//! Media packet wire format — the ONE non-VSF datagram in the system, because 100 packets/second earns a fixed 5-byte header instead of a parsed document.
//!
//! `[magic:1 = C7] [seq:4 LE] [ciphertext = the bare FEC symbol] [tag:4]`
//!
//! STRIPPED to the bone (Nick, 2026-08-19 — the first cut carried 42 fixed bytes + an MTU-padded symbol, ~13× the audio rate at the ladder floor): every field an endpoint can DERIVE is derived, every field the tag already proves is deleted.
//! - `step` — gone: it is `seq / PACKETS_PER_STEP` by construction; the old header field was checked-redundant.
//! - `dir` — gone: each direction seals under its own StepChain key, so a wrong-direction packet simply fails the tag.
//! - `call_id` — gone: the key is basket-derived per call (one live call per handle), so a stale call's straggler fails under the live key.
//! - `window_id`, the FEC symbol id, AND the ladder tier — gone (round 2, same day): the geometry is invariantly 2 packets per window, so `window_id = seq >> 1` and `esi = seq & 1`; the rungs have distinct window sizes, so the sealed LENGTH is the tier. The payload is the bare symbol.
//!
//! The single magic byte lives in the HIGH half of ASCII, which no other frame on the wire touches: VSF opens 'R' (0x52), PT DATA a lowercase stream id (0x61-0x7A) — every legitimate first byte is ≤ 0x7F. The recv worker checks this one byte FIRST and routes matches raw to the call engine — no PT ack, no StatusUpdate, no parse ladder — or silently drops them when no call is active.
//!
//! **Truncated tag (Nick's call, 2026-08-19): 4 bytes of the RFC 8439 Poly1305 tag** — the SRTP-32 profile. Why that's sound HERE: truncation touches integrity only (confidentiality is XChaCha20's and never changes); forgery is a purely ONLINE per-packet game at 2⁻³² against a key that exists only while the call lives (teardown zeroizes, so the attack budget is call-duration × injection rate, and a rate that matters is a visible flood); a landed forgery yields ONE garbled 10ms window, not keys or plaintext. The RustCrypto AEAD API can't verify truncated tags, so the composition is hand-assembled from `chacha20` + `poly1305` per RFC 8439 and PINNED bit-exact against the house `chacha20poly1305` library by the `composition_matches_the_house_aead` KAT.
//!
//! Nonce = the global sequence number in a 24-byte field (unique per key by construction — a step spans exactly [`keys::PACKETS_PER_STEP`] seqs, and seq never repeats within a call).

use super::keys::StepChain;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::XChaCha20;
use poly1305::universal_hash::KeyInit as _;
use poly1305::{Key as PolyKey, Poly1305};
use subtle::ConstantTimeEq;

pub const MEDIA_MAGIC: u8 = 0xC7;
pub const HEADER_LEN: usize = 1 + 4;
/// Truncated Poly1305 tag riding every sealed payload (SRTP-32 profile — see the module doc).
pub const TAG_LEN: usize = 4;

/// A parsed (still-sealed) media packet header. Step and direction are NOT wire fields — step derives from seq, direction from which key opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaHeader {
    pub seq: u32,
}

/// Is this datagram a media packet at all? The recv worker's one-byte fast check (plus a floor: header + tag + at least one symbol byte). Junk that happens to lead 0xC7 dies at the engine's tag/shape gates, counted.
pub fn is_media_packet(bytes: &[u8]) -> bool {
    bytes.len() > HEADER_LEN + TAG_LEN && bytes[0] == MEDIA_MAGIC
}

/// The RFC 8439 setup: XChaCha20 positioned at block 1 for the payload, plus the one-time Poly1305 key from block 0.
fn cipher_and_poly_key(key: &[u8; 32], nonce: &[u8; 24]) -> (XChaCha20, [u8; 32]) {
    let mut cipher = XChaCha20::new(key.into(), nonce.into());
    let mut block0 = [0u8; 64];
    cipher.apply_keystream(&mut block0);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&block0[..32]);
    (cipher, poly_key)
}

/// The full 16-byte RFC 8439 tag over the ciphertext (no AAD): ct ‖ pad16 ‖ le64(aad_len=0) ‖ le64(ct_len). Truncation to TAG_LEN happens at the wire, never here — the KAT pins this against the library.
fn full_tag(poly_key: &[u8; 32], ct: &[u8]) -> [u8; 16] {
    let pad = (16 - (ct.len() % 16)) % 16;
    let mut m = Vec::with_capacity(ct.len() + pad + 16);
    m.extend_from_slice(ct);
    m.resize(ct.len() + pad, 0);
    m.extend_from_slice(&0u64.to_le_bytes());
    m.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    Poly1305::new(PolyKey::from_slice(poly_key))
        .compute_unpadded(&m)
        .into()
}

fn nonce_for(seq: u32) -> [u8; 24] {
    let mut nonce = [0u8; 24];
    nonce[..4].copy_from_slice(&seq.to_le_bytes());
    nonce
}

/// Seal one encoded-audio payload into a wire packet.
pub fn seal(chain: &StepChain, seq: u32, payload: &[u8]) -> Option<Vec<u8>> {
    debug_assert_eq!(
        StepChain::step_for_seq(seq),
        chain.step(),
        "seal called with a chain off the seq's step"
    );
    let nonce = nonce_for(seq);
    let (mut cipher, poly_key) = cipher_and_poly_key(chain.key(), &nonce);
    let mut ct = payload.to_vec();
    cipher.apply_keystream(&mut ct);
    let tag = full_tag(&poly_key, &ct);

    let mut out = Vec::with_capacity(HEADER_LEN + ct.len() + TAG_LEN);
    out.push(MEDIA_MAGIC);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&ct);
    out.extend_from_slice(&tag[..TAG_LEN]);
    Some(out)
}

/// Parse the header without opening the seal (step-advance decisions happen first).
pub fn parse_header(bytes: &[u8]) -> Option<(MediaHeader, &[u8])> {
    if !is_media_packet(bytes) {
        return None;
    }
    let seq = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
    Some((MediaHeader { seq }, &bytes[HEADER_LEN..]))
}

/// Open a sealed payload with the direction's chain, advancing it to the seq's step first (forward-only: a packet from a destroyed step returns None — silence, never a rewind). The truncated tag is the whole gate: it proves call membership AND direction, since both live in the key. Constant-time compare, then decrypt.
pub fn open(chain: &mut StepChain, header: &MediaHeader, sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() <= TAG_LEN {
        return None;
    }
    if !chain.advance_to(StepChain::step_for_seq(header.seq)) {
        return None; // behind the chain — that step's key no longer exists anywhere
    }
    let (ct, wire_tag) = sealed.split_at(sealed.len() - TAG_LEN);
    let nonce = nonce_for(header.seq);
    let (mut cipher, poly_key) = cipher_and_poly_key(chain.key(), &nonce);
    let expected = full_tag(&poly_key, ct);
    if expected[..TAG_LEN].ct_eq(wire_tag).unwrap_u8() != 1 {
        return None;
    }
    let mut pt = ct.to_vec();
    cipher.apply_keystream(&mut pt);
    Some(pt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::keys::{derive_call_secret, Direction, PACKETS_PER_STEP};
    use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305};

    fn secret() -> [u8; 32] {
        derive_call_secret(
            &[1; 32],
            &[2; 32],
            &[3; 32],
            &[4; 16],
            &[5; 32],
            &[6; 32],
        )
    }

    /// The hand-assembled RFC 8439 composition must be BIT-IDENTICAL to the house AEAD library (ciphertext and the full 16-byte tag) — the wire merely truncates the tag. This KAT is what makes the hand-rolling safe to trust.
    #[test]
    fn composition_matches_the_house_aead() {
        let key = [0x42u8; 32];
        let nonce = nonce_for(0xDEADBEEF);
        let pt = b"the quick brown fox jumps over the lazy dog, twice over";
        let lib = XChaCha20Poly1305::new_from_slice(&key)
            .unwrap()
            .encrypt(&nonce.into(), pt.as_ref())
            .unwrap();
        let (mut cipher, poly_key) = cipher_and_poly_key(&key, &nonce);
        let mut ct = pt.to_vec();
        cipher.apply_keystream(&mut ct);
        let tag = full_tag(&poly_key, &ct);
        assert_eq!(&lib[..pt.len()], &ct[..], "ciphertext must match the library");
        assert_eq!(&lib[pt.len()..], &tag[..], "full tag must match the library");
    }

    #[test]
    fn media_round_trips_across_steps() {
        let s = secret();
        let mut tx = StepChain::new(&s, Direction::CallerToCallee);
        let mut rx = StepChain::new(&s, Direction::CallerToCallee);

        // First packet of step 0 and first of step 1 — the receiver walks forward mid-stream.
        for seq in [0u32, 7, PACKETS_PER_STEP, PACKETS_PER_STEP + 3] {
            tx.advance_to(StepChain::step_for_seq(seq));
            let payload = vec![seq as u8; 40];
            let wire = seal(&tx, seq, &payload).unwrap();
            assert!(is_media_packet(&wire));
            let (h, sealed) = parse_header(&wire).unwrap();
            assert_eq!(h.seq, seq);
            let opened = open(&mut rx, &h, sealed).unwrap();
            assert_eq!(opened, payload);
        }
    }

    #[test]
    fn direction_lives_in_the_key_not_the_wire() {
        // The dir byte is deleted from the header — key separation must be what rejects a cross-direction packet.
        let s = secret();
        let tx = StepChain::new(&s, Direction::CallerToCallee);
        let mut wrong_rx = StepChain::new(&s, Direction::CalleeToCaller);
        let wire = seal(&tx, 0, b"hello-hello").unwrap();
        let (h, sealed) = parse_header(&wire).unwrap();
        assert!(
            open(&mut wrong_rx, &h, sealed).is_none(),
            "a packet must only open under its own direction's key"
        );
    }

    #[test]
    fn dead_steps_stay_dead_and_tampering_fails() {
        let s = secret();
        let mut tx = StepChain::new(&s, Direction::CalleeToCaller);
        let mut rx = StepChain::new(&s, Direction::CalleeToCaller);

        let early = seal(&tx, 5, b"early-early").unwrap();
        // Receiver ratchets past step 0 (as if the stream ran on)…
        rx.advance_to(2);
        let (h, sealed) = parse_header(&early).unwrap();
        assert!(
            open(&mut rx, &h, sealed).is_none(),
            "a destroyed step's packet must be silence, not a rewind"
        );

        // Tampering the ciphertext OR the truncated tag fails the gate.
        tx.advance_to(2);
        for i in 0..2 {
            let mut wire = seal(&tx, PACKETS_PER_STEP * 2, b"xxxxxx-xxxxxx").unwrap();
            let idx = if i == 0 { HEADER_LEN + 2 } else { wire.len() - 1 };
            wire[idx] ^= 1;
            let (h, sealed) = parse_header(&wire).unwrap();
            assert!(open(&mut rx, &h, sealed).is_none(), "flip at {} must fail", idx);
        }
    }

    #[test]
    fn magic_collides_with_nothing_in_the_ladder() {
        // Every legitimate frame's first byte is plain ASCII (VSF 'R', PT lowercase stream ids) — the media magic claims the untouched high half.
        assert!(MEDIA_MAGIC > 0x7F, "media magic must live in the high half of ASCII");
        assert_ne!(MEDIA_MAGIC, b'R', "VSF opens with RÅ<");
        assert!(
            !(MEDIA_MAGIC as char).is_ascii_lowercase(),
            "PT DATA opens with a lowercase stream id"
        );
    }
}
