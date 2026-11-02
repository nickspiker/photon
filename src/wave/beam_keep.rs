//! KEPT BEAM VIDEO (Nick 2026-11-01: "Def H.264 and add that format to VSF"; saved only when Settings → Beam → "Save beam video" is on): the beam spool (wave/spool.rs) drained at the wave's keep into ONE complete VSF document, `beam.video`, stored as an ordinary content-addressed blob beside the wave's audio recording.
//!
//! The document holds one `track` section per camera that was in the wave, in the order the tracks first appear — our own camera first when it ran. Each section names its person (`party`, hb) and then lists its records in time order, each a typed multi-value field stamped with the Eagle time the device took it:
//! - `info` = [e time, hR the beam's self-description as its wire bytes (`beam::encode_info`): size, frame rate, codec, colour];
//! - `orient` = [e time, u quarter turns];
//! - `frame` = [e time, u frame number, u gain × 4 (Q12, the ISP's), v(b'm', the H.264 access unit, Annex B, verbatim)].
//! Nothing is decoded or re-encoded at keep: the access units are the bytes that crossed the wire. The replayed wave screen decodes them in step with the audio (replay.rs).

use crate::wave::spool::{beam_discard, beam_drain, BeamRecKind, BeamTicket};
use vsf::VsfType;

/// The document's one section kind.
const TRACK: &str = "track";

/// A kept video: its blob, and each track's person in the document's order.
pub struct VideoKept {
    pub hash: [u8; 32],
    pub size: u64,
    pub tracks: Vec<[u8; 32]>,
}

/// Drain a beam spool into the `beam.video` document and store it. `None` when the spool holds no frame (nobody beamed), or the vault refuses the blob. The spool file goes either way.
pub fn keep(ticket: BeamTicket, identity_seed: &[u8; 32]) -> Option<VideoKept> {
    let recs = beam_drain(&ticket);
    // Stored FIRST, then the spool and its register go: a crash between the two recovers the spool again, never loses both.
    let Some((bytes, tracks)) = document(&recs) else {
        beam_discard(ticket);
        return None;
    };
    drop(recs);
    let hash = *blake3::hash(&bytes).as_bytes();
    let size = bytes.len() as u64;
    if let Err(e) = crate::storage::blob_store_any(identity_seed, &hash, &bytes) {
        crate::logf!("BEAM: video keep failed — the vault refused the {} byte document: {}", size, e);
        beam_discard(ticket);
        return None;
    }
    beam_discard(ticket);
    crate::logf!("BEAM: video kept — {} track(s), {} bytes as blob {}…", tracks.len(), size, hex::encode(&hash[..4]));
    Some(VideoKept { hash, size, tracks })
}

/// The `beam.video` document for a drained spool, and each track's party in its order. `None` when no track carries a frame.
pub(crate) fn document(recs: &[crate::wave::spool::BeamRec]) -> Option<(Vec<u8>, Vec<[u8; 32]>)> {
    if !recs.iter().any(|r| r.kind == BeamRecKind::Frame) {
        return None;
    }
    // Tracks in the order they first appear; each track's party from its Party record.
    let mut order: Vec<u8> = Vec::new();
    let mut party: Vec<(u8, [u8; 32])> = Vec::new();
    for r in recs {
        if !order.contains(&r.track) {
            order.push(r.track);
        }
        if r.kind == BeamRecKind::Party {
            if let Ok(p) = <[u8; 32]>::try_from(r.payload.as_slice()) {
                party.retain(|(t, _)| *t != r.track);
                party.push((r.track, p));
            }
        }
    }
    // Only tracks that carry frames become sections (a slot that never beamed has nothing to show).
    order.retain(|t| recs.iter().any(|r| r.track == *t && r.kind == BeamRecKind::Frame));
    let first_osc = recs.first().map_or(0, |r| r.osc);
    let mut doc = vsf::VsfBuilder::new().creation_time_oscillations(first_osc).provenance_only();
    let mut tracks = Vec::with_capacity(order.len());
    for t in &order {
        let mut sec = vsf::file_format::VsfSection::new(TRACK);
        let p = party.iter().find(|(pt, _)| pt == t).map_or([0u8; 32], |(_, p)| *p);
        sec.add_field("party", VsfType::hb(p.to_vec()));
        tracks.push(p);
        for r in recs.iter().filter(|r| r.track == *t) {
            let at = VsfType::e(vsf::types::EtType::e6(r.osc));
            match r.kind {
                BeamRecKind::Info => sec.add_field_multi("info", vec![at, VsfType::hR(r.payload.clone())]),
                BeamRecKind::Orient if !r.payload.is_empty() => sec.add_field_multi("orient", vec![at, VsfType::u(r.payload[0] as usize, false)]),
                // WHY/PROOF: a frame payload is [frame_no u32][gains 4 × u16][au] as the spool wrote it — one shorter than its 12-byte head is torn and skipped.
                BeamRecKind::Frame if r.payload.len() > 12 => {
                    let p = &r.payload;
                    let n = u32::from_le_bytes(p[0..4].try_into().unwrap());
                    let mut values = vec![at, VsfType::u(n as usize, false)];
                    for k in 0..4 {
                        values.push(VsfType::u(u16::from_le_bytes(p[4 + 2 * k..6 + 2 * k].try_into().unwrap()) as usize, false));
                    }
                    values.push(VsfType::v(vsf::builders::ENCODING_H264, p[12..].to_vec()));
                    sec.add_field_multi("frame", values);
                }
                _ => {}
            }
        }
        doc = doc.add_section_direct(sec);
    }
    match doc.build() {
        Ok(b) => Some((b, tracks)),
        Err(e) => {
            crate::logf!("BEAM: video keep failed to build its document — {}", e);
            None
        }
    }
}

/// One event of a kept track, in time order.
pub enum TrackEvent {
    Info(crate::wave::beam::Info),
    Orient(u8),
    Frame { frame_no: u32, gains: crate::wave::beam::Gains, au: Vec<u8> },
}

/// One kept track: whose camera, and its events with their Eagle times.
pub struct KeptTrack {
    pub party: [u8; 32],
    pub events: Vec<(i64, TrackEvent)>,
}

/// Read a `beam.video` document back (verified: the provenance hash must hold before a field is trusted). Fields of a kind this build does not know are skipped.
pub fn read(bytes: &[u8]) -> Option<Vec<KeptTrack>> {
    let (header, end) = vsf::verification::read_verified(bytes, None).ok()?;
    let sections = header.sections(bytes, end).ok()?;
    let mut out = Vec::new();
    for s in sections.iter().filter(|s| s.name == TRACK) {
        let party = s.bytes32("party").unwrap_or([0u8; 32]);
        let mut events = Vec::new();
        for f in &s.fields {
            let at = f.values.first().and_then(|v| v.as_eagle());
            let Some(at) = at else { continue };
            let ev = match f.name.as_str() {
                "info" => match f.values.get(1) {
                    Some(VsfType::hR(b)) => crate::wave::beam::decode_info(b).map(TrackEvent::Info),
                    _ => None,
                },
                "orient" => f.values.get(1).and_then(|v| v.as_u64()).map(|q| TrackEvent::Orient((q & 3) as u8)),
                "frame" if f.values.len() == 7 => {
                    let u = |k: usize| f.values.get(k).and_then(|v| v.as_u64());
                    match (u(1), u(2), u(3), u(4), u(5), f.values.get(6)) {
                        // WHY/PROOF: the document is a stored blob — a frame number or gain wider than its type reads as malformed, never wrapped.
                        (Some(n), Some(g0), Some(g1), Some(g2), Some(g3), Some(VsfType::v(tag, au))) if *tag == vsf::builders::ENCODING_H264 => {
                            let g = |x: u64| u16::try_from(x).ok();
                            match (u32::try_from(n).ok(), g(g0), g(g1), g(g2), g(g3)) {
                                (Some(n), Some(a), Some(b), Some(c), Some(d)) => Some(TrackEvent::Frame { frame_no: n, gains: crate::wave::beam::Gains([a, b, c, d]), au: au.clone() }),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(ev) = ev {
                events.push((at, ev));
            }
        }
        out.push(KeptTrack { party, events });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::spool::{beam_frame_payload, BeamRec};

    /// A kept beam reads back as it was spooled: tracks in first-seen order, each named, frames verbatim with their numbers and gains, the description and the turns in place; a track that never sent a frame is left out.
    #[test]
    fn a_kept_beam_round_trips_through_its_document() {
        let us = [1u8; 32];
        let them = [2u8; 32];
        let info = crate::wave::beam::Info { width: 640, height: 480, fps: 30, codec: crate::wave::beam::Codec::H264, colour: vec![9, 8, 7] };
        let au0 = vec![0, 0, 0, 1, 0x67, 1, 2, 3];
        let au1 = vec![0, 0, 0, 1, 0x41, 4, 5];
        let recs = vec![
            BeamRec { kind: BeamRecKind::Party, track: 0, osc: 10, payload: us.to_vec() },
            BeamRec { kind: BeamRecKind::Party, track: 1, osc: 10, payload: them.to_vec() },
            BeamRec { kind: BeamRecKind::Party, track: 2, osc: 10, payload: [3u8; 32].to_vec() },
            BeamRec { kind: BeamRecKind::Info, track: 1, osc: 11, payload: crate::wave::beam::encode_info(&info) },
            BeamRec { kind: BeamRecKind::Orient, track: 1, osc: 11, payload: vec![3] },
            BeamRec { kind: BeamRecKind::Frame, track: 1, osc: 12, payload: beam_frame_payload(7, [4096, 4000, 3900, 3800], &au0) },
            BeamRec { kind: BeamRecKind::Frame, track: 0, osc: 13, payload: beam_frame_payload(1, [4096; 4], &au1) },
        ];
        let (bytes, tracks) = document(&recs).expect("frames make a document");
        assert_eq!(tracks, vec![us, them], "first-seen order; the frameless track is left out");
        let read_back = read(&bytes).expect("verified read");
        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back[1].party, them);
        let ev = &read_back[1].events;
        assert!(matches!(&ev[0], (11, TrackEvent::Info(i)) if *i == info));
        assert!(matches!(&ev[1], (11, TrackEvent::Orient(3))));
        assert!(matches!(&ev[2], (12, TrackEvent::Frame { frame_no: 7, gains, au }) if gains.0 == [4096, 4000, 3900, 3800] && *au == au0));
        assert!(matches!(&read_back[0].events[0], (13, TrackEvent::Frame { frame_no: 1, au, .. }) if *au == au1));
        assert!(document(&recs[..5]).is_none(), "no frame, no document");
    }
}
