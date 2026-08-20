//! The media engine (docs/calls.md) — one thread per call: mic frames → Opus → RaptorQ window → sealed packets out; packets in → window decode → Opus → speaker.
//!
//! Shape choices, and why:
//! - **Opus RESTRICTED_LOWDELAY (CELT), CBR, on a channel-aware ladder.** No SILK prediction, no in-band FEC, 2.5ms lookahead — loss repair belongs to the fountain code, not psychoacoustic guesswork. Within a rung the wire is constant-size CBR (traffic-shape privacy); the rung climbs/drops only on channel evidence (see the TIER_RATES block).
//! - **RaptorQ over a tier-sized window (4×10ms at the floor, 2×10ms above), one datagram per window.** Frames length-prefix into that rung's fixed slots; the sealed payload is [ctrl][source(N)][repair(N−1)] — the repair PIGGYBACKS on the next window's datagram, so the two copies ride 20-40ms apart (burst-loss immunity) at half the packet rate, and seq IS the window id. The ctrl byte names both rungs; steady-state latency is untouched (only loss RECOVERY waits one window).
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
// Frames per window is PER-RUNG: 4 at the floor (40ms batching amortizes the fixed per-packet cost where bandwidth is scarcest — the floor is for links where +20ms is noise and slow-start's first second, where the jitter buffer is priming anyway), 2 everywhere above (~10ms batching for latency). Packets per window stays invariantly 2, so the seq derivations hold at every rung.
const TIER_FRAMES: [usize; 4] = [4, 2, 2, 2];
// Repair symbols per window — with the symbol spanning the WHOLE window (see `oti`), 1 repair = 2 packets per window and the window survives EITHER packet lost. This beats the old 3-source+2-repair spread on both axes: fewer bytes (2 packets not 5) AND better loss odds (window dies only when BOTH packets drop, p² vs the old ≥3-of-5 tail).
const REPAIR_PACKETS: u32 = 1;

// CHANNEL-AWARE CBR LADDER (Nick's call 2026-08-19, flag day #2): four rungs 16k → 128k, every call starts at rung 0 and climbs on evidence — TCP-slow-start for voice.
// The rate is CHANNEL-driven, never content-driven: within a rung everything is constant-size CBR (the VBR phoneme side channel stays closed), and a rung switch only tells an observer what the network already shows them.
// Slots and windows are TIER-SIZED so low rungs are genuinely cheap on the wire — a fixed max-size slot would pad 16k out to 128k's cost (the padding trap).
// The ctrl byte names each symbol's rung (a switch between windows makes the bundle's two symbols different sizes — length alone went ambiguous when the repair started piggybacking); a mid-call switch decodes seamlessly.
// Dynamics are AIMD on edges, not timers: CLIMB_CLEAN_WINDOWS completed windows → one rung up; a lost window → DROP_RUNGS_ON_LOSS down.
// Climb evidence is RECEIVE-side cleanliness — a proxy for the channel both ways until a call_stats feedback frame exists (deferred in docs/calls.md); comment here so nobody mistakes it for measured TX loss.
// Opus bandwidth follows bitrate automatically (NB at 16k thru fullband at 128k), so this ladder IS the 8kHz→48kHz ramp with the PCM interface pinned at 48k.
// FLAG-DAY: pre-ladder builds cannot parse this wire at all; the whole fleet updates together.
const TIER_RATES: [i32; 4] = [16_000, 32_000, 64_000, 128_000];
/// Max encoded bytes per 10ms frame at each rung: hard CBR emits exactly rate/800 bytes, +2 headroom — sized so every rung's WINDOW is a multiple of 8, which makes the RaptorQ symbol exactly the window (its alignment rounds max_packet_size down to a multiple of 8; an unaligned window would split into two padded symbols and re-grow the wire). Trimmed from +6 in the piggyback flag day: at the floor the slot padding was 21% of the wire.
const TIER_MAX_ENC: [usize; 4] = [22, 42, 82, 162];
/// Completed-window streak that earns one rung up (~0.5s at 20ms windows, ~1s at the floor's 40ms) — the fast first climb keeps the POTS-ish floor a blink on a clean link.
const CLIMB_CLEAN_WINDOWS: u32 = 25;
/// Rungs dropped on a lost window.
const DROP_RUNGS_ON_LOSS: usize = 2;

/// Slot per encoded frame at a rung: 2-byte length prefix + that rung's max payload.
const fn tier_slot(tier: usize) -> usize {
    2 + TIER_MAX_ENC[tier]
}

/// FEC window bytes at a rung.
const fn tier_window_bytes(tier: usize) -> usize {
    TIER_FRAMES[tier] * tier_slot(tier)
}

// SOFT ENERGY DUCK (echo layer 2, docs/calls.md) — built but DISARMED (Nick, 2026-08-19): the first field passes run STRICT CLEAN echo-prone audio to hear the raw truth. Two reasons: the peak-hold far_level (~80ms hang) smears the gate's timing, which is exactly what clips double-talk and pumps; and Android capture still runs VOICE_COMMUNICATION, whose vendor mic-side AEC may or may not already cancel against the MEDIA-path output (device-dependent) — the raw run reveals which.
// While disarmed the trigger is still MEASURED: far-active frames (what the duck WOULD have gated) are counted into the engine-down log, so the raw run quantifies both the echo and the would-be gate before any audio is touched. Flip DUCK_ENABLED to re-arm the glide.
const DUCK_ENABLED: bool = false;
const DUCK_FAR_LEVEL: u32 = 200;
const DUCK_GAIN_FLOOR: f32 = 0.1;
const DUCK_ATTACK: f32 = 0.5;
const DUCK_RELEASE: f32 = 0.05;

pub struct EngineParams {
    pub secret: [u8; 32],
    pub we_are_caller: bool,
    pub peer_addr: SocketAddr,
    /// Recording spool (key, path) — recording by default; None only when the spool couldn't be minted (disk trouble; the call proceeds unrecorded, logged).
    pub spool: Option<([u8; 32], std::path::PathBuf)>,
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

/// The FEC geometry at a rung — both ends derive it from the sealed payload's length, so it needs no negotiation. The symbol IS the window (max_packet_size = window, and windows are 8-aligned so raptorq's alignment rounding changes nothing): one source symbol, zero pad bytes — the MTU-padding trap (140-byte symbols carrying 26 real bytes at the floor) is dead.
fn oti(tier: usize) -> raptorq::ObjectTransmissionInformation {
    raptorq::ObjectTransmissionInformation::with_defaults(
        tier_window_bytes(tier) as u64,
        tier_window_bytes(tier) as u16,
    )
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
            let _ = e.set_vbr(false); // CBR — traffic-shape privacy (VBR leaks the speech envelope via packet sizes)
            let _ = e.set_bitrate(opus::Bitrate::Bits(TIER_RATES[0])); // every call starts at the ladder floor and climbs on evidence
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

    let mut spool = params
        .spool
        .as_ref()
        .and_then(|(k, p)| super::spool::SpoolWriter::create(k, p));
    if spool.is_none() {
        crate::log("CALL: no spool — this call is not being recorded");
    }
    let mut peer = params.peer_addr;
    // seq IS the window id — one datagram per window, no independent counter to drift.
    let mut window_id: u32 = 0;
    // The completed window's repair symbol (tier, bytes), waiting to piggyback on the NEXT window's datagram.
    let mut prev_repair: Option<(usize, Vec<u8>)> = None;
    let mut window_buf: Vec<u8> = Vec::with_capacity(tier_window_bytes(TIER_RATES.len() - 1));
    let mut frames_in_window = 0usize;

    // Ladder state: `tier` is what the CURRENT window encodes at, `pending_tier` is where the evidence says to go — switches land only on window boundaries because a window's slot geometry is fixed the moment its first frame lands.
    let mut tier: usize = 0;
    let mut pending_tier: usize = 0;
    let mut clean_rx_windows: u32 = 0;
    let (mut tier_ups, mut tier_downs) = (0u32, 0u32);
    // Duck state: current gain glides between 1.0 and the floor; route is cached at engine start (Headset = no acoustic path = never duck; Unknown ducks, the safe default).
    let mut duck_gain: f32 = 1.0;
    let mut ducked_frames: u64 = 0;
    let mut far_active_frames: u64 = 0;
    let route_ducks = !matches!(
        crate::platform::audio::route(),
        crate::platform::audio::AudioRoute::Headset
    );

    // RX reassembly: per-window (tier, fountain decoder) + decoded-PCM stash, played strictly in window order (a hole is skipped, not synthesized — the dry playback queue renders the silence).
    let mut rx_decoders: std::collections::BTreeMap<u32, (usize, raptorq::Decoder)> =
        Default::default();
    let mut rx_done: std::collections::BTreeMap<u32, Vec<Vec<i16>>> = Default::default();
    let mut next_play: Option<u32> = None;

    let (mut pkts_out, mut pkts_in, mut windows_lost) = (0u64, 0u64, 0u64);
    // RX drop-reason tally — see the RX loop for why each is counted apart (addressing vs secret-desync diagnosis). Shape = opened fine but the payload geometry is wrong (truncation bug or a mixed-version peer).
    let (mut rx_seen, mut rx_drop_parse, mut rx_drop_shape, mut rx_drop_open) = (0u64, 0u64, 0u64, 0u64);
    // Audio ENERGY readout — mean |sample| of what we CAPTURED (tx) and what we DECODED for playback (rx). A silent direction shows as ~0 here: near-zero tx = our mic content is dead (route/gain/AEC over-duck, NOT a permission miss — that path never reaches capture); non-zero rx that the user still didn't hear = a playback/route problem downstream. Separates "one side heard" into capture-silent vs playback-silent without guessing (field 2026-08-19).
    let (mut tx_energy, mut tx_frames, mut rx_energy, mut rx_frames) = (0u64, 0u64, 0u64, 0u64);

    crate::logf!(
        "CALL: engine up — tx {} → {}, ladder {}..{} kbps (start {}), floor window {} frames, repair {}, duck {}",
        if params.we_are_caller { "c>e" } else { "e>c" },
        peer,
        TIER_RATES[0] / 1000,
        TIER_RATES[TIER_RATES.len() - 1] / 1000,
        TIER_RATES[0] / 1000,
        TIER_FRAMES[0],
        REPAIR_PACKETS,
        if !DUCK_ENABLED {
            "disarmed (raw echo run)"
        } else if route_ducks {
            "armed"
        } else {
            "bypassed (headset)"
        }
    );

    while !stop.load(Ordering::Relaxed) {
        // ---- TX: mic → opus → window → fountain → sealed packets ----
        for frame in crate::platform::audio::captured_frames() {
            if muted.load(Ordering::Relaxed) || frame.len() != FRAME_SAMPLES {
                continue;
            }
            // Rung switches land only between windows — a window's slot geometry is fixed at its first frame.
            if frames_in_window == 0 && pending_tier != tier {
                tier = pending_tier;
                let _ = encoder.set_bitrate(opus::Bitrate::Bits(TIER_RATES[tier]));
            }
            // Mic health level BEFORE the duck, so a heavy duck never reads as a dead mic in the tally.
            tx_energy += frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>();
            tx_frames += 1;
            // Duck trigger — measured always, APPLIED only when armed (see the DUCK_ENABLED block: first field passes run raw echo-prone audio and just count what the duck would have gated).
            let mut frame = frame;
            let far_active = route_ducks && crate::platform::audio::far_level() > DUCK_FAR_LEVEL;
            if far_active {
                far_active_frames += 1;
            }
            if DUCK_ENABLED {
                let target = if far_active { DUCK_GAIN_FLOOR } else { 1.0 };
                let coef = if target < duck_gain { DUCK_ATTACK } else { DUCK_RELEASE };
                duck_gain += (target - duck_gain) * coef;
                if duck_gain < 0.995 {
                    ducked_frames += 1;
                    for s in frame.iter_mut() {
                        *s = (*s as f32 * duck_gain) as i16;
                    }
                }
            }
            let mut enc = vec![0u8; TIER_MAX_ENC[tier]];
            let n = match encoder.encode(&frame, &mut enc) {
                Ok(n) => n,
                Err(e) => {
                    crate::logf!("CALL: opus encode error: {}", e);
                    continue;
                }
            };
            if let Some(w) = spool.as_mut() {
                w.append(0, vsf::eagle_time_oscillations(), &enc[..n]);
            }
            window_buf.extend_from_slice(&(n as u16).to_le_bytes());
            window_buf.extend_from_slice(&enc[..n]);
            window_buf.resize((frames_in_window + 1) * tier_slot(tier), 0);
            frames_in_window += 1;

            if frames_in_window == TIER_FRAMES[tier] {
                // PIGGYBACK BUNDLE (flag day, 2026-08-20): ONE datagram per window — sealed payload [ctrl:1][source(N)][repair(N−1)]. Halves the packet rate (per-packet header cost was 19% of the floor wire), and the window's two copies now ride datagrams one window APART, so a burst must kill two consecutive datagrams to lose audio — strictly better than the old back-to-back pair. Steady-state latency unchanged: the source still ships the instant the window closes; only loss RECOVERY waits one extra window. seq = window id (the nonce, the step index, everything); ctrl = tier_src:2 | rep_present:1<<2 | tier_rep:2<<3, because a rung switch between windows makes the two symbols different sizes and the length alone goes ambiguous.
                let fec = raptorq::Encoder::new(&window_buf, oti(tier));
                let pkts = fec.get_encoded_packets(REPAIR_PACKETS);
                let rep = prev_repair.take();
                let mut payload = Vec::with_capacity(1 + tier_window_bytes(tier) * 2);
                let ctrl = tier as u8
                    | rep
                        .as_ref()
                        .map_or(0, |(rt, _)| 0b100 | ((*rt as u8) << 3));
                payload.push(ctrl);
                payload.extend_from_slice(pkts[0].data());
                if let Some((_, r)) = &rep {
                    payload.extend_from_slice(r);
                }
                let seq = window_id;
                tx_chain.advance_to(StepChain::step_for_seq(seq));
                if let Some(wire) = packet::seal(&tx_chain, seq, &payload) {
                    if !super::send_media(wire, peer) {
                        crate::log("CALL: media TX channel gone — engine stopping");
                        stop.store(true, Ordering::SeqCst);
                    }
                    pkts_out += 1;
                }
                // The repair symbol rides the NEXT window's datagram. The final window's repair never ships (the call ended); its ~20-40ms tail is protected only by its source — accepted.
                prev_repair = Some((tier, pkts[1].data().to_vec()));
                window_id = window_id.wrapping_add(1);
                window_buf.clear();
                frames_in_window = 0;
            }
        }

        // ---- RX: sealed packets → fountain windows → opus → speaker ----
        while let Ok((bytes, src)) = sink_rx.try_recv() {
            // DROP-REASON TALLY (docs/calls.md diagnostics): every RX reject below is a silent `continue`, so a dead call is indistinguishable at engine-down between "packets never reached this device" (addressing/NAT) and "packets arrived but won't decrypt" (basket-secret desync). Count them apart. Field 2026-08-19: a call went Active but engine-down read "0 in" with zero other signal — this tally is the tripwire that says which half broke. `rx_seen` counts datagrams the recv-worker fast-path actually handed us (magic already matched), so `rx_seen > 0 && pkts_in == 0` = arrived-but-undecryptable = secret mismatch; `rx_seen == 0` = never arrived = look at the target address / relay.
            rx_seen += 1;
            let Some((header, sealed)) = packet::parse_header(&bytes) else {
                rx_drop_parse += 1;
                continue;
            };
            // No call-id or direction check — both live in the key now: the AEAD below is the whole gate (a stale call's straggler or a cross-direction packet just fails to open).
            let Some(payload) = packet::open(&mut rx_chain, &header, sealed) else {
                rx_drop_open += 1;
                continue; // wrong key/step/tamper — silence, never a guess
            };
            pkts_in += 1;
            // Authenticated source: the peer's address follows its packets (NAT rebind / future handoff, no signaling needed).
            if src != peer && src != crate::network::status::RELAY_ADDR {
                crate::logf!("CALL: peer media now from {} (was {})", src, peer);
                peer = src;
            }
            // Bundle payload: [ctrl:1][source(seq)][repair(seq−1) if flagged]. seq IS the window id; the ctrl byte names both rungs (a rung switch between windows makes the two symbols different sizes, so length alone is ambiguous). The EXACT-length check is LOAD-BEARING: raptorq panics on mis-sized symbols, so nothing unchecked may reach a decoder.
            if payload.is_empty() {
                rx_drop_shape += 1;
                continue;
            }
            let ctrl = payload[0];
            let tier_src = (ctrl & 0b11) as usize;
            let rep_present = ctrl & 0b100 != 0;
            let tier_rep = ((ctrl >> 3) & 0b11) as usize;
            let src_len = tier_window_bytes(tier_src);
            let expected = 1 + src_len + if rep_present { tier_window_bytes(tier_rep) } else { 0 };
            if payload.len() != expected {
                rx_drop_shape += 1;
                continue;
            }
            let body = &payload[1..];
            // Feed the source symbol to window seq, and the piggybacked repair to window seq−1 — same per-symbol pipeline for both (dedup → fountain → slot walk → Opus → climb evidence).
            let mut inputs: [(u32, usize, u32, &[u8]); 2] =
                [(header.seq, tier_src, 0, &body[..src_len]), (0, 0, 1, &[])];
            let n_inputs = if rep_present && header.seq > 0 {
                inputs[1] = (header.seq - 1, tier_rep, 1, &body[src_len..]);
                2
            } else {
                1
            };
            for &(wid, wtier, esi, sym) in inputs.iter().take(n_inputs) {
                let np = *next_play.get_or_insert(wid);
                if wid < np || rx_done.contains_key(&wid) {
                    continue; // already played or already decoded
                }
                let ep = raptorq::EncodingPacket::new(raptorq::PayloadId::new(0, esi), sym.to_vec());
                let entry = rx_decoders
                    .entry(wid)
                    .or_insert_with(|| (wtier, raptorq::Decoder::new(oti(wtier))));
                let dtier = entry.0;
                // A same-window symbol at a DIFFERENT rung can't happen from a healthy sender (rung switches land on window boundaries) — feeding it would panic the decoder, so it's a shape-drop too.
                if dtier != wtier {
                    rx_drop_shape += 1;
                    continue;
                }
                if let Some(data) = entry.1.decode(ep) {
                    rx_decoders.remove(&wid);
                    let mut frames = Vec::with_capacity(TIER_FRAMES[dtier]);
                    for slot in 0..TIER_FRAMES[dtier] {
                        let base = slot * tier_slot(dtier);
                        let n = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
                        if n == 0 || n > TIER_MAX_ENC[dtier] {
                            continue;
                        }
                        if let Some(w) = spool.as_mut() {
                            w.append(1, vsf::eagle_time_oscillations(), &data[base + 2..base + 2 + n]);
                        }
                        let mut pcm = vec![0i16; FRAME_SAMPLES];
                        match decoder.decode(&data[base + 2..base + 2 + n], &mut pcm, false) {
                            Ok(s) if s == FRAME_SAMPLES => {
                                rx_energy += pcm.iter().map(|v| v.unsigned_abs() as u64).sum::<u64>();
                                rx_frames += 1;
                                frames.push(pcm);
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    rx_done.insert(wid, frames);
                    // Receive-side cleanliness is the climb evidence (channel proxy — see the ladder comment): a full streak of completed windows earns one rung up.
                    clean_rx_windows += 1;
                    if clean_rx_windows >= CLIMB_CLEAN_WINDOWS && pending_tier + 1 < TIER_RATES.len() {
                        pending_tier += 1;
                        clean_rx_windows = 0;
                        tier_ups += 1;
                        crate::logf!("CALL: tier up → {} kbps", TIER_RATES[pending_tier] / 1000);
                    }
                }
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
                    // A lost window is the AIMD drop edge: two rungs down, evidence streak restarts.
                    clean_rx_windows = 0;
                    if pending_tier > 0 {
                        pending_tier = pending_tier.saturating_sub(DROP_RUNGS_ON_LOSS);
                        tier_downs += 1;
                        crate::logf!(
                            "CALL: tier down → {} kbps (window lost)",
                            TIER_RATES[pending_tier] / 1000
                        );
                    }
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

        // 1ms poll granularity (was 4ms): captured frames and just-arrived packets wait at most 1ms for their loop pass, shaving ~6ms off the round trip for the cost of a few more wakeups — cheap on a call-dedicated thread.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    crate::logf!(
        "CALL: engine down — {} pkts out, {} in, {} windows lost",
        pkts_out,
        pkts_in,
        windows_lost
    );
    // The diagnostic that separates the two silent-failure worlds (see the RX loop): rx_seen=0 → media never arrived (target address / NAT / relay); rx_seen>0 with pkts_in=0 and rx_drop_open>0 → arrived but the basket secret didn't match (key derivation desync). Only logged when something was received or dropped, so a clean call stays quiet.
    if rx_seen > 0 || rx_drop_parse > 0 || rx_drop_shape > 0 || rx_drop_open > 0 {
        crate::logf!(
            "CALL: rx tally — seen {} → parse-drop {}, open-drop {}, shape-drop {}, decoded {}",
            rx_seen,
            rx_drop_parse,
            rx_drop_open,
            rx_drop_shape,
            pkts_in
        );
    }
    // Mean |sample| each way (0..32767). ~0 on a side = that direction carried silence; compare tx (our mic) vs rx (what we played) to place a "one-way heard" report at capture or playback.
    let tx_level = if tx_frames > 0 { tx_energy / (tx_frames * FRAME_SAMPLES as u64) } else { 0 };
    let rx_level = if rx_frames > 0 { rx_energy / (rx_frames * FRAME_SAMPLES as u64) } else { 0 };
    crate::logf!(
        "CALL: audio level — tx(mic) {} over {} frames, rx(play) {} over {} frames",
        tx_level,
        tx_frames,
        rx_level,
        rx_frames
    );
    // Ladder + duck field-tuning readout: where the call ended up, how it moved, and — while the duck is disarmed — how often the far-active trigger WOULD have gated the mic (the raw-echo run's key number).
    crate::logf!(
        "CALL: ladder — ended {} kbps, {} up(s), {} down(s); duck {} — ducked {}, far-active {} of {} frames",
        TIER_RATES[tier] / 1000,
        tier_ups,
        tier_downs,
        if DUCK_ENABLED { "armed" } else { "disarmed" },
        ducked_frames,
        far_active_frames,
        tx_frames
    );
    teardown();
    // tx_chain/rx_chain drop here — zeroized; the call is cryptographically gone.
}

fn teardown() {
    super::clear_media_sink();
    crate::platform::audio::stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_slots_fit_cbr_frames() {
        // CBR emits exactly rate/800 bytes per 10ms frame; every rung's slot must hold that with margin, every window must be 8-aligned so raptorq's symbol is EXACTLY the window (unaligned would split + pad — the trap this design kills), and both rung indices must fit the ctrl byte's 2-bit fields.
        assert!(TIER_RATES.len() <= 4, "tiers ride 2-bit ctrl fields");
        for t in 0..TIER_RATES.len() {
            assert!(TIER_RATES[t] as usize / 800 + 2 <= TIER_MAX_ENC[t], "rung {} slot too tight", t);
            assert_eq!(tier_window_bytes(t) % 8, 0, "rung {} window must be 8-aligned", t);
        }
    }

    #[test]
    fn every_rung_is_two_exact_packets_and_survives_either_loss() {
        // The stripped geometry: symbol = whole window → exactly 1 source + REPAIR_PACKETS packets, each carrying window-sized data with ZERO pad bytes, and the window reassembles from EITHER packet alone (the repair symbol alone must suffice — that's the loss story).
        for t in 0..TIER_RATES.len() {
            let data: Vec<u8> = (0..tier_window_bytes(t)).map(|i| (i * 7 + t) as u8).collect();
            let enc = raptorq::Encoder::new(&data, oti(t));
            let pkts = enc.get_encoded_packets(REPAIR_PACKETS);
            assert_eq!(pkts.len(), 1 + REPAIR_PACKETS as usize, "rung {}", t);
            for p in &pkts {
                assert_eq!(p.data().len(), tier_window_bytes(t), "rung {} symbol must be exactly the window", t);
                // The symbol id must fit the ctrl byte's 6 bits (it's 0 or 1 here).
                assert!(p.payload_id().encoding_symbol_id() < 64, "rung {}", t);
            }
            for lost in 0..pkts.len() {
                let mut dec = raptorq::Decoder::new(oti(t));
                let mut out: Option<Vec<u8>> = None;
                for (i, p) in pkts.iter().enumerate() {
                    if i == lost {
                        continue;
                    }
                    // Round-trip thru the wire packing: esi into the ctrl byte, symbol bytes raw, reconstructed exactly as the RX loop does.
                    let esi = p.payload_id().encoding_symbol_id();
                    let rebuilt = raptorq::EncodingPacket::new(
                        raptorq::PayloadId::new(0, esi),
                        p.data().to_vec(),
                    );
                    if let Some(d) = dec.decode(rebuilt) {
                        out = Some(d);
                        break;
                    }
                }
                assert_eq!(
                    out.expect("window must reassemble from the surviving packet"),
                    data,
                    "rung {} lost packet {}",
                    t,
                    lost
                );
            }
        }
    }
}
