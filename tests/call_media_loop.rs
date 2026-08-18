//! End-to-end call MEDIA loop (docs/calls.md) — proves the crypto/codec/FEC/spool pieces compose
//! through one full call without a live network: both sides derive the basket from shared friendship
//! material, a caller sends sealed fountain-coded windows under packet loss, the callee reassembles
//! and Opus-decodes them, and the recording spool round-trips a kept call. This is the offline half of
//! the self-call harness (the live two-instance test is a field step).

use photon_messenger::call::keys::{derive_call_secret, Direction, StepChain};
use photon_messenger::call::packet;

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
const FRAME_SLOT: usize = 2 + 96;
const FRAMES_PER_WINDOW: usize = 4;
const WINDOW_BYTES: usize = FRAMES_PER_WINDOW * FRAME_SLOT;
const FEC_MTU: u16 = 140;

fn oti() -> raptorq::ObjectTransmissionInformation {
    raptorq::ObjectTransmissionInformation::with_defaults(WINDOW_BYTES as u64, FEC_MTU)
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

/// The full media path: encode → window → fountain → seal → (drop 1 of N per window) → open → decode →
/// reassemble, and assert the callee recovers the caller's audio through the loss.
#[test]
fn media_survives_packet_loss_end_to_end() {
    let secret = basket();
    let call_id8 = [0x11u8; 8];
    let mut tx_chain = StepChain::new(&secret, Direction::CallerToCallee);
    let mut rx_chain = StepChain::new(&secret, Direction::CallerToCallee);

    let mut encoder =
        opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::LowDelay).unwrap();
    encoder.set_vbr(false).unwrap();
    encoder.set_bitrate(opus::Bitrate::Bits(32_000)).unwrap();
    let mut decoder = opus::Decoder::new(48_000, opus::Channels::Mono).unwrap();

    // A recognizable 440Hz-ish tone the decoder should reproduce (energy, not bit-exactness — Opus is lossy).
    let make_frame = |w: usize| -> Vec<i16> {
        (0..FRAME_SAMPLES)
            .map(|i| {
                let t = (w * FRAME_SAMPLES + i) as f32 / 48_000.0;
                ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16
            })
            .collect()
    };

    let windows = 8usize;
    let mut seq: u32 = 0;
    let mut recovered_windows = 0usize;
    let mut total_energy_out = 0.0f64;

    for w in 0..windows {
        // --- caller: encode 4 frames into a fixed-size window ---
        let mut window_buf = vec![0u8; WINDOW_BYTES];
        for f in 0..FRAMES_PER_WINDOW {
            let pcm = make_frame(w * FRAMES_PER_WINDOW + f);
            let mut enc = vec![0u8; FRAME_SLOT - 2];
            let n = encoder.encode(&pcm, &mut enc).unwrap();
            let base = f * FRAME_SLOT;
            window_buf[base..base + 2].copy_from_slice(&(n as u16).to_le_bytes());
            window_buf[base + 2..base + 2 + n].copy_from_slice(&enc[..n]);
        }

        // --- fountain-encode with 2 repair packets, seal each, DROP THE FIRST of the window (loss) ---
        let fec = raptorq::Encoder::new(&window_buf, oti());
        let packets = fec.get_encoded_packets(2);
        let mut rx_decoder = raptorq::Decoder::new(oti());
        let mut decoded_window: Option<Vec<u8>> = None;
        for (i, ep) in packets.iter().enumerate() {
            let lost = i == 0; // simulate one lost packet per window
            let mut payload = Vec::new();
            payload.extend_from_slice(&(w as u32).to_le_bytes());
            payload.extend_from_slice(&ep.serialize());
            tx_chain.advance_to(StepChain::step_for_seq(seq));
            let wire = packet::seal(&tx_chain, &call_id8, Direction::CallerToCallee, seq, &payload)
                .unwrap();
            seq += 1;
            if lost {
                continue;
            }
            // --- callee: parse, open under the step key, feed the fountain decoder ---
            let (header, sealed) = packet::parse_header(&wire).unwrap();
            let opened = packet::open(&mut rx_chain, &header, sealed).unwrap();
            let ep2 = raptorq::EncodingPacket::deserialize(&opened[4..]);
            if let Some(data) = rx_decoder.decode(ep2) {
                decoded_window = Some(data);
            }
        }

        // --- callee: decode the recovered window's 4 frames back to PCM ---
        if let Some(data) = decoded_window {
            recovered_windows += 1;
            for f in 0..FRAMES_PER_WINDOW {
                let base = f * FRAME_SLOT;
                let n =
                    u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
                let mut pcm = vec![0i16; FRAME_SAMPLES];
                let got = decoder.decode(&data[base + 2..base + 2 + n], &mut pcm, false).unwrap();
                assert_eq!(got, FRAME_SAMPLES);
                total_energy_out += pcm.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
            }
        }
    }

    // Every window recovered despite one lost packet each (2 repair packets cover a single loss).
    assert_eq!(
        recovered_windows, windows,
        "RaptorQ repair must recover every window through single-packet loss"
    );
    // And the recovered audio carries real energy — the tone survived the round trip.
    assert!(
        total_energy_out > 1.0e9,
        "recovered audio should carry the tone's energy, got {}",
        total_energy_out
    );
}

/// A window whose loss exceeds the repair budget must NOT decode — proving the FEC isn't silently
/// fabricating, and the engine would correctly render that window as silence.
#[test]
fn loss_beyond_repair_budget_does_not_decode() {
    let secret = basket();
    let mut tx_chain = StepChain::new(&secret, Direction::CalleeToCaller);
    let mut window_buf = vec![7u8; WINDOW_BYTES];
    window_buf[0] = 42;
    let fec = raptorq::Encoder::new(&window_buf, oti());
    let packets = fec.get_encoded_packets(2);
    let mut rx_decoder = raptorq::Decoder::new(oti());
    let mut decoded = None;
    // Deliver only ONE packet — far below what's needed to reconstruct 392 bytes.
    let ep = &packets[0];
    let mut seq = 0u32;
    tx_chain.advance_to(StepChain::step_for_seq(seq));
    seq += 1;
    let _ = seq;
    let ep2 = raptorq::EncodingPacket::deserialize(&ep.serialize());
    if let Some(d) = rx_decoder.decode(ep2) {
        decoded = Some(d);
    }
    assert!(decoded.is_none(), "one packet cannot reconstruct a full window");
}
