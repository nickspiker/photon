//! End-to-end call MEDIA loop (docs/calls.md) — proves the crypto/codec/FEC pieces compose
//! through one full call without a live network: both sides derive the basket from shared friendship
//! material, a caller sends sealed bundles under datagram loss, the callee reassembles and
//! Opus-decodes them. This is the offline half of the self-call harness (the live two-instance
//! test is a field step). Mirrors the engine's PIGGYBACK wire (2026-08-20): ONE datagram per
//! window, sealed payload = [ctrl:1][source(N)][repair(N−1)], seq = window id, ctrl names both
//! symbols' rungs — a lost datagram's window recovers from the repair riding the NEXT datagram.

use photon_messenger::call::keys::{derive_call_secret, Direction, StepChain};
use photon_messenger::call::packet;
use std::collections::BTreeMap;

/// The shared friendship material both fleets hold (stand-ins for lane_root + history_key + the offer's
/// doomed lane key). In a real call these come from FriendshipChains; here they're the agreed inputs.
fn basket() -> [u8; 32] {
    derive_call_secret(
        &[0xA1; 32], // lane_root
        &[0xB2; 32], // history_key
        &[0xC3; 32], // offer_lane_key (doomed egg)
        &[0x11; 16], // call_id
        &[0x44; 32], // caller_nonce
        &[0x55; 32], // callee_nonce
    )
}

const FRAME_SAMPLES: usize = 480; // 10ms @ 48k mono, == platform::audio::FRAME_SAMPLES
// The engine's ladder, mirrored: rates, per-rung max encoded bytes (+2 over hard-CBR's exact rate/800, windows 8-aligned = exact single RaptorQ symbol), per-rung frames/window.
const TIER_RATES: [i32; 4] = [16_000, 32_000, 64_000, 128_000];
const TIER_MAX_ENC: [usize; 4] = [22, 42, 82, 162];
const TIER_FRAMES: [usize; 4] = [4, 2, 2, 2];

fn tier_slot(t: usize) -> usize {
    2 + TIER_MAX_ENC[t]
}

fn tier_window_bytes(t: usize) -> usize {
    TIER_FRAMES[t] * tier_slot(t)
}

fn oti(t: usize) -> raptorq::ObjectTransmissionInformation {
    raptorq::ObjectTransmissionInformation::with_defaults(
        tier_window_bytes(t) as u64,
        tier_window_bytes(t) as u16,
    )
}

/// Both sides derive the identical secret from the same basket — the receive-anywhere property.
#[test]
fn both_ends_agree_on_the_secret() {
    let caller = basket();
    let callee = basket();
    assert_eq!(caller, callee);
    // And each direction's step-0 keys match across the two independently-built chains.
    let a_tx = StepChain::new(&caller, Direction::CallerToCallee);
    let b_rx = StepChain::new(&callee, Direction::CallerToCallee);
    assert_eq!(a_tx.key(), b_rx.key());
}

/// The full media path across EVERY ladder rung with the piggyback bundle wire: encode → window →
/// fountain → bundle [ctrl][source(N)][repair(N−1)] → seal → DROP every third datagram → open →
/// ctrl split → feed both symbols → decode, asserting every non-final window recovers (a dropped
/// datagram's window rebuilds from the repair riding the next datagram — including across rung
/// switches, where the bundle's two symbols are DIFFERENT sizes and the ctrl byte is what splits them).
#[test]
fn media_survives_datagram_loss_via_piggybacked_repair() {
    let secret = basket();
    let mut tx_chain = StepChain::new(&secret, Direction::CallerToCallee);
    let mut rx_chain = StepChain::new(&secret, Direction::CallerToCallee);

    let mut encoder =
        opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::LowDelay).unwrap();
    encoder.set_vbr(false).unwrap();
    let mut decoder = opus::Decoder::new(48_000, opus::Channels::Mono).unwrap();

    // A recognizable 440Hz-ish tone the decoder should reproduce (energy, not bit-exactness — Opus is lossy).
    let make_frame = |i: usize| -> Vec<i16> {
        (0..FRAME_SAMPLES)
            .map(|s| {
                let t = (i * FRAME_SAMPLES + s) as f32 / 48_000.0;
                ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16
            })
            .collect()
    };

    let windows_per_tier = 4usize;
    let total_windows = TIER_RATES.len() * windows_per_tier;
    let mut frame_i = 0usize;
    let mut wid: u32 = 0;
    let mut prev_repair: Option<(usize, Vec<u8>)> = None;

    // RX state, engine-shaped: per-window (tier, fountain decoder) + recovered windows.
    let mut rx_decoders: BTreeMap<u32, (usize, raptorq::Decoder)> = BTreeMap::new();
    let mut rx_done: BTreeMap<u32, Vec<u8>> = BTreeMap::new();

    for tier in 0..TIER_RATES.len() {
        encoder.set_bitrate(opus::Bitrate::Bits(TIER_RATES[tier])).unwrap();
        for _ in 0..windows_per_tier {
            // --- caller: encode this rung's frame count into its window ---
            let mut window_buf = vec![0u8; tier_window_bytes(tier)];
            for f in 0..TIER_FRAMES[tier] {
                let pcm = make_frame(frame_i);
                frame_i += 1;
                let mut enc = vec![0u8; TIER_MAX_ENC[tier]];
                let n = encoder.encode(&pcm, &mut enc).unwrap();
                let base = f * tier_slot(tier);
                window_buf[base..base + 2].copy_from_slice(&(n as u16).to_le_bytes());
                window_buf[base + 2..base + 2 + n].copy_from_slice(&enc[..n]);
            }

            // --- bundle: [ctrl][source(wid)][repair(wid−1)], exactly as the engine builds it ---
            let fec = raptorq::Encoder::new(&window_buf, oti(tier));
            let pkts = fec.get_encoded_packets(1);
            assert_eq!(pkts.len(), 2);
            assert_eq!(pkts[0].data().len(), tier_window_bytes(tier));
            let rep = prev_repair.take();
            let mut payload = Vec::new();
            let ctrl =
                tier as u8 | rep.as_ref().map_or(0, |(rt, _)| 0b1000 | ((*rt as u8) << 4));
            payload.push(ctrl);
            payload.extend_from_slice(pkts[0].data());
            if let Some((_, r)) = &rep {
                payload.extend_from_slice(r);
            }
            tx_chain.advance_to(StepChain::step_for_seq(wid));
            let wire = packet::seal(&tx_chain, wid, &payload).unwrap();
            prev_repair = Some((tier, pkts[1].data().to_vec()));

            // --- DROP every third datagram (never two consecutive — the piggyback's design point) ---
            let lost = wid % 3 == 1;
            wid += 1;
            if lost {
                continue;
            }

            // --- callee: parse, open, ctrl split, feed BOTH symbols exactly as the engine does ---
            let (header, sealed) = packet::parse_header(&wire).unwrap();
            let opened = packet::open(&mut rx_chain, &header, sealed).unwrap();
            let ctrl = opened[0];
            let tier_src = (ctrl & 0b111) as usize;
            let rep_present = ctrl & 0b1000 != 0;
            let tier_rep = ((ctrl >> 4) & 0b111) as usize;
            let src_len = tier_window_bytes(tier_src);
            let expect =
                1 + src_len + if rep_present { tier_window_bytes(tier_rep) } else { 0 };
            assert_eq!(opened.len(), expect, "bundle shape must be exact");
            let body = &opened[1..];
            let mut inputs: Vec<(u32, usize, u32, &[u8])> =
                vec![(header.seq, tier_src, 0, &body[..src_len])];
            if rep_present && header.seq > 0 {
                inputs.push((header.seq - 1, tier_rep, 1, &body[src_len..]));
            }
            for (w, t, esi, sym) in inputs {
                if rx_done.contains_key(&w) {
                    continue;
                }
                let entry = rx_decoders
                    .entry(w)
                    .or_insert_with(|| (t, raptorq::Decoder::new(oti(t))));
                assert_eq!(entry.0, t, "same window must never change rung");
                let ep = raptorq::EncodingPacket::new(
                    raptorq::PayloadId::new(0, esi),
                    sym.to_vec(),
                );
                if let Some(data) = entry.1.decode(ep) {
                    rx_decoders.remove(&w);
                    rx_done.insert(w, data);
                }
            }
        }
    }

    // Every window except the final one must have recovered: dropped datagrams' windows rebuilt from
    // the repair riding the NEXT datagram (the final window's repair never ships — the accepted tail).
    let last = (total_windows - 1) as u32;
    let mut total_energy_out = 0.0f64;
    for w in 0..last {
        let data = rx_done
            .get(&w)
            .unwrap_or_else(|| panic!("window {} must reassemble (source or piggybacked repair)", w));
        let t = (w as usize) / windows_per_tier;
        for f in 0..TIER_FRAMES[t] {
            let base = f * tier_slot(t);
            let n = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
            let mut pcm = vec![0i16; FRAME_SAMPLES];
            let got = decoder.decode(&data[base + 2..base + 2 + n], &mut pcm, false).unwrap();
            assert_eq!(got, FRAME_SAMPLES);
            total_energy_out += pcm.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
        }
    }
    // And the recovered audio carries real energy — the tone survived the round trip.
    assert!(
        total_energy_out > 1.0e9,
        "recovered audio should carry the tone's energy, got {}",
        total_energy_out
    );
}

/// A foreign basket's packet must not open under ours — call identity lives in the key, not a header field. (A mis-SIZED symbol is the engine's shape-drop's job: raptorq PANICS on wrong-size symbols rather than returning None, so the exact-length check in the RX loop is load-bearing — never feed the decoder unchecked sizes.)
#[test]
fn foreign_baskets_stay_sealed() {
    let secret = basket();
    let foreign = derive_call_secret(&[9; 32], &[9; 32], &[9; 32], &[9; 16], &[9; 32], &[9; 32]);
    let foreign_tx = StepChain::new(&foreign, Direction::CalleeToCaller);
    let wire = packet::seal(&foreign_tx, 0, b"not ours, plenty long enough").unwrap();
    let (h, sealed) = packet::parse_header(&wire).unwrap();
    let mut our_rx = StepChain::new(&secret, Direction::CalleeToCaller);
    assert!(
        packet::open(&mut our_rx, &h, sealed).is_none(),
        "a foreign basket's packet must be silence here"
    );
}
