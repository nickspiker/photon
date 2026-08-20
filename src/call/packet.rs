//! Media packet wire format — the ONE non-VSF datagram in the system, because 100 packets/second earns a fixed 5-byte header instead of a parsed document.
//!
//! `[magic:1 = C7] [seq:4 LE] [ciphertext = the bare FEC symbol, +tag]`
//!
//! STRIPPED to the bone (Nick, 2026-08-19 — the first cut carried 42 fixed bytes + an MTU-padded symbol, ~13× the audio rate at the ladder floor): every field an endpoint can DERIVE is derived, every field the AEAD already proves is deleted.
//! - `step` — gone: it is `seq / PACKETS_PER_STEP` by construction; the old header field was checked-redundant.
//! - `dir` — gone: each direction seals under its own StepChain key, so a wrong-direction packet simply fails the AEAD.
//! - `call_id` — gone: the key is basket-derived per call, so a stale call's straggler fails decrypt under the live key (v1 is one-call-singular; even with concurrent calls, demux-by-trial is one AEAD per stray).
//! - `window_id`, the FEC symbol id, AND the ladder tier — gone (round 2, same day): the geometry is invariantly 2 packets per window, so `window_id = seq >> 1` and `esi = seq & 1`; the four rungs have four distinct window sizes, so the sealed LENGTH is the tier. The payload is the bare symbol.
//!
//! The single magic byte lives in the HIGH half of ASCII, which no other frame on the wire touches: VSF opens 'R' (0x52), PT DATA a lowercase stream id (0x61-0x7A) — every legitimate first byte is ≤ 0x7F. The recv worker checks this one byte FIRST and routes matches raw to the call engine — no PT ack, no StatusUpdate, no parse ladder — or silently drops them when no call is active.
//!
//! Seal: XChaCha20-Poly1305 (the house AEAD) under the direction's CURRENT step key ([`keys::StepChain`]); nonce = the global sequence number in a 24-byte field (unique per key by construction — a step spans exactly [`keys::PACKETS_PER_STEP`] seqs, and seq never repeats within a call).

use super::keys::StepChain;
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305};

pub const MEDIA_MAGIC: u8 = 0xC7;
pub const HEADER_LEN: usize = 1 + 4;
/// Poly1305 tag riding every sealed payload.
pub const TAG_LEN: usize = 16;

/// A parsed (still-sealed) media packet header. Step and direction are NOT wire fields — step derives from seq, direction from which key opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaHeader {
    pub seq: u32,
}

/// Is this datagram a media packet at all? The recv worker's one-byte fast check (plus a floor: header + tag + at least one symbol byte). Junk that happens to lead 0xC7 dies at the engine's AEAD/shape gates, counted.
pub fn is_media_packet(bytes: &[u8]) -> bool {
    bytes.len() > HEADER_LEN + TAG_LEN && bytes[0] == MEDIA_MAGIC
}

/// Seal one encoded-audio payload into a wire packet.
pub fn seal(chain: &StepChain, seq: u32, payload: &[u8]) -> Option<Vec<u8>> {
    debug_assert_eq!(
        StepChain::step_for_seq(seq),
        chain.step(),
        "seal called with a chain off the seq's step"
    );
    let cipher = XChaCha20Poly1305::new_from_slice(chain.key()).ok()?;
    let mut nonce = [0u8; 24];
    nonce[..4].copy_from_slice(&seq.to_le_bytes());
    let sealed = cipher.encrypt(&nonce.into(), payload).ok()?;

    let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
    out.push(MEDIA_MAGIC);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&sealed);
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

/// Open a sealed payload with the direction's chain, advancing it to the seq's step first (forward-only: a packet from a destroyed step returns None — silence, never a rewind). The AEAD is the whole gate: it proves call membership AND direction, since both live in the key.
pub fn open(chain: &mut StepChain, header: &MediaHeader, sealed: &[u8]) -> Option<Vec<u8>> {
    if !chain.advance_to(StepChain::step_for_seq(header.seq)) {
        return None; // behind the chain — that step's key no longer exists anywhere
    }
    let cipher = XChaCha20Poly1305::new_from_slice(chain.key()).ok()?;
    let mut nonce = [0u8; 24];
    nonce[..4].copy_from_slice(&header.seq.to_le_bytes());
    cipher.decrypt(&nonce.into(), sealed).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::keys::{derive_call_secret, Direction, PACKETS_PER_STEP};

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

        // Tampered ciphertext fails the AEAD.
        tx.advance_to(2);
        let mut wire = seal(&tx, PACKETS_PER_STEP * 2, b"xxxxxx").unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 1;
        let (h, sealed) = parse_header(&wire).unwrap();
        assert!(open(&mut rx, &h, sealed).is_none());
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
