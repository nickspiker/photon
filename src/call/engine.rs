//! The media engine (docs/calls.md) — one thread per call: mic frames → Opus → RaptorQ window → sealed packets out; packets in → window decode → Opus → speaker.
//!
//! Shape choices, and why:
//! - **Opus RESTRICTED_LOWDELAY (CELT), CBR.** No SILK prediction, no in-band FEC, 2.5ms lookahead — loss repair belongs to the fountain code, not psychoacoustic guesswork. CBR + a fixed packet count per window means the wire looks identical whoever's talking (traffic-shape privacy).
//! - **RaptorQ over a 4×10ms window.** Fixed window byte-size (frames length-prefixed into fixed slots) → ONE ObjectTransmissionInformation both ends derive from constants, no per-window negotiation. Repair count is the loss dial; the FEC adds a bounded 40ms, which restricted-lowdelay just paid for.
//! - **No PLC.** A window that can't decode is silence (the playback queue runs dry and renders zeros) — never synthesized guesswork.
//! - **The peer's address FOLLOWS its authenticated packets**: a media packet that opens under the call key re-points our TX at its source address. NAT rebinds and (later) device handoff work without any signaling — the AEAD is the authorization.
//! - Teardown zeroizes both step chains ([`keys::StepChain`] Drop) — the call becomes undecryptable everywhere, forever.

use super::keys::{Direction, StepChain};
use super::packet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 10ms @ 48kHz mono — must match platform::audio::FRAME_SAMPLES.
const FRAME_SAMPLES: usize = crate::platform::audio::FRAME_SAMPLES;
const FRAMES_PER_WINDOW: usize = 4;
/// Fixed slot per encoded frame: 2-byte length prefix + up to 96 bytes of CELT (32kbps CBR ≈ 40; headroom to 76.8kbps).
const FRAME_SLOT: usize = 2 + 96;
const WINDOW_BYTES: usize = FRAMES_PER_WINDOW * FRAME_SLOT; // 392
const FEC_MTU: u16 = 140;
/// Repair packets per window — the redundancy dial (start comfortable, tune on field loss stats).
const REPAIR_PACKETS: u32 = 2;

pub struct EngineParams {
    pub secret: [u8; 32],
    pub call_id8: [u8; 8],
    pub we_are_caller: bool,
    pub peer_addr: SocketAddr,
}

/// Handle held by the UI's ActiveCall. Dropping it does NOT stop the engine — call `stop()` (teardown is an explicit edge).
pub struct EngineHandle {
    stop: Arc<AtomicBool>,
    pub muted: Arc<AtomicBool>,
}

impl EngineHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// The one OTI both ends derive from constants — constant window size makes it call-invariant.
fn oti() -> raptorq::ObjectTransmissionInformation {
    raptorq::ObjectTransmissionInformation::with_defaults(WINDOW_BYTES as u64, FEC_MTU)
}

pub fn start(params: EngineParams) -> EngineHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let muted = Arc::new(AtomicBool::new(false));
    let handle = EngineHandle {
        stop: stop.clone(),
        muted: muted.clone(),
    };
    let (sink_tx, sink_rx) = std::sync::mpsc::channel::<(Vec<u8>, SocketAddr)>();
    super::install_media_sink(sink_tx);
    crate::platform::audio::start();
    if std::thread::Builder::new()
        .name("call-engine".into())
        .spawn(move || run(params, stop, muted, sink_rx))
        .is_err()
    {
        crate::log("CALL: engine thread spawn failed");
        super::clear_media_sink();
        crate::platform::audio::stop();
    }
    handle
}

fn run(
    params: EngineParams,
    stop: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    sink_rx: std::sync::mpsc::Receiver<(Vec<u8>, SocketAddr)>,
) {
    let (tx_dir, rx_dir) = if params.we_are_caller {
        (Direction::CallerToCallee, Direction::CalleeToCaller)
    } else {
        (Direction::CalleeToCaller, Direction::CallerToCallee)
    };
    let mut tx_chain = StepChain::new(&params.secret, tx_dir);
    let mut rx_chain = StepChain::new(&params.secret, rx_dir);

    let mut encoder = match opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::LowDelay) {
        Ok(mut e) => {
            let _ = e.set_vbr(false); // CBR — traffic-shape privacy
            let _ = e.set_bitrate(opus::Bitrate::Bits(32_000));
            e
        }
        Err(e) => {
            crate::logf!("CALL: opus encoder init failed: {}", e);
            teardown();
            return;
        }
    };
    let mut decoder = match opus::Decoder::new(48_000, opus::Channels::Mono) {
        Ok(d) => d,
        Err(e) => {
            crate::logf!("CALL: opus decoder init failed: {}", e);
            teardown();
            return;
        }
    };

    let mut peer = params.peer_addr;
    let mut seq: u32 = 0;
    let mut window_id: u32 = 0;
    let mut window_buf: Vec<u8> = Vec::with_capacity(WINDOW_BYTES);
    let mut frames_in_window = 0usize;

    // RX reassembly: per-window fountain decoders + decoded-PCM stash, played strictly in window order (a hole is skipped, not synthesized — the dry playback queue renders the silence).
    let mut rx_decoders: std::collections::BTreeMap<u32, raptorq::Decoder> = Default::default();
    let mut rx_done: std::collections::BTreeMap<u32, Vec<Vec<i16>>> = Default::default();
    let mut next_play: Option<u32> = None;

    let (mut pkts_out, mut pkts_in, mut windows_lost) = (0u64, 0u64, 0u64);

    crate::logf!(
        "CALL: engine up — tx {} → {}, window {}B × {} frames, repair {}",
        if params.we_are_caller { "c>e" } else { "e>c" },
        peer,
        WINDOW_BYTES,
        FRAMES_PER_WINDOW,
        REPAIR_PACKETS
    );

    while !stop.load(Ordering::Relaxed) {
        // ---- TX: mic → opus → window → fountain → sealed packets ----
        for frame in crate::platform::audio::captured_frames() {
            if muted.load(Ordering::Relaxed) || frame.len() != FRAME_SAMPLES {
                continue;
            }
            let mut enc = vec![0u8; FRAME_SLOT - 2];
            let n = match encoder.encode(&frame, &mut enc) {
                Ok(n) => n,
                Err(e) => {
                    crate::logf!("CALL: opus encode error: {}", e);
                    continue;
                }
            };
            window_buf.extend_from_slice(&(n as u16).to_le_bytes());
            window_buf.extend_from_slice(&enc[..n]);
            window_buf.resize((frames_in_window + 1) * FRAME_SLOT, 0);
            frames_in_window += 1;

            if frames_in_window == FRAMES_PER_WINDOW {
                let fec = raptorq::Encoder::new(&window_buf, oti());
                for ep in fec.get_encoded_packets(REPAIR_PACKETS) {
                    let mut payload = Vec::with_capacity(4 + FEC_MTU as usize + 8);
                    payload.extend_from_slice(&window_id.to_le_bytes());
                    payload.extend_from_slice(&ep.serialize());
                    tx_chain.advance_to(StepChain::step_for_seq(seq));
                    if let Some(wire) =
                        packet::seal(&tx_chain, &params.call_id8, tx_dir, seq, &payload)
                    {
                        if !super::send_media(wire, peer) {
                            crate::log("CALL: media TX channel gone — engine stopping");
                            stop.store(true, Ordering::SeqCst);
                        }
                        pkts_out += 1;
                    }
                    seq = seq.wrapping_add(1);
                }
                window_id = window_id.wrapping_add(1);
                window_buf.clear();
                frames_in_window = 0;
            }
        }

        // ---- RX: sealed packets → fountain windows → opus → speaker ----
        while let Ok((bytes, src)) = sink_rx.try_recv() {
            let Some((header, sealed)) = packet::parse_header(&bytes) else {
                continue;
            };
            if header.call_id8 != params.call_id8 || header.dir != rx_dir {
                continue;
            }
            let Some(payload) = packet::open(&mut rx_chain, &header, sealed) else {
                continue; // wrong key/step/tamper — silence, never a guess
            };
            pkts_in += 1;
            // Authenticated source: the peer's address follows its packets (NAT rebind / future handoff, no signaling needed).
            if src != peer && src != crate::network::status::RELAY_ADDR {
                crate::logf!("CALL: peer media now from {} (was {})", src, peer);
                peer = src;
            }
            if payload.len() < 4 {
                continue;
            }
            let wid = u32::from_le_bytes(payload[..4].try_into().unwrap());
            let np = *next_play.get_or_insert(wid);
            if wid < np || rx_done.contains_key(&wid) {
                continue; // already played or already decoded
            }
            let ep = raptorq::EncodingPacket::deserialize(&payload[4..]);
            let dec = rx_decoders
                .entry(wid)
                .or_insert_with(|| raptorq::Decoder::new(oti()));
            if let Some(data) = dec.decode(ep) {
                rx_decoders.remove(&wid);
                let mut frames = Vec::with_capacity(FRAMES_PER_WINDOW);
                for slot in 0..FRAMES_PER_WINDOW {
                    let base = slot * FRAME_SLOT;
                    let n = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
                    if n == 0 || n > FRAME_SLOT - 2 {
                        continue;
                    }
                    let mut pcm = vec![0i16; FRAME_SAMPLES];
                    match decoder.decode(&data[base + 2..base + 2 + n], &mut pcm, false) {
                        Ok(s) if s == FRAME_SAMPLES => frames.push(pcm),
                        Ok(_) | Err(_) => {}
                    }
                }
                rx_done.insert(wid, frames);
            }
        }

        // ---- Play in strict window order; a hole with two later windows complete is LOST (skip — dry queue = silence). ----
        if let Some(np) = next_play {
            let mut np = np;
            loop {
                if let Some(frames) = rx_done.remove(&np) {
                    for f in frames {
                        crate::platform::audio::queue_playback(f);
                    }
                    np = np.wrapping_add(1);
                } else if rx_done.range(np..).nth(1).is_some() {
                    // Two completed windows beyond the hole — declare it lost, move on.
                    windows_lost += 1;
                    rx_decoders.remove(&np);
                    np = np.wrapping_add(1);
                } else {
                    break;
                }
            }
            next_play = Some(np);
            // Prune stale fountain state behind the play head.
            rx_decoders.retain(|w, _| *w >= np);
        }

        std::thread::sleep(std::time::Duration::from_millis(4));
    }

    crate::logf!(
        "CALL: engine down — {} pkts out, {} in, {} windows lost",
        pkts_out,
        pkts_in,
        windows_lost
    );
    teardown();
    // tx_chain/rx_chain drop here — zeroized; the call is cryptographically gone.
}

fn teardown() {
    super::clear_media_sink();
    crate::platform::audio::stop();
}
