//! End-to-end call MEDIA loop (docs/calls.md) — proves the crypto/codec/FEC pieces compose
//! through one full call without a live network: both sides derive the basket from shared friendship
//! material, a caller sends sealed fountain-coded windows under packet loss, the callee reassembles
//! and Opus-decodes them. This is the offline half of the self-call harness (the live two-instance
//! test is a field step). Mirrors the engine's STRIPPED wire (2026-08-19, round 2): 5-byte clear
//! header (magic C7 + seq), sealed payload = the BARE symbol — window_id = seq >> 1, esi = seq & 1,
//! and the rung derives from the payload LENGTH; 1 source + 1 repair packet per window.

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
const FRAMES_PER_WINDOW: usize = 2;
// The engine's ladder, mirrored: rates and per-rung max encoded bytes (slots sized so windows are 8-aligned = exact single RaptorQ symbol).
const TIER_RATES: [i32; 4] = [16_000, 32_000, 64_000, 128_000];
const TIER_MAX_ENC: [usize; 4] = [26, 46, 86, 166];

fn tier_slot(t: usize) -> usize {
    2 + TIER_MAX_ENC[t]
}

fn tier_window_bytes(t: usize) -> usize {
    FRAMES_PER_WINDOW * tier_slot(t)
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

/// The full media path at EVERY ladder rung: encode → window → fountain → seal → (drop one of the two
/// packets, alternating source/repair) → open → derive wid/esi/tier from seq + length → decode, asserting the
/// callee recovers the caller's audio through the loss at each rung's own bitrate + geometry.
#[test]
fn media_survives_packet_loss_end_to_end_at_every_rung() {
    let secret = basket();
    let mut tx_chain = StepChain::new(&secret, Direction::CallerToCallee);
    let mut rx_chain = StepChain::new(&secret, Direction::CallerToCallee);

    let mut encoder =
        opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::LowDelay).unwrap();
    encoder.set_vbr(false).unwrap();
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

    let windows_per_tier = 4usize;
    let mut wid: u32 = 0;
    let mut recovered_windows = 0usize;
    let mut total_energy_out = 0.0f64;

    for tier in 0..TIER_RATES.len() {
        encoder.set_bitrate(opus::Bitrate::Bits(TIER_RATES[tier])).unwrap();
        for w in 0..windows_per_tier {
            // --- caller: encode 2 frames into this rung's window ---
            let mut window_buf = vec![0u8; tier_window_bytes(tier)];
            for f in 0..FRAMES_PER_WINDOW {
                let pcm = make_frame((tier * windows_per_tier + w) * FRAMES_PER_WINDOW + f);
                let mut enc = vec![0u8; TIER_MAX_ENC[tier]];
                let n = encoder.encode(&pcm, &mut enc).unwrap();
                let base = f * tier_slot(tier);
                window_buf[base..base + 2].copy_from_slice(&(n as u16).to_le_bytes());
                window_buf[base + 2..base + 2 + n].copy_from_slice(&enc[..n]);
            }

            // --- fountain: 1 source + 1 repair, exactly window-sized symbols; DROP one, alternating which ---
            let fec = raptorq::Encoder::new(&window_buf, oti(tier));
            let packets = fec.get_encoded_packets(1);
            assert_eq!(packets.len(), 2);
            let mut rx_decoder = raptorq::Decoder::new(oti(tier));
            let mut decoded_window: Option<Vec<u8>> = None;
            for (i, ep) in packets.iter().enumerate() {
                let lost = i == (w % 2); // alternate losing the source and the repair
                assert_eq!(ep.data().len(), tier_window_bytes(tier), "symbol must be the exact window");
                // seq is DERIVED: window_id * 2 + esi — the payload is the bare symbol, nothing else.
                let esi = ep.payload_id().encoding_symbol_id();
                let pseq = wid * 2 + esi;
                tx_chain.advance_to(StepChain::step_for_seq(pseq));
                let wire = packet::seal(&tx_chain, pseq, ep.data()).unwrap();
                if lost {
                    continue;
                }
                // --- callee: parse the 5-byte header, open under the step key, DERIVE the bookkeeping ---
                let (header, sealed) = packet::parse_header(&wire).unwrap();
                let opened = packet::open(&mut rx_chain, &header, sealed).unwrap();
                let rx_wid = header.seq >> 1;
                let rx_esi = header.seq & 1;
                assert_eq!(rx_wid, wid);
                let rx_tier = (0..TIER_RATES.len())
                    .find(|&t| opened.len() == tier_window_bytes(t))
                    .expect("payload length must name a rung");
                assert_eq!(rx_tier, tier);
                let ep2 = raptorq::EncodingPacket::new(raptorq::PayloadId::new(0, rx_esi), opened);
                if let Some(data) = rx_decoder.decode(ep2) {
                    decoded_window = Some(data);
                }
            }
            wid += 1;

            // --- callee: decode the recovered window's frames back to PCM ---
            if let Some(data) = decoded_window {
                recovered_windows += 1;
                for f in 0..FRAMES_PER_WINDOW {
                    let base = f * tier_slot(tier);
                    let n = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
                    let mut pcm = vec![0i16; FRAME_SAMPLES];
                    let got = decoder.decode(&data[base + 2..base + 2 + n], &mut pcm, false).unwrap();
                    assert_eq!(got, FRAME_SAMPLES);
                    total_energy_out += pcm.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
                }
            }
        }
    }

    // Every window recovered despite one lost packet each — at every rung, whichever packet died.
    assert_eq!(
        recovered_windows,
        TIER_RATES.len() * windows_per_tier,
        "the window must reassemble from either surviving packet at every rung"
    );
    // And the recovered audio carries real energy — the tone survived the round trip.
    assert!(
        total_energy_out > 1.0e9,
        "recovered audio should carry the tone's energy, got {}",
        total_energy_out
    );
}

/// A foreign basket's packet must not open under ours — call identity lives in the key, not a header field. (A mis-SIZED symbol is the engine's shape-drop's job: raptorq PANICS on wrong-size symbols rather than returning None, so the length check in the RX loop is load-bearing, verified by the shape-drop tally — never feed the decoder unchecked sizes.)
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
