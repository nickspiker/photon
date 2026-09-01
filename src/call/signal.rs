//! Call signaling — encrypted control rows ON THE LANES, never a bare wire frame.
//!
//! An offer/answer/hangup is ordinary lane-sealed message content carrying the [`crate::types::CALL_PREFIX`] marker: to every relay, queue, and wire observer a call is indistinguishable from a text message. What that buys, for free: fold trust, receive-anywhere (every callee device decrypts the offer → every device can ring and any can answer), dedup, retransmit, and the offer's lane key falling out of the decrypt as the basket's doomed egg.
//!
//! Record grammar (STX-separated fields after the prefix, the attachment-row convention):
//! `PREFIX kind \u{2} call_id_hex32 [\u{2} nonce_hex64]` — nonce present on offer/answer only.
//!
//! These rows are CONTROL content: hidden from every surface, never dinged by the normal path (the RING is its own edge at offer receipt, bypassing claim/attention suppression — a call is the one always-ring event). The visible record of a call (missed/completed/duration) is a separate summary row minted at the end.

use crate::types::CALL_PREFIX;

// ---------------------------------------------------------------------------
// EXPRESS SIGNALS — the out-of-band copy that beats the lane (2026-09-01 Emma/Nick field logs).
// The lane is the CANONICAL signal path (fold trust, fleet fan-out, dedup, the offer's doomed-egg key capture) — but it is strictly ordered and sequentially keyed, so ONE unfilled gap upstream buffers a call signal undecryptable behind it (a call answer sat 12s in the gap buffer while the caller rang out; an earlier instance is cited at the receiver-driven gap heal, status.rs). A doorbell cannot wait for history.
// So every signal ALSO fires as a fire-and-forget datagram sealed under a friendship-derived key with a random nonce — no ordering, no chain, decryptable the instant it lands. The lane row still travels; both sides are idempotent (dup call_id → no-op), so whichever arrives first wins and the loser is a no-op.
// The offer's express copy CARRIES the doomed lane key (the callee normally captures it by decrypting the lane row pre-advance — express skips that decrypt, so the egg rides inside the sealed payload instead; it is material the callee is entitled to and the seal is to the same friendship).
// Privacy trade, eyes open: unlike the lane row (indistinguishable from a text), this frame is recognizable as "a call-signal happened" to a wire observer — but active-call media is already a recognizable 50pps CBR stream seconds later, so the marginal leak is a declined/missed call's existence, accepted for setup reliability.
// ---------------------------------------------------------------------------

/// Express frame magic — high-ASCII like MEDIA_MAGIC (0xC7), colliding with nothing on the wire (VSF opens "RÅ<", PT lowercase).
pub const EXPRESS_MAGIC: u8 = 0xC9;
const EXPRESS_NONCE_LEN: usize = 24;

/// Wire shape: [EXPRESS_MAGIC][nonce:24][AEAD(payload)]. Payload: [ts:8 LE][has_lane_key:1][lane_key:32?][content utf8].
pub fn is_express_frame(bytes: &[u8]) -> bool {
    bytes.len() > 1 + EXPRESS_NONCE_LEN + 16 && bytes[0] == EXPRESS_MAGIC
}

/// The per-friendship express key — derivable by BOTH ends from standing chain material alone (no per-call state), so an express frame is openable even before any call context exists.
pub fn express_key(lane_root: &[u8; 32], history_key: &[u8; 32]) -> [u8; 32] {
    let mut material = [0u8; 64];
    material[..32].copy_from_slice(lane_root);
    material[32..].copy_from_slice(history_key);
    blake3::derive_key("PHOTON_CALL_v1 express signal", &material)
}

/// Seal one signal for the express wire. `lane_key` rides only on offers (the doomed egg for the callee's basket).
pub fn seal_express(
    key: &[u8; 32],
    ts: i64,
    lane_key: Option<&[u8; 32]>,
    sig: &CallSignal,
) -> Option<Vec<u8>> {
    use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
    let mut payload = Vec::with_capacity(9 + 32 + 64);
    payload.extend_from_slice(&ts.to_le_bytes());
    match lane_key {
        Some(k) => {
            payload.push(1);
            payload.extend_from_slice(k);
        }
        None => payload.push(0),
    }
    payload.extend_from_slice(sig.to_content().as_bytes());
    let nonce_bytes: [u8; EXPRESS_NONCE_LEN] = rand::random();
    let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let sealed = cipher.encrypt(XNonce::from_slice(&nonce_bytes), payload.as_slice()).ok()?;
    let mut out = Vec::with_capacity(1 + EXPRESS_NONCE_LEN + sealed.len());
    out.push(EXPRESS_MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Some(out)
}

/// Open an express frame with one friendship's key. `None` = not ours (the receiver trial-opens across friendships — a wrong key fails the tag, never a panic).
pub fn open_express(key: &[u8; 32], bytes: &[u8]) -> Option<(i64, Option<[u8; 32]>, CallSignal)> {
    use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
    if !is_express_frame(bytes) {
        return None;
    }
    let nonce = &bytes[1..1 + EXPRESS_NONCE_LEN];
    let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let payload = cipher.decrypt(XNonce::from_slice(nonce), &bytes[1 + EXPRESS_NONCE_LEN..]).ok()?;
    if payload.len() < 9 {
        return None;
    }
    let ts = i64::from_le_bytes(payload[..8].try_into().ok()?);
    let (lane_key, content_at) = match payload[8] {
        1 if payload.len() >= 41 => {
            let k: [u8; 32] = payload[9..41].try_into().ok()?;
            (Some(k), 41)
        }
        0 => (None, 9),
        _ => return None,
    };
    let content = std::str::from_utf8(&payload[content_at..]).ok()?;
    Some((ts, lane_key, CallSignal::parse(content)?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSignal {
    Offer { call_id: [u8; 16], nonce: [u8; 32] },
    Answer { call_id: [u8; 16], nonce: [u8; 32] },
    /// Callee refused. Ring stops fleet-wide (the decline fans to the callee's siblings as a row like any other).
    Decline { call_id: [u8; 16] },
    /// Callee is already in a call — automatic, not a human edge.
    Busy { call_id: [u8; 16] },
    /// Either side ended it (also the caller's give-up on an unanswered ring — the human IS the timeout).
    Hangup { call_id: [u8; 16] },
    /// Caller → a losing answerer: another device won the race.
    Taken { call_id: [u8; 16] },
}

impl CallSignal {
    pub fn call_id(&self) -> &[u8; 16] {
        match self {
            CallSignal::Offer { call_id, .. }
            | CallSignal::Answer { call_id, .. }
            | CallSignal::Decline { call_id }
            | CallSignal::Busy { call_id }
            | CallSignal::Hangup { call_id }
            | CallSignal::Taken { call_id } => call_id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            CallSignal::Offer { .. } => "offer",
            CallSignal::Answer { .. } => "answer",
            CallSignal::Decline { .. } => "decline",
            CallSignal::Busy { .. } => "busy",
            CallSignal::Hangup { .. } => "hangup",
            CallSignal::Taken { .. } => "taken",
        }
    }

    /// The row content string this signal rides as.
    pub fn to_content(&self) -> String {
        let base = format!("{}{}\u{2}{}", CALL_PREFIX, self.kind(), hex::encode(self.call_id()));
        match self {
            CallSignal::Offer { nonce, .. } | CallSignal::Answer { nonce, .. } => {
                format!("{}\u{2}{}", base, hex::encode(nonce))
            }
            _ => base,
        }
    }

    /// Parse a row's content. None for non-call content or a malformed record (malformed = dropped, never guessed).
    pub fn parse(content: &str) -> Option<CallSignal> {
        let rest = content.strip_prefix(CALL_PREFIX)?;
        let mut parts = rest.split('\u{2}');
        let kind = parts.next()?;
        let call_id: [u8; 16] = hex::decode(parts.next()?).ok()?.try_into().ok()?;
        let nonce: Option<[u8; 32]> = parts
            .next()
            .and_then(|h| hex::decode(h).ok())
            .and_then(|b| b.try_into().ok());
        match (kind, nonce) {
            ("offer", Some(nonce)) => Some(CallSignal::Offer { call_id, nonce }),
            ("answer", Some(nonce)) => Some(CallSignal::Answer { call_id, nonce }),
            ("decline", None) => Some(CallSignal::Decline { call_id }),
            ("busy", None) => Some(CallSignal::Busy { call_id }),
            ("hangup", None) => Some(CallSignal::Hangup { call_id }),
            ("taken", None) => Some(CallSignal::Taken { call_id }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_round_trip_and_are_control() {
        let id = [0xAB; 16];
        let n = [0xCD; 32];
        for sig in [
            CallSignal::Offer { call_id: id, nonce: n },
            CallSignal::Answer { call_id: id, nonce: n },
            CallSignal::Decline { call_id: id },
            CallSignal::Busy { call_id: id },
            CallSignal::Hangup { call_id: id },
            CallSignal::Taken { call_id: id },
        ] {
            let content = sig.to_content();
            assert_eq!(CallSignal::parse(&content), Some(sig));
            assert!(
                crate::types::is_control_content(&content),
                "call signals must be hidden machinery rows"
            );
        }
        assert_eq!(CallSignal::parse("hello"), None);
        // A truncated offer (missing nonce) is malformed, not a lesser signal.
        let bad = format!("{}offer\u{2}{}", CALL_PREFIX, hex::encode([1u8; 16]));
        assert_eq!(CallSignal::parse(&bad), None);
    }

    #[test]
    fn express_round_trip() {
        let key = [7u8; 32];
        let sig = CallSignal::Offer { call_id: [1; 16], nonce: [2; 32] };
        let wire = seal_express(&key, 42, Some(&[9u8; 32]), &sig).unwrap();
        assert!(is_express_frame(&wire));
        assert!(!crate::call::packet::is_media_packet(&wire), "express and media magics must not collide");
        let (ts, lane_key, got) = open_express(&key, &wire).unwrap();
        assert_eq!((ts, lane_key, got), (42, Some([9u8; 32]), sig));
        // A wrong friendship key fails the AEAD tag — trial-open across friendships is safe.
        assert!(open_express(&[8u8; 32], &wire).is_none());
        // Non-offer signals carry no lane key.
        let ans = CallSignal::Answer { call_id: [1; 16], nonce: [3; 32] };
        let wire2 = seal_express(&key, 7, None, &ans).unwrap();
        assert_eq!(open_express(&key, &wire2).unwrap(), (7, None, ans));
    }
}
