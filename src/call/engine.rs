//! The media engine (docs/calls.md) — one thread per call: mic frames (5ms CELT, the 2026-09-08 latency flag day) → Opus → RaptorQ window → sealed packets out; packets in → window decode → Opus → speaker.
//!
//! Shape choices, and why:
//! - **Opus RESTRICTED_LOWDELAY (CELT), CBR, on a channel-aware ladder.** No SILK prediction, no in-band FEC, 2.5ms lookahead — loss repair belongs to the fountain code, not psychoacoustic guesswork. Within a rung the wire is constant-size CBR (traffic-shape privacy); the rung climbs/drops only on channel evidence (see the TIER_RATES block).
//! - **RaptorQ over a tier-sized window (8×5ms at the floor, down to 2×5ms at the top rungs), one datagram per window.** Frames length-prefix into that rung's fixed slots; the sealed payload is [ctrl][source(N)][repair(N−1)] — the repair PIGGYBACKS on the next window's datagram, so the two copies ride 10-40ms apart (burst-loss immunity), and seq IS the window id. The ctrl byte names both rungs; steady-state latency is untouched (only loss RECOVERY waits one window).
//! - **No PLC.** A window that can't decode is silence (the playback queue runs dry and renders zeros) — never synthesized guesswork.
//! - **The peer's address FOLLOWS its authenticated packets**: a media packet that opens under the call key re-points our TX at its source address. NAT rebinds and (later) device handoff work without any signaling — the AEAD is the authorization.
//! - Teardown zeroizes both step chains ([`keys::StepChain`] Drop) — the call becomes undecryptable everywhere, forever.

use super::keys::{Direction, StepChain};
use super::packet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 5ms @ 48kHz mono — must match platform::audio::FRAME_SAMPLES.
const FRAME_SAMPLES: usize = crate::platform::audio::FRAME_SAMPLES;
// Frames per window is PER-RUNG, in 5ms frames (flag day 2026-09-08, Nick: "batching nuke"): 8 at the floor (the same 40ms batching where bandwidth is scarcest), 4 at 32k (20ms), 2 at the top rungs — 10ms windows, one datagram per 10ms = double the old packet rate, and the jitter floor rides down with it. Packets per window stays invariantly 2, so the seq derivations hold at every rung.
const TIER_FRAMES: [usize; 5] = [8, 4, 2, 2, 1];
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
const TIER_RATES: [i32; 5] = [16_000, 32_000, 64_000, 128_000, 768_000];
/// The rungs' names for the call panel (Nick 2026-09-11: "spaceball themed names for the quality rungs"): Spaceballs' speeds, bottom to top, the top rung already being plaid.
pub const TIER_NAMES: [&str; 5] = ["sublight", "light speed", "ridiculous speed", "ludicrous speed", "plaid"];

/// A rung's name; anything off the ladder reads as the floor.
pub fn tier_name(tier: usize) -> &'static str {
    TIER_NAMES.get(tier).copied().unwrap_or(TIER_NAMES[0])
}
// PLAID (Nick 2026-09-10, "stupid plaid mode"): the top rung is RAW 48 kHz mono 16-bit PCM — no codec at all, one 5 ms frame per datagram, no repair symbol. A lost datagram is skipped outright (a 5 ms hole, faded not synthesized) so the jitter buffer stays hot instead of paying standing latency for everyone. Reached only on a LAN-class direct path (EngineParams::plaid_allowed) after a full second of clean 10 ms windows; left only when losses run past a rate, not on a lone pair. What it buys is the codec's lookahead and CPU, not fidelity — 128 kbps CELT is already transparent for speech.
const RAW_TIER: usize = 4;
const RAW_FRAME_BYTES: usize = FRAME_SAMPLES * 2;
/// Clean 10 ms windows in a row that earn the plaid rung (1 s at the 128 kbps rung's cadence).
const PLAID_CLIMB_CLEAN_WINDOWS: u32 = 100;
/// Lost windows inside LOSS_WINDOW that push a plaid call back to 128 kbps: 20 of ~400 = a 5 % loss rate. Below that the crispies are the price of the hot buffer.
const PLAID_LOSSES_TO_DROP: usize = 20;
// LOSS-RATE JITTER LOOP (Nick 2026-09-10: "we just always assume packet loss and we PID loop it to keep packet loss under 1/256 and fill in the rest"): a late window is a lost window, never waited for; a 256-slot ring (u8 index, ~1.3-2.5 s — an Earth round trip is under half a second) records lost/played per window slot (an underrun since the last window counts as lost too); the loop drives the jitter TARGET so the loss rate sits at LOSS_SETPOINT. Error in STOPS (log2 of measured over setpoint, floored at −4 stops for a clean window), P + a slow I, no D (loss is too noisy for it). Holes are faded (the crispy), never synthesized.
const LOSS_RING: usize = 256;
/// A learner coupling at or below this (volume-normalized envelope ratio) is a CLEAN route: the duck retires to its floor and the canceller is dropped (engine 1 s control plane).
const CLEAN_G_NORM: f32 = 0.02;
const LOSS_SETPOINT: f32 = 1.0 / 256.0;
const LOSS_KP: f32 = 1.0; // frames per stop of error, immediately
const LOSS_KI: f32 = 0.01; // frames per stop per window, accumulated
const JITTER_TARGET_CAP: usize = 24;
/// Every bundle carries a 10-byte LINK TAIL: [our send stamp ms u32][echo of the peer's newest stamp u32][ms we held it u16] — an RTT measured on every packet, no separate probe.
const LINK_TAIL: usize = 10;
// RECORDING FILLS (Nick 2026-09-10: "get the missing pieces the other party has… fill in as we go and only lose a second or so"): what one side lost is exactly what the other side SENT and spooled, so every window this side declares lost is asked back over a FILL datagram (packet.rs FILL_MAGIC, its own chain), and the peer serves it straight off its spool by window seq. Fills go to the RECORDING only (FILL_FLAG records slotted by seq at transcode) — the live ear already heard the hole. After hangup both engines DRAIN: audio off, the wanted list (declared losses + the tail up to the peer's final window) asked in bulk, each side exits when both are satisfied or the drain deadline passes.
/// Window seqs asked per fill datagram.
const FILL_REQ_PER_PACKET: usize = 8;
/// Bytes of served windows per fill datagram (one plaid window is ~500 B; a floor-rung window ~110 B).
const FILL_PACKET_BUDGET: usize = 1100;
/// Live re-request cadence: a wanted window is asked again this often until it lands or is nacked (an RTT and change on any real link).
const FILL_REQ_LIVE: std::time::Duration = std::time::Duration::from_millis(40);
/// Drain re-request cadence.
const FILL_REQ_DRAIN: std::time::Duration = std::time::Duration::from_millis(8);
/// Drain heartbeat: flags + our final window count go out at least this often so the peer's tail list closes.
const FILL_HEARTBEAT: std::time::Duration = std::time::Duration::from_millis(40);
/// The post-hangup drain deadline — a peer that vanished (the call died with the link) must not hold the recording open; what landed by now is the recording.
const DRAIN_MAX: std::time::Duration = std::time::Duration::from_millis(2500);
/// The wanted set's cap: a link losing more than this many windows is not one a drain can mend.
const FILL_WANTED_CAP: usize = 4096;
/// Fill-plane wire byte for "I do not have that window" (never a rung).
const FILL_NACK: u8 = 0xFF;
/// Sent-frame index cap (seq, slot, tier, spool position) — ~90 min of plaid, half a day at the floor rung; older windows can no longer be served.
const TX_INDEX_CAP: usize = 1 << 20;
/// Max encoded bytes per 5ms frame at each rung: hard CBR emits exactly rate/1600 bytes, +2 headroom — sized so every rung's WINDOW is a multiple of 8, which makes the RaptorQ symbol exactly the window (its alignment rounds max_packet_size down to a multiple of 8; an unaligned window would split into two padded symbols and re-grow the wire).
const TIER_MAX_ENC: [usize; 5] = [12, 22, 42, 82, 486];
/// Completed-window streak that earns one rung up (~0.25s at 10ms windows, ~1s at the floor's 40ms) — the fast first climb keeps the POTS-ish floor a blink on a clean link.
const CLIMB_CLEAN_WINDOWS: u32 = 25;
/// LADDER HYSTERESIS (field 2026-09-09, the Emma+Nick LAN call: 27 ups / 13 downs in 53s — a burst of paired losses dropped two rungs per lost window and 25 clean windows climbed back in a quarter second at the top rungs, so the rate flapped 16↔64 kbps every 300ms for five seconds). Three edges-not-timers rules on top of AIMD: a climb needs CLIMB_HOLD since the last change as well as the clean streak (so the streak means the same at every rung); a drop needs LOSSES_TO_DROP lost windows inside LOSS_WINDOW (one lost pair on an otherwise clean channel is not a congestion signal); and no climb for DROP_HOLD after a drop.
const CLIMB_HOLD: std::time::Duration = std::time::Duration::from_millis(1000);
const LOSS_WINDOW: std::time::Duration = std::time::Duration::from_millis(2000);
const LOSSES_TO_DROP: usize = 2;
const DROP_HOLD: std::time::Duration = std::time::Duration::from_millis(2000);
/// Rungs dropped on a lost window.
const DROP_RUNGS_ON_LOSS: usize = 1; // 2026-09-09: was 2 — on a 5ms-RTT LAN the losses are the radio's bursts, not congestion, and the top rung is the MORE burst-robust one (10ms per datagram); the drop is a nudge now, the diversity below is the fix

/// Slot per encoded frame at a rung: 2-byte length prefix + that rung's max payload.
const fn tier_slot(tier: usize) -> usize {
    2 + TIER_MAX_ENC[tier]
}

/// FEC window bytes at a rung.
const fn tier_window_bytes(tier: usize) -> usize {
    TIER_FRAMES[tier] * tier_slot(tier)
}

// INLINE PID LEVEL + DUCK (echo layer 2 + level management in ONE loop — Nick's call 2026-09-01: "auto ducking, level management we can do with a PID loop in line, basically another 1ms for the adjustment pass"; actual cost is microseconds per 20ms frame). Replaces the disarmed two-coefficient glide, whose peak-hold trigger smeared timing and clipped double-talk.
// The loop tracks ONE target gain per frame: AGC term (mic mean-|sample| toward TX_TARGET_LEVEL in the log domain — quiet talkers lift, shouters trim, error is symmetrical in dB) × duck term (far-end energy squashes the target continuously — no binary gate, so double-talk dips instead of chopping). PID output slews the applied gain; the integrator is clamped (anti-windup) so a long silence doesn't bank gain and blast the first syllable.
const PID_DUCK_ENABLED: bool = true;
/// Mic AGC setpoint: mean |sample| of a talking frame lands here (~-18 dBFS of i16 — comfortable headroom above the floor window's repair math, below clipping).
const TX_TARGET_LEVEL: f32 = 4000.0;
/// Far-end energy that squashes the duck term to half — continuous, not a threshold. Also the "far end is talking" line for the side-aware gate.
const DUCK_FAR_HALF: f32 = 400.0;
/// Hard floor so the SOFT duck (double-talk) never fully mutes the mic (the near talker stays audible under echo).
const DUCK_GAIN_FLOOR: f32 = 0.15;
/// SIDE-AWARE ECHO GATE (Nick's call 2026-09-01: "a block when the other is talking, PID reduction for the silence"). When the far end is talking and the near mic is quieter than a plausible ECHO of it — i.e. the near human is NOT talking, the mic is carrying only speaker bleed — hard-gate the mic toward silence instead of the 0.15 soft floor. Real near speech on a close mic runs comparable to or above the far render level, so `near < far × ratio` cleanly separates echo-only from double-talk. Below the ratio = pure echo = gate; above = double-talk = keep the soft floor so an interruption survives.
const ECHO_GATE_RATIO: f32 = 0.5;
/// The hard-gate target gain for echo-only frames — near-mute (~-34 dB). The existing slew (0.19/frame at 5ms) eases in/out over ~60ms so the gate never clicks, and the applied-gain clamp already reaches this low (GAIN_MIN×DUCK_GAIN_FLOOR ≈ 0.019); the old code just never asked for it.
const ECHO_GATE_GAIN: f32 = 0.02;
/// PID gains on the log2-domain level error, evaluated per 5ms frame (the 2026-09-08 flag day doubled the eval rate; KI halved and KD doubled to keep the same time-domain response — integral accumulates per eval, derivative reads a half-sized per-eval delta).
const PID_KP: f32 = 0.20;
const PID_KI: f32 = 0.01;
const PID_KD: f32 = 0.16;
const PID_I_CLAMP: f32 = 2.0;
/// Applied-gain bounds: ±3 octaves of AGC authority.
const GAIN_MIN: f32 = 0.125;
const GAIN_MAX: f32 = 8.0;
/// Live-playback output pad in stops — defined beside the ringback that shares it (see [`super::OUTPUT_PAD_STOPS`]).
use super::OUTPUT_PAD_STOPS;

pub struct EngineParams {
    pub secret: [u8; 32],
    pub we_are_caller: bool,
    pub peer_addr: SocketAddr,
    /// Recording spool (key, path) — recording by default; None only when the spool couldn't be minted (disk trouble; the call proceeds unrecorded, logged).
    pub spool: Option<([u8; 32], std::path::PathBuf)>,
    /// The stored calibration for the route/mic this call starts on — read on the UI thread (the engine can't touch settings). None = uncalibrated: reactive duck + PID until the in-call learner reaches Usable and arms the predictive path itself.
    pub cal: Option<CalSnapshot>,
    /// The peer sits on a LAN-class direct path — the ladder may climb past 128 kbps to the raw PCM plaid rung.
    pub plaid_allowed: bool,
}

/// The profile snapshot the predictive duck starts from (Cal 4). g is volume-normalized (the engine re-scales by live vol_lin); delay in 10ms bins.
#[derive(Debug, Clone, Copy)]
pub struct CalSnapshot {
    pub g_norm: f32,
    pub delay_bins: usize,
    /// Fixed mic gain from the voice profile — replaces the chasing PID when present.
    pub mic_gain: Option<f32>,
    /// Room floor from the voice profile — the gate's second leg until the live learner refines it.
    pub floor: f32,
}

/// Handle held by the UI's ActiveCall. Dropping it does NOT stop the engine — call `stop()` (teardown is an explicit edge).
pub struct EngineHandle {
    stop: Arc<AtomicBool>,
    pub muted: Arc<AtomicBool>,
    /// The engine thread — the keep transcode joins it first, because the post-hangup fill drain is still writing the spool after `stop()`.
    thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl EngineHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Hand the engine thread's join handle to whoever must wait for the spool to go quiet (end_call → the keep).
    pub fn take_thread(&self) -> Option<std::thread::JoinHandle<()>> {
        self.thread.lock().ok().and_then(|mut t| t.take())
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
        thread: std::sync::Mutex::new(None),
    };
    let (sink_tx, sink_rx) = std::sync::mpsc::channel::<(Vec<u8>, SocketAddr)>();
    let sink_gen = super::install_media_sink(sink_tx);
    crate::platform::audio::start();
    match std::thread::Builder::new()
        .name("call-engine".into())
        .spawn(move || {
            // Android: the engine thread runs the 5ms capture→send cadence; at default priority both phones in the 2026-09-09 LAN call produced 194 of 200 frames a second (the receiver underruns, the jitter target ratchets). URGENT_AUDIO's nice (-19) is what the platform grants an app's own audio threads.
            #[cfg(target_os = "android")]
            {
                let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -19) };
                crate::logf!("CALL: engine thread priority → -19 ({})", if rc == 0 { "ok" } else { "refused" });
            }
            run(params, stop, muted, sink_rx, sink_gen)
        }) {
        Ok(j) => *handle.thread.lock().unwrap() = Some(j),
        Err(_) => {
            crate::log("CALL: engine thread spawn failed");
            super::clear_media_sink_gen(sink_gen);
            crate::platform::audio::stop();
        }
    }
    handle
}

fn run(
    params: EngineParams,
    stop: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    sink_rx: std::sync::mpsc::Receiver<(Vec<u8>, SocketAddr)>,
    sink_gen: u64,
) {
    let (tx_dir, rx_dir) = if params.we_are_caller {
        (Direction::CallerToCallee, Direction::CalleeToCaller)
    } else {
        (Direction::CalleeToCaller, Direction::CallerToCallee)
    };
    let mut tx_chain = StepChain::new(&params.secret, tx_dir);
    let mut rx_chain = StepChain::new(&params.secret, rx_dir);

    // THE ARCHIVE ENCODER (the ordering fix, Nick 2026-09-12: "save the encoded stream… ideally before the duck(s)"): one high-end Opus encode of the CLEAN mic runs live beside the wire encoder — 10ms frames, VBR 160k, complexity 6 — and THAT is what the spool keeps. Plaid raw-PCM spooling dies (it ate ~5.5MB/min), the keep repackages these packets verbatim (no second lossy generation), and the ducking rides as a profile, not a second stream.
    let mut arch_enc = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).ok().map(|mut e| {
        let _ = e.set_vbr(true);
        let _ = e.set_bitrate(opus::Bitrate::Bits(160_000));
        let _ = e.set_complexity(6);
        e
    });
    let mut arch_buf: Vec<i16> = Vec::with_capacity(480);
    // The receive twin: received plaid frames re-encode at the high rung before spooling (5ms — the RX grid stays on its lattice).
    let mut rx_arch_enc = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).ok().map(|mut e| {
        let _ = e.set_vbr(true);
        let _ = e.set_bitrate(opus::Bitrate::Bits(160_000));
        let _ = e.set_complexity(6);
        e
    });
    let mut rx_arch_pkt = vec![0u8; 4000];
    let mut arch_meta: Option<(i64, u32, u8)> = None;
    let mut arch_pkt = vec![0u8; 4000];
    // Archive record index (window id, first slot, spool position) — the serve fallback for plaid windows, whose wire copies no longer spool (a plaid window is exactly one 10ms archive record).
    let mut arch_index: std::collections::VecDeque<(u32, u8, super::spool::RecordAt)> = std::collections::VecDeque::new();
    let mut arch_dec: Option<opus::Decoder> = None;
    let mut encoder = match opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::LowDelay) {
        Ok(mut e) => {
            let _ = e.set_vbr(false); // CBR — traffic-shape privacy (VBR leaks the speech envelope via packet sizes)
            let _ = e.set_bitrate(opus::Bitrate::Bits(TIER_RATES[0])); // every call starts at the ladder floor and climbs on evidence
            e
        }
        Err(e) => {
            crate::logf!("CALL: opus encoder init failed: {}", e);
            teardown(sink_gen);
            return;
        }
    };
    let mut decoder = match opus::Decoder::new(48_000, opus::Channels::Mono) {
        Ok(d) => d,
        Err(e) => {
            crate::logf!("CALL: opus decoder init failed: {}", e);
            teardown(sink_gen);
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
    // Repair symbols waiting to ship: window n's datagram carries the repair of window n−2 (2026-09-09, the Emma/Nick and Brittany/Nick LAN calls: losses came in consecutive PAIRS, and with the repair one datagram behind its source a two-datagram burst killed the window every time — 65 and 121 lost windows on a 5ms LAN). Two back, a two-datagram burst can never take both symbols of one window; a lost source now waits one extra window for its repair, and only when it was lost.
    let mut repair_queue: std::collections::VecDeque<(usize, Vec<u8>)> = std::collections::VecDeque::new();
    let mut window_buf: Vec<u8> = Vec::with_capacity(tier_window_bytes(TIER_RATES.len() - 1));
    let mut frames_in_window = 0usize;

    // Ladder state: `tier` is what the CURRENT window encodes at, `pending_tier` is where the evidence says to go — switches land only on window boundaries because a window's slot geometry is fixed the moment its first frame lands.
    let mut tier: usize = 0;
    let mut pending_tier: usize = 0;
    let mut clean_rx_windows: u32 = 0;
    // Ladder hysteresis state: when the tier last moved (either way), when it last DROPPED, and the recent loss edges inside LOSS_WINDOW.
    let mut last_tier_change = std::time::Instant::now();
    let mut last_tier_drop: Option<std::time::Instant> = None;
    let mut recent_losses: std::collections::VecDeque<std::time::Instant> = std::collections::VecDeque::new();
    let (mut tier_ups, mut tier_downs) = (0u32, 0u32);
    // PID state: applied gain + (integral, last-error) on the log2 level error; route is cached at engine start (Headset = no acoustic path = never duck; Unknown ducks, the safe default).
    let mut duck_gain: f32 = 1.0;
    let mut pid_i: f32 = 0.0;
    let mut pid_last_e: f32 = 0.0;
    let mut ducked_frames: u64 = 0;
    let mut gated_frames: u64 = 0;
    // THE HARD MUTE IS BOUNDED (field 2026-09-12: Brittany could not hear Nick for the last fifteen seconds — his mic sat under the predicted echo for the whole tail and the gate held it at 0.02, and the canceller that could have demoted it had disarmed). A run of consecutive Gate verdicts longer than this is double-talk, not echo: a person talking over the far end. Past the bound the verdict demotes to the soft duck until a frame comes in clearly under the prediction again.
    const GATE_RUN_MAX: u32 = 100; // 100 × 5 ms = half a second
    let mut gate_run: u32 = 0;
    // When the canceller disarms, the chirp's prediction it was meant to verify is unverified: gate REACTIVELY (the measured far level, not the predicted echo) for the rest of the wave.
    let mut prediction_trusted = true;
    let mut far_active_frames: u64 = 0;
    // `mut`: live route tracking below re-evaluates this on a mid-call swap (the old engine cached it once — a BT headset connecting mid-call kept ducking a route with no acoustic path).
    let mut route_ducks = !matches!(
        crate::platform::audio::route(),
        crate::platform::audio::AudioRoute::Headset
    );

    // In-call calibration learner: fed every raw envelope ABOVE the mute/uncalibrated skip on purpose — a muted user is GUARANTEED silent, the cleanest echo windows there are (envelope-only, the mic session is already open — no new privacy surface). Its estimates drive the PREDICTIVE duck below (Cal 4) and persist at teardown; the ritual snapshot in params seeds it.
    let start_route = crate::platform::audio::route_id();
    let mut learner = crate::call::learn::Learner::new(
        start_route.starts_with("bt:"),
        params.cal.as_ref().map(|c| c.delay_bins),
        params.cal.as_ref().map(|c| c.floor),
    );
    let mut renv_cursor = 0usize;
    // NLMS canceller (Nick 2026-09-08: "go for the NLMS" — subtract first, duck the residual). Armed ONLY by a chirp Coupled verdict (the profile IS the license); a route swap disarms (new physics, next call's chirp re-seeds). Alignment: mic and reference advance by pure sample count from one-time osc anchors — stamp wobble never smears the taps (it folds into the PRE-roll window once).
    let mut nlms: Option<crate::call::nlms::Nlms> = None;
    let mut nlms_seed_vol: f32 = 1.0;
    let mut ref_ring = crate::call::nlms::RefRing::new(96_000); // 2s of rendered reference
    // Start the reference cursor at NOW, not zero: on the caller the audio session carries over from the ringback ("engine has the session"), and the render reference ring already holds the ringback's audible frames — the chirp's render anchor then landed 3.7s early and the fit rejected its own seed at the grid bound (Nick 2026-09-10, no filter on his side of an otherwise clean wave).
    let mut ref_cursor = crate::platform::audio::render_ref_since(usize::MAX).1;
    let mut ref_anchor_osc: Option<i64> = None;
    let mut mic_abs: u64 = 0;
    let mut mic_anchor_osc: Option<i64> = None;
    // LEARNER CADENCE ADAPTER (flag day 2026-09-08): the learner's KAT-locked contract is ONE envelope per 10ms bin (its stamp regularizer advances a bin per push — two 5ms pushes would run its lattice at 2× time and re-anchor forever). The engine pairs adjacent 5ms envelopes: (osc of the first half, mean env) per 10ms.
    let mut far_pair: Option<(i64, f32)> = None;
    let mut mic_pair: Option<(i64, f32)> = None;
    // The APPLIED calibration the duck predicts from: (g_norm, delay_bins). Seeded by the stored profile; the live learner slews it (τ≈2s at the 1s update cadence) once Usable — so an uncalibrated route arms itself mid-call. Live floor rides the learner's minimum-statistics tracker.
    let mut applied: Option<(f32, usize)> = params.cal.as_ref().map(|c| (c.g_norm, c.delay_bins));
    let fixed_mic_gain: Option<f32> = params.cal.as_ref().and_then(|c| c.mic_gain);
    let mut live_floor: f32 = params.cal.as_ref().map_or(40.0, |c| c.floor);
    let mut pred_gate = crate::call::learn::PredGate::new();
    let mut clean_declared = false;
    let mut live_route = start_route.clone();
    let mut last_est = std::time::Instant::now();
    // Echo stats cadence: a line every ten seconds while a filter is armed (recent + lifetime ERLE, adapt ratio) — the field's view of the whitener at work.
    let mut last_echo_stats = std::time::Instant::now();
    let mut last_live_stats = std::time::Instant::now();
    let mut vol_lin_now: f32 = crate::platform::audio::current_volume_db()
        .map_or(1.0, |db| 10f32.powf(db / 20.0));

    // RX reassembly: per-window (tier, fountain decoder) + decoded-PCM stash, played strictly in window order (a hole is skipped, not synthesized — the dry playback queue renders the silence).
    let mut rx_decoders: std::collections::BTreeMap<u32, (usize, raptorq::Decoder)> =
        Default::default();
    // Highest authenticated seq seen — the media re-point's forward-progress gate (see the RX loop).
    let mut rx_max_seq: Option<u32> = None;
    let mut rx_done: std::collections::BTreeMap<u32, Vec<Vec<i16>>> = Default::default();
    let mut next_play: Option<u32> = None;

    let (mut pkts_out, mut pkts_in, mut windows_lost) = (0u64, 0u64, 0u64);
    // Plaid forensics: raw frames each way, and the holes the fade covered.
    let (mut raw_out, mut raw_in, mut holes_faded) = (0u64, 0u64, 0u64);
    // Loss-rate loop state: the 256-bit ring, its cursor, the integral, the last underrun count sampled, and the target we last set.
    let mut loss_bits = [0u64; LOSS_RING / 64];
    let mut loss_pos: u8 = 0;
    let mut loss_integ: f32 = 0.0;
    let mut last_underruns: usize = 0;
    let mut jitter_target: usize = TIER_FRAMES[0];
    // Link tail state: the peer's newest stamp + when it arrived (for the hold), and RTT statistics (min / EMA / max, sample count) over the call and over the last stats window.
    let mut peer_stamp: Option<(u32, std::time::Instant)> = None;
    let (mut rtt_min, mut rtt_max, mut rtt_ema, mut rtt_n) = (u32::MAX, 0u32, 0f32, 0u64);
    let (mut win_rtt_min, mut win_rtt_max, mut win_rtt_n) = (u32::MAX, 0u32, 0u64);
    let mut win_losses_at = 0u32;
    let mut last_played: Option<Vec<i16>> = None;
    // RX drop-reason tally — see the RX loop for why each is counted apart (addressing vs secret-desync diagnosis). Shape = opened fine but the payload geometry is wrong (truncation bug or a mixed-version peer).
    let (mut rx_seen, mut rx_drop_parse, mut rx_drop_shape, mut rx_drop_open) = (0u64, 0u64, 0u64, 0u64);
    // Recording-fill plane (see the FILL consts): its own chains + seq, the sent-frame index the peer's requests are served from, the arrived-window bits, the wanted set, the peer's requests we owe, and the drain state.
    let fill_root = super::keys::fill_secret(&params.secret);
    let mut tx_fill = StepChain::new(&fill_root, tx_dir);
    let mut rx_fill = StepChain::new(&fill_root, rx_dir);
    let mut fill_seq: u32 = 0;
    let mut tx_index: std::collections::VecDeque<SentFrame> = std::collections::VecDeque::new();
    let mut spool_reader: Option<super::spool::SpoolReader> = None;
    let mut rx_have: Vec<u64> = Vec::new();
    let mut wanted: std::collections::BTreeSet<u32> = Default::default();
    let mut wanted_cursor: u32 = 0;
    let mut serve_queue: std::collections::BTreeSet<u32> = Default::default();
    let mut peer_fills = false;
    let mut peer_windows: Option<u32> = None;
    let (mut peer_draining, mut peer_satisfied) = (false, false);
    let mut draining: Option<std::time::Instant> = None;
    let mut fill_hello_due = true;
    let mut last_req_tx = std::time::Instant::now() - FILL_REQ_LIVE;
    let mut last_fill_tx = std::time::Instant::now();
    let (mut fills_asked, mut fills_got_live, mut fills_got_drain, mut fills_nacked, mut fills_served, mut fills_served_drain) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    // Audio ENERGY readout — mean |sample| of what we CAPTURED (tx) and what we DECODED for playback (rx). A silent direction shows as ~0 here: near-zero tx = our mic content is dead (route/gain/AEC over-duck, NOT a permission miss — that path never reaches capture); non-zero rx that the user still didn't hear = a playback/route problem downstream. Separates "one side heard" into capture-silent vs playback-silent without guessing (field 2026-08-19).
    let (mut tx_energy, mut tx_frames, mut rx_energy, mut rx_frames) = (0u64, 0u64, 0u64, 0u64);
    // Capture cadence forensics (2026-09-09: both phones, both calls, 191-194 of 200 frames a second, priority made no difference): the HAL-stamped span of captured frames against the count splits "the input delivers short" from "frames go missing on the way".
    let (mut cap_first_osc, mut cap_last_osc): (Option<i64>, i64) = (None, 0);

    crate::logf!(
        "CALL: engine up — tx {} → {}, ladder {}..{} kbps (start {}{}), floor window {} frames, repair {}, duck {}, route \"{}\" vol {}",
        if params.we_are_caller { "c>e" } else { "e>c" },
        peer,
        TIER_RATES[0] / 1000,
        TIER_RATES[TIER_RATES.len() - 1] / 1000,
        if params.plaid_allowed { ", plaid armed" } else { ", plaid off — not a LAN path" },
        TIER_RATES[0] / 1000,
        TIER_FRAMES[0],
        REPAIR_PACKETS,
        if !PID_DUCK_ENABLED {
            "disarmed (raw echo run)"
        } else if route_ducks {
            "PID (level+duck inline)"
        } else {
            "PID (level only — headset)"
        },
        // Calibration substrate readout: WHICH output path + volume this call runs on — the profile key the calibrated duck will look up, loggable now so field logs start naming routes before the calibration lands.
        crate::platform::audio::route_id(),
        crate::platform::audio::current_volume_db()
            .map(|db| format!("{db:.1}dB"))
            .unwrap_or_else(|| "?".into())
    );

    // V-CHIRP CONNECT PROBE (Nick 2026-09-07, docs/audio-paths.md §5): the call's first sound is the probe — queued before the first loop pass, played unpadded on the live route while BOTH directions hold (no mic TX, RX decoded but not rendered), so the chirp is the only thing in the room and the only render in RENDER_ENV. Both sides run the same window off their own connect edge, so the holds overlap and nobody's voice is lost. When the capture closes, audio connects immediately and the fit seeds the duck a beat later; the deadline covers a mic that never grants (Android prompts at the call).
    let start_instant = std::time::Instant::now();
    let mut probing = true;
    let mut probe_cap: Vec<i16> = Vec::with_capacity(crate::call::vchirp::CAPTURE_SAMPLES);
    let mut probe_render_osc: Option<i64> = None;
    let mut probe_anchor_osc: Option<i64> = None;
    let probe_deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    let _ = crate::call::vchirp::take_verdict(); // a prior call's late fit must never seed this one
    // Drought baseline + stale-state drain: the UI measures receive drought against max(start, last rx), and a previous call's redirect must never re-point this one.
    super::MEDIA_START_OSC.store(vsf::eagle_time_oscillations(), Ordering::Relaxed);
    super::LAST_MEDIA_RX_OSC.store(0, Ordering::Relaxed);
    let _ = super::take_peer_redirect();
    // LOCAL SOURCE while the probe owns the queue, and the chirp FEEDS PACED from the loop (never dumped): every bounded stage ahead of the DAC (queue drop-oldest, ceiling trims) beheads a bulk dump — both field beheadings. Top-up keeps ≤200ms queued; the 1ms loop never starves the drain.
    crate::platform::audio::set_local_source(true);
    let mut chirp_frames = crate::call::vchirp::frames();
    let mut chirp_idx = 0usize;
    // LOW-VOLUME PROBE BOOST (field 2026-09-08, Nick at 1/3 media = −43dB device curve: the chirp emitted at whisper level, psr scraping the gate): below −20dB the digital level rises toward full scale (+9dB of SNR headroom above the −9dBFS default). The fit correlates against the UNSCALED template, so g/taps inflate by the boost — finish() divides it back out.
    let chirp_scale: f32 = {
        let vol_db = crate::platform::audio::current_volume_db().unwrap_or(0.0);
        if vol_db <= -20.0 { 32_760.0 / 11_585.0 } else { 1.0 }
    };
    if chirp_scale > 1.0 {
        for f in &mut chirp_frames {
            for s in f.iter_mut() {
                *s = (*s as f32 * chirp_scale).clamp(-32768.0, 32767.0) as i16;
            }
        }
        crate::logf!(
            "CALL: v-chirp digital boost x{} (low media volume)",
            format!("{chirp_scale:.2}")
        );
    }

    loop {
        // STOP → DRAIN (recording fills): audio is over, but a fill-capable peer can still hand us the windows we lost — and wants ours. Both engines stay up on the fill plane until both are satisfied or the deadline passes. A peer that never spoke the fill plane ends the engine at once, exactly as before.
        if stop.load(Ordering::Relaxed) && draining.is_none() {
            if peer_fills && spool.is_some() {
                draining = Some(std::time::Instant::now());
                crate::logf!("CALL: draining — {} window(s) wanted so far, peer at {} window(s)", wanted.len(), peer_windows.map_or("?".to_string(), |w| w.to_string()));
            } else {
                break;
            }
        }
        if let Some(t0) = draining {
            // The tail joins the wanted set from the peer's FRESH count — the live count is up to ten seconds stale (the hello cadence), so nothing is 'satisfied' until the peer has been heard draining (field 2026-09-10: the hung-up-on side exited at 0 ms before asking for its tail, and the hanging-up side then waited the whole deadline for a heartbeat that never came).
            if let (Some(pw), Some(np), true) = (peer_windows, next_play, peer_draining) {
                let mut s = np;
                while s < pw && wanted.len() < FILL_WANTED_CAP {
                    if !have_get(&rx_have, s) && !rx_done.contains_key(&s) {
                        wanted.insert(s);
                    }
                    s = s.wrapping_add(1);
                }
            }
            let satisfied = peer_draining && wanted.is_empty();
            if (satisfied && peer_satisfied && serve_queue.is_empty()) || t0.elapsed() >= DRAIN_MAX {
                // Last words, twice: our satisfied heartbeat is what lets the peer's own drain close without waiting on its deadline.
                for _ in 0..2 {
                    let msg = FillMsg { draining: true, satisfied: wanted.is_empty(), windows: window_id, reqs: Vec::new(), fills: Vec::new() };
                    tx_fill.advance_to(StepChain::step_for_seq(fill_seq));
                    if let Some(wire) = packet::seal_fill(&tx_fill, fill_seq, &fill_encode(&msg)) {
                        let _ = super::send_media(wire, peer);
                    }
                    fill_seq = fill_seq.wrapping_add(1);
                }
                break;
            }
        }
        // Paced chirp feed (probe phase): top the queue up to 40 frames (200ms) per pass.
        if probing {
            while chirp_idx < chirp_frames.len() && crate::platform::audio::playback_depth() < 40 {
                crate::platform::audio::queue_playback(chirp_frames[chirp_idx].clone());
                chirp_idx += 1;
            }
        }
        // Learner far feed: drain the render-envelope tap (post-jitter post-splice, osc-stamped at DAC-enqueue).
        {
            let (entries, cur) = crate::platform::audio::render_env_since(renv_cursor);
            renv_cursor = cur;
            for (osc, env) in entries {
                // Probe render anchor: RX is held while probing, so the first audible render IS the chirp's first frame at DAC-enqueue.
                if probing && probe_render_osc.is_none() && env > 100.0 {
                    probe_render_osc = Some(osc);
                }
                match far_pair.take() {
                    None => far_pair = Some((osc, env)),
                    Some((o, e)) => learner.push_far(o, (e + env) * 0.5),
                }
            }
        }
        // NLMS reference feed: every frame the DAC pulled, flattened into the absolute-indexed ring.
        {
            let (frames, cur) = crate::platform::audio::render_ref_since(ref_cursor);
            ref_cursor = cur;
            for (osc, f) in frames {
                if ref_anchor_osc.is_none() {
                    ref_anchor_osc = Some(osc);
                }
                ref_ring.push(&f);
            }
        }
        // ---- TX: mic → opus → window → fountain → sealed packets ----
        // Each captured frame carries the eagle time its first sample left the ADC (the HAL's clock on Android, the capture callback on desktop) — every mic stamp below reads THAT, never the drain moment.
        for (cap_osc, frame) in crate::platform::audio::captured_frames() {
            if draining.is_some() {
                continue; // audio is over — the mic is closed, anything left in the queue is not part of the wave
            }
            cap_first_osc.get_or_insert(cap_osc);
            cap_last_osc = cap_osc;
            // Learner mic feed FIRST — raw pre-gain pre-duck envelope (the separation invariant), stamped at drain, unconditionally (muted frames are the cleanest echo windows). Paired to the learner's 10ms cadence (see far_pair).
            {
                let e = crate::call::calibrate::env(&frame);
                match mic_pair.take() {
                    None => mic_pair = Some((cap_osc, e)),
                    Some((o, e0)) => learner.push_mic(o, (e0 + e) * 0.5),
                }
            }
            // Probe capture: raw samples into the fit buffer, and NO voice TX until the window closes. The anchor marks the first frame's start (drain stamp minus one frame); later frames extend the lattice by index — drain wobble is ±ms against a 10ms-bin consumer.
            if probing {
                if probe_anchor_osc.is_none() {
                    probe_anchor_osc = Some(cap_osc);
                }
                probe_cap.extend_from_slice(&frame);
                continue;
            }
            if frame.len() != FRAME_SAMPLES {
                continue;
            }
            // MUTE TRANSMITS ZEROS, NOT ABSENCE (2026-09-08, the drought tick's contract): the CBR cadence never breaks — a muted stretch is invisible to a traffic observer, NAT pinholes stay held open, and the peer's receive-drought measurement can't mistake a long mute for a dead path. Zeroed BEFORE the energy tally so tx(mic) honestly reads what was transmitted.
            let mut frame = frame;
            if muted.load(Ordering::Relaxed) {
                frame.fill(0);
            }
            // THE MIC AS CAPTURED (Nick 2026-09-12, the ordering fix): the clean pre-canceller pre-duck frame feeds the ARCHIVE encoder below; the verdict the live path applies rides beside it as the ducking profile.
            let pre_frame: Vec<i16> = frame.clone();
            let mut proc_verdict: u8 = 0;
            // NLMS SUBTRACT — before the tally and the duck, so both see the residual (the duck is the RESIDUAL suppressor once a filter is armed). Mic timeline: pure frame count from a one-time anchor.
            if mic_anchor_osc.is_none() {
                mic_anchor_osc = Some(cap_osc);
            }
            let this_mic_abs = mic_abs;
            mic_abs += frame.len() as u64;
            if let Some(c) = nlms.as_mut() {
                if let (Some(m0), Some(r0)) = (mic_anchor_osc, ref_anchor_osc) {
                    // One-time anchor offset maps the mic count onto the reference timeline; the chirp's ir_start carries the same osc-anchored convention, and the PRE-roll absorbs the wobble.
                    let off_samples = (m0 - r0) * crate::call::vchirp::SAMPLE_RATE as i64
                        / vsf::OSCILLATIONS_PER_SECOND as i64;
                    let pos = this_mic_abs as i64 + off_samples;
                    // Adapt only in far-talks-alone (the side-aware law): far active AND the raw mic no louder than a plausible echo. Double-talk freezes the taps. "Plausible echo" is the PREDICTION when a calibration is applied (2026-09-09: the raw-far ratio never opened on a phone whose echo path gain sits near 7 — the echo itself failed the ratio, the filter adapted for a second or two per call and died) — the same enter line the gate uses; the raw-far ratio stays as the uncalibrated fallback.
                    let raw_mean = frame.iter().map(|s| s.unsigned_abs() as u32).sum::<u32>() as f32
                        / frame.len().max(1) as f32;
                    let far_now = crate::platform::audio::far_level() as f32;
                    let adapt = route_ducks
                        && match applied {
                            Some((g_norm, delay)) => {
                                let far_del = learner.far_env_at(delay);
                                let pred = g_norm * vol_lin_now * far_del;
                                far_del > crate::call::learn::FAR_ACT && raw_mean < (pred * 2.0).max(live_floor * 2.0)
                            }
                            None => far_now > DUCK_FAR_HALF && raw_mean < far_now * ECHO_GATE_RATIO,
                        };
                    let ref_gain = (vol_lin_now / nlms_seed_vol.max(1e-6)).clamp(0.05, 20.0);
                    c.cancel_frame(&mut frame, &ref_ring, pos, adapt, ref_gain);
                }
            }
            // Rung switches land only between windows — a window's slot geometry is fixed at its first frame.
            if frames_in_window == 0 && pending_tier != tier {
                tier = pending_tier;
                super::LAST_LINK_TIER.store(tier as u32, Ordering::Relaxed);
                if tier != RAW_TIER {
                    let _ = encoder.set_bitrate(opus::Bitrate::Bits(TIER_RATES[tier]));
                }
            }
            // Mic health level BEFORE the duck, so a heavy duck never reads as a dead mic in the tally.
            tx_energy += frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>();
            tx_frames += 1;
            // PID level+duck (see the PID_DUCK_ENABLED consts): one inline loop regulates mic level toward the setpoint and squashes it under far-end energy.
            let mut frame = frame;
            let far = crate::platform::audio::far_level() as f32;
            if route_ducks && far > DUCK_FAR_HALF {
                far_active_frames += 1;
            }
            if PID_DUCK_ENABLED {
                // Frame mean |sample| — the level the AGC regulates. Silence (below a hair above the noise floor) freezes the loop: regulating silence toward TX_TARGET_LEVEL is how AGCs learn to amplify room hiss.
                let mean = frame.iter().map(|s| s.unsigned_abs() as u32).sum::<u32>() as f32
                    / frame.len().max(1) as f32;
                // Duck term, two modes:
                //   PREDICTIVE (Cal 4, an applied calibration exists): expected echo = g_norm × vol_lin × far_env(t−delay) from the learner's delay-aligned bins — the gate compares the mic against what the SPEAKER PHYSICALLY EMITTED one acoustic round-trip ago, not the peak-hold of what's rendering now; hysteresis kills threshold chatter.
                //   REACTIVE (uncalibrated fallback): the peak-hold far_level + absolute ratio gate, exactly the pre-calibration behavior — until the live learner reaches Usable and arms the predictive path itself.
                let duck_term = if let (Some((g_norm, delay)), true) = (applied, prediction_trusted) {
                    let far_del = learner.far_env_at(delay);
                    let far_talking = route_ducks && far_del > crate::call::learn::FAR_ACT;
                    let pred = g_norm * vol_lin_now * far_del;
                    match pred_gate.decide(mean, pred, live_floor, far_talking) {
                        crate::call::learn::GateVerdict::Full => {
                            gate_run = 0;
                            1.0
                        }
                        crate::call::learn::GateVerdict::Gate if gate_run >= GATE_RUN_MAX => {
                            // Bounded: half a second of "echo only" is a person talking — soft duck, never the mute.
                            proc_verdict = 1;
                            (1.0 / (1.0 + far_del / DUCK_FAR_HALF)).max(DUCK_GAIN_FLOOR)
                        }
                        crate::call::learn::GateVerdict::Gate => {
                            gated_frames += 1;
                            gate_run += 1;
                            proc_verdict = 2;
                            if nlms.as_ref().is_some_and(|c| c.erle_recent_db().is_some_and(|e| e >= 6.0)) {
                                // A filter that has PROVEN itself (≥6dB recent ERLE) has already subtracted the echo — the hard near-mute demotes to the soft residual duck, and double-talk survives (full duplex). An armed-but-weak filter keeps the gate (2026-09-09: a 2dB filter demoting the gate to a −16dB duck was the field's "echoey").
                                (1.0 / (1.0 + far_del / DUCK_FAR_HALF)).max(DUCK_GAIN_FLOOR)
                            } else {
                                ECHO_GATE_GAIN
                            }
                        }
                        crate::call::learn::GateVerdict::Duck => {
                            gate_run = 0;
                            proc_verdict = 1;
                            (1.0 / (1.0 + far_del / DUCK_FAR_HALF)).max(DUCK_GAIN_FLOOR)
                        }
                    }
                } else {
                    let far_talking = route_ducks && far > DUCK_FAR_HALF;
                    let echo_only = far_talking && mean < far * ECHO_GATE_RATIO;
                    if !route_ducks || !far_talking {
                        gate_run = 0;
                        1.0
                    } else if echo_only && gate_run >= GATE_RUN_MAX {
                        proc_verdict = 1;
                        (1.0 / (1.0 + far / DUCK_FAR_HALF)).max(DUCK_GAIN_FLOOR)
                    } else if echo_only {
                        gated_frames += 1;
                        gate_run += 1;
                        proc_verdict = 2;
                        ECHO_GATE_GAIN
                    } else {
                        proc_verdict = 1;
                        (1.0 / (1.0 + far / DUCK_FAR_HALF)).max(DUCK_GAIN_FLOOR)
                    }
                };
                if let Some(fg) = fixed_mic_gain {
                    // Cal 4: the voice profile's FIXED gain replaces the chasing PID — the AGC fighting the duck was the field's "tx(mic) 279 against a 4000 target". The duck term still scales it; the same slew keeps onsets pump-free.
                    let target = (fg * duck_term).clamp(GAIN_MIN * DUCK_GAIN_FLOOR, GAIN_MAX);
                    duck_gain += (target - duck_gain) * 0.19;
                } else if mean > 40.0 {
                    // Log2-domain level error toward the setpoint, PID'd, then the duck term scales the RESULT — level management and echo duck in the one inline loop (+~µs per 20ms frame).
                    let e = (TX_TARGET_LEVEL / (mean / duck_gain).max(1.0)).log2().clamp(-4.0, 4.0);
                    pid_i = (pid_i + e * PID_KI).clamp(-PID_I_CLAMP, PID_I_CLAMP);
                    let d = e - pid_last_e;
                    pid_last_e = e;
                    let target = (PID_KP * e + pid_i + PID_KD * d).exp2().clamp(GAIN_MIN, GAIN_MAX)
                        * duck_term;
                    // Slew toward the PID target (one step per 5ms frame keeps onsets pump-free; 0.19 at 5ms = the old 0.35 at 10ms).
                    duck_gain += (target - duck_gain) * 0.19;
                } else {
                    // Silence: hold gain, bleed the derivative memory, still honour the duck squash.
                    pid_last_e = 0.0;
                    duck_gain += (duck_gain.min(duck_gain * duck_term) - duck_gain) * 0.19;
                }
                duck_gain = duck_gain.clamp(GAIN_MIN * DUCK_GAIN_FLOOR, GAIN_MAX);
                if (duck_gain - 1.0).abs() > 0.005 {
                    if duck_gain < 1.0 {
                        ducked_frames += 1;
                    }
                    for s in frame.iter_mut() {
                        *s = (*s as f32 * duck_gain).clamp(-32768.0, 32767.0) as i16;
                    }
                }
            }
            let (enc, n) = if tier == RAW_TIER {
                // Plaid: the frame IS the payload — little-endian i16, no codec in the path.
                let mut raw = Vec::with_capacity(RAW_FRAME_BYTES);
                for s in &frame {
                    raw.extend_from_slice(&s.to_le_bytes());
                }
                raw_out += 1;
                (raw, RAW_FRAME_BYTES)
            } else {
                let mut enc = vec![0u8; TIER_MAX_ENC[tier]];
                match encoder.encode(&frame, &mut enc) {
                    Ok(n) => (enc, n),
                    Err(e) => {
                        crate::logf!("CALL: opus encode error: {}", e);
                        continue;
                    }
                }
            };
            if let Some(w) = spool.as_mut() {
                let osc = vsf::eagle_time_oscillations();
                let gain_q8 = (duck_gain * 256.0).round().clamp(0.0, 65535.0) as u16;
                // The archive stream: clean mic pairs encode once at the high rung, the ducking profile rides the record's proc fields (10ms resolution — the gain slews far slower than that).
                if arch_buf.is_empty() {
                    arch_meta = Some((osc, window_id, frames_in_window as u8));
                }
                arch_buf.extend_from_slice(&pre_frame);
                if arch_buf.len() >= 480 {
                    if let (Some(enc2), Some((osc0, wid0, slot0))) = (arch_enc.as_mut(), arch_meta.take()) {
                        if let Ok(an) = enc2.encode(&arch_buf[..480], &mut arch_pkt) {
                            if let Some(at) = w.append_seq_proc(super::spool::ARCH_CHAN | super::spool::PROC_FLAG, osc0, Some((wid0, slot0)), Some((gain_q8, proc_verdict)), &arch_pkt[..an]) {
                                if arch_index.len() >= TX_INDEX_CAP {
                                    arch_index.pop_front();
                                }
                                arch_index.push_back((wid0, slot0, at));
                            }
                        }
                    }
                    arch_buf.clear();
                }
                // The wire copy spools ONLY when compressed (bit-exact fill service, small); a plaid window reconstructs from its archive record at serve time.
                if tier != RAW_TIER {
                    if let Some(at) = w.append_seq(0, osc, Some((window_id, frames_in_window as u8)), &enc[..n]) {
                        if tx_index.len() >= TX_INDEX_CAP {
                            tx_index.pop_front();
                        }
                        tx_index.push_back(SentFrame { seq: window_id, slot: frames_in_window as u8, tier: tier as u8, at });
                    }
                }
            }
            window_buf.extend_from_slice(&(n as u16).to_le_bytes());
            window_buf.extend_from_slice(&enc[..n]);
            window_buf.resize((frames_in_window + 1) * tier_slot(tier), 0);
            frames_in_window += 1;

            if frames_in_window == TIER_FRAMES[tier] {
                // PIGGYBACK BUNDLE (flag day, 2026-08-20): ONE datagram per window — sealed payload [ctrl:1][source(N)][repair(N−1)]. Halves the packet rate (per-packet header cost was 19% of the floor wire), and the window's two copies now ride datagrams one window APART, so a burst must kill two consecutive datagrams to lose audio — strictly better than the old back-to-back pair. Steady-state latency unchanged: the source still ships the instant the window closes; only loss RECOVERY waits one extra window. seq = window id (the nonce, the step index, everything); ctrl = tier_src:3 | rep_present:1<<3 | tier_rep:3<<4 — THREE-bit tier fields so the ladder can grow to 8 rungs (survival rung below, stereo rung above) WITHOUT another flag day; a rung switch between windows makes the two symbols different sizes, which is why the tiers ride explicitly at all.
                // Plaid ships the window bare (no fountain, no repair — a lost datagram is a skipped 5 ms); every other rung is one source symbol + the repair of two windows back.
                let (source, own_repair): (Vec<u8>, Vec<u8>) = if tier == RAW_TIER {
                    (window_buf.clone(), Vec::new())
                } else {
                    let fec = raptorq::Encoder::new(&window_buf, oti(tier));
                    let pkts = fec.get_encoded_packets(REPAIR_PACKETS);
                    (pkts[0].data().to_vec(), pkts[1].data().to_vec())
                };
                // An empty entry is a plaid window's placeholder: it keeps the two-back spacing honest and never flags a repair.
                let rep = if repair_queue.len() >= 2 { repair_queue.pop_front() } else { None };
                let rep = rep.filter(|(_, r)| !r.is_empty());
                let mut payload = Vec::with_capacity(1 + tier_window_bytes(tier) * 2);
                let ctrl = tier as u8
                    | rep
                        .as_ref()
                        .map_or(0, |(rt, _)| 0b1000 | ((*rt as u8) << 4));
                payload.push(ctrl);
                payload.extend_from_slice(&source);
                if let Some((_, r)) = &rep {
                    payload.extend_from_slice(r);
                }
                // LINK TAIL: our stamp, the peer's newest stamp echoed, and how long we held it — the receiver subtracts the hold from its own round trip.
                {
                    let now_ms = start_instant.elapsed().as_millis() as u32;
                    let (echo, hold) = match peer_stamp {
                        Some((st, at)) => (st, at.elapsed().as_millis().min(u16::MAX as u128) as u16),
                        None => (0, 0),
                    };
                    payload.extend_from_slice(&now_ms.to_le_bytes());
                    payload.extend_from_slice(&echo.to_le_bytes());
                    payload.extend_from_slice(&hold.to_le_bytes());
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
                repair_queue.push_back((tier, own_repair));
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
            // FILL datagram: its own chain; carries the peer's window count + flags, their requests (we owe those windows), and served windows (ours to record).
            if header.fill {
                let Some(payload) = packet::open(&mut rx_fill, &header, sealed) else {
                    rx_drop_open += 1;
                    continue;
                };
                let Some(msg) = fill_decode(&payload) else {
                    rx_drop_shape += 1;
                    continue;
                };
                peer_fills = true;
                peer_windows = Some(msg.windows);
                peer_draining = msg.draining;
                peer_satisfied = msg.satisfied;
                for r in msg.reqs {
                    if serve_queue.len() < FILL_WANTED_CAP {
                        serve_queue.insert(r);
                    }
                }
                for (seq, tier_b, bytes) in msg.fills {
                    if tier_b == FILL_NACK {
                        if wanted.remove(&seq) {
                            fills_nacked += 1;
                        }
                        continue;
                    }
                    let t = tier_b as usize;
                    if t >= TIER_RATES.len() || bytes.len() != tier_window_bytes(t) || have_get(&rx_have, seq) {
                        continue;
                    }
                    if let Some(w) = spool.as_mut() {
                        let osc = vsf::eagle_time_oscillations();
                        for slot in 0..TIER_FRAMES[t] {
                            let base = slot * tier_slot(t);
                            let n = u16::from_le_bytes(bytes[base..base + 2].try_into().unwrap()) as usize;
                            if n == 0 || n > TIER_MAX_ENC[t] || (t == RAW_TIER && n != RAW_FRAME_BYTES) {
                                continue;
                            }
                            let chan = 1 | super::spool::FILL_FLAG | if t == RAW_TIER { super::spool::RAW_FLAG } else { 0 };
                            w.append_seq(chan, osc, Some((seq, slot as u8)), &bytes[base + 2..base + 2 + n]);
                        }
                    }
                    have_set(&mut rx_have, seq);
                    wanted.remove(&seq);
                    if draining.is_some() {
                        fills_got_drain += 1;
                    } else {
                        fills_got_live += 1;
                    }
                }
                continue;
            }
            // No call-id or direction check — both live in the key now: the AEAD below is the whole gate (a stale call's straggler or a cross-direction packet just fails to open).
            let Some(payload) = packet::open(&mut rx_chain, &header, sealed) else {
                rx_drop_open += 1;
                continue; // wrong key/step/tamper — silence, never a guess
            };
            pkts_in += 1;
            super::LAST_MEDIA_RX_OSC.store(vsf::eagle_time_oscillations(), Ordering::Relaxed);
            // Authenticated source: the peer's address follows its packets (NAT rebind / future handoff, no signaling needed).
            // FORWARD-PROGRESS GATE (2026-09-07, the address-trust doctrine): the step ratchet already kills cross-step replays, but a CURRENT-step packet replayed from an attacker's address would open fine and re-point our TX — so only a strictly-newer seq may steer. An off-path attacker never has a newer authentic packet; an on-path one could already drop the stream, gaining nothing.
            let newer = rx_max_seq.map_or(true, |m| header.seq > m);
            if newer {
                rx_max_seq = Some(header.seq);
                if src != peer && src != crate::network::status::RELAY_ADDR {
                    crate::logf!("CALL: peer media now from {} (was {})", src, peer);
                    peer = src;
                    super::set_call_tx_addr(Some(peer));
                }
            }
            // Bundle payload: [ctrl:1][source(seq)][repair(seq−1) if flagged]. seq IS the window id; the ctrl byte names both rungs (a rung switch between windows makes the two symbols different sizes, so length alone is ambiguous). The EXACT-length check is LOAD-BEARING: raptorq panics on mis-sized symbols, so nothing unchecked may reach a decoder.
            if payload.is_empty() {
                rx_drop_shape += 1;
                continue;
            }
            let ctrl = payload[0];
            let tier_src = (ctrl & 0b111) as usize;
            let rep_present = ctrl & 0b1000 != 0;
            let tier_rep = ((ctrl >> 4) & 0b111) as usize;
            // Bounds-check BEFORE any geometry lookup: a rung this build doesn't know (a newer peer's future ladder entry) is a shape-drop, never an index panic — which also makes ADDING rungs a graceful degrade instead of a flag day.
            if tier_src >= TIER_RATES.len() || (rep_present && (tier_rep >= TIER_RATES.len() || tier_rep == RAW_TIER)) {
                rx_drop_shape += 1;
                continue;
            }
            let src_len = tier_window_bytes(tier_src);
            let expected = 1 + src_len + if rep_present { tier_window_bytes(tier_rep) } else { 0 };
            if payload.len() != expected && payload.len() != expected + LINK_TAIL {
                rx_drop_shape += 1;
                continue;
            }
            // Link tail (a pre-tail peer sends none — every shape still parses): remember their stamp for our echo, and turn their echo of ours into an RTT sample.
            if payload.len() == expected + LINK_TAIL {
                let t = &payload[expected..];
                let stamp = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
                let echo = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
                let hold = u16::from_le_bytes([t[8], t[9]]) as u32;
                peer_stamp = Some((stamp, std::time::Instant::now()));
                if echo != 0 {
                    let now_ms = start_instant.elapsed().as_millis() as u32;
                    let rtt = now_ms.wrapping_sub(echo).wrapping_sub(hold);
                    if rtt < 10_000 {
                        rtt_min = rtt_min.min(rtt);
                        rtt_max = rtt_max.max(rtt);
                        rtt_ema = if rtt_n == 0 { rtt as f32 } else { rtt_ema + (rtt as f32 - rtt_ema) * 0.05 };
                        rtt_n += 1;
                        win_rtt_min = win_rtt_min.min(rtt);
                        win_rtt_max = win_rtt_max.max(rtt);
                        win_rtt_n += 1;
                    }
                }
            }
            let body = &payload[1..expected];
            // Feed the source symbol to window seq, and the piggybacked repair to window seq−2 (two back for burst diversity — see repair_queue) — same per-symbol pipeline for both (dedup → fountain → slot walk → Opus → climb evidence).
            let mut inputs: [(u32, usize, u32, &[u8]); 2] =
                [(header.seq, tier_src, 0, &body[..src_len]), (0, 0, 1, &[])];
            let n_inputs = if rep_present && header.seq > 1 {
                inputs[1] = (header.seq - 2, tier_rep, 1, &body[src_len..]);
                2
            } else {
                1
            };
            for &(wid, wtier, esi, sym) in inputs.iter().take(n_inputs) {
                let np = *next_play.get_or_insert(wid);
                if wid < np || rx_done.contains_key(&wid) {
                    continue; // already played or already decoded
                }
                // Plaid windows arrive whole — the symbol IS the window, no fountain state to keep.
                let decoded: Option<(usize, Vec<u8>)> = if wtier == RAW_TIER {
                    if esi != 0 {
                        continue;
                    }
                    Some((RAW_TIER, sym.to_vec()))
                } else {
                    let ep = raptorq::EncodingPacket::new(raptorq::PayloadId::new(0, esi), sym.to_vec());
                    let entry = rx_decoders
                        .entry(wid)
                        .or_insert_with(|| (wtier, raptorq::Decoder::new(oti(wtier))));
                    let dtier = entry.0;
                    // A same-window symbol at a DIFFERENT rung can't happen from a healthy sender (rung switches land on window boundaries) — feeding it would panic the decoder, so it's a shape drop.
                    if dtier != wtier {
                        rx_drop_shape += 1;
                        continue;
                    }
                    entry.1.decode(ep).map(|d| (dtier, d))
                };
                if let Some((dtier, data)) = decoded {
                    rx_decoders.remove(&wid);
                    let mut frames = Vec::with_capacity(TIER_FRAMES[dtier]);
                    for slot in 0..TIER_FRAMES[dtier] {
                        let base = slot * tier_slot(dtier);
                        let n = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
                        if n == 0 || n > TIER_MAX_ENC[dtier] {
                            continue;
                        }
                        let body = &data[base + 2..base + 2 + n];
                        if dtier == RAW_TIER {
                            if n != RAW_FRAME_BYTES {
                                continue;
                            }
                            let pcm: Vec<i16> = body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
                            // Received plaid compresses BEFORE it rests (the symmetric half of the ordering fix): high-end 5ms Opus into the spool instead of raw PCM — plaid now exists on the wire alone, and at N participants the receive side was N−1 plaid streams of disk.
                            if let Some(w) = spool.as_mut() {
                                let done = rx_arch_enc.as_mut().and_then(|e| e.encode(&pcm, &mut rx_arch_pkt).ok()).map(|an| {
                                    w.append_seq(1, vsf::eagle_time_oscillations(), Some((wid, slot as u8)), &rx_arch_pkt[..an]);
                                });
                                if done.is_none() {
                                    w.append_seq(1 | super::spool::RAW_FLAG, vsf::eagle_time_oscillations(), Some((wid, slot as u8)), body);
                                }
                            }
                            rx_energy += pcm.iter().map(|v| v.unsigned_abs() as u64).sum::<u64>();
                            rx_frames += 1;
                            raw_in += 1;
                            frames.push(pcm);
                            continue;
                        }
                        if let Some(w) = spool.as_mut() {
                            w.append_seq(1, vsf::eagle_time_oscillations(), Some((wid, slot as u8)), body);
                        }
                        let mut pcm = vec![0i16; FRAME_SAMPLES];
                        match decoder.decode(body, &mut pcm, false) {
                            Ok(s) if s == FRAME_SAMPLES => {
                                rx_energy += pcm.iter().map(|v| v.unsigned_abs() as u64).sum::<u64>();
                                rx_frames += 1;
                                frames.push(pcm);
                            }
                            Ok(_) | Err(_) => {}
                        }
                    }
                    // (The jitter target is the loss loop's, set in the play loop below; the arrival granularity is its floor.)
                    // Tell the jitter buffer the arrival granularity: a target below the window size structurally underruns between datagrams (the call-start latency ratchet, field 2026-09-08).
                    rx_done.insert(wid, frames);
                    have_set(&mut rx_have, wid);
                    wanted.remove(&wid);
                    // Receive-side cleanliness is the climb evidence (channel proxy — see the ladder comment): a full streak of completed windows earns one rung up.
                    clean_rx_windows += 1;
                    let now = std::time::Instant::now();
                    let held = now.duration_since(last_tier_change) >= CLIMB_HOLD
                        && last_tier_drop.map_or(true, |t| now.duration_since(t) >= DROP_HOLD);
                    let next = pending_tier + 1;
                    let need = if next == RAW_TIER { PLAID_CLIMB_CLEAN_WINDOWS } else { CLIMB_CLEAN_WINDOWS };
                    let allowed = next < TIER_RATES.len() && (next != RAW_TIER || params.plaid_allowed);
                    if clean_rx_windows >= need && held && allowed {
                        pending_tier = next;
                        clean_rx_windows = 0;
                        tier_ups += 1;
                        last_tier_change = now;
                        if pending_tier == RAW_TIER {
                            crate::log("CALL: tier up → plaid (raw 48 kHz PCM, 768 kbps, one 5 ms frame per datagram)");
                        } else {
                            crate::logf!("CALL: tier up → {} kbps", TIER_RATES[pending_tier] / 1000);
                        }
                    }
                }
            }
        }

        // ---- Play in strict window order; a hole with two later windows complete is LOST (skip — dry queue = silence). ----
        if let Some(np) = next_play {
            let mut np = np;
            loop {
                if let Some(frames) = rx_done.remove(&np) {
                    for mut f in frames {
                        // Probe hold: the far end's early frames (their own probe window — silence) are dropped, not rendered, so the chirp stays the only sound in the room.
                        if probing {
                            continue;
                        }
                        // Output pad: 4 stops down on live wave playback (Nick 2026-09-03) — the headset default now that the speaker toggle is parked; the loudspeaker at max media volume ran ~2.5 stops hot in the field and near-unity echo coupling came with it. The duck drops further from here when the physics demand it. Ritual prompts and recording preview enqueue directly and stay unpadded; the RENDER_ENV/RENDER_REF taps sit downstream, so the learner still measures the true emitted level.
                        for s in &mut f {
                            *s >>= OUTPUT_PAD_STOPS;
                        }
                        if draining.is_some() {
                            continue;
                        }
                        last_played = Some(f.clone());
                        crate::platform::audio::queue_playback(f);
                    }
                    // Loss loop: a played window slot (an underrun since the last slot counts as lost — silence reached the ear either way).
                    let underruns = crate::platform::audio::jitter_stats().2;
                    let lost = underruns > last_underruns;
                    last_underruns = underruns;
                    jitter_target = loss_loop_step(&mut loss_bits, &mut loss_pos, &mut loss_integ, lost, TIER_FRAMES[tier]);
                    np = np.wrapping_add(1);
                } else if rx_done.range(np..).nth(1).is_some() {
                    // Two completed windows beyond the hole — declare it lost, move on.
                    windows_lost += 1;
                    // …and ask for it back for the recording (the peer spooled what it sent).
                    if !have_get(&rx_have, np) && wanted.len() < FILL_WANTED_CAP {
                        wanted.insert(np);
                    }
                    let underruns = crate::platform::audio::jitter_stats().2;
                    last_underruns = underruns;
                    jitter_target = loss_loop_step(&mut loss_bits, &mut loss_pos, &mut loss_integ, true, TIER_FRAMES[tier]);
                    // CRISPY, NOT CLICK: the hole is filled with the last played frame fading to silence over its own length — a decaying tail at the edge instead of a hard cut to zero. Once per run of holes (the fade ends at zero, so a second hole needs no fade). Never a synthesized guess at the missing sound.
                    if !probing && draining.is_none() {
                        if let Some(prev) = last_played.take() {
                            let len = prev.len().max(1) as i32;
                            let fade: Vec<i16> = prev.iter().enumerate().map(|(i, s)| ((*s as i32) * (len - i as i32) / len) as i16).collect();
                            crate::platform::audio::queue_playback(fade);
                            holes_faded += 1;
                        }
                    }
                    // A lost window restarts the climb evidence; it is the AIMD drop edge only when losses cluster (LOSSES_TO_DROP inside LOSS_WINDOW) — one lost pair on a clean channel is noise, not congestion.
                    clean_rx_windows = 0;
                    let now = std::time::Instant::now();
                    recent_losses.push_back(now);
                    while recent_losses.front().is_some_and(|t| now.duration_since(*t) > LOSS_WINDOW) {
                        recent_losses.pop_front();
                    }
                    // Plaid drops on a loss RATE (the crispies are the deal); every Opus rung drops on a clustered pair — and never twice inside DROP_HOLD (2026-09-10: one burst of lost windows cascaded plaid → 128 → 64 → 32 → 16 in a single tick; one rung per burst is the rule).
                    let need = if pending_tier == RAW_TIER { PLAID_LOSSES_TO_DROP } else { LOSSES_TO_DROP };
                    let drop_held = last_tier_drop.map_or(true, |t| now.duration_since(t) >= DROP_HOLD);
                    if recent_losses.len() >= need && pending_tier > 0 && drop_held {
                        pending_tier = pending_tier.saturating_sub(DROP_RUNGS_ON_LOSS);
                        tier_downs += 1;
                        last_tier_change = now;
                        last_tier_drop = Some(now);
                        recent_losses.clear();
                        crate::logf!(
                            "CALL: tier down → {} kbps ({} windows lost within {}s)",
                            TIER_RATES[pending_tier] / 1000,
                            need,
                            LOSS_WINDOW.as_secs()
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

        // ---- FILL plane TX: requests for what we lost, the windows the peer asked for, and (draining) the flags that let both sides finish. ----
        {
            // Drain: the tail — every window from the play head to the peer's final count that never arrived — joins the wanted set as the peer's count becomes known.
            if let (Some(_), Some(pw), Some(np)) = (draining, peer_windows, next_play) {
                let mut s = np;
                while s < pw && wanted.len() < FILL_WANTED_CAP {
                    if !have_get(&rx_have, s) && !rx_done.contains_key(&s) {
                        wanted.insert(s);
                    }
                    s = s.wrapping_add(1);
                }
            }
            let req_every = if draining.is_some() { FILL_REQ_DRAIN } else { FILL_REQ_LIVE };
            let mut reqs: Vec<u32> = Vec::new();
            if peer_fills && !wanted.is_empty() && last_req_tx.elapsed() >= req_every {
                // Round-robin thru the wanted set so a nack-less peer (or a lost request) never starves the rest.
                let mut it = wanted.range(wanted_cursor..).chain(wanted.range(..wanted_cursor));
                for _ in 0..FILL_REQ_PER_PACKET {
                    match it.next() {
                        Some(w) => reqs.push(*w),
                        None => break,
                    }
                }
                if let Some(last) = reqs.last() {
                    wanted_cursor = last.wrapping_add(1);
                }
                last_req_tx = std::time::Instant::now();
                fills_asked += reqs.len() as u64;
            }
            let mut fills: Vec<(u32, u8, Vec<u8>)> = Vec::new();
            let mut budget = FILL_PACKET_BUDGET;
            while let Some(seq) = serve_queue.iter().next().copied() {
                let served = serve_window(&params, &mut spool_reader, &tx_index, seq).or_else(|| serve_window_from_archive(&params, &mut spool_reader, &arch_index, &mut arch_dec, seq));
                let cost = 5 + served.as_ref().map_or(0, |(_, b)| b.len());
                if cost > budget && !fills.is_empty() {
                    break;
                }
                serve_queue.remove(&seq);
                budget = budget.saturating_sub(cost);
                match served {
                    Some((t, b)) => {
                        fills.push((seq, t, b));
                        fills_served += 1;
                        if draining.is_some() {
                            fills_served_drain += 1;
                        }
                    }
                    None => fills.push((seq, FILL_NACK, Vec::new())),
                }
            }
            let heartbeat = draining.is_some() && last_fill_tx.elapsed() >= FILL_HEARTBEAT;
            if fill_hello_due || heartbeat || !reqs.is_empty() || !fills.is_empty() {
                let msg = FillMsg {
                    draining: draining.is_some(),
                    satisfied: draining.is_some() && peer_draining && wanted.is_empty(),
                    windows: window_id,
                    reqs,
                    fills,
                };
                tx_fill.advance_to(StepChain::step_for_seq(fill_seq));
                if let Some(wire) = packet::seal_fill(&tx_fill, fill_seq, &fill_encode(&msg)) {
                    let _ = super::send_media(wire, peer);
                }
                fill_seq = fill_seq.wrapping_add(1);
                fill_hello_due = false;
                last_fill_tx = std::time::Instant::now();
            }
        }

        // Signal-plane re-anchor: an authenticated express Anchor named a fresh peer address (both-sides-moved heal) — re-point TX there. The media plane's own follow rule keeps refining from packet sources as usual.
        if let Some(a) = super::take_peer_redirect() {
            // A path that has carried authenticated media is never yanked off by a signal (field 2026-09-12: the other side's rescue probe re-anchored a live LAN wave onto a dead IPv6 address). The media plane's own forward-progress rule still re-points TX when the PEER'S packets arrive from somewhere new — that is the heal that stays.
            if a != peer && rx_max_seq.is_none() {
                crate::logf!("CALL: peer re-anchored via express → {} (was {})", a, peer);
                peer = a;
                super::set_call_tx_addr(Some(peer));
            } else if a != peer {
                crate::logf!("CALL: express named {} but media has flowed from {} — keeping the working path", a, peer);
            }
        }

        // Probe close: capture full → audio connects NOW, the fit runs off-thread and seeds a beat later. Deadline = mic never granted / device never spun up — connect anyway, the duck runs on its prior seed.
        if probing {
            if probe_cap.len() >= crate::call::vchirp::CAPTURE_SAMPLES {
                probing = false;
                crate::platform::audio::set_local_source(false); // network audio owns the queue from here
                let cap = std::mem::take(&mut probe_cap);
                match (probe_render_osc, probe_anchor_osc) {
                    (Some(r), Some(a)) => {
                        crate::logf!(
                            "CALL: v-chirp played + sampled ({} samples) — audio connected, fit running",
                            cap.len()
                        );
                        crate::call::vchirp::finish(cap, vol_lin_now, r, a, live_route.clone(), chirp_scale);
                    }
                    _ => crate::log("CALL: v-chirp probe had no render anchor — abandoned, audio connected"),
                }
            } else if std::time::Instant::now() >= probe_deadline {
                probing = false;
                crate::platform::audio::set_local_source(false);
                crate::logf!(
                    "CALL: v-chirp probe abandoned at deadline ({} of {} samples) — audio connected",
                    probe_cap.len(),
                    crate::call::vchirp::CAPTURE_SAMPLES
                );
                probe_cap = Vec::new();
            }
        }
        // Live readout once a second (the call panel's stats line on every build); the log line keeps its 10 s cadence below.
        if last_live_stats.elapsed() >= std::time::Duration::from_secs(1) {
            last_live_stats = std::time::Instant::now();
            if rtt_n > 0 {
                super::LAST_LINK_RTT_MS.store(rtt_ema.round().max(1.0) as u32, Ordering::Relaxed);
            }
            super::LAST_LINK_LOSS.store(loss_bits.iter().map(|w| w.count_ones()).sum::<u32>(), Ordering::Relaxed);
            super::LAST_LINK_TARGET.store(jitter_target as u32, Ordering::Relaxed);
            super::LAST_LINK_TIER.store(tier as u32, Ordering::Relaxed);
        }
        // Periodic link + echo stats (10s cadence on the engine loop — a measurement cadence, not UI timing).
        if last_echo_stats.elapsed() >= std::time::Duration::from_secs(10) {
            last_echo_stats = std::time::Instant::now();
            fill_hello_due = true; // the fill plane's presence beacon — a peer learns we speak it from any fill datagram
            {
                let losses = loss_bits.iter().map(|w| w.count_ones()).sum::<u32>();
                let js = crate::platform::audio::jitter_stats();
                if rtt_n > 0 {
                    super::LAST_LINK_RTT_MS.store(rtt_ema.round().max(1.0) as u32, Ordering::Relaxed);
                }
                super::LAST_LINK_LOSS.store(losses, Ordering::Relaxed);
                super::LAST_LINK_TARGET.store(jitter_target as u32, Ordering::Relaxed);
                crate::logf!(
                    "CALL: link — rtt {} ms (min {} max {}, {} samples this window), loss {}/256 ring ({} lost this window), jitter target {} depth {} underruns {}",
                    if win_rtt_n > 0 { format!("{:.0}", rtt_ema) } else { "?".to_string() },
                    if win_rtt_n > 0 { win_rtt_min.to_string() } else { "?".to_string() },
                    win_rtt_max,
                    win_rtt_n,
                    losses,
                    windows_lost.saturating_sub(win_losses_at as u64),
                    jitter_target,
                    js.1,
                    js.2
                );
                win_rtt_min = u32::MAX;
                win_rtt_max = 0;
                win_rtt_n = 0;
                win_losses_at = windows_lost as u32;
            }
            if let Some(c) = nlms.as_ref() {
                crate::logf!(
                    "CALL: echo — filter recent {}dB lifetime {}dB, adapted {} of {} frames; gated {} ducked {} far-active {}",
                    c.erle_recent_db().map_or("?".to_string(), |e| format!("{e:.1}")),
                    c.erle_db().map_or("?".to_string(), |e| format!("{e:.1}")),
                    c.adapted_frames,
                    c.run_frames,
                    gated_frames,
                    ducked_frames,
                    far_active_frames
                );
            }
        }
        // NLMS self-check: a canceller that measured itself making echo WORSE across its probation window disarms — the duck (unchanged, still running on the same frames) carries alone. A garbage seed (a barely-passed low-volume fit) can't keep injecting.
        if nlms.as_ref().is_some_and(|c| c.is_net_harmful()) {
            if nlms.as_ref().is_some_and(|c| c.diverged) {
                crate::log("CALL: nlms disarmed — taps DIVERGED (non-finite), raw audio passed thru, duck carries");
            } else {
                let erle = nlms.as_ref().and_then(|c| c.erle_recent_db()).unwrap_or(0.0);
                crate::logf!("CALL: nlms disarmed — net harmful (recent {:.1}dB), duck carries", erle);
            }
            nlms = None;
            // The prediction the canceller was meant to verify is unverified: reactive gating from here on.
            if prediction_trusted {
                prediction_trusted = false;
                crate::log("CALL: gate falls back to reactive (measured far level) — the predictive seed is unverified without the canceller");
            }
        }
        // Fit verdict: a fresh measurement of THIS route outranks any stored/ringback seed; Clean keeps the duck reactive (a headset-class route barely ducks anyway) but takes the measured floor.
        if let Some(v) = crate::call::vchirp::take_verdict() {
            match v {
                crate::call::vchirp::Verdict::Coupled { g_norm, delay_bins, floor, ir } => {
                    crate::logf!(
                        "CALL: v-chirp seeded the predictive duck — g {} delay {}ms{}; nlms armed ({} taps @ {})",
                        format!("{g_norm:.4}"),
                        delay_bins * 10,
                        if applied.is_some() { " (overrode prior seed)" } else { "" },
                        ir.1.len(),
                        ir.0
                    );
                    applied = Some((g_norm, delay_bins));
                    live_floor = floor.max(1.0);
                    nlms = Some(crate::call::nlms::Nlms::new(ir.0, ir.1));
                    nlms_seed_vol = vol_lin_now.max(1e-6);
                }
                crate::call::vchirp::Verdict::Clean { floor } => {
                    live_floor = floor.max(1.0);
                }
            }
        }

        // Learner estimator + the 1s control plane (estimate refresh, live floor, route tracking). The tick itself is internally cadence-gated; the Instant is a measurement cadence on the engine thread (like PT's RTO), not UI timing.
        vol_lin_now = crate::platform::audio::current_volume_db()
            .map_or(1.0, |db| 10f32.powf(db / 20.0));
        learner.tick(vol_lin_now);
        if last_est.elapsed() >= std::time::Duration::from_secs(1) {
            last_est = std::time::Instant::now();
            let est = learner.estimate();
            live_floor = est.floor;
            // A Usable-or-better estimate refines (or ARMS) the predictive duck: slew g (τ≈2s at this cadence), step delay only between far bursts — a mid-burst delay step misaligns the prediction and mis-gates real speech.
            // ACTIVE PROFILING → CLEAN (Nick 2026-09-10): a confident in-call estimate that finds no coupling worth a duck retires the predictive duck to its clean floor and drops the canceller — the route IS clean, whatever the chirp said (or failed to say).
            if est.confidence >= crate::call::learn::Confidence::Usable {
                if let Some(g) = est.g_norm {
                    if g <= CLEAN_G_NORM && est.windows >= 3 && !clean_declared {
                        clean_declared = true;
                        crate::logf!("CALL: learner reads the route CLEAN (g {} over {} windows) — duck to floor, canceller off", format!("{g:.4}"), est.windows);
                        applied = Some((0.0, est.delay_bins.unwrap_or(1)));
                        nlms = None;
                    } else if g > CLEAN_G_NORM * 2.0 && clean_declared {
                        clean_declared = false;
                        crate::logf!("CALL: learner sees coupling again (g {}) — duck back on its estimate", format!("{g:.4}"));
                    }
                }
                if let (Some(g), Some(d)) = (est.g_norm, est.delay_bins) {
                    match &mut applied {
                        Some((ag, ad)) => {
                            *ag += (g - *ag) * 0.4;
                            if learner.far_env_at(0) < crate::call::learn::FAR_ACT {
                                *ad = d;
                            }
                        }
                        None => {
                            crate::logf!(
                                "CALL: learner armed the predictive duck — g {} delay {}ms ({} windows)",
                                format!("{g:.4}"),
                                d * 10,
                                est.windows
                            );
                            applied = Some((g, d));
                        }
                    }
                }
            }
            // Live route tracking (the cached-route bug — a mid-call BT swap was invisible): a swap finalizes the old route's learning, reverts the duck to reactive, and starts a fresh accumulator for the new physics.
            let rid = crate::platform::audio::route_id();
            if rid != live_route && !rid.is_empty() {
                crate::logf!(
                    "CALL: route swapped \"{}\" → \"{}\" — learning finalized, duck reverts to reactive until re-learned",
                    live_route,
                    rid
                );
                crate::call::calibrate::post_learned(learned_results(&learner.estimate(), &live_route));
                learner = crate::call::learn::Learner::new(rid.starts_with("bt:"), None, None);
                applied = None;
                nlms = None; // the taps are the OLD route's physics — duck-only until the next call's chirp
                pred_gate = crate::call::learn::PredGate::new();
                route_ducks = !matches!(
                    crate::platform::audio::route(),
                    crate::platform::audio::AudioRoute::Headset
                );
                live_route = rid;
            }
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
    if peer_fills || fills_asked > 0 {
        crate::logf!(
            "CALL: fills — asked {} ({} still wanted), got {} live + {} in the drain, {} nacked; served {} ({} in the drain); drain {} ms, peer {}",
            fills_asked,
            wanted.len(),
            fills_got_live,
            fills_got_drain,
            fills_nacked,
            fills_served,
            fills_served_drain,
            draining.map_or(0, |t| t.elapsed().as_millis()),
            if peer_satisfied { "satisfied" } else if peer_draining { "draining" } else { "silent" }
        );
    }
    if rtt_n > 0 {
        crate::logf!(
            "CALL: link — rtt {} ms over the call (min {} max {} ema {:.0}, {} samples); final jitter target {}",
            format!("{:.0}", rtt_ema),
            rtt_min,
            rtt_max,
            rtt_ema,
            rtt_n,
            jitter_target
        );
    }
    if raw_out > 0 || raw_in > 0 || holes_faded > 0 {
        crate::logf!(
            "CALL: plaid — {} raw frames out, {} in; {} hole(s) faded",
            raw_out,
            raw_in,
            holes_faded
        );
    }
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
    // CAPTURE/RENDER CADENCE (field 2026-09-08: Brittany's phone TX ran 15808 frames over a ~40s call = 2x realtime, the phone trimmed half at playout = the scratchy; a device whose fast-path delivers double must NAME itself). Frames-per-second each way against the call's wall-clock; a healthy 5ms path reads ~200.
    let call_secs = start_instant.elapsed().as_secs_f64().max(0.001);
    crate::logf!(
        "CALL: cadence — tx {} fps, rx {} fps over {}s (nominal 200)",
        format!("{:.0}", tx_frames as f64 / call_secs),
        format!("{:.0}", rx_frames as f64 / call_secs),
        format!("{:.1}", call_secs)
    );
    if let Some(first) = cap_first_osc {
        let hal_secs = (cap_last_osc - first).max(0) as f64 / vsf::OSCILLATIONS_PER_SECOND as f64;
        crate::logf!(
            "CALL: capture — {} frames over {}s of HAL time ({} fps by the HAL clock; 200 nominal)",
            tx_frames,
            format!("{hal_secs:.1}"),
            format!("{:.0}", if hal_secs > 0.0 { tx_frames as f64 / hal_secs } else { 0.0 })
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
    // Ladder + duck field-tuning readout: where the call ended up, how it moved, and the echo numbers — `gated` = hard near-mutes (echo-only frames), `ducked` = any attenuation, `far-active` = frames the far end was talking. A healthy speakerphone call wants gated ≈ (far-active − double-talk): most far-talk-alone frames should hard-gate.
    crate::logf!(
        "CALL: ladder — ended {}, {} up(s), {} down(s); duck {} — gated {}, ducked {}, far-active {} of {} frames",
        if tier == RAW_TIER { "plaid (raw PCM)".to_string() } else { format!("{} kbps", TIER_RATES[tier] / 1000) },
        tier_ups,
        tier_downs,
        if PID_DUCK_ENABLED { "PID+gate" } else { "disarmed" },
        gated_frames,
        ducked_frames,
        far_active_frames,
        tx_frames
    );
    // SHADOW learner readout (Stage 3 field telemetry): the learned physics beside the profile the ritual measured — the convergence proof the gate softening waits on. rejects = per-gate window rejections [short, skew, hole, inactive, quiet, xcorr, cluster-reserved, edge, badg, r].
    {
        let e = learner.estimate();
        crate::logf!(
            "CALL: learner — g {} delay {} conf {} over {} window(s); floor {} talk {} ({} voiced bins); rejects {:?}; route \"{}\"",
            e.g_norm.map_or("?".into(), |g| format!("{g:.4}")),
            e.delay_bins.map_or("?".into(), |d| format!("{}ms", d * 10)),
            format!("{:?}", e.confidence),
            e.windows,
            format!("{:.0}", e.floor),
            e.talk.map_or("?".into(), |t| format!("{t:.0}")),
            e.voiced_bins,
            format!("{:?}", e.rejects),
            crate::platform::audio::route_id()
        );
        // The canceller's report card: honest ERLE measured only over far-talks-alone frames (where echo dominates the mic).
        if let Some(c) = &nlms {
            crate::logf!(
                "CALL: nlms — recent {}dB over {} adapted of {} frames; whitener scales {}",
                c.erle_recent_db().map_or("?".to_string(), |e| format!("{e:.1}")),
                c.adapted_frames,
                c.run_frames,
                format!("{:?}", crate::call::nlms::WHITEN_SCALES)
            );
            crate::logf!(
                "CALL: nlms — erle {} over {} adapted frame(s)",
                c.erle_db().map_or("?".into(), |d| format!("{d:.1}dB")),
                c.adapted_frames
            );
        }
        // Which duck ran (field forensics): pred = the calibrated prediction gated the mic; reactive = the peak-hold fallback (uncalibrated, or a mid-call route swap reset it).
        crate::logf!(
            "CALL: duck mode at teardown — {}{}",
            if applied.is_some() { "predictive" } else { "reactive" },
            applied.map_or(String::new(), |(g, d)| format!(" (applied g {g:.4} delay {}ms)", d * 10))
        );
        // Persist what the call proved (Stage 4): echo posts only at SOLID confidence (the persisted tier); voice posts on its own evidence gate (≥5s of voiced far-quiet speech ⇒ talk is Some). The drain blends against the stored profile — ritual outranks, learned refines.
        crate::call::calibrate::post_learned(learned_results(&e, &live_route));
    }
    teardown(sink_gen);
    // tx_chain/rx_chain drop here — zeroized; the call is cryptographically gone.
}

/// One step of the loss-rate loop: record this window slot (lost or played) in the 256-bit ring, compute the loss rate's error in stops against LOSS_SETPOINT, P + I it onto the jitter target above the arrival floor, and set the buffer's target. Returns the target set.
fn loss_loop_step(bits: &mut [u64; LOSS_RING / 64], pos: &mut u8, integ: &mut f32, lost: bool, floor_frames: usize) -> usize {
    let i = *pos as usize;
    let (w, b) = (i / 64, i % 64);
    if lost {
        bits[w] |= 1u64 << b;
    } else {
        bits[w] &= !(1u64 << b);
    }
    *pos = pos.wrapping_add(1);
    let losses = bits.iter().map(|x| x.count_ones()).sum::<u32>() as f32;
    let rate = (losses / LOSS_RING as f32).max(1.0 / 4096.0);
    let err_stops = (rate / LOSS_SETPOINT).log2();
    *integ = (*integ + LOSS_KI * err_stops).clamp(0.0, JITTER_TARGET_CAP as f32);
    let floor = floor_frames.max(1) as f32;
    let target = (floor + LOSS_KP * err_stops + *integ).round().clamp(floor, JITTER_TARGET_CAP as f32) as usize;
    crate::platform::audio::set_jitter_target(target);
    target
}

/// Package a learner estimate as persistable results (used at teardown AND on a mid-call route swap). Echo requires SOLID (the persisted tier); voice requires its own ≥5s-voiced evidence (talk is Some). g is per-window vol-normalized, so the stored reference is 0dB where a volume mirror exists, absent on desktop.
fn learned_results(
    e: &crate::call::learn::Estimate,
    route_id: &str,
) -> Vec<crate::call::calibrate::LearnedResult> {
    let mut learned: Vec<crate::call::calibrate::LearnedResult> = Vec::new();
    if e.confidence == crate::call::learn::Confidence::Solid {
        if let (Some(g), Some(d)) = (e.g_norm, e.delay_bins) {
            learned.push(crate::call::calibrate::LearnedResult {
                result: crate::call::calibrate::CalResult::Echo(
                    crate::call::calibrate::EchoProfile {
                        g_norm: g,
                        delay_ms: (d * 10) as u32,
                        cal_vol_db: crate::platform::audio::current_volume_db().map(|_| 0.0),
                        route_id: route_id.to_string(),
                    },
                ),
                windows: e.windows as u32,
                solid: true,
            });
        }
    }
    if let Some(talk) = e.talk {
        let mic_gain = (4000.0 / talk.max(1.0)).clamp(0.125, 8.0);
        learned.push(crate::call::calibrate::LearnedResult {
            result: crate::call::calibrate::CalResult::Voice(
                crate::call::calibrate::VoiceProfile {
                    mic_gain,
                    floor: e.floor,
                    mic_id: crate::platform::audio::mic_id(),
                },
            ),
            windows: (e.voiced_bins / 100) as u32,
            solid: e.confidence == crate::call::learn::Confidence::Solid,
        });
    }
    learned
}

fn teardown(sink_gen: u64) {
    super::clear_media_sink_gen(sink_gen);
    crate::platform::audio::stop();
}

/// One frame this side SENT: where it sits in the spool, so the peer's request for its window can be served from disk.
struct SentFrame {
    seq: u32,
    slot: u8,
    tier: u8,
    at: super::spool::RecordAt,
}

/// The FILL datagram's plaintext: `[flags u8][windows u32 LE][nreq u8][seq u32 LE × nreq][nfill u8][(seq u32 LE, tier u8, window bytes) × nfill]` — a `tier` of FILL_NACK carries no bytes.
struct FillMsg {
    draining: bool,
    satisfied: bool,
    /// Our source window count (the next seq we would send) — the peer's tail list runs up to it.
    windows: u32,
    reqs: Vec<u32>,
    fills: Vec<(u32, u8, Vec<u8>)>,
}

fn fill_encode(m: &FillMsg) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + m.reqs.len() * 4 + m.fills.iter().map(|(_, _, b)| 5 + b.len()).sum::<usize>());
    out.push((m.draining as u8) | ((m.satisfied as u8) << 1));
    out.extend_from_slice(&m.windows.to_le_bytes());
    out.push(m.reqs.len().min(255) as u8);
    for r in m.reqs.iter().take(255) {
        out.extend_from_slice(&r.to_le_bytes());
    }
    out.push(m.fills.len().min(255) as u8);
    for (seq, tier, bytes) in m.fills.iter().take(255) {
        out.extend_from_slice(&seq.to_le_bytes());
        out.push(*tier);
        if *tier != FILL_NACK {
            out.extend_from_slice(bytes);
        }
    }
    out
}

fn fill_decode(b: &[u8]) -> Option<FillMsg> {
    let mut i = 0usize;
    let take = |i: &mut usize, n: usize| -> Option<&[u8]> {
        let s = b.get(*i..*i + n)?;
        *i += n;
        Some(s)
    };
    let flags = take(&mut i, 1)?[0];
    let windows = u32::from_le_bytes(take(&mut i, 4)?.try_into().ok()?);
    let nreq = take(&mut i, 1)?[0] as usize;
    let mut reqs = Vec::with_capacity(nreq);
    for _ in 0..nreq {
        reqs.push(u32::from_le_bytes(take(&mut i, 4)?.try_into().ok()?));
    }
    let nfill = take(&mut i, 1)?[0] as usize;
    let mut fills = Vec::with_capacity(nfill);
    for _ in 0..nfill {
        let seq = u32::from_le_bytes(take(&mut i, 4)?.try_into().ok()?);
        let tier = take(&mut i, 1)?[0];
        if tier == FILL_NACK {
            fills.push((seq, tier, Vec::new()));
            continue;
        }
        if tier as usize >= TIER_RATES.len() {
            return None;
        }
        let bytes = take(&mut i, tier_window_bytes(tier as usize))?.to_vec();
        fills.push((seq, tier, bytes));
    }
    if i != b.len() {
        return None;
    }
    Some(FillMsg { draining: flags & 1 != 0, satisfied: flags & 2 != 0, windows, reqs, fills })
}

fn have_get(bits: &[u64], w: u32) -> bool {
    bits.get((w / 64) as usize).is_some_and(|b| b & (1u64 << (w % 64)) != 0)
}

fn have_set(bits: &mut Vec<u64>, w: u32) {
    let i = (w / 64) as usize;
    if i >= bits.len() {
        if i >= (1 << 22) {
            return; // 2^28 windows — not a call
        }
        bits.resize(i + 1, 0);
    }
    bits[i] |= 1u64 << (w % 64);
}

/// Rebuild the window bundle for `seq` from the frames we spooled for it: `(tier, [len u16][frame] × TIER_FRAMES padded to the rung's slots)`. None = not held (older than the index, or never sent).
fn serve_window(params: &EngineParams, reader: &mut Option<super::spool::SpoolReader>, index: &std::collections::VecDeque<SentFrame>, seq: u32) -> Option<(u8, Vec<u8>)> {
    let first = index.partition_point(|f| f.seq < seq);
    if first >= index.len() || index[first].seq != seq {
        return None;
    }
    if reader.is_none() {
        let (key, path) = params.spool.as_ref()?;
        *reader = Some(super::spool::SpoolReader::open(key, path)?);
    }
    let r = reader.as_mut()?;
    let tier = index[first].tier as usize;
    if tier >= TIER_RATES.len() {
        return None;
    }
    let mut window = vec![0u8; tier_window_bytes(tier)];
    let mut any = false;
    for f in index.range(first..).take_while(|f| f.seq == seq) {
        let slot = f.slot as usize;
        if slot >= TIER_FRAMES[tier] {
            continue;
        }
        let Some(rec) = r.read_at(f.at) else {
            continue;
        };
        if rec.bytes.len() > TIER_MAX_ENC[tier] {
            continue;
        }
        let base = slot * tier_slot(tier);
        window[base..base + 2].copy_from_slice(&(rec.bytes.len() as u16).to_le_bytes());
        window[base + 2..base + 2 + rec.bytes.len()].copy_from_slice(&rec.bytes);
        any = true;
    }
    any.then_some((tier as u8, window))
}

/// Plaid fills reconstruct from the ARCHIVE (whose wire copies never spool): a plaid window is exactly 2 frames = one 10ms archive record — decode it, split the halves, bundle at the RAW tier. Serve-time decode, and only on the pristine-path rung where losses are rare anyway.
fn serve_window_from_archive(params: &EngineParams, reader: &mut Option<super::spool::SpoolReader>, arch_index: &std::collections::VecDeque<(u32, u8, super::spool::RecordAt)>, dec: &mut Option<opus::Decoder>, seq: u32) -> Option<(u8, Vec<u8>)> {
    let first = arch_index.partition_point(|(w, _, _)| *w < seq);
    if first >= arch_index.len() || arch_index[first].0 != seq {
        return None;
    }
    if reader.is_none() {
        let (key, path) = params.spool.as_ref()?;
        *reader = Some(super::spool::SpoolReader::open(key, path)?);
    }
    let r = reader.as_mut()?;
    if dec.is_none() {
        *dec = opus::Decoder::new(48_000, opus::Channels::Mono).ok();
    }
    let d = dec.as_mut()?;
    let tier = RAW_TIER;
    let mut window = vec![0u8; tier_window_bytes(tier)];
    let mut any = false;
    for (_, slot0, at) in arch_index.range(first..).take_while(|(w, _, _)| *w == seq) {
        let Some(rec) = r.read_at(*at) else {
            continue;
        };
        let mut pcm = vec![0i16; 480];
        let _ = d.reset_state();
        let Ok(got) = d.decode(&rec.bytes, &mut pcm, false) else {
            continue;
        };
        if got < 480 {
            continue;
        }
        for half in 0..2usize {
            let slot = *slot0 as usize + half;
            if slot >= TIER_FRAMES[tier] {
                continue;
            }
            let base = slot * tier_slot(tier);
            window[base..base + 2].copy_from_slice(&(RAW_FRAME_BYTES as u16).to_le_bytes());
            for (i, sample) in pcm[half * 240..half * 240 + 240].iter().enumerate() {
                window[base + 2 + i * 2..base + 2 + i * 2 + 2].copy_from_slice(&sample.to_le_bytes());
            }
            any = true;
        }
    }
    any.then_some((tier as u8, window))
}

#[cfg(test)]
mod fill_tests {
    use super::*;

    #[test]
    fn fill_message_round_trips_with_nacks_and_refuses_trailing_bytes() {
        let m = FillMsg {
            draining: true,
            satisfied: false,
            windows: 90_001,
            reqs: vec![7, 9, 4_000_000_000],
            fills: vec![(7, 0, vec![3u8; tier_window_bytes(0)]), (9, FILL_NACK, Vec::new()), (12, RAW_TIER as u8, vec![5u8; tier_window_bytes(RAW_TIER)])],
        };
        let bytes = fill_encode(&m);
        let back = fill_decode(&bytes).unwrap();
        assert!(back.draining && !back.satisfied && back.windows == 90_001);
        assert_eq!(back.reqs, m.reqs);
        assert_eq!(back.fills, m.fills);
        let mut long = bytes.clone();
        long.push(0);
        assert!(fill_decode(&long).is_none());
        assert!(fill_decode(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn have_bits_grow_on_demand() {
        let mut bits = Vec::new();
        assert!(!have_get(&bits, 700));
        have_set(&mut bits, 700);
        assert!(have_get(&bits, 700) && !have_get(&bits, 699) && !have_get(&bits, 701));
    }

    #[test]
    fn served_window_comes_back_off_the_spool_by_seq() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("photon-fill-serve-test-{}.tmp", std::process::id()));
        let key = [3u8; 32];
        let mut w = super::super::spool::SpoolWriter::create(&key, &path).unwrap();
        let mut index = std::collections::VecDeque::new();
        // Two floor-rung windows (8 frames each), a remote frame in between, then a plaid window.
        for seq in 0u32..2 {
            for slot in 0..TIER_FRAMES[0] {
                let at = w.append_seq(0, 1, Some((seq, slot as u8)), &[seq as u8 + 1; 12]).unwrap();
                index.push_back(SentFrame { seq, slot: slot as u8, tier: 0, at });
            }
            w.append(1, 2, &[9; 12]);
        }
        let at = w.append_seq(super::super::spool::RAW_FLAG, 3, Some((2, 0)), &[8; RAW_FRAME_BYTES]).unwrap();
        index.push_back(SentFrame { seq: 2, slot: 0, tier: RAW_TIER as u8, at });
        drop(w);
        let params = EngineParams { secret: [0; 32], we_are_caller: true, peer_addr: "127.0.0.1:1".parse().unwrap(), spool: Some((key, path.clone())), cal: None, plaid_allowed: false };
        let mut reader = None;
        let (t, win) = serve_window(&params, &mut reader, &index, 1).unwrap();
        assert_eq!(t, 0);
        assert_eq!(win.len(), tier_window_bytes(0));
        for slot in 0..TIER_FRAMES[0] {
            let base = slot * tier_slot(0);
            assert_eq!(u16::from_le_bytes([win[base], win[base + 1]]), 12);
            assert_eq!(&win[base + 2..base + 14], &[2u8; 12]);
        }
        let (t, win) = serve_window(&params, &mut reader, &index, 2).unwrap();
        assert_eq!(t as usize, RAW_TIER);
        assert_eq!(u16::from_le_bytes([win[0], win[1]]) as usize, RAW_FRAME_BYTES);
        assert!(serve_window(&params, &mut reader, &index, 3).is_none());
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loss_loop_holds_the_floor_when_clean_and_lifts_on_loss() {
        let mut bits = [0u64; LOSS_RING / 64];
        let (mut pos, mut integ) = (0u8, 0f32);
        let mut t = 0;
        for _ in 0..600 {
            t = loss_loop_step(&mut bits, &mut pos, &mut integ, false, 2);
        }
        assert_eq!(t, 2, "a clean link rests on the arrival floor");
        // Eight lost windows in a row: two stops over the setpoint → the target lifts at once and keeps climbing while the loss persists in the ring.
        for _ in 0..8 {
            t = loss_loop_step(&mut bits, &mut pos, &mut integ, true, 2);
        }
        assert!(t >= 4, "loss lifts the target, got {t}");
        let lifted = t;
        // 300 clean windows: the burst leaves the ring after 256 and the target is already falling; the integral bleeds out over the next few hundred and the floor returns.
        for _ in 0..300 {
            t = loss_loop_step(&mut bits, &mut pos, &mut integ, false, 2);
        }
        assert!(t < lifted, "clean windows shrink it back, got {t} (lifted {lifted})");
        for _ in 0..600 {
            t = loss_loop_step(&mut bits, &mut pos, &mut integ, false, 2);
        }
        assert_eq!(t, 2, "a clean link returns to the floor");
    }

    #[test]
    fn plaid_window_is_one_bare_frame() {
        // The raw rung: one 5 ms frame per window, slot = 2 + 480 (+ pad to the 8-aligned 488), and the bytes come back as the identical samples with no codec in the path.
        assert_eq!(TIER_FRAMES[RAW_TIER], 1);
        assert_eq!(RAW_FRAME_BYTES, 480);
        assert!(RAW_FRAME_BYTES + 2 <= tier_slot(RAW_TIER));
        let frame: Vec<i16> = (0..FRAME_SAMPLES).map(|i| ((i as i32 * 137) % 65536 - 32768) as i16).collect();
        let mut window = Vec::new();
        window.extend_from_slice(&(RAW_FRAME_BYTES as u16).to_le_bytes());
        for s in &frame {
            window.extend_from_slice(&s.to_le_bytes());
        }
        window.resize(tier_window_bytes(RAW_TIER), 0);
        let n = u16::from_le_bytes(window[..2].try_into().unwrap()) as usize;
        let back: Vec<i16> = window[2..2 + n].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(back, frame);
    }

    #[test]
    fn tier_slots_fit_cbr_frames() {
        // CBR emits exactly rate/1600 bytes per 5ms frame; every rung's slot must hold that with margin, every window must be 8-aligned so raptorq's symbol is EXACTLY the window (unaligned would split + pad — the trap this design kills), and both rung indices must fit the ctrl byte's 3-bit fields.
        assert!(TIER_RATES.len() <= 8, "tiers ride 3-bit ctrl fields");
        for t in 0..TIER_RATES.len() {
            assert!(TIER_RATES[t] as usize / 1600 + 2 <= TIER_MAX_ENC[t], "rung {} slot too tight", t);
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
