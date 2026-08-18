//! Media packet wire format — the ONE non-VSF datagram in the system, because 50 packets/second earns a fixed 17-byte header instead of a parsed document.
//!
//! `[magic:2 = C7 11] [call_id8:8] [dir:1] [step:2 LE] [seq:4 LE] [ciphertext+tag]`
//!
//! The magic collides with nothing in the recv trial-parse ladder: VSF frames open with "RÅ<" (0x52...), PT DATA with a lowercase-ASCII stream id (0x61-0x7A). The recv worker checks these two bytes FIRST and routes matches raw to the call engine — no PT ack, no StatusUpdate, no parse ladder — or silently drops them when no call is active.
//!
//! Seal: XChaCha20-Poly1305 (the house AEAD) under the direction's CURRENT step key ([`keys::StepChain`]); nonce = the global sequence number in a 24-byte field (unique per key by construction — a step spans exactly [`keys::PACKETS_PER_STEP`] seqs, and seq never repeats within a call). `call_id8` is the first 8 bytes of the call id — a wire demux hint, not a secret; the AEAD under the basket-derived key is what authenticates membership.

use super::keys::{Direction, StepChain};
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305};

pub const MEDIA_MAGIC: [u8; 2] = [0xC7, 0x11];
pub const HEADER_LEN: usize = 2 + 8 + 1 + 2 + 4;

/// A parsed (still-sealed) media packet header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaHeader {
    pub call_id8: [u8; 8],
    pub dir: Direction,
    pub step: u16,
    pub seq: u32,
}

/// Is this datagram a media packet at all? The recv worker's two-byte fast check.
pub fn is_media_packet(bytes: &[u8]) -> bool {
    bytes.len() > HEADER_LEN && bytes[..2] == MEDIA_MAGIC
}

/// Seal one encoded-audio payload into a wire packet.
pub fn seal(
    chain: &StepChain,
    call_id8: &[u8; 8],
    dir: Direction,
    seq: u32,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let step = StepChain::step_for_seq(seq);
    debug_assert_eq!(step, chain.step(), "seal called with a chain off the seq's step");
    let cipher = XChaCha20Poly1305::new_from_slice(chain.key()).ok()?;
    let mut nonce = [0u8; 24];
    nonce[..4].copy_from_slice(&seq.to_le_bytes());
    let sealed = cipher.encrypt(&nonce.into(), payload).ok()?;

    let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
    out.extend_from_slice(&MEDIA_MAGIC);
    out.extend_from_slice(call_id8);
    out.push(dir.wire());
    out.extend_from_slice(&(step as u16).to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&sealed);
    Some(out)
}

/// Parse the header without opening the seal (demux + step-advance decisions happen first).
pub fn parse_header(bytes: &[u8]) -> Option<(MediaHeader, &[u8])> {
    if !is_media_packet(bytes) {
        return None;
    }
    let call_id8: [u8; 8] = bytes[2..10].try_into().ok()?;
    let dir = Direction::from_wire(bytes[10])?;
    let step = u16::from_le_bytes(bytes[11..13].try_into().ok()?);
    let seq = u32::from_le_bytes(bytes[13..17].try_into().ok()?);
    Some((
        MediaHeader {
            call_id8,
            dir,
            step,
            seq,
        },
        &bytes[HEADER_LEN..],
    ))
}

/// Open a sealed payload with the direction's chain, advancing it to the packet's step first (forward-only: a packet from a destroyed step returns None — silence, never a rewind). The header's step field is advisory against seq (they must agree, or the packet is malformed/forged).
pub fn open(chain: &mut StepChain, header: &MediaHeader, sealed: &[u8]) -> Option<Vec<u8>> {
    let expected_step = StepChain::step_for_seq(header.seq);
    if expected_step != header.step as u32 {
        return None;
    }
    if !chain.advance_to(expected_step) {
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
    use crate::call::keys::{derive_call_secret, PACKETS_PER_STEP};

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
        let id8 = [9u8; 8];

        // First packet of step 0 and first of step 1 — the receiver walks forward mid-stream.
        for seq in [0u32, 7, PACKETS_PER_STEP, PACKETS_PER_STEP + 3] {
            tx.advance_to(StepChain::step_for_seq(seq));
            let payload = vec![seq as u8; 40];
            let wire = seal(&tx, &id8, Direction::CallerToCallee, seq, &payload).unwrap();
            assert!(is_media_packet(&wire));
            let (h, sealed) = parse_header(&wire).unwrap();
            assert_eq!(h.seq, seq);
            let opened = open(&mut rx, &h, sealed).unwrap();
            assert_eq!(opened, payload);
        }
    }

    #[test]
    fn dead_steps_stay_dead_and_tampering_fails() {
        let s = secret();
        let mut tx = StepChain::new(&s, Direction::CalleeToCaller);
        let mut rx = StepChain::new(&s, Direction::CalleeToCaller);
        let id8 = [7u8; 8];

        let early = seal(&tx, &id8, Direction::CalleeToCaller, 5, b"early").unwrap();
        // Receiver ratchets past step 0 (as if the stream ran on)…
        rx.advance_to(2);
        let (h, sealed) = parse_header(&early).unwrap();
        assert!(
            open(&mut rx, &h, sealed).is_none(),
            "a destroyed step's packet must be silence, not a rewind"
        );

        // Tampered ciphertext fails the AEAD.
        tx.advance_to(2);
        let mut wire = seal(&tx, &id8, Direction::CalleeToCaller, PACKETS_PER_STEP * 2, b"x").unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 1;
        let (h, sealed) = parse_header(&wire).unwrap();
        assert!(open(&mut rx, &h, sealed).is_none());

        // A header whose step disagrees with its seq is malformed.
        let ok = seal(&tx, &id8, Direction::CalleeToCaller, PACKETS_PER_STEP * 2 + 1, b"y").unwrap();
        let (mut h, sealed) = parse_header(&ok).unwrap();
        h.step += 1;
        assert!(open(&mut rx, &h, sealed).is_none());
    }

    #[test]
    fn magic_collides_with_nothing_in_the_ladder() {
        assert_ne!(MEDIA_MAGIC[0], b'R', "VSF opens with RÅ<");
        assert!(
            !(MEDIA_MAGIC[0] as char).is_ascii_lowercase(),
            "PT DATA opens with a lowercase stream id"
        );
    }
}
