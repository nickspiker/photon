//! A BEAM in a wave (docs/beams.md): the session that joins the transport (beam.rs), the codec (h264.rs on desktops, MediaCodec through Kotlin on phones) and the receive colour chain (beam_colour.rs) to a camera and to the wave screen's square.
//!
//! Two halves, independent:
//! - **Receiving** starts WITH the wave engine and costs nothing until a beam datagram arrives: the beam-rx thread reassembles frames, this module's decode thread decodes them, runs the colour chain and posts the newest picture to [`picture`], which the wave screen's paint reads. A peer's beam therefore shows up with no signalling at all — the beam describes itself in-band.
//! - **Sending** starts on the Beam button: on a desktop a capture thread pulls frames from the camera, encodes and hands them to a `BeamTx`; on Android, Kotlin's Camera2 + MediaCodec pipeline pushes ENCODED frames thru [`push_encoded`] and the `BeamTx` lives behind a static.
//!
//! The beam's self-description carries the camera's VSF characterization entry (docs/beams.md: `creative`/`assumed`/`srgb` for a camera we know nothing about, `absolute`/`model`/`gamma2` for a phone with the maker's matrix, `relative`/`unit` for a chameleon scan) as the bytes of a one-section VSF document, [`colour_bytes`] / [`colour_from_bytes`].

use super::beam::{self, BeamTx, Codec, Gains, Info};
use super::beam_colour::{Characterization, Converter};
use super::h264;
use super::keys::Direction;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The newest decoded picture, γ2 Rec.2020 RGB triples, for the wave screen's blit.
#[derive(Debug, Clone)]
pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub frame_no: u32,
    pub rgb: Vec<u8>,
    /// Blocks the decoder has never painted (h264::I420::unpainted_mask, turned with the picture): one byte per 2×2 block, (w/2) × (h/2), non-zero = show the background thru. `None` = whole.
    pub holes: Option<Vec<u8>>,
}

/// One picture per far slot (platform::audio FAR_SLOTS): slot 0 is the 1:1 wave's peer, a group wave's links show theirs in their own slots.
static PICTURES: [Mutex<Option<Picture>>; crate::platform::audio::FAR_SLOTS] = [const { Mutex::new(None) }; crate::platform::audio::FAR_SLOTS];
/// Bumped on every new picture: the paint compares it with what it last drew.
pub static PICTURE_GEN: AtomicU64 = AtomicU64::new(0);

/// The newest picture the peer's beam produced, if a beam is flowing.
pub fn picture() -> Option<Picture> {
    picture_slot(0)
}

/// The newest picture one far slot's beam produced (a group link's peer).
pub fn picture_slot(slot: usize) -> Option<Picture> {
    PICTURES[slot].lock().unwrap().clone()
}

/// Is any beam picture up (theirs or our own inset)? The wave screen's switch to the beam layout, without cloning a frame.
pub fn showing() -> bool {
    PICTURES.iter().any(|p| p.lock().unwrap().is_some()) || SELF_PICTURE.lock().unwrap().is_some()
}

/// THE SELF-VIEW (2026-10-10, Nick: "when you choose beam, still no video shows"): what this device's camera is sending, converted thru the same chain the receiver runs — so the inset shows the colour the far side gets, tungsten-yellow included.
static SELF_PICTURE: Mutex<Option<Picture>> = Mutex::new(None);

pub fn self_picture() -> Option<Picture> {
    SELF_PICTURE.lock().unwrap().clone()
}

/// The self-view: the camera frame thru the receiver's chain, turned to this screen's orientation (the same quarter turns the far side applies), and MIRRORED for a front camera — you see yourself as a mirror shows you; the far side gets the true image.
fn post_self(conv: &Converter, frame: &h264::I420, gains: Gains, n: u32, rgb: &mut Vec<u8>, quarter_turns: u8, mirror: bool) {
    conv.convert(frame, gains, n, rgb);
    let (mut out, w, h) = super::beam_colour::rotate_rgb(rgb, frame.w, frame.h, quarter_turns);
    if mirror {
        super::beam_colour::mirror_rgb(&mut out, w, h);
    }
    *SELF_PICTURE.lock().unwrap() = Some(Picture { w, h, frame_no: n, rgb: out, holes: None });
    PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
}

fn clear_self() {
    *SELF_PICTURE.lock().unwrap() = None;
    PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
}

/// The phone's self-view converter (its characterization arrives on the pipeline-up edge).
static ANDROID_SELF_CONV: Mutex<Option<Converter>> = Mutex::new(None);

/// One downscaled camera frame from the phone (I420 planes packed y‖u‖v) for the self-view — not the wire, just the inset.
pub fn android_self_frame(w: usize, h: usize, planes: &[u8]) {
    if w < 2 || h < 2 || planes.len() < w * h + 2 * (w / 2) * (h / 2) {
        return;
    }
    let mut f = h264::I420::new(w, h);
    let (cw, ch) = (w / 2, h / 2);
    f.y.copy_from_slice(&planes[..w * h]);
    f.u.copy_from_slice(&planes[w * h..w * h + cw * ch]);
    f.v.copy_from_slice(&planes[w * h + cw * ch..w * h + 2 * cw * ch]);
    let g = ANDROID_SELF_CONV.lock().unwrap();
    if let Some(conv) = g.as_ref() {
        let mut rgb = Vec::new();
        let o = ANDROID_ORIENT.load(Ordering::Relaxed);
        post_self(conv, &f, Gains::NONE, 0, &mut rgb, android_turns(o), o & 4 != 0);
    }
}

/// The phone's picture rotation: bits 0..1 = quarter turns clockwise (from the camera's mounting angle and facing combined with photon's own gravity snap — never Android's display rotation), bit 2 = a front camera (the self-view mirrors). Set by Kotlin on every change.
static ANDROID_ORIENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// The value last put on the wire, so a change goes out at once.
static ANDROID_ORIENT_SENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0xFF);
/// The phone's quarter turns as sent: Kotlin's mounting-and-gravity turns plus this lens's own turns from Settings → Beam (a phone held in a rig upside down), mod 4.
fn android_turns(orient: u8) -> u8 {
    let lens = ANDROID_CAMERA_NAME.lock().unwrap().clone().unwrap_or_default();
    ((orient & 3) + super::beam_settings::turns_for(&lens)) & 3
}

/// The phone's ladder, the camera timestamp (ns) of the last frame it kept, and the beam-settings generation it last read.
static ANDROID_LADDER: Mutex<Option<(super::beam_ladder::Ladder, i64, u32)>> = Mutex::new(None);

/// Per camera frame (Kotlin, before it touches the encoder): step the ladder with any new link verdict and say what to do with this frame — packed `keep | band << 1 | soft << 4 | kbit/s << 8 | coded width << 24 | coded height << 40` (14 bits of kbit/s, 16 of each dimension); −1 when no beam is running. A band change also re-describes the beam at once (the receiver's geometry).
pub fn android_plan(t_ns: i64) -> i64 {
    let mut g = ANDROID_LADDER.lock().unwrap();
    let Some((ladder, last_kept, seen)) = g.as_mut() else { return -1 };
    let before = ladder.plan();
    let (seq, congested, clean) = super::link_verdict();
    if follow_settings(ladder, seen) | ladder.verdict(seq, congested, clean) {
        let p = ladder.plan();
        if (p.w, p.h) != (before.w, before.h) {
            let (nw, nh) = (p.w, p.h);
            crate::logf!("BEAM: phone ladder → band {}×{} (soft {}, {} fps, {} kbit/s) — the encoder restarts at the new size", nw, nh, p.soft, p.fps, p.bitrate / 1000);
            if let Some((tx, info, _)) = ENCODED_TX.lock().unwrap().as_mut() {
                info.width = nw;
                info.height = nh;
                tx.send_info(info);
            }
        }
    }
    let p = ladder.plan();
    // Frame rate (below the smallest band): keep a frame once a period has passed since the last kept one — on the camera's own clock.
    let period = 1_000_000_000i64 / p.fps as i64; // the ladder floors fps at 5
    let keep = *last_kept == 0 || t_ns - *last_kept >= period - period / 8;
    if keep {
        *last_kept = t_ns;
    }
    pack_plan(keep, &p)
}

/// The packed plan Kotlin reads (see `android_plan`).
pub fn pack_plan(keep: bool, p: &super::beam_ladder::Plan) -> i64 {
    (keep as i64) | (((p.band as i64) & 7) << 1) | (((p.soft as i64) & 7) << 4) | ((((p.bitrate / 1000) as i64) & 0x3FFF) << 8) | ((p.w as i64) << 24) | ((p.h as i64) << 40)
}

pub fn android_orient(quarter_turns: u8, front: bool) {
    ANDROID_ORIENT.store((quarter_turns & 3) | if front { 4 } else { 0 }, Ordering::Relaxed);
}

/// A camera frame source for the desktop sender: blocks for the next frame.
pub trait FrameSource: Send {
    /// The frame geometry (even width and height).
    fn dimensions(&self) -> (usize, usize);
    fn fps(&self) -> u32;
    /// What this camera's samples are, for the beam's self-description.
    fn characterization(&self) -> vsf::spectral_image::ProfileEntry;
    /// The next frame as planar I420 plus the gains the camera applied (none for a UVC camera). `None` = the camera is gone.
    fn next(&mut self) -> Option<(h264::I420, Gains)>;
}

/// The receiving half: the beam-rx thread (transport) plus this decode thread.
pub struct Receiver {
    _rx: beam::RxHandle,
    slot: usize,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // The stop is an edge, not a poll: wake the decoder parked on the frame queue so the join returns at once (it used to wait out a 250 ms timeout at every hangup, on the thread hanging up).
        self._rx.shared.wake();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        *PICTURES[self.slot].lock().unwrap() = None;
        // The parked transport is the 1:1 wave's; a group link's receiver leaves it alone.
        if self.slot == 0 {
            *PARKED_TX.lock().unwrap() = None;
        }
        PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Start receiving the peer's beam for a wave. `we_are_origin` picks the chain: the peer sends in the other direction.
/// THE BEAM'S TRANSPORT OUTLIVES ITS CAMERA (field 2026-10-14, Emma's Samsung on v118: the beam turned off and on mid-wave, the restart built a NEW BeamTx whose step chain began again at 0, the receiver's chain had moved past it, and all 3,700 packets after the restart failed to open — the far side froze on the last picture). One transport per wave and direction, with its frame counter: parked when the camera stops, taken back when it starts again. Keyed by a one-way hash of the wave secret and our side, so a new wave never inherits it; dropped when a wave's receiver starts or stops (the wave's own edges).
/// The lens the phone is beaming from, as Kotlin names it ("Front", "Back · 24 mm"), for the self-view caption.
static ANDROID_CAMERA_NAME: Mutex<Option<String>> = Mutex::new(None);

static PARKED_TX: Mutex<Option<([u8; 32], BeamTx, u32)>> = Mutex::new(None);

fn tx_key(wave_secret: &[u8; 32], we_are_origin: bool) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key("photon beam transport park v0");
    h.update(wave_secret);
    h.update(&[we_are_origin as u8]);
    *h.finalize().as_bytes()
}

/// This wave's transport: the parked one if it is this wave's (its chain runs on), else a new one.
fn take_tx(wave_secret: &[u8; 32], we_are_origin: bool, peer: std::net::SocketAddr) -> (BeamTx, u32) {
    let key = tx_key(wave_secret, we_are_origin);
    if let Some((k, mut tx, n)) = PARKED_TX.lock().unwrap().take() {
        if k == key {
            tx.set_peer(peer);
            crate::logf!("BEAM: the wave's beam transport resumes at frame {} — its key chain runs on", n);
            return (tx, n);
        }
    }
    let dir = if we_are_origin { Direction::OriginToAnswer } else { Direction::AnswerToOrigin };
    (BeamTx::new(wave_secret, dir, peer), 0)
}

fn park_tx(key: [u8; 32], tx: BeamTx, frame_no: u32) {
    *PARKED_TX.lock().unwrap() = Some((key, tx, frame_no));
}

/// Who a beam sender sends to: the 1:1 wave's peer (its wave secret and our side), or every link of the group wave (the fan, docs/group-waves.md).
#[derive(Clone, Copy)]
pub enum Target {
    Pair([u8; 32], bool),
    Group,
}

/// GROUP BEAMS (docs/group-waves.md): one transport per live link of the group wave — its far slot, its BeamTx from the link's secret, and whether it has heard the beam's self-description yet.
/// The group wave keeps it as links come and go (`fan_add` / `fan_remove`), beaming or not, so a transport's chain runs from the link's start and a camera that stops and starts again never restarts it; a sender in group mode sends every frame to all of them.
static GROUP_FAN: Mutex<Vec<(usize, BeamTx, bool)>> = Mutex::new(Vec::new());
/// The group beam's frame counter, kept across a camera stopping and starting within the wave: a receiver gives up frame numbers behind what it has already shown, so a restart from 0 would be dropped until it passed them.
static GROUP_FRAME_NO: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A group link came up: its beam transport joins the fan.
pub fn fan_add(slot: usize, link_secret: &[u8; 32], we_are_origin: bool) {
    let dir = if we_are_origin { Direction::OriginToAnswer } else { Direction::AnswerToOrigin };
    let peer = super::slot_tx_addr(slot).unwrap_or(crate::network::status::RELAY_ADDR);
    let mut fan = GROUP_FAN.lock().unwrap();
    fan.retain(|(s, _, _)| *s != slot);
    fan.push((slot, BeamTx::new(link_secret, dir, peer), true));
}

/// A group link went down: its transport leaves the fan.
pub fn fan_remove(slot: usize) {
    GROUP_FAN.lock().unwrap().retain(|(s, _, _)| *s != slot);
}

/// The group wave ended: the whole fan goes.
pub fn fan_clear() {
    GROUP_FAN.lock().unwrap().clear();
    GROUP_FRAME_NO.store(0, Ordering::Relaxed);
}

/// One sender's way out: the 1:1 wave's single transport, or the group's fan (with the beam's latest self-description and rotation, which a link that joins mid-beam is sent first).
enum Outbound {
    Pair(BeamTx),
    Group { info: Option<Info>, orient: u8, sent: u64, fail: u64 },
}

impl Outbound {
    /// Follow the peers' current addresses (the wave's own re-points, address-follows-auth).
    fn follow(&mut self) {
        match self {
            Outbound::Pair(tx) => {
                if let Some(addr) = super::wave_tx_addr() {
                    if addr != tx.peer() {
                        tx.set_peer(addr);
                    }
                }
            }
            Outbound::Group { .. } => {
                for (slot, tx, _) in GROUP_FAN.lock().unwrap().iter_mut() {
                    if let Some(addr) = super::slot_tx_addr(*slot) {
                        if addr != tx.peer() {
                            tx.set_peer(addr);
                        }
                    }
                }
            }
        }
    }

    fn send_info(&mut self, i: &Info) {
        match self {
            Outbound::Pair(tx) => tx.send_info(i),
            Outbound::Group { info, .. } => {
                *info = Some(i.clone());
                for (_, tx, fresh) in GROUP_FAN.lock().unwrap().iter_mut() {
                    tx.send_info(i);
                    *fresh = false;
                }
            }
        }
    }

    fn send_orient(&mut self, q: u8) {
        match self {
            Outbound::Pair(tx) => tx.send_orient(q),
            Outbound::Group { orient, .. } => {
                *orient = q;
                for (_, tx, _) in GROUP_FAN.lock().unwrap().iter_mut() {
                    tx.send_orient(q);
                }
            }
        }
    }

    fn send_frame(&mut self, frame_no: u32, gains: Gains, au: &[u8]) {
        match self {
            Outbound::Pair(tx) => {
                tx.send_frame(frame_no, gains, au);
            }
            Outbound::Group { info, orient, sent, fail } => {
                for (_, tx, fresh) in GROUP_FAN.lock().unwrap().iter_mut() {
                    // A link that joined mid-beam hears what the beam is before its first frame.
                    if *fresh {
                        if let Some(i) = info.as_ref() {
                            tx.send_info(i);
                            tx.send_orient(*orient);
                            *fresh = false;
                        }
                    }
                    let (s0, f0) = (tx.sent, tx.send_fail);
                    tx.send_frame(frame_no, gains, au);
                    *sent += tx.sent - s0;
                    *fail += tx.send_fail - f0;
                }
            }
        }
    }

    /// Datagrams sent and refused so far.
    fn counts(&self) -> (u64, u64) {
        match self {
            Outbound::Pair(tx) => (tx.sent, tx.send_fail),
            Outbound::Group { sent, fail, .. } => (*sent, *fail),
        }
    }

    /// The way out for a target, and the frame number to continue from: the 1:1 wave's parked transport resumes its chain; the group's fan already runs.
    fn for_target(target: &Target) -> Result<(Outbound, u32, Option<[u8; 32]>), String> {
        match target {
            Target::Pair(secret, origin) => {
                let Some(peer) = super::wave_tx_addr() else {
                    return Err("BEAM: no peer address yet — the wave's first authenticated packet sets it".into());
                };
                let (tx, n) = take_tx(secret, *origin, peer);
                Ok((Outbound::Pair(tx), n, Some(tx_key(secret, *origin))))
            }
            Target::Group => Ok((Outbound::Group { info: None, orient: 0, sent: 0, fail: 0 }, GROUP_FRAME_NO.load(Ordering::Relaxed), None)),
        }
    }

    /// The sender stopped: a 1:1 transport parks under its key so a restart runs its chain on; the fan stays with the group wave.
    fn park(self, key: Option<[u8; 32]>, frame_no: u32) {
        match (self, key) {
            (Outbound::Pair(tx), Some(k)) => park_tx(k, tx, frame_no),
            (Outbound::Group { .. }, _) => GROUP_FRAME_NO.store(frame_no, Ordering::Relaxed),
            _ => {}
        }
    }
}

pub fn start_receiver(wave_secret: &[u8; 32], we_are_origin: bool) -> Receiver {
    *PARKED_TX.lock().unwrap() = None;
    start_receiver_slot(wave_secret, we_are_origin, 0)
}

/// The same for a group link (docs/group-waves.md): its own transport chain from the link's secret, its picture in the link's far slot.
pub fn start_receiver_slot(wave_secret: &[u8; 32], we_are_origin: bool, slot: usize) -> Receiver {
    let dir = if we_are_origin { Direction::AnswerToOrigin } else { Direction::OriginToAnswer };
    let rx = beam::start_rx(wave_secret, dir);
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let shared = rx.shared.clone();
    let thread = std::thread::Builder::new()
        .name("beam-decode".into())
        .spawn(move || decode_loop(stop2, shared, slot))
        .expect("beam-decode thread");
    Receiver { _rx: rx, slot, stop, thread: Some(thread) }
}

fn decode_loop(stop: Arc<AtomicBool>, shared: Arc<beam::RxShared>, slot: usize) {
    let mut decoder: Option<h264::Decoder> = None;
    let mut conv = Converter::new(Characterization::IDENTITY_GAMMA2);
    let mut rgb = Vec::new();
    let mut decoded = 0u64;
    let mut failed = 0u64;
    let mut skipped = 0u64;
    let mut holey = 0u64;
    let mut last_info: Option<Info> = None;
    // The reorder hold (see below): frames waiting for their predecessor, and the next frame number to decode.
    let mut pending: std::collections::BTreeMap<u32, beam::Frame> = std::collections::BTreeMap::new();
    let mut next: Option<u32> = None;
    const REORDER: u32 = 6;
    while !stop.load(Ordering::Relaxed) {
        let frames = shared.take_frames_wait(&stop);
        if frames.is_empty() {
            continue;
        }
        // The self-description edge: a changed characterization reconfigures the colour chain.
        if let Some(info) = shared.info() {
            if last_info.as_ref() != Some(&info) {
                if let Some(chr) = colour_from_bytes(&info.colour) {
                    conv.set_characterization(chr);
                }
                crate::logf!("BEAM: peer beam {}×{} @ {} fps, {}, {} colour bytes", info.width, info.height, info.fps, info.codec.name(), info.colour.len());
                last_info = Some(info);
            }
        }
        // EVERY FRAME, IN ORDER (field 2026-10-13, Emma's phone: decoding only the newest of a queued batch skipped a reference, and with no keyframes ever the decoder never recovered). Frames complete in fountain order, not encode order, so a short reorder hold: decode `next` the moment it is here; give up on it (a loss — concealment paints over it) once the queue runs REORDER frames past it. Only the newest decoded picture of a batch is converted for display.
        for f in frames {
            if next.is_none() {
                next = Some(f.frame_no);
            }
            if f.frame_no >= next.unwrap_or(0) {
                pending.insert(f.frame_no, f);
            }
        }
        let mut shown: Option<(h264::I420, Gains, u32)> = None;
        loop {
            let Some(n) = next else { break };
            let frame = if let Some(f) = pending.remove(&n) {
                f
            } else {
                match pending.keys().next().copied() {
                    Some(lowest) if pending.keys().next_back().copied().unwrap_or(lowest) >= n + REORDER => {
                        skipped += (lowest - n) as u64;
                        next = Some(lowest);
                        continue;
                    }
                    _ => break,
                }
            };
            next = Some(n.wrapping_add(1));
            if decoder.is_none() {
                match h264::Decoder::new() {
                    Ok(d) => decoder = Some(d),
                    Err(e) => {
                        crate::logf!("BEAM: {}", e);
                        continue;
                    }
                }
            }
            let Some(dec) = decoder.as_mut() else { continue };
            match dec.decode(&frame.bytes) {
                Ok(Some(pic)) => {
                    decoded += 1;
                    shown = Some((pic, frame.gains, frame.frame_no));
                }
                Ok(None) => {}
                Err(e) => {
                    failed += 1;
                    if failed <= 3 || failed % 100 == 0 {
                        crate::logf!("BEAM: decode failed ({} so far): {}", failed, e);
                    }
                }
            }
        }
        if let Some((pic, gains, n)) = shown {
            conv.convert(&pic, gains, n, &mut rgb);
            // The sender's rotation, applied here at display (the wire stays sensor-native).
            let rot = shared.rotation();
            let (out, w, h) = super::beam_colour::rotate_rgb(&rgb, pic.w, pic.h, rot);
            // THE SWIPE IN (Nick 2026-10-14: "the swipe to start is really nice… Nice and empty!"): what the intra refresh has not reached yet shows the screen's own background, not the decoder's green.
            let holes = pic.unpainted_mask().map(|m| super::beam_colour::rotate_px(&m, pic.w / 2, pic.h / 2, rot, 1).0);
            holey += holes.is_some() as u64;
            *PICTURES[slot].lock().unwrap() = Some(Picture { w, h, frame_no: n, rgb: out, holes });
            PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
        }
    }
    crate::logf!(
        "BEAM: decode thread down — {} pictures ({} concealed, {} shown with holes), {} failures, {} frames given up as lost",
        decoded,
        decoder.as_ref().map_or(0, |d| d.concealed),
        holey,
        failed,
        skipped
    );
}

/// The sending half.
pub struct Sender {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some((tx, _, n)) = ENCODED_TX.lock().unwrap().take() {
            tx.park(ENCODED_KEY.lock().unwrap().take(), n);
        }
    }
}

/// The ladder's ceiling for a capture's bands: the whole capture on a LAN-fast path (a round trip of 15 ms or less), else the highest band no wider than 1280 — the far side's software decoder is the limit off the LAN — and two bands lower again on a metered network.
fn ceiling(bands: &[(u16, u16)]) -> usize {
    let lan = super::LAST_LINK_RTT_MS.load(Ordering::Relaxed) <= 15;
    // WHY/PROOF: `bands` is the camera's mode list as the OS reports it, which can be empty — the ceiling is then rung 0, not a wrapped rung past the list.
    let mut top = if lan { bands.len().saturating_sub(1) } else { super::beam_ladder::top_within(bands, 1280) };
    if crate::network::wfd::net_metered() {
        // WHY/PROOF: the ladder's floor — a capture with fewer than three bands stays at rung 0 on a metered link, never wraps to a rung that does not exist.
        top = top.saturating_sub(2);
    }
    // Settings → Beams: the user's size cap lowers it further, never raises it.
    if let Some(cap) = super::beam_settings::current().size_cap {
        top = top.min(super::beam_ladder::top_within(bands, cap as usize));
    }
    top
}

/// A new ladder for a capture's bands, tuned to the beam settings; returns it with the settings generation it read.
fn new_ladder(bands: Vec<(u16, u16)>) -> (super::beam_ladder::Ladder, u32) {
    let seen = super::beam_settings::generation();
    let max_band = ceiling(&bands);
    let mut ladder = super::beam_ladder::Ladder::new(bands, max_band);
    ladder.set_tuning(super::beam_settings::current().tuning);
    (ladder, seen)
}

/// Settings → Beams, live: a newer generation retunes the ladder and moves its ceiling. True when the plan changed.
fn follow_settings(ladder: &mut super::beam_ladder::Ladder, seen: &mut u32) -> bool {
    let g = super::beam_settings::generation();
    if g == *seen {
        return false;
    }
    *seen = g;
    let before = ladder.plan();
    ladder.set_tuning(super::beam_settings::current().tuning);
    let max_band = ceiling(ladder.bands());
    ladder.set_max_band(max_band);
    let after = ladder.plan();
    crate::logf!("BEAM: settings changed — now {}×{} soft {} at {} fps, {} kbit/s", after.w, after.h, after.soft, after.fps, after.bitrate / 1000);
    after != before
}

/// Start sending a camera's frames (desktop): capture → H.264 → the beam track, toward the wave's current peer address (re-read every frame, so a redirect is followed).
pub fn start_sender(target: Target, mut source: Box<dyn FrameSource>, _bitrate_bps: u32) -> Result<Sender, String> {
    let (sw, sh) = source.dimensions();
    let cam_fps = source.fps().max(1);
    // THE LADDER (beam_ladder.rs): bands from the camera's own frame; the ceiling per `ceiling`.
    let bands = super::beam_ladder::bands_for(sw, sh);
    let (mut ladder, mut seen) = new_ladder(bands);
    let max_band = ceiling(ladder.bands());
    let mut plan = ladder.plan();
    let (bw, bh) = (plan.w, plan.h);
    let mut enc = h264::Encoder::new(bw as usize, bh as usize, cam_fps, plan.bitrate)?;
    let (mut tx, first_frame, key) = Outbound::for_target(&target)?;
    let self_conv = Converter::new(Characterization::from_entry(&source.characterization()));
    let colour = colour_bytes(&source.characterization());
    let mut info = Info { width: bw, height: bh, fps: cam_fps as u8, codec: Codec::H264, colour };
    let top = ladder.bands()[max_band];
    crate::logf!("BEAM: ladder up — camera {}×{}, ceiling {}×{}, starting {}×{} at {} kbit/s", sw, sh, top.0, top.1, bw, bh, plan.bitrate / 1000);
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("beam-send".into())
        .spawn(move || {
            let mut frame_no = first_frame;
            let mut cam_frames = 0u64;
            let mut rate_acc = 0u32;
            let mut sent_bytes = 0u64;
            let mut self_rgb = Vec::new();
            let mut coded = h264::I420::new(bw as usize, bh as usize);
            // This camera's turns (Settings → Beam), sent now and again on every change — a change bumps the settings generation, read on each frame's edge.
            let cam = camera_name().unwrap_or_default();
            let mut turns = super::beam_settings::turns_for(&cam);
            let mut turns_seen = super::beam_settings::generation();
            tx.send_orient(turns);
            while !stop2.load(Ordering::Relaxed) {
                let Some((frame, gains)) = source.next() else {
                    crate::log("BEAM: camera gone — sender stops");
                    break;
                };
                if super::beam_settings::generation() != turns_seen {
                    turns_seen = super::beam_settings::generation();
                    let t = super::beam_settings::turns_for(&cam);
                    if t != turns {
                        turns = t;
                        tx.send_orient(turns);
                        crate::logf!("BEAM: {} turned {} quarter turn(s)", cam, turns);
                    }
                }
                cam_frames += 1;
                tx.follow();
                // The self-view, every other camera frame — the camera as it is, whatever the ladder sends.
                if cam_frames % 2 == 0 {
                    post_self(&self_conv, &frame, gains, frame_no, &mut self_rgb, turns, true);
                }
                // The link verdict moves the ladder (an edge: a new verdict once a second, nothing in between).
                let (seq, congested, clean) = super::link_verdict();
                // Settings → Beams moves it too (a tap on the page, read on this frame's edge); `|` so both run.
                if follow_settings(&mut ladder, &mut seen) | ladder.verdict(seq, congested, clean) {
                    let next = ladder.plan();
                    if next.band != plan.band {
                        // A band crossing: the one place the coded size — and so a keyframe — changes.
                        let (nw, nh) = (next.w, next.h);
                        match h264::Encoder::new(nw as usize, nh as usize, cam_fps, next.bitrate) {
                            Ok(e) => {
                                enc = e;
                                coded = h264::I420::new(nw as usize, nh as usize);
                                info.width = nw;
                                info.height = nh;
                                tx.send_info(&info);
                                crate::logf!("BEAM: ladder → band {}×{} (soft {} half-stops, {} fps, {} kbit/s) — a keyframe for the new size", nw, nh, next.soft, next.fps, next.bitrate / 1000);
                            }
                            Err(e) => crate::logf!("BEAM: ladder band change failed: {}", e),
                        }
                    } else if next.bitrate != plan.bitrate {
                        enc.set_bitrate(next.bitrate);
                    }
                    plan = next;
                }
                // The plan's frame rate (the ladder's lever below the smallest band, and the user's cap): keep plan.fps of every cam_fps camera frames, spread evenly — 24 of 30 keeps four in five (free in H.264, no keyframe). A camera slower than the plan keeps every frame.
                rate_acc += plan.fps.min(cam_fps);
                if rate_acc < cam_fps {
                    continue;
                }
                rate_acc -= cam_fps;
                if frame_no % beam::INFO_EVERY_FRAMES == 0 {
                    tx.send_info(&info);
                    tx.send_orient(turns);
                }
                // The camera frame → the coded size at the ladder's softness (crop to 16:9, shrink, grow).
                let mut f = frame;
                let (fw, fh) = (f.w, f.h);
                let (cw, ch) = (coded.w, coded.h);
                let src = [
                    super::beam_scale::Plane::from_slice(&mut f.y, fw, 1, fw, fh),
                    super::beam_scale::Plane::from_slice(&mut f.u, fw / 2, 1, fw / 2, fh / 2),
                    super::beam_scale::Plane::from_slice(&mut f.v, fw / 2, 1, fw / 2, fh / 2),
                ];
                let dst = [
                    super::beam_scale::Plane::from_slice(&mut coded.y, cw, 1, cw, ch),
                    super::beam_scale::Plane::from_slice(&mut coded.u, cw / 2, 1, cw / 2, ch / 2),
                    super::beam_scale::Plane::from_slice(&mut coded.v, cw / 2, 1, cw / 2, ch / 2),
                ];
                super::beam_scale::resample_frame(src, dst, plan.soft);
                match enc.encode(&coded) {
                    Ok(au) if !au.is_empty() => {
                        sent_bytes += au.len() as u64;
                        tx.send_frame(frame_no, gains, &au);
                    }
                    Ok(_) => {}
                    Err(e) => crate::logf!("BEAM: encode: {}", e),
                }
                frame_no = frame_no.wrapping_add(1);
                if frame_no % (cam_fps * 10) == 0 {
                    let (sent, fail) = tx.counts();
                    crate::logf!("BEAM: sent {} frames, {} KB, {} datagrams ({} refused); ladder {}×{} soft {} @ {} fps", frame_no, sent_bytes / 1024, sent, fail, coded.w, coded.h, plan.soft, plan.fps);
                }
            }
            clear_self();
            tx.park(key, frame_no);
        })
        .map_err(|e| format!("beam-send thread: {e}"))?;
    Ok(Sender { stop, thread: Some(thread) })
}

/// The ENCODED-frame feed (Android: Kotlin's Camera2 + MediaCodec push access units here). Installed by [`start_encoded_sender`], read by [`push_encoded`].
static ENCODED_TX: Mutex<Option<(Outbound, Info, u32)>> = Mutex::new(None);
/// The parking key of the transport in ENCODED_TX (see PARKED_TX); `None` for a group beam, whose fan stays with the group wave.
static ENCODED_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// Start a sender that is fed encoded frames from outside (the phone). Returns the handle whose drop clears the feed.
pub fn start_encoded_sender(target: Target, info: Info) -> Result<Sender, String> {
    let (mut tx, first_frame, key) = Outbound::for_target(&target)?;
    tx.send_info(&info);
    *ENCODED_KEY.lock().unwrap() = key;
    *ENCODED_TX.lock().unwrap() = Some((tx, info, first_frame));
    Ok(Sender { stop: Arc::new(AtomicBool::new(false)), thread: None })
}

/// One encoded frame from the platform encoder. `gains` are what the ISP applied (Q12), `au` the H.264 access unit. Returns false when no encoded sender is up.
pub fn push_encoded(gains: Gains, au: &[u8]) -> bool {
    let mut g = ENCODED_TX.lock().unwrap();
    let Some((tx, info, frame_no)) = g.as_mut() else { return false };
    tx.follow();
    let o = android_turns(ANDROID_ORIENT.load(Ordering::Relaxed));
    let changed = ANDROID_ORIENT_SENT.swap(o, Ordering::Relaxed) != o;
    if *frame_no % beam::INFO_EVERY_FRAMES == 0 {
        tx.send_info(info);
    }
    // The rotation goes out the moment it changes, and with every self-description for a receiver that joined late.
    if changed || *frame_no % beam::INFO_EVERY_FRAMES == 0 {
        tx.send_orient(o);
    }
    tx.send_frame(*frame_no, gains, au);
    *frame_no = frame_no.wrapping_add(1);
    true
}

/// Is an encoded-frame sender up (the phone's camera pipeline asks before encoding anything)?
pub fn encoded_sender_live() -> bool {
    ENCODED_TX.lock().unwrap().is_some()
}

// ───────────────────────── the phone ─────────────────────────

/// The Beam button armed a send on the phone: the wave secret and our side, waiting for Kotlin's pipeline-up edge (`android_started`). Cleared by `android_stop`.
static ANDROID_ARMED: Mutex<Option<Target>> = Mutex::new(None);
/// The phone's encoded sender handle (the UI thread cannot own it: the pipeline-up edge arrives on the service thread).
static ANDROID_SENDER: Mutex<Option<Sender>> = Mutex::new(None);

/// Arm a phone send for the active wave and ask Kotlin to open the camera + encoder. Returns false when the service bridge is down.
#[cfg(target_os = "android")]
pub fn android_arm(target: Target) -> bool {
    *ANDROID_ARMED.lock().unwrap() = Some(target);
    crate::platform::jni_android::wave_service_void("startBeamCapture")
}

/// The pipeline-up edge from Kotlin: build the beam's self-description from what the camera is, and start the encoded sender the button armed. `camera` names the lens ("1", "0/3" — logical id / physical id), the key a calibration is kept under.
/// A CAMERA SWITCH mid-beam (Nick 2026-10-14: "tap and switch between the front and back cam, or the other three back cams… cycles thru all when I tap on my preview") arrives as this same edge while the sender is live: the transport stays — a new BeamTx would restart its key chain — and the ladder rebuilds from the new capture keeping the link's detail; the new size, colour and orientation go out at once, and the encoder's restart opens with the keyframe the new size needs.
pub fn android_started(w: usize, h: usize, fps: u32, maker: Option<[f32; 9]>, straight: bool, curve: bool, hdr: bool, camera: &str, camera_name: &str) -> Result<(), String> {
    *ANDROID_CAMERA_NAME.lock().unwrap() = Some(camera_name.to_string());
    let Some(target) = *ANDROID_ARMED.lock().unwrap() else {
        return Err("BEAM: pipeline up with nothing armed".into());
    };
    // A calibration made earlier this run for THIS lens (beam_calibrate.rs) outranks the maker's matrix — on the straight path only, where the frames are the colour it was solved from.
    let on_straight = straight && curve && !hdr;
    super::beam_calibrate::set_camera(camera, on_straight);
    let calibrated = super::beam_calibrate::calibration().filter(|_| on_straight);
    let entry = if hdr { hdr_entry() } else { calibrated.as_ref().map_or_else(|| phone_entry(maker, straight, curve), |(e, _)| e.clone()) };
    *ANDROID_SELF_CONV.lock().unwrap() = Some(Converter::new(Characterization::from_entry(&entry)));
    let colour = colour_bytes_cal(&entry, calibrated.map(|(_, c)| c));
    // THE LADDER (beam_ladder.rs): bands from the capture itself (Kotlin picked it: ≤ ~5 MP, ≤ 30 fps, least cropped), the ceiling per `ceiling`.
    let bands = super::beam_ladder::bands_for(w, h);
    let max_band = ceiling(&bands);
    let top = bands[max_band];
    let mut lg = ANDROID_LADDER.lock().unwrap();
    if let (Some((ladder, last_kept, _)), Some((tx, info, _))) = (lg.as_mut(), ENCODED_TX.lock().unwrap().as_mut()) {
        ladder.rebuild(bands, max_band);
        *last_kept = 0;
        let p = ladder.plan();
        info.width = p.w;
        info.height = p.h;
        info.fps = fps.min(255) as u8;
        info.colour = colour;
        tx.send_info(info);
        ANDROID_ORIENT_SENT.store(0xFF, Ordering::Relaxed);
        crate::logf!("BEAM: camera switched to {} — capture {}×{} @ {} fps, ceiling {}×{}, now {}×{} (detail kept)", camera, w, h, fps, top.0, top.1, p.w, p.h);
        return Ok(());
    }
    let (ladder, seen) = new_ladder(bands);
    let p = ladder.plan();
    crate::logf!("BEAM: phone ladder up — camera {}, capture {}×{} @ {} fps, ceiling {}×{}, starting {}×{}", camera, w, h, fps, top.0, top.1, p.w, p.h);
    *lg = Some((ladder, 0, seen));
    drop(lg);
    let info = Info { width: p.w, height: p.h, fps: fps.min(255) as u8, codec: Codec::H264, colour };
    let s = start_encoded_sender(target, info)?;
    *ANDROID_SENDER.lock().unwrap() = Some(s);
    Ok(())
}

/// The self-view was tapped: the next camera (Kotlin cycles front → each back lens by field of view → front).
#[cfg(target_os = "android")]
pub fn android_cycle_camera() -> bool {
    android_sending() && crate::platform::jni_android::wave_service_void("cycleBeamCamera")
}

/// Stop a phone send: the sender handle, the arm, and Kotlin's pipeline.
pub fn android_stop() {
    *ANDROID_SENDER.lock().unwrap() = None;
    *ANDROID_ARMED.lock().unwrap() = None;
    *ANDROID_SELF_CONV.lock().unwrap() = None;
    *ANDROID_LADDER.lock().unwrap() = None;
    super::beam_calibrate::set_straight(false);
    ANDROID_ORIENT_SENT.store(0xFF, Ordering::Relaxed);
    clear_self();
    #[cfg(target_os = "android")]
    crate::platform::jni_android::wave_service_void("stopBeamCapture");
}

/// A calibration just landed (beam_calibrate.rs): the self-view and the beam's self-description switch to it at once — the receiver reconfigures on the info edge.
pub fn android_apply_calibration(entry: &vsf::spectral_image::ProfileEntry, cal: vsf::spectral_image::CalProvenance) {
    if let Some(conv) = ANDROID_SELF_CONV.lock().unwrap().as_mut() {
        conv.set_characterization(Characterization::from_entry(entry));
    }
    if let Some((tx, info, _)) = ENCODED_TX.lock().unwrap().as_mut() {
        info.colour = colour_bytes_cal(entry, Some(cal));
        tx.send_info(info);
    }
}

pub fn android_sending() -> bool {
    ANDROID_ARMED.lock().unwrap().is_some()
}

/// The phone's characterization (docs/beams.md): with the maker's XYZ→camera matrix AND the straight-thru request in force, `absolute` / `model` / `gamma2` with camera→VSF-RGB = XYZ→VSF-RGB × inv(XYZ→camera); otherwise `creative` / `assumed` / `gamma2` (the HAL white-balanced and matrixed toward sRGB under our γ2 curve), the sRGB→VSF-RGB matrix.
/// `curve` = our γ2 tonemap curve took; without it the camera's own curve is in force, treated as sRGB. Off the straight path the camera white-balances and colour-corrects toward sRGB itself (the fallback, 2026-10-13 — never raw, unbalanced sensor colour), so the entry is the assumed sRGB one at whichever transfer the frames really carry.
pub fn phone_entry(maker: Option<[f32; 9]>, straight: bool, curve: bool) -> vsf::spectral_image::ProfileEntry {
    if let (Some(m), true) = (maker, straight && curve) {
        if let Some(inv) = invert3(&m) {
            return vsf::spectral_image::ProfileEntry {
                matrix: mat_mul3(&super::beam_colour::vsf_rows(vsf::colour::XYZ2VSF_RGB), &inv),
                source: "dng_colormatrix2".into(),
                class: vsf::spectral_image::IdtClass::Absolute,
                tier: vsf::spectral_image::ProfileTier::Model,
                illuminant: 21, // D65 (EXIF LightSource), the daylight set
                transfer: vsf::spectral_image::Transfer::Gamma2,
                scale: None,
            };
        }
    }
    vsf::spectral_image::ProfileEntry {
        matrix: super::beam_colour::vsf_rows(vsf::colour::SRGB2VSF_RGB),
        source: "assumed_srgb".into(),
        class: vsf::spectral_image::IdtClass::Creative,
        tier: vsf::spectral_image::ProfileTier::Assumed,
        illuminant: 0,
        transfer: if curve { vsf::spectral_image::Transfer::Gamma2 } else { vsf::spectral_image::Transfer::Srgb },
        scale: None,
    }
}

pub(super) fn invert3(m: &[f32; 9]) -> Option<[f32; 9]> {
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    Some([
        (m[4] * m[8] - m[5] * m[7]) / det,
        (m[2] * m[7] - m[1] * m[8]) / det,
        (m[1] * m[5] - m[2] * m[4]) / det,
        (m[5] * m[6] - m[3] * m[8]) / det,
        (m[0] * m[8] - m[2] * m[6]) / det,
        (m[2] * m[3] - m[0] * m[5]) / det,
        (m[3] * m[7] - m[4] * m[6]) / det,
        (m[1] * m[6] - m[0] * m[7]) / det,
        (m[0] * m[4] - m[1] * m[3]) / det,
    ])
}

pub(super) fn mat_mul3(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut o = [0f32; 9];
    for r in 0..3 {
        for c in 0..3 {
            o[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    o
}

// ───────────────────────── the colour entry on the wire ─────────────────────────

const COLOUR_SECTION: &str = "colour_profile";

/// The camera's characterization entry as a one-section VSF document — VSF's own `colour_profile` codec, so the beam's colour label is the same bytes a still would carry.
pub fn colour_bytes(entry: &vsf::spectral_image::ProfileEntry) -> Vec<u8> {
    colour_bytes_cal(entry, None)
}

/// The same, carrying the calibration's provenance (which target, when it was measured) beside a chameleon entry.
pub fn colour_bytes_cal(entry: &vsf::spectral_image::ProfileEntry, cal: Option<vsf::spectral_image::CalProvenance>) -> Vec<u8> {
    let profile = vsf::spectral_image::ColourProfile {
        target: "vsf_rgb".into(),
        entries: vec![entry.clone()],
        dng_colormatrix: [None, None],
        patches: None,
        cal,
    };
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section(COLOUR_SECTION, vsf::spectral_image::profile_fields(&profile))
        .build()
        .unwrap_or_default()
}

/// Read the entry back; `None` = no or unreadable colour bytes (the receiver keeps identity + γ2).
pub fn colour_from_bytes(bytes: &[u8]) -> Option<Characterization> {
    if bytes.is_empty() {
        return None;
    }
    // The un-skippable front door (docs/vsf-trust-remediation.md): the whole document verified, the section located by its TOC name, the fields parsed against a schema. Non-strict, so a profile that grows a field still reads.
    use vsf::schema::TypeConstraint::Any;
    let schema = vsf::schema::SectionSchema::new(COLOUR_SECTION)
        .field("count", Any)
        .field("target", Any)
        .field("matrices", Any)
        .field("sources", Any)
        .field("classes", Any)
        .field("tiers", Any)
        .field("illuminants", Any)
        .field("transfers", Any);
    let section = vsf::schema::SectionBuilder::parse_document(schema, bytes, None).ok()?;
    let mut fields: Vec<vsf::file_format::VsfField> = Vec::new();
    for name in ["count", "target", "matrices", "sources", "classes", "tiers", "illuminants", "transfers"] {
        for f in section.get_fields(name) {
            fields.push(vsf::file_format::VsfField { name: f.name.clone(), values: f.values.clone() });
        }
    }
    let profile = vsf::spectral_image::profile_from_fields(&fields).ok()?;
    profile.entries.first().map(Characterization::from_entry)
}

/// An HDR beam's entry (docs/beams.md "HDR"): the camera's own colour carried to BT.2020 by its HLG10 profile, so `creative` / `assumed` with the Rec.2020→VSF-RGB matrix, and the samples `g2t` (beam_hdr.rs converts HLG to it on the phone) — the receiver keeps their light past white.
pub fn hdr_entry() -> vsf::spectral_image::ProfileEntry {
    vsf::spectral_image::ProfileEntry {
        matrix: super::beam_colour::vsf_rows(vsf::colour::REC2020_2VSF_RGB),
        source: "camera_hlg10".into(),
        class: vsf::spectral_image::IdtClass::Creative,
        tier: vsf::spectral_image::ProfileTier::Assumed,
        illuminant: 0,
        transfer: vsf::spectral_image::Transfer::Gamma2Tanh,
        scale: None,
    }
}

/// The entry for a camera we know nothing about (a UVC webcam, a Mac): `creative` / `assumed` / `srgb`, the sRGB→VSF-RGB matrix.
pub fn assumed_srgb_entry() -> vsf::spectral_image::ProfileEntry {
    vsf::spectral_image::ProfileEntry {
        matrix: super::beam_colour::vsf_rows(vsf::colour::SRGB2VSF_RGB),
        source: "assumed_srgb".into(),
        class: vsf::spectral_image::IdtClass::Creative,
        tier: vsf::spectral_image::ProfileTier::Assumed,
        illuminant: 0,
        transfer: vsf::spectral_image::Transfer::Srgb,
        scale: None,
    }
}

/// The desktop camera the user picked (Nick 2026-10-17: "cycle thru cameras on all platforms"), by its platform id — a V4L2 path or an AVFoundation uniqueID. `None` = the first / system default.
static DESKTOP_CAMERA: Mutex<Option<String>> = Mutex::new(None);

/// The cameras as last enumerated: the render reads this every frame, and enumerating opens every /dev/video node, so it refreshes on edges only (a beam starting, a switch).
static DESKTOP_CAMS: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

/// Every camera this desktop can beam from, as (id, name) — the cached list.
pub fn desktop_cameras() -> Vec<(String, String)> {
    let mut cached = DESKTOP_CAMS.lock().unwrap();
    if cached.is_none() {
        *cached = Some(enumerate_desktop_cameras());
    }
    cached.clone().unwrap_or_default()
}

/// Forget the cached list: the next read enumerates again (a camera plugged in or out since).
pub fn refresh_desktop_cameras() {
    *DESKTOP_CAMS.lock().unwrap() = None;
}

/// Enumerate now and swap the list in whole (the camera worker): the render never finds the cache empty, so it never enumerates on the UI thread.
pub fn reenumerate_desktop_cameras() {
    let list = enumerate_desktop_cameras();
    *DESKTOP_CAMS.lock().unwrap() = Some(list);
}

fn enumerate_desktop_cameras() -> Vec<(String, String)> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        crate::platform::camera_v4l2::list()
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::camera_avf::list()
    }
    #[cfg(not(any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos")))]
    {
        Vec::new()
    }
}

pub fn desktop_camera_pick() -> Option<String> {
    DESKTOP_CAMERA.lock().unwrap().clone()
}

pub fn set_desktop_camera_pick(id: Option<String>) {
    *DESKTOP_CAMERA.lock().unwrap() = id;
}

/// Advance the pick to the next camera (wrapping), returning the new pick — `None` when there is nothing to cycle to.
pub fn next_desktop_camera() -> Option<String> {
    refresh_desktop_cameras();
    let cams = desktop_cameras();
    if cams.len() < 2 {
        return None;
    }
    let cur = desktop_camera_pick();
    let at = cur.as_ref().and_then(|c| cams.iter().position(|(id, _)| id == c)).unwrap_or(0);
    let next = cams[(at + 1) % cams.len()].0.clone();
    set_desktop_camera_pick(Some(next.clone()));
    Some(next)
}

/// The name of the camera this device is beaming from (desktop: the pick, else the first listed; phone: the lens Kotlin opened).
pub fn camera_name() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        ANDROID_CAMERA_NAME.lock().unwrap().clone()
    }
    #[cfg(not(target_os = "android"))]
    {
        let cams = desktop_cameras();
        let pick = desktop_camera_pick();
        pick.as_ref().and_then(|p| cams.iter().find(|(id, _)| id == p)).or(cams.first()).map(|(_, n)| n.clone())
    }
}

/// The desktop's camera, if the platform has one we can open: the user's pick, else the first one.
pub fn open_desktop_camera() -> Result<Box<dyn FrameSource>, String> {
    let pick = desktop_camera_pick();
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        crate::platform::camera_v4l2::open(pick.as_deref()).map(|c| Box::new(c) as Box<dyn FrameSource>)
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::camera_avf::open(pick.as_deref()).map(|c| Box::new(c) as Box<dyn FrameSource>)
    }
    #[cfg(not(any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos")))]
    {
        Err("BEAM: no camera path on this platform yet (docs/beams.md stage 5)".into())
    }
}

/// THE CAMERA OPENS OFF THE UI THREAD (field 2026-10-21, Nick's MacBook: `PERF: drain beam_armed took 1572ms` — AVCaptureSession's startRunning and the wait for the first frame ran inside the wave tick).
/// A worker enumerates, opens the picked camera and starts the sender; the wave tick drains the answer from the returned channel.
/// The worker first waits for any sender still stopping (`retire`), so a restart takes back the parked transport and the key chain runs on.
#[cfg(not(target_os = "android"))]
pub fn start_desktop_async(target: Target) -> std::sync::mpsc::Receiver<Result<Sender, String>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new().name("beam-open".into()).spawn(move || {
        await_retired();
        // The camera the preview already holds, when a beam we placed rang with it open; else open it now.
        let opened = match take_preview_source() {
            Some(s) => Ok(s),
            None => {
                reenumerate_desktop_cameras();
                open_desktop_camera()
            }
        };
        let started = opened.and_then(|source| {
            let (w, h) = source.dimensions();
            let s = start_sender(target, source, bitrate_for(w, h))?;
            crate::logf!("BEAM: sending {}×{} from {}", w, h, camera_name().unwrap_or_default());
            Ok(s)
        });
        // A receiver already gone hands the sender back here, so its camera closes on this thread.
        let _ = tx.send(started);
    });
    if let Err(e) = spawned {
        crate::logf!("BEAM: no camera thread: {}", e);
    }
    rx
}


/// THE PREVIEW (Nick 2026-10-31: "Should show your video before it connects, seems to only start the cam after the other party beams back"): a beam we place opens the camera at once and shows the self-view while it rings; nothing is encoded or sent until the wave is live and its secret exists.
/// The sender then takes the SAME open camera (`take_preview_source`), so the far side's first frame does not wait on a second open.
#[cfg(not(target_os = "android"))]
struct Preview {
    stop: Arc<AtomicBool>,
    /// The capture thread; it hands back the open camera when stopped, or nothing when the camera never opened or went away.
    thread: std::thread::JoinHandle<Option<Box<dyn FrameSource>>>,
}

#[cfg(not(target_os = "android"))]
static PREVIEW: Mutex<Option<Preview>> = Mutex::new(None);

/// Open the camera for the self-view only, on its own thread (the UI thread never opens a camera); a preview already running is left alone.
#[cfg(not(target_os = "android"))]
pub fn preview_start() {
    let mut slot = PREVIEW.lock().unwrap();
    // A preview whose camera never opened (a Mac still asking for it) has finished: a later start tries again.
    if slot.as_ref().is_some_and(|p| !p.thread.is_finished()) {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let spawned = std::thread::Builder::new().name("beam-preview".into()).spawn(move || {
        await_retired();
        reenumerate_desktop_cameras();
        let mut source = match open_desktop_camera() {
            Ok(s) => s,
            Err(e) => {
                crate::logf!("BEAM: preview — {}", e);
                return None;
            }
        };
        let (w, h) = source.dimensions();
        crate::logf!("BEAM: preview {}×{} from {} while the beam rings", w, h, camera_name().unwrap_or_default());
        let conv = Converter::new(Characterization::from_entry(&source.characterization()));
        let mut rgb = Vec::new();
        let mut n = 0u32;
        while !stop2.load(Ordering::Relaxed) {
            let Some((frame, gains)) = source.next() else {
                crate::log("BEAM: preview — camera gone");
                clear_self();
                return None;
            };
            n = n.wrapping_add(1);
            // Every other camera frame, as the sender's self-view does.
            if n % 2 == 0 {
                post_self(&conv, &frame, gains, n, &mut rgb, super::beam_settings::turns_for(&camera_name().unwrap_or_default()), true);
            }
        }
        Some(source)
    });
    match spawned {
        Ok(thread) => *slot = Some(Preview { stop, thread }),
        Err(e) => crate::logf!("BEAM: no preview thread: {}", e),
    }
}

/// Stop the preview without a sender to hand the camera to (the beam ended before it went live): closed off the UI thread, the self-view cleared.
#[cfg(not(target_os = "android"))]
pub fn preview_stop() {
    let Some(p) = PREVIEW.lock().unwrap().take() else {
        return;
    };
    p.stop.store(true, Ordering::Relaxed);
    retire_with(move || {
        drop(p.thread.join());
        clear_self();
    });
}

/// The camera a running preview holds, for the sender (a worker thread: the join waits at most one camera frame, or out a camera still opening).
#[cfg(not(target_os = "android"))]
fn take_preview_source() -> Option<Box<dyn FrameSource>> {
    let p = PREVIEW.lock().unwrap().take()?;
    p.stop.store(true, Ordering::Relaxed);
    p.thread.join().ok().flatten()
}

/// A phone's camera opens with its sender (Kotlin's capture pipeline); there is no desktop preview to start or stop.
#[cfg(target_os = "android")]
pub fn preview_start() {}
#[cfg(target_os = "android")]
pub fn preview_stop() {}
/// The stopping that runs off the UI thread, in order: each retirement joins the one before it, and a camera open waits for the last.
static RETIRING: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

fn retire_with(f: impl FnOnce() + Send + 'static) {
    let prev = RETIRING.lock().unwrap().take();
    let spawned = std::thread::Builder::new().name("beam-retire".into()).spawn(move || {
        if let Some(p) = prev {
            let _ = p.join();
        }
        f();
    });
    match spawned {
        Ok(h) => *RETIRING.lock().unwrap() = Some(h),
        Err(e) => crate::logf!("BEAM: no retire thread ({}) — stopped in place", e),
    }
}

/// Stop a sender off the UI thread (field 2026-10-21, Nick's MacBook: the hangup's `drain wave_express took 292ms` — the drop joins the capture thread, and a Mac's camera stops inside it); its join parks the transport.
pub fn retire(s: Sender) {
    retire_with(move || drop(s));
}

/// Cancel a camera still opening: wait for the worker's answer off the UI thread and stop whatever it started.
pub fn retire_opening(rx: std::sync::mpsc::Receiver<Result<Sender, String>>) {
    retire_with(move || drop(rx.recv()));
}

/// The camera worker's first step: every stopping sender has parked its transport.
fn await_retired() {
    let h = RETIRING.lock().unwrap().take();
    if let Some(h) = h {
        let _ = h.join();
    }
}

/// Can this device SEND a beam at all: a codec and a camera path.
pub fn can_send() -> bool {
    if cfg!(target_os = "android") {
        return true;
    }
    h264::AVAILABLE && (cfg!(target_os = "linux") || cfg!(target_os = "macos"))
}

/// Is the camera ready to open now (not waiting on the system prompt)? The armed-beam edge waits for the user's answer instead of re-asking every tick.
pub fn camera_ready() -> bool {
    crate::platform::mic_permission::camera_status() != crate::platform::mic_permission::MicAccess::Undetermined
}

/// Ask the system for the camera — once per run (field 2026-10-14, Nick's MacBook on v118: Beam back armed the beam, the fresh binary's camera grant was undetermined, and the armed beam waited for an answer to a question nobody had asked — the Mac never sent). The answer's edge flips `camera_ready` and the armed beam goes out.
pub fn ask_camera_once() {
    static ASKED: AtomicBool = AtomicBool::new(false);
    if !ASKED.swap(true, Ordering::Relaxed) {
        crate::log("BEAM: asking for the camera (the grant is undetermined)");
        crate::platform::mic_permission::request_camera();
    }
}

/// Ask at the first beam intent — the Beam slat, Beam back, the in-wave Beam button — so the system prompt is up while the wave connects (field 2026-10-21, Nick's MacBook: asked 0.7 s into the live wave, granted 6.4 s later, the beam waited on it).
/// Nothing where no prompt exists: `camera_ready` is true there.
pub fn ask_camera_early() {
    // Android's grant lives with the service, which prompts only when it is missing.
    #[cfg(target_os = "android")]
    {
        let _ = crate::platform::jni_android::wave_service_void("askCameraEarly");
        return;
    }
    #[cfg(not(target_os = "android"))]
    if !camera_ready() {
        ask_camera_once();
    }
}

/// The sender's bitrate for a frame size, from the ladder in docs/beams.md: ~600 kbit/s at 640×480, scaled by area.
pub fn bitrate_for(w: usize, h: usize) -> u32 {
    let area = (w * h) as u64;
    // WHY/PROOF: the size is the camera's (OS) — the ladder's bitrate floor and ceiling from docs/beams.md hold a thumbnail-sized or 8K capture to a rate the link and the far decoder can carry.
    ((600_000u64 * area) / (640 * 480)).clamp(150_000, 2_000_000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// White in, white out (field 2026-10-14: the matrices were read transposed and an `assumed` beam's white came out green). Every row of every entry's matrix maps the source white to VSF white [1, 1, 1], and the receive chain keeps a neutral grey neutral.
    #[test]
    /// Colorimetric, no chromatic adaptation (vsf 02076e6; Nick 2026-10-20, option a): sRGB white is D65 light, which sits bluer than VSF white (Illuminant E), and reaches a Rec.2020 display as D65 thru the 1931 observer, not quite the display's own D65 thru the 10° fundamentals — about 1.4 % red, −0.4 % green, +2.9 % blue in linear light. That cast is sRGB's 1931 definition showing, and the reason VSF RGB is the connection space; nothing here neutralises it.
    fn grey_keeps_its_light_thru_every_entry_and_the_chain() {
        let rows = |m: &[f32; 9]| [m[0] + m[1] + m[2], m[3] + m[4] + m[5], m[6] + m[7] + m[8]];
        for (name, e) in [("assumed sRGB", assumed_srgb_entry()), ("phone fallback", phone_entry(None, false, false))] {
            let [r, g, b] = rows(&e.matrix);
            assert!(b > g && g > r, "{name}: D65 white lands bluer than VSF white, got {r} {g} {b}");
        }
        // XYZ→camera = the XYZ white (Illuminant E, [1,1,1]) onto camera [1,1,1]: camera white must land on VSF white.
        let e = phone_entry(Some([1., 0., 0., 0., 1., 0., 0., 0., 1.]), true, true);
        for v in rows(&e.matrix) {
            assert!((v - 1.0).abs() < 2e-3, "absolute entry maps camera white to {v}");
        }
        let conv = super::super::beam_colour::Converter::new(Characterization::from_entry(&assumed_srgb_entry()));
        let mut grey = h264::I420::new(64, 64);
        grey.y.iter_mut().for_each(|v| *v = 128);
        let mut rgb = Vec::new();
        conv.convert(&grey, Gains::NONE, 0, &mut rgb);
        // Each channel's mean LIGHT, decoded from the g2t codes.
        let mean = |c: usize| rgb.iter().skip(c).step_by(3).map(|&v| fluor::theme::G2T_LIGHT_Q16[v as usize] as f64).sum::<f64>() / (64.0 * 64.0);
        let (r, g, b) = (mean(0) as f32, mean(1) as f32, mean(2) as f32);
        // Expected: sRGB white thru VSF RGB onto Rec.2020, as ratios of light.
        let w = vsf::colour::convert::apply_matrix_3x3_f32(&vsf::colour::VSF_RGB2REC2020, &vsf::colour::convert::apply_matrix_3x3_f32(&vsf::colour::SRGB2VSF_RGB, &[1., 1., 1.]));
        let (er, eb) = (w[0] / w[1], w[2] / w[1]);
        assert!((r / g - er).abs() < 0.015 && (b / g - eb).abs() < 0.015, "grey keeps D65's light: got {r} {g} {b}, want r/g {er} b/g {eb}");
    }

    #[test]
    fn the_colour_entry_round_trips_as_vsf() {
        let e = assumed_srgb_entry();
        let bytes = colour_bytes(&e);
        assert!(!bytes.is_empty());
        let chr = colour_from_bytes(&bytes).expect("reads back");
        assert_eq!(chr.transfer, super::super::beam_colour::Transfer::Srgb);
        assert_eq!(chr.matrix, super::super::beam_colour::vsf_rows(vsf::colour::SRGB2VSF_RGB));
        assert!(colour_from_bytes(&[]).is_none());
        assert!(colour_from_bytes(b"not a vsf document").is_none());
    }

    /// An HDR beam says `g2t` on the wire and its receiver reads it as such (an older receiver that lacks the word refuses the entry, never misreads it).
    #[test]
    fn the_hdr_entry_round_trips_as_g2t() {
        let chr = colour_from_bytes(&colour_bytes(&hdr_entry())).expect("reads back");
        assert_eq!(chr.transfer, super::super::beam_colour::Transfer::G2t);
        assert_eq!(chr.matrix, super::super::beam_colour::vsf_rows(vsf::colour::REC2020_2VSF_RGB));
    }

    #[test]
    fn the_phone_entry_is_absolute_only_with_a_matrix_and_a_straight_request() {
        let e = phone_entry(None, true, true);
        assert_eq!(e.class, vsf::spectral_image::IdtClass::Creative);
        let e = phone_entry(Some([1., 0., 0., 0., 1., 0., 0., 0., 1.]), false, true);
        assert_eq!(e.class, vsf::spectral_image::IdtClass::Creative);
        let e = phone_entry(Some([2., 0., 0., 0., 2., 0., 0., 0., 2.]), true, true);
        assert_eq!(e.class, vsf::spectral_image::IdtClass::Absolute);
        // XYZ→camera = 2·I, so camera→VSF = XYZ2VSF × ½·I.
        for i in 0..9 {
            assert!((e.matrix[i] - super::super::beam_colour::vsf_rows(vsf::colour::XYZ2VSF_RGB)[i] * 0.5).abs() < 1e-6);
        }
        assert!(invert3(&[0.; 9]).is_none());
        // Off the straight path with the camera's own curve, the label says sRGB transfer — never γ2 it does not carry.
        assert_eq!(phone_entry(None, false, false).transfer, vsf::spectral_image::Transfer::Srgb);
    }

    #[test]
    fn bitrate_scales_with_area_inside_the_ladder() {
        assert_eq!(bitrate_for(640, 480), 600_000);
        assert_eq!(bitrate_for(320, 240), 150_000);
        assert_eq!(bitrate_for(1280, 720), 1_800_000);
    }
}
