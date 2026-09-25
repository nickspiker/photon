//! Wave signaling — encrypted control rows ON THE LANES, never a bare wire frame.
//!
//! An offer/answer/hangup is an ordinary lane-sealed message whose typed control field is `RowControl::Wave`: to every relay, queue, and wire observer a wave is indistinguishable from a text message. What that buys, for free: fold trust, receive-anywhere (every answering device decrypts the offer → every device can ring and any can answer), dedup, retransmit, and the offer's lane key falling out of the decrypt as the basket's doomed egg.
//!
//! Every field is typed (`types::row_control`): the kind, the 16-byte wave id, and on offer/answer the nonce and the device key. Nothing is ever encoded into the row's text (flag day 2026-09-24).
//!
//! These rows are CONTROL content: hidden from every surface, never dinged by the normal path (the RING is its own edge at offer receipt, bypassing claim/attention suppression — a wave is the one always-ring event). The visible record of a wave (missed/completed/duration) is a separate summary row minted at the end.


// ---------------------------------------------------------------------------
// EXPRESS SIGNALS — the out-of-band copy that beats the lane (2026-09-01 Emma/Nick field logs).
// The lane is the CANONICAL signal path (fold trust, fleet fan-out, dedup, the offer's doomed-egg key capture) — but it is strictly ordered and sequentially keyed, so ONE unfilled gap upstream buffers a wave signal undecryptable behind it (a wave answer sat 12s in the gap buffer while the origin rang out; an earlier instance is cited at the receiver-driven gap heal, status.rs). A doorbell cannot wait for history.
// So every signal ALSO fires as a fire-and-forget datagram sealed under a friendship-derived key with a random nonce — no ordering, no chain, decryptable the instant it lands. The lane row still travels; both sides are idempotent (dup wave_id → no-op), so whichever arrives first wins and the loser is a no-op.
// The offer's express copy CARRIES the doomed lane key (the answering side normally captures it by decrypting the lane row pre-advance — express skips that decrypt, so the egg rides inside the sealed payload instead; it is material the answering side is entitled to and the seal is to the same friendship).
// Privacy trade, eyes open: unlike the lane row (indistinguishable from a text), this frame is recognizable as "a wave-signal happened" to a wire observer — but active-wave media is already a recognizable 50pps CBR stream seconds later, so the marginal leak is a declined/missed wave's existence, accepted for setup reliability.
// ---------------------------------------------------------------------------

/// Express frame magic — high-ASCII like MEDIA_MAGIC (0xC7), colliding with nothing on the wire (VSF opens "RÅ<", PT lowercase).
pub const EXPRESS_MAGIC: u8 = 0xC9;
const EXPRESS_NONCE_LEN: usize = 24;

/// Wire shape: [EXPRESS_MAGIC][nonce:24][AEAD(payload)]. Payload: a complete VSF document — the stamp, the optional lane key, and the signal's typed control fields.
pub fn is_express_frame(bytes: &[u8]) -> bool {
    bytes.len() > 1 + EXPRESS_NONCE_LEN + 16 && bytes[0] == EXPRESS_MAGIC
}

/// The per-friendship express key — derivable by BOTH ends from standing chain material alone (no per-wave state), so an express frame is openable even before any wave context exists.
pub fn express_key(lane_root: &[u8; 32], history_key: &[u8; 32]) -> [u8; 32] {
    let mut material = [0u8; 64];
    material[..32].copy_from_slice(lane_root);
    material[32..].copy_from_slice(history_key);
    blake3::derive_key("PHOTON_WAVE_v1 express signal", &material)
}

/// Seal one signal for the express wire. `lane_key` rides only on offers (the doomed egg for the answering side's basket).
pub fn seal_express(
    key: &[u8; 32],
    ts: i64,
    lane_key: Option<&[u8; 32]>,
    sig: &WaveSignal,
) -> Option<Vec<u8>> {
    use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
    let payload = express_payload(ts, lane_key, sig)?;
    let nonce_bytes: [u8; EXPRESS_NONCE_LEN] = rand::random();
    let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let sealed = cipher.encrypt(&XNonce::from(nonce_bytes), payload.as_slice()).ok()?;
    let mut out = Vec::with_capacity(1 + EXPRESS_NONCE_LEN + sealed.len());
    out.push(EXPRESS_MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Some(out)
}

/// How far an express frame's stamp may sit from now and still be acted on. Generous because two devices' eagle clocks are anchored independently (seconds of skew is normal) and an offer's stamp is its ROW's stamp, not the moment it fired; tight enough that a frame captured off the wire is worthless minutes later. Paired with the nonce cache in the drain, which kills an exact replay inside the window.
pub const EXPRESS_MAX_SKEW_OSC: i64 = 30 * vsf::OSCILLATIONS_PER_SECOND as i64;

/// The frame's nonce — the drain's replay key (random per frame, so two legitimate frames never collide).
pub fn express_nonce(bytes: &[u8]) -> Option<[u8; EXPRESS_NONCE_LEN]> {
    is_express_frame(bytes).then(|| bytes[1..1 + EXPRESS_NONCE_LEN].try_into().ok())?
}

/// Open an express frame with one friendship's key. `None` = not ours (the receiver trial-opens across friendships — a wrong key fails the tag, never a panic).
pub fn open_express(key: &[u8; 32], bytes: &[u8]) -> Option<(i64, Option<[u8; 32]>, WaveSignal)> {
    use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
    if !is_express_frame(bytes) {
        return None;
    }
    let nonce = &bytes[1..1 + EXPRESS_NONCE_LEN];
    let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let payload = cipher.decrypt(&XNonce::try_from(nonce).ok()?, &bytes[1 + EXPRESS_NONCE_LEN..]).ok()?;
    read_express_payload(&payload)
}

const EXPRESS_SECTION: &str = "wexp";

fn express_schema() -> vsf::schema::SectionSchema {
    crate::types::row_control::declare_row_fields(
        vsf::schema::SectionSchema::new(EXPRESS_SECTION)
            .field("ts", vsf::schema::TypeConstraint::Any) // e6 the signal's stamp
            .field("lk", vsf::schema::TypeConstraint::Any), // hR the offer's lane key (the doomed egg), offers only
    )
}

/// The sealed plaintext: a complete VSF document, the signal as typed fields.
fn express_payload(ts: i64, lane_key: Option<&[u8; 32]>, sig: &WaveSignal) -> Option<Vec<u8>> {
    use vsf::VsfType;
    let mut b = express_schema().build().set("ts", VsfType::e(vsf::types::EtType::e6(ts))).ok()?;
    if let Some(k) = lane_key {
        b = b.set("lk", VsfType::hR(k.to_vec())).ok()?;
    }
    b = crate::types::row_control::put_control(b, &crate::types::RowControl::Wave(*sig)).ok()?;
    let section = b.encode().ok()?;
    vsf::VsfBuilder::new().creation_time_oscillations(ts).provenance_only().add_unboxed(EXPRESS_SECTION, section).build().ok()
}

fn read_express_payload(payload: &[u8]) -> Option<(i64, Option<[u8; 32]>, WaveSignal)> {
    use vsf::VsfType;
    let section = vsf::schema::SectionBuilder::parse_document(express_schema(), payload, None).ok()?;
    let ts = match section.get_fields("ts").first().and_then(|f| f.values.first())? {
        VsfType::e(vsf::types::EtType::e6(t)) => *t,
        _ => return None,
    };
    let lane_key = section.get_fields("lk").first().and_then(|f| f.values.first()).and_then(|v| match v {
        VsfType::hR(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
        _ => None,
    });
    match crate::types::row_control::get_control(&section)? {
        crate::types::RowControl::Wave(w) => Some((ts, lane_key, w)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveSignal {
    /// `device` = the ORIGINATING device's pubkey (fleet lifecycle, 2026-09-08): the answering side routes its reply to THAT device's freshest address instead of the offer's possibly-stale source, and the origin's siblings get a name for the presence chip. Optional on the wire — a pre-device-id peer's offer parses with `None` and everything falls back to source-address routing.
    Offer { wave_id: [u8; 16], nonce: [u8; 32], device: Option<[u8; 32]> },
    /// `device` = the ANSWERING device: the origin pins media routing to it, and the answering side's own siblings read it off the replicated row for "wave in progress on <name>".
    Answer { wave_id: [u8; 16], nonce: [u8; 32], device: Option<[u8; 32]> },
    /// The answering side refused. Ring stops fleet-wide (the decline fans to the answering side's siblings as a row like any other).
    Decline { wave_id: [u8; 16] },
    /// The answering side is already in a wave — automatic, not a human edge.
    Busy { wave_id: [u8; 16] },
    /// Either side ended it (also the origin's give-up on an unanswered ring — the human IS the timeout).
    Hangup { wave_id: [u8; 16] },
    /// Caller → a losing answerer: another device won the race.
    Taken { wave_id: [u8; 16] },
    /// Media re-anchor (EXPRESS-ONLY, never a lane row — transport plumbing, not conversation history): fired into a receive drought so the far end re-points its media at this frame's SOURCE address. Heals the both-sides-moved case where "address follows authenticated packets" deadlocks on two dead addresses.
    Anchor { wave_id: [u8; 16] },
}

impl WaveSignal {
    pub fn wave_id(&self) -> &[u8; 16] {
        match self {
            WaveSignal::Offer { wave_id, .. }
            | WaveSignal::Answer { wave_id, .. }
            | WaveSignal::Decline { wave_id }
            | WaveSignal::Busy { wave_id }
            | WaveSignal::Hangup { wave_id }
            | WaveSignal::Taken { wave_id }
            | WaveSignal::Anchor { wave_id } => wave_id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            WaveSignal::Offer { .. } => "offer",
            WaveSignal::Answer { .. } => "answer",
            WaveSignal::Decline { .. } => "decline",
            WaveSignal::Busy { .. } => "busy",
            WaveSignal::Hangup { .. } => "hangup",
            WaveSignal::Taken { .. } => "taken",
            WaveSignal::Anchor { .. } => "anchor",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_guard_can_read_a_nonce_and_two_frames_never_share_one() {
        let key = [7u8; 32];
        let sig = WaveSignal::Anchor { wave_id: [3; 16] };
        let a = seal_express(&key, 1000, None, &sig).unwrap();
        let b = seal_express(&key, 1000, None, &sig).unwrap();
        let (na, nb) = (express_nonce(&a).unwrap(), express_nonce(&b).unwrap());
        assert_ne!(na, nb, "each frame carries its own random nonce — the drain dedups on it");
        assert_eq!(express_nonce(&a).unwrap(), na, "reading the nonce is stable");
        assert!(express_nonce(b"nope").is_none(), "a non-express frame has no nonce");
        // The stamp rides INSIDE the seal, so the drain's freshness check reads an authenticated value.
        let (ts, _, _) = open_express(&key, &a).unwrap();
        assert_eq!(ts, 1000);
        assert!(EXPRESS_MAX_SKEW_OSC > 0);
    }

    #[test]
    fn express_round_trip() {
        let key = [7u8; 32];
        let sig = WaveSignal::Offer { wave_id: [1; 16], nonce: [2; 32], device: Some([4; 32]) };
        let wire = seal_express(&key, 42, Some(&[9u8; 32]), &sig).unwrap();
        assert!(is_express_frame(&wire));
        assert!(!crate::wave::packet::is_media_packet(&wire), "express and media magics must not collide");
        let (ts, lane_key, got) = open_express(&key, &wire).unwrap();
        assert_eq!((ts, lane_key, got), (42, Some([9u8; 32]), sig));
        // A wrong friendship key fails the AEAD tag — trial-open across friendships is safe.
        assert!(open_express(&[8u8; 32], &wire).is_none());
        // Non-offer signals carry no lane key.
        let ans = WaveSignal::Answer { wave_id: [1; 16], nonce: [3; 32], device: None };
        let wire2 = seal_express(&key, 7, None, &ans).unwrap();
        assert_eq!(open_express(&key, &wire2).unwrap(), (7, None, ans));
    }
}
