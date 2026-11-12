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

/// One picture per far party (platform::audio::PartyTable): slot 0 is the 1:1 wave's peer, a group wave's links show theirs in their own slots.
static PICTURES: crate::platform::audio::PartyTable<Mutex<Option<Picture>>> = crate::platform::audio::PartyTable::new(no_picture);

fn no_picture() -> Mutex<Option<Picture>> {
    Mutex::new(None)
}
/// Bumped on every new picture: the paint compares it with what it last drew.
pub static PICTURE_GEN: AtomicU64 = AtomicU64::new(0);

/// The newest picture the peer's beam produced, if a beam is flowing.
pub fn picture() -> Option<Picture> {
    picture_slot(0)
}

/// The newest picture one far slot's beam produced (a group link's peer).
pub fn picture_slot(slot: usize) -> Option<Picture> {
    PICTURES.at(slot).lock().unwrap().clone()
}

/// Is any beam picture up (theirs or our own inset)? The wave screen's switch to the beam layout, without cloning a frame.
pub fn showing() -> bool {
    PICTURES.rows().iter().any(|p| p.lock().unwrap().is_some()) || SELF_PICTURE.lock().unwrap().is_some()
}

/// THE SELF-VIEW (2026-10-10, Nick: "when you choose beam, still no video shows"): what this device's camera is sending, converted thru the same chain the receiver runs — so the inset shows the colour the far side gets, tungsten-yellow included.
static SELF_PICTURE: Mutex<Option<Picture>> = Mutex::new(None);

pub fn self_picture() -> Option<Picture> {
    SELF_PICTURE.lock().unwrap().clone()
}

/// THE BEAM'S NUMBERS for the wave screen (Nick 2026-10-31: "show cam and mic stats for wave and beam"): what the camera delivers and what goes out, measured by the sender; what arrives, measured by the pair's decoder. Each is a window's average, published at the window's end and cleared when its thread stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendStats {
    /// The camera's frame as captured, and the frames per second it actually delivered.
    pub cam_w: usize,
    pub cam_h: usize,
    pub cam_fps: u32,
    /// The coded size on the wire, the frames per second sent, and the bits per second they took.
    pub sent_w: usize,
    pub sent_h: usize,
    pub sent_fps: u32,
    pub kbps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecvStats {
    pub w: usize,
    pub h: usize,
    pub fps: u32,
}

static SEND_STATS: Mutex<Option<SendStats>> = Mutex::new(None);
static RECV_STATS: Mutex<Option<RecvStats>> = Mutex::new(None);

pub fn send_stats() -> Option<SendStats> {
    *SEND_STATS.lock().unwrap()
}

pub fn recv_stats() -> Option<RecvStats> {
    *RECV_STATS.lock().unwrap()
}

/// Events per second over `ms` milliseconds, rounded.
fn per_second(n: u64, ms: u64) -> u32 {
    // WHY/PROOF: a window can close inside one millisecond — a camera's queued buffers all hand over at once after a stall — and a zero divisor would panic the thread that measured it; one millisecond is the clock's own resolution here.
    let ms = ms.max(1);
    ((n * 1000 + ms / 2) / ms) as u32
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

/// The phone's ladder: the ladder, the camera timestamp (ns) of the last frame it kept, the beam-settings generation it last read, and the watch over its links.
struct PhoneLadder {
    ladder: super::beam_ladder::Ladder,
    last_kept: i64,
    seen: u32,
    watch: LinkWatch,
}

static ANDROID_LADDER: Mutex<Option<PhoneLadder>> = Mutex::new(None);

/// Per camera frame (Kotlin, before it touches the encoder): step the ladder with any new link verdict and say what to do with this frame — packed `keep | band << 1 | soft << 4 | kbit/s << 8 | coded width << 24 | coded height << 40` (14 bits of kbit/s, 16 of each dimension); −1 when no beam is running. A band change also re-describes the beam at once (the receiver's geometry).
pub fn android_plan(t_ns: i64) -> i64 {
    let mut g = ANDROID_LADDER.lock().unwrap();
    let Some(PhoneLadder { ladder, last_kept, seen, watch }) = g.as_mut() else { return -1 };
    let before = ladder.plan();
    // Lock order as everywhere: the ladder, then the feed.
    let takes = ENCODED_TX.lock().unwrap().as_ref().map(|f| f.out.takes()).unwrap_or_default();
    if follow_settings(ladder, seen) | steer(ladder, watch, &takes) {
        let p = ladder.plan();
        if (p.w, p.h) != (before.w, before.h) {
            let (nw, nh) = (p.w, p.h);
            crate::logf!("BEAM: phone ladder → band {}×{} (soft {}, {} fps, {} kbit/s) — the encoder restarts at the new size", nw, nh, p.soft, p.fps, p.bitrate / 1000);
            if let Some(feed) = ENCODED_TX.lock().unwrap().as_mut() {
                feed.info.width = nw;
                feed.info.height = nh;
                feed.out.send_info(&feed.info);
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
    /// Let the next frame go by unconverted: the sender keeps fewer frames than the camera delivers, and a frame neither encoded nor shown is not worth decoding. False = the camera is gone.
    /// A source with nothing cheaper to do converts it and drops it.
    fn skip(&mut self) -> bool {
        self.next().is_some()
    }
}

/// The receiving half: the beam-rx thread (transport) plus this decode thread.
pub struct Receiver {
    _rx: beam::RxHandle,
    slot: usize,
    back: Back,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.slot == 0 {
            *RECV_STATS.lock().unwrap() = None;
        }
        // The stop is an edge, not a poll: wake the decoder parked on the frame queue so the join returns at once (it used to wait out a 250 ms timeout at every hangup, on the thread hanging up).
        self._rx.shared.wake();
        if let Some(t) = self.thread.take() {
            // A panicked thread's Err is the panic the hook already logged; the join is for the ordering.
            drop(t.join());
        }
        *PICTURES.at(self.slot).lock().unwrap() = None;
        // The 1:1 wave's transport ends with the wave; a group link's receiver leaves it alone (its link's transport is the fan's).
        if let Back::Pair(key) = self.back {
            let mut g = PAIR_TX.lock().unwrap();
            if g.as_ref().is_some_and(|(k, _, _)| *k == key) {
                *g = None;
            }
        }
        PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// The lens the phone is beaming from, as Kotlin names it ("Front", "Back · 24 mm"), for the self-view caption.
static ANDROID_CAMERA_NAME: Mutex<Option<String>> = Mutex::new(None);

/// THE 1:1 WAVE'S BEAM TRANSPORT, our direction, with its frame counter: up with the wave's receiver and down with it, beaming or not, as a group link's transport lives in the fan.
/// THE BEAM'S TRANSPORT OUTLIVES ITS CAMERA (field 2026-10-14, Emma's Samsung on v118: the beam turned off and on mid-wave, the restart built a NEW BeamTx whose step chain began again at 0, the receiver's chain had moved past it, and all 3,700 packets after the restart failed to open — the far side froze on the last picture): a sender borrows this one and leaves its frame counter here when it stops.
/// It is up for the whole wave because a RECEIVER sends on it too: its asks for a picture to start from ride our direction of the link whether or not we beam (2026-11-11).
/// Keyed by a one-way hash of the wave secret and our side, so a new wave never inherits it.
static PAIR_TX: Mutex<Option<([u8; 32], BeamTx, u32)>> = Mutex::new(None);

fn tx_key(wave_secret: &[u8; 32], we_are_origin: bool) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key("photon beam transport park v0");
    h.update(wave_secret);
    h.update(&[we_are_origin as u8]);
    *h.finalize().as_bytes()
}

/// This wave's transport, made if it is not up: the one already up keeps its chain (a second BeamTx under the same key would reuse its nonces).
/// Returns the frame number a sender continues from.
fn ensure_pair_tx(wave_secret: &[u8; 32], we_are_origin: bool) -> u32 {
    let key = tx_key(wave_secret, we_are_origin);
    let mut g = PAIR_TX.lock().unwrap();
    if let Some((k, _, n)) = g.as_ref() {
        if *k == key {
            return *n;
        }
    }
    let dir = if we_are_origin { Direction::OriginToAnswer } else { Direction::AnswerToOrigin };
    let peer = super::wave_tx_addr().unwrap_or(crate::network::status::RELAY_ADDR);
    *g = Some((key, BeamTx::new(wave_secret, dir, peer), 0));
    0
}

/// Run `f` on the 1:1 wave's transport, aimed at the wave's current peer address, if the wave keyed `key` is the one up.
fn with_pair<R>(key: &[u8; 32], f: impl FnOnce(&mut BeamTx) -> R) -> Option<R> {
    let mut g = PAIR_TX.lock().unwrap();
    match g.as_mut() {
        Some((k, tx, _)) if k == key => {
            if let Some(addr) = super::wave_tx_addr() {
                if addr != tx.peer() {
                    tx.set_peer(addr);
                }
            }
            Some(f(tx))
        }
        _ => None,
    }
}

/// Which link a receiver's asks go back on: the 1:1 wave's transport (its key), or a group link's in the fan (its far slot).
#[derive(Clone, Copy)]
enum Back {
    Pair([u8; 32]),
    Group(usize),
}

/// A receiver asks its sender for a picture to start from, on its own direction of the link (beam.rs `Kind::KeyAsk`).
fn send_key_ask(back: Back, held: u32) {
    match back {
        Back::Pair(key) => {
            with_pair(&key, |tx| tx.send_key_ask(held));
        }
        Back::Group(slot) => {
            if let Some((_, tx, _)) = GROUP_FAN.lock().unwrap().iter_mut().find(|(s, _, _)| *s == slot) {
                if let Some(addr) = super::slot_tx_addr(slot) {
                    if addr != tx.peer() {
                        tx.set_peer(addr);
                    }
                }
                tx.send_key_ask(held);
            }
        }
    }
}

/// A RECEIVER'S NEED for a picture to start from (Nick 2026-11-11: "if they need one, I'd send it now, not in 2 minutes"): a frame the decoder could not make a clean picture of — no parameter sets yet (a late join), or a lost reference it is still concealing — and the first such frame asks at once.
/// A receiver still stuck asks again only after [`KEY_ASK_AGAIN`] more such frames (the answer was lost, or is still crossing a long round trip); a clean picture ends the need.
#[derive(Debug, Default)]
struct KeyNeed {
    stuck: u32,
}

/// Stuck frames between two asks from a receiver that stays stuck: a second at 30 fps, longer than any round trip a beam survives.
const KEY_ASK_AGAIN: u32 = 1 << 5;

impl KeyNeed {
    /// One more stuck frame: true when it should ask.
    fn stuck(&mut self) -> bool {
        let ask = self.stuck % KEY_ASK_AGAIN == 0;
        self.stuck += 1;
        ask
    }

    fn pictured(&mut self) {
        self.stuck = 0;
    }
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

/// Our beam's self-description as last sent (Info, quarter turns) — what a newly armed video spool writes before our first frame in it.
static TX_DESC: Mutex<(Option<Info>, u8)> = Mutex::new((None, 0));
/// The video spool generation our track last described itself to (spool::beam_gen).
static TX_SPOOL_GEN: AtomicU64 = AtomicU64::new(0);
/// The group beam's frame counter, kept across a camera stopping and starting within the wave: a receiver gives up frame numbers behind what it has already shown, so a restart from 0 would be dropped until it passed them.
static GROUP_FRAME_NO: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A group link came up: its beam transport joins the fan.
pub fn fan_add(slot: usize, link_secret: &[u8; 32], we_are_origin: bool) {
    let dir = if we_are_origin { Direction::OriginToAnswer } else { Direction::AnswerToOrigin };
    let peer = super::slot_tx_addr(slot).unwrap_or(crate::network::status::RELAY_ADDR);
    let mut fan = GROUP_FAN.lock().unwrap();
    fan.retain(|(s, _, _)| *s != slot);
    fan.push((slot, BeamTx::new(link_secret, dir, peer), true));
    // A link added mid-beam starts decoding at once: the next frame carries the parameter sets (a desktop's keyframe, a phone's sets ahead of its rolling refresh) instead of waiting for the newcomer to fail and ask.
    // With no beam going out this asks nothing: a sender clears asks when it starts, its first frame being a starting picture anyway.
    super::beam::ask_key_now();
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
    /// The 1:1 wave's transport in PAIR_TX, by its key.
    Pair([u8; 32]),
    Group { info: Option<Info>, orient: u8, sent: u64, fail: u64 },
}

impl Outbound {
    /// Follow the peers' current addresses (the wave's own re-points, address-follows-auth).
    fn follow(&mut self) {
        match self {
            // `with_pair` aims the transport at the wave's current address on every use.
            Outbound::Pair(_) => {}
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
        TX_DESC.lock().unwrap().0 = Some(i.clone());
        if crate::wave::spool::beam_gen() != 0 {
            crate::wave::spool::beam_record(crate::wave::spool::BeamRecKind::Info, 0, &beam::encode_info(i));
        }
        match self {
            Outbound::Pair(key) => {
                with_pair(key, |tx| tx.send_info(i));
            }
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
        TX_DESC.lock().unwrap().1 = q;
        if crate::wave::spool::beam_gen() != 0 {
            crate::wave::spool::beam_record(crate::wave::spool::BeamRecKind::Orient, 0, &[q]);
        }
        match self {
            Outbound::Pair(key) => {
                with_pair(key, |tx| tx.send_orient(q));
            }
            Outbound::Group { orient, .. } => {
                *orient = q;
                for (_, tx, _) in GROUP_FAN.lock().unwrap().iter_mut() {
                    tx.send_orient(q);
                }
            }
        }
    }

    fn send_frame(&mut self, frame_no: u32, gains: Gains, au: &[u8]) {
        // SAVING BEAM VIDEO (opt-in, wave/spool.rs): our track 0 — its description first in each new recording, then every access unit as encoded (one encode, whatever the fan-out).
        let gen = crate::wave::spool::beam_gen();
        if gen != 0 {
            if TX_SPOOL_GEN.swap(gen, Ordering::Relaxed) != gen {
                let (info, q) = TX_DESC.lock().unwrap().clone();
                if let Some(i) = info {
                    crate::wave::spool::beam_record(crate::wave::spool::BeamRecKind::Info, 0, &beam::encode_info(&i));
                }
                crate::wave::spool::beam_record(crate::wave::spool::BeamRecKind::Orient, 0, &[q]);
            }
            crate::wave::spool::beam_record(crate::wave::spool::BeamRecKind::Frame, 0, &crate::wave::spool::beam_frame_payload(frame_no, gains.0, au));
        }
        match self {
            Outbound::Pair(key) => {
                with_pair(key, |tx| tx.send_frame(frame_no, gains, au));
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
            Outbound::Pair(key) => with_pair(key, |tx| (tx.sent, tx.send_fail)).unwrap_or((0, 0)),
            Outbound::Group { sent, fail, .. } => (*sent, *fail),
        }
    }

    /// Every link this beam goes out on, as the link watch takes it: its far slot, its engine's reading, and the beam bytes sent to it so far.
    fn takes(&self) -> Vec<LinkTake> {
        match self {
            Outbound::Pair(key) => with_pair(key, |tx| tx.bytes).map(|bytes| vec![LinkTake { slot: 0, now: super::link_reading(0), bytes }]).unwrap_or_default(),
            Outbound::Group { .. } => GROUP_FAN.lock().unwrap().iter().map(|(slot, tx, _)| LinkTake { slot: *slot, now: super::link_reading(*slot), bytes: tx.bytes }).collect(),
        }
    }

    /// The way out for a target, and the frame number to continue from: the 1:1 wave's transport runs on from where the last sender left it; the group's fan already runs.
    fn for_target(target: &Target) -> Result<(Outbound, u32, Option<[u8; 32]>), String> {
        match target {
            Target::Pair(secret, origin) => {
                if super::wave_tx_addr().is_none() {
                    return Err("BEAM: no peer address yet — the wave's first authenticated packet sets it".into());
                }
                let n = ensure_pair_tx(secret, *origin);
                if n != 0 {
                    crate::logf!("BEAM: the wave's beam transport resumes at frame {} — its key chain runs on", n);
                }
                let key = tx_key(secret, *origin);
                Ok((Outbound::Pair(key), n, Some(key)))
            }
            Target::Group => Ok((Outbound::Group { info: None, orient: 0, sent: 0, fail: 0 }, GROUP_FRAME_NO.load(Ordering::Relaxed), None)),
        }
    }

    /// The sender stopped: the 1:1 transport keeps the frame counter so a restart runs its chain on; the fan stays with the group wave.
    fn park(self, key: Option<[u8; 32]>, frame_no: u32) {
        match (self, key) {
            (Outbound::Pair(_), Some(k)) => {
                if let Some((pk, _, n)) = PAIR_TX.lock().unwrap().as_mut() {
                    if *pk == k {
                        *n = frame_no;
                    }
                }
            }
            (Outbound::Group { .. }, _) => GROUP_FRAME_NO.store(frame_no, Ordering::Relaxed),
            _ => {}
        }
    }
}

pub fn start_receiver(wave_secret: &[u8; 32], we_are_origin: bool) -> Receiver {
    // The wave's transport comes up with its receiver: a receiver's asks ride it before this end ever beams.
    ensure_pair_tx(wave_secret, we_are_origin);
    start_receiver_with(wave_secret, we_are_origin, 0, Back::Pair(tx_key(wave_secret, we_are_origin)))
}

/// The same for a group link (docs/group-waves.md): its own transport chain from the link's secret, its picture in the link's far slot.
pub fn start_receiver_slot(wave_secret: &[u8; 32], we_are_origin: bool, slot: usize) -> Receiver {
    start_receiver_with(wave_secret, we_are_origin, slot, Back::Group(slot))
}

fn start_receiver_with(wave_secret: &[u8; 32], we_are_origin: bool, slot: usize, back: Back) -> Receiver {
    let dir = if we_are_origin { Direction::AnswerToOrigin } else { Direction::OriginToAnswer };
    let rx = beam::start_rx(wave_secret, dir);
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let shared = rx.shared.clone();
    let thread = std::thread::Builder::new()
        .name("beam-decode".into())
        .spawn(move || decode_loop(stop2, shared, slot, back))
        .expect("beam-decode thread");
    Receiver { _rx: rx, slot, back, stop, thread: Some(thread) }
}

fn decode_loop(stop: Arc<AtomicBool>, shared: Arc<beam::RxShared>, slot: usize, back: Back) {
    let mut need = KeyNeed::default();
    let mut asks = 0u64;
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
    // SAVING BEAM VIDEO (opt-in, wave/spool.rs): this receive slot's track (1 + slot; a slot past the u8 track space is not saved), the spool generation it last described itself to, and the rotation it last wrote.
    let track = u8::try_from(slot + 1).ok();
    let (mut spool_gen, mut spool_rot) = (0u64, u8::MAX);
    // The pair's receive stats (slot 0): frames decoded since the window opened (counted from `decoded` at its start); it closes every RECV_WINDOW frames.
    const RECV_WINDOW: u64 = 1 << 5;
    let (mut win_t, mut win_base) = (std::time::Instant::now(), 0u64);
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
                if let (Some(t), true) = (track, crate::wave::spool::beam_gen() != 0) {
                    crate::wave::spool::beam_record(crate::wave::spool::BeamRecKind::Info, t, &beam::encode_info(&info));
                }
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
            // The access unit as received, in order, before decode — with the track's description first in each new recording and every rotation change.
            let gen = crate::wave::spool::beam_gen();
            if let (Some(t), true) = (track, gen != 0) {
                use crate::wave::spool::{beam_record, BeamRecKind};
                if spool_gen != gen {
                    spool_gen = gen;
                    spool_rot = u8::MAX;
                    if let Some(i) = shared.info() {
                        beam_record(BeamRecKind::Info, t, &beam::encode_info(&i));
                    }
                }
                let rot = shared.rotation();
                if rot != spool_rot {
                    spool_rot = rot;
                    beam_record(BeamRecKind::Orient, t, &[rot]);
                }
                beam_record(BeamRecKind::Frame, t, &crate::wave::spool::beam_frame_payload(frame.frame_no, frame.gains.0, &frame.bytes));
            }
            let Some(dec) = decoder.as_mut() else { continue };
            let concealed_before = dec.concealed;
            // A frame the decoder could not make a clean picture of: none at all (no parameter sets yet, a reference it never had) or one painted over a lost reference.
            let stuck = match dec.decode(&frame.bytes) {
                Ok(Some(pic)) => {
                    decoded += 1;
                    shown = Some((pic, frame.gains, frame.frame_no));
                    dec.concealed != concealed_before
                }
                Ok(None) => false,
                Err(e) => {
                    failed += 1;
                    if failed <= 3 || failed % 100 == 0 {
                        crate::logf!("BEAM: decode failed ({} so far): {}", failed, e);
                    }
                    true
                }
            };
            if !stuck {
                need.pictured();
            } else if need.stuck() {
                send_key_ask(back, frame.frame_no);
                asks += 1;
                if asks <= 3 || asks % 16 == 0 {
                    crate::logf!("BEAM: asked the sender for a picture to start from at frame {} ({} asks so far)", frame.frame_no, asks);
                }
            }
        }
        if let Some((pic, gains, n)) = shown {
            if slot == 0 && decoded - win_base >= RECV_WINDOW {
                *RECV_STATS.lock().unwrap() = Some(RecvStats { w: pic.w, h: pic.h, fps: per_second(decoded - win_base, win_t.elapsed().as_millis() as u64) });
                (win_t, win_base) = (std::time::Instant::now(), decoded);
            }
            conv.convert(&pic, gains, n, &mut rgb);
            // The sender's rotation, applied here at display (the wire stays sensor-native).
            let rot = shared.rotation();
            let (out, w, h) = super::beam_colour::rotate_rgb(&rgb, pic.w, pic.h, rot);
            // THE SWIPE IN (Nick 2026-10-14: "the swipe to start is really nice… Nice and empty!"): what the intra refresh has not reached yet shows the screen's own background, not the decoder's green.
            let holes = pic.unpainted_mask().map(|m| super::beam_colour::rotate_px(&m, pic.w / 2, pic.h / 2, rot, 1).0);
            holey += holes.is_some() as u64;
            *PICTURES.at(slot).lock().unwrap() = Some(Picture { w, h, frame_no: n, rgb: out, holes });
            PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
        }
    }
    crate::logf!(
        "BEAM: decode thread down — {} pictures ({} concealed, {} shown with holes), {} failures, {} frames given up as lost, {} asks for a picture to start from",
        decoded,
        decoder.as_ref().map_or(0, |d| d.concealed),
        holey,
        failed,
        skipped,
        asks
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
            // A panicked thread's Err is the panic the hook already logged; the join is for the ordering.
            drop(t.join());
        }
        if let Some(feed) = ENCODED_TX.lock().unwrap().take() {
            feed.out.park(ENCODED_KEY.lock().unwrap().take(), feed.frame_no);
        }
    }
}

// ───────────────────────── the ceiling and the link watch ─────────────────────────

/// THE NETWORK'S PART OF THE CEILING (Nick 2026-11-11: "same network isn't a decision, it's a latency and bandwidth question"): the widest coded frame the live links have shown they carry, from each link's measured round trip and achieved throughput, re-read on every one of its verdicts ([`LinkWatch`]).
/// It replaced a LAN boolean (a round trip of 15 ms or less, read once at the sender's start from the previous wave's figure — 0 for a fresh wave, so a phone on cellular got 1920×1080 on 2026-11-11).
/// Reset to [`CONSERVATIVE_W`] whenever a sender starts.
static NET_W: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(CONSERVATIVE_W as u32);
/// Until a link has measured anything (a round trip and a clean second of beam on it), the beam stays within this width: 640 for a 2560 or 1280 capture, 960 for 1920.
const CONSERVATIVE_W: usize = 1 << 10;
/// The far side's decoder, not the network (2026-11-01, Nick and Emma: on one LAN the beam reached the whole 2560×1920 capture at 7.4 Mbit/s and Emma's phone decoded about ten frames a second): no beam wider than this, the old LAN ceiling kept as a decoder's.
/// The receiver stating its own decode ceiling is the real answer and needs a wire field; until then the far side is assumed to be a phone.
const FAR_DECODE_W: usize = 1920;
/// A clean second counts as throughput the link carried only while the queue on it stays under this (the round trip's average over its floor, ms): a link already holding a queue is not carrying with room to spare, however clean its loss looks.
const QUEUE_OK_MS: u32 = 1 << 5;
/// A link's verdicts before it counts (its first second carries its start: the connect probe, the first round trips, and no beam byte count to measure against).
const SUSTAINED_VERDICTS: u32 = 1 << 1;
/// How far over what a link has carried the beam may reach to enter a band, in quarters: 2¾×, about a stop and a half — the entry rung of the band above costs twice the top of the band below, so a link that carried that top (or up to about a quarter under it, for an encoder that undershoots) opens the next band and no further.
const HEADROOM_Q2: u64 = 11;

/// One link as the watch takes it on a frame edge: its far slot, its engine's reading, and the beam bytes sent to it so far.
#[derive(Debug, Clone, Copy)]
struct LinkTake {
    slot: usize,
    now: super::LinkNow,
    bytes: u64,
}

/// One link as the watch keeps it.
#[derive(Debug, Clone)]
struct Watched {
    slot: usize,
    epoch: u32,
    /// The last verdict taken, and how many have been.
    seq: u32,
    verdicts: u32,
    /// The beam bytes sent to the link, and the watch's clock (ms), at that verdict.
    bytes: u64,
    at_ms: u64,
    /// The highest rate (bit/s) the link carried through a clean second with no queue to speak of; 0 = none yet. A congested second caps it at the rate that congested.
    carried_bps: u64,
    /// This link has given a verdict in the current round, and all of its verdicts in the round were clean.
    in_round: bool,
    round_clean: bool,
}

/// CONGESTION ON EVERY LINK (Nick 2026-11-11): the ladder hears every live link, not only the lead, because the mesh sends each frame on each link and the uplink they share fills from all of them.
/// Any sustained link congested steps the beam down at once; a round in which every sustained link spoke and all were clean counts as one clean verdict, so two links climb no faster than one.
/// The same watch measures each link's achieved throughput for the ceiling ([`LinkWatch::net_top`]); the weakest link sets it.
#[derive(Debug, Default)]
struct LinkWatch {
    links: Vec<Watched>,
    seq: u32,
}

impl LinkWatch {
    fn new() -> Self {
        LinkWatch { links: Vec::new(), seq: 0 }
    }

    /// Every live link's reading at `now_ms` (any monotonic clock in ms): the combined verdict `(sequence, congested, clean)` when one lands, for `Ladder::verdict`.
    fn step(&mut self, now_ms: u64, takes: &[LinkTake]) -> Option<(u32, bool, bool)> {
        // A link that left the beam (or whose engine ended, the slot perhaps carrying a new link) leaves the watch.
        self.links.retain(|l| takes.iter().any(|t| t.slot == l.slot && t.now.epoch == l.epoch));
        let mut congested = false;
        for t in takes {
            if t.now.seq == 0 {
                continue;
            }
            let Some(l) = self.links.iter_mut().find(|l| l.slot == t.slot) else {
                // First sight: whatever the link said before, the watch has no byte count from its start, so its next verdict is its first measured second.
                self.links.push(Watched { slot: t.slot, epoch: t.now.epoch, seq: t.now.seq, verdicts: 1, bytes: t.bytes, at_ms: now_ms, carried_bps: 0, in_round: false, round_clean: true });
                continue;
            };
            if l.seq == t.now.seq {
                continue;
            }
            // WHY/PROOF: per_second's case — a camera's queued buffers hand over at once after a stall, so two frame edges with a verdict between them can land inside one millisecond, and a zero divisor would panic the sending thread; one millisecond is the clock's resolution.
            let ms = (now_ms - l.at_ms).max(1);
            // The link's transport only counts up while its epoch holds (a new link in the slot is a new epoch and a new entry).
            let rate = (t.bytes - l.bytes) * 8 * 1000 / ms;
            l.seq = t.now.seq;
            l.bytes = t.bytes;
            l.at_ms = now_ms;
            l.verdicts += 1;
            // Carried: the best clean second's rate, a running maximum; a congested second says the link could not take the rate it was given, so the carried rate never stands above it.
            if t.now.clean && t.now.rtt_ms != 0 && t.now.queue_ms <= QUEUE_OK_MS {
                l.carried_bps = l.carried_bps.max(rate);
            }
            if t.now.congested {
                l.carried_bps = l.carried_bps.min(rate);
            }
            if l.verdicts >= SUSTAINED_VERDICTS {
                congested |= t.now.congested;
                l.in_round = true;
                l.round_clean &= t.now.clean;
            }
        }
        let sustained: Vec<&Watched> = self.links.iter().filter(|l| l.verdicts >= SUSTAINED_VERDICTS).collect();
        let (c, k) = if congested {
            (true, false)
        } else if !sustained.is_empty() && sustained.iter().all(|l| l.in_round) {
            (false, sustained.iter().all(|l| l.round_clean))
        } else {
            return None;
        };
        for l in self.links.iter_mut() {
            l.in_round = false;
            l.round_clean = true;
        }
        // The ladder ignores a repeated sequence and starts from 0, so the first verdict is 1.
        self.seq += 1;
        Some((self.seq, c, k))
    }

    /// The highest band of `ladder`'s the weakest sustained link carries (the lowest of their tops: the mesh sends every frame to every link); [`CONSERVATIVE_W`] until one has measured anything.
    fn net_top(&self, ladder: &super::beam_ladder::Ladder) -> usize {
        self.links.iter().filter(|l| l.verdicts >= SUSTAINED_VERDICTS).map(|l| link_top(ladder, l.carried_bps)).min().unwrap_or_else(|| super::beam_ladder::top_within(ladder.bands(), CONSERVATIVE_W))
    }
}

/// One link's top band: [`CONSERVATIVE_W`] until it has carried anything, else the highest band whose entry rung costs no more than [`HEADROOM_Q2`] quarters of what it carried.
fn link_top(ladder: &super::beam_ladder::Ladder, carried_bps: u64) -> usize {
    let bands = ladder.bands();
    if carried_bps == 0 {
        return super::beam_ladder::top_within(bands, CONSERVATIVE_W);
    }
    let budget = carried_bps * HEADROOM_Q2 >> 2;
    // The smallest band is always open; each band above costs more to enter than the one below, so the first that does not fit ends the climb.
    let mut top = 0;
    for b in 1..bands.len() {
        if ladder.entry_bitrate(b) as u64 > budget {
            break;
        }
        top = b;
    }
    top
}

/// The watch's clock: milliseconds since the first read, monotonic.
fn now_ms() -> u64 {
    static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    BASE.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// One frame edge's steering: the link watch's combined verdict moves the ladder, and the links' measured ceiling moves its top. True when the plan changed.
fn steer(ladder: &mut super::beam_ladder::Ladder, watch: &mut LinkWatch, takes: &[LinkTake]) -> bool {
    let before = ladder.plan();
    let verdict = watch.step(now_ms(), takes);
    // WHY the band held is kept: the network's part of the ceiling only gates CLIMBING — stepping down is the ladder's answer to the per-link verdicts, with its one-stop overlap — so a link that just joined or a second that congested never forces a band crossing (and a keyframe for everyone) from here.
    let hold = watch.net_top(ladder).max(before.band);
    let w = ladder.bands()[hold].0 as u32;
    if NET_W.swap(w, Ordering::Relaxed) != w {
        ladder.set_max_band(ceiling(ladder.bands()));
        let top = ladder.bands()[ceiling(ladder.bands())];
        crate::logf!("BEAM: the links carry up to {} wide — ceiling now {}×{}", w, top.0, top.1);
    }
    if let Some((seq, congested, clean)) = verdict {
        ladder.verdict(seq, congested, clean);
    }
    ladder.plan() != before
}

/// The ladder's ceiling for a capture's bands: the narrower of what the links carry ([`NET_W`]) and the far decoder ([`FAR_DECODE_W`]), two bands lower again on a metered network, and never past the user's size cap or the phone encoder's own.
fn ceiling(bands: &[(u16, u16)]) -> usize {
    // Two ceilings compose: the lower one binds.
    let mut top = super::beam_ladder::top_within(bands, (NET_W.load(Ordering::Relaxed) as usize).min(FAR_DECODE_W));
    if crate::network::wfd::net_metered() {
        // WHY/PROOF: the ladder's floor — a capture with fewer than three bands stays at rung 0 on a metered link, never wraps to a rung that does not exist.
        top = top.saturating_sub(2);
    }
    // Settings → Beams: the user's size cap lowers it further, never raises it.
    if let Some(cap) = super::beam_settings::current().size_cap {
        top = top.min(super::beam_ladder::top_within(bands, cap as usize));
    }
    // The phone's encoder: no wider than it can actually keep up with (ENCODER_CAP_W).
    let enc = ENCODER_CAP_W.load(Ordering::Relaxed);
    if enc != u32::MAX {
        top = top.min(super::beam_ladder::top_within(bands, enc as usize));
    }
    top
}

/// THE PHONE ENCODER'S CEILING (Emma's phone 2026-10-31, c2.qti.avc.encoder: at 2592×1944 it took 95 frames and then offered no input slot again — the camera ran, nothing was encoded, the far side froze, and a camera switch revived it only until the ladder climbed back). The widest coded frame this run may ask of the encoder: lowered by the encoder's own capability report at pipeline-up and by every stall (`android_encoder_stalled`); u32::MAX until either speaks. Kept for the run — the encoder does not get faster.
static ENCODER_CAP_W: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// Kotlin's capability report: the widest band of this capture the encoder says it can encode at the capture's rate.
pub fn android_encoder_cap(width: u32) {
    let was = ENCODER_CAP_W.fetch_min(width, Ordering::Relaxed);
    if width < was {
        crate::logf!("BEAM: the phone's encoder supports at most {} wide at this rate — the ladder stops there", width);
        android_reclamp();
    }
}

/// The encoder stopped taking frames: the ceiling drops below the band it hung on, and the ladder moves down so Kotlin restarts the encoder at the smaller size. False when it is already on the smallest band (Kotlin then restarts it where it is).
pub fn android_encoder_stalled() -> bool {
    let mut g = ANDROID_LADDER.lock().unwrap();
    let Some(PhoneLadder { ladder, .. }) = g.as_mut() else { return false };
    let p = ladder.plan();
    if p.band == 0 {
        crate::logf!("BEAM: the phone's encoder stalled at {}×{}, the smallest band — restarting it there", p.w, p.h);
        return false;
    }
    let below = ladder.bands()[p.band - 1].0 as u32;
    ENCODER_CAP_W.fetch_min(below, Ordering::Relaxed);
    let top = ceiling(ladder.bands());
    ladder.set_max_band(top);
    let q = ladder.plan();
    crate::logf!("BEAM: the phone's encoder stalled at {}×{} — ceiling now {} wide, restarting at {}×{}", p.w, p.h, below, q.w, q.h);
    // The new size goes out now (android_plan announces only the changes it makes itself); same lock order as android_plan: the ladder, then the feed.
    if let Some(feed) = ENCODED_TX.lock().unwrap().as_mut() {
        feed.info.width = q.w;
        feed.info.height = q.h;
        feed.out.send_info(&feed.info);
    }
    true
}

/// Re-apply the ceiling to a live phone ladder.
fn android_reclamp() {
    if let Some(PhoneLadder { ladder, .. }) = ANDROID_LADDER.lock().unwrap().as_mut() {
        let top = ceiling(ladder.bands());
        ladder.set_max_band(top);
    }
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

/// The self-view inset's width target: a desktop frame is shrunk by whole blocks (a power of two) to no narrower than this before its colour conversion.
const SELF_MIN_W: usize = 1 << 8;

/// The power of two a `w`-wide camera frame shrinks by for the self-view (1 = as it is).
fn self_view_shrink(w: usize) -> usize {
    let mut k = 1;
    while w / (k << 1) >= SELF_MIN_W {
        k <<= 1;
    }
    k
}

/// Start sending a camera's frames (desktop): capture → H.264 → the beam track, toward the wave's current peer address (re-read every frame, so a redirect is followed).
pub fn start_sender(target: Target, mut source: Box<dyn FrameSource>, _bitrate_bps: u32) -> Result<Sender, String> {
    let (sw, sh) = source.dimensions();
    let cam_fps = source.fps().max(1);
    // A new beam measures its links from nothing, and owes nothing an earlier beam was asked: its first frame is a keyframe anyway.
    NET_W.store(CONSERVATIVE_W as u32, Ordering::Relaxed);
    beam::reset_key_asks();
    // THE LADDER (beam_ladder.rs): bands from the camera's own frame; the ceiling per `ceiling`.
    let bands = super::beam_ladder::bands_for(sw, sh);
    let (mut ladder, mut seen) = new_ladder(bands);
    let max_band = ceiling(ladder.bands());
    let mut plan = ladder.plan();
    let (bw, bh) = (plan.w, plan.h);
    // THE RATE ACTUALLY ENCODED (2026-11-11: the encoder was built and stamped at the camera's 60 fps while it was handed the plan's 30, so rate control spread the bitrate over frames that never came): the plan's rate, or the camera's own when the camera is the slower one, since a slow camera's every frame is kept.
    let mut enc_fps = plan.fps.min(cam_fps);
    let mut enc = h264::Encoder::new(bw as usize, bh as usize, enc_fps, plan.bitrate)?;
    let (mut tx, first_frame, key) = Outbound::for_target(&target)?;
    // CALIBRATION (beam_calibrate.rs, Nick 2026-10-31: "that should work for all platforms"): this camera is the one Calibrate scans, keyed by its name; a calibration made earlier this run outranks the assumed sRGB entry.
    let cam_name = camera_name().unwrap_or_default();
    let base = source.characterization();
    super::beam_calibrate::set_desktop_camera(Some((&cam_name, base.transfer)));
    let (entry, cal) = match super::beam_calibrate::calibration_for(&cam_name) {
        Some((e, c)) => (e, Some(c)),
        None => (base, None),
    };
    let mut self_conv = Converter::new(Characterization::from_entry(&entry));
    let colour = colour_bytes_cal(&entry, cal);
    let mut info = Info { width: bw, height: bh, fps: enc_fps as u8, codec: Codec::H264, colour };
    let mut landed = super::beam_calibrate::LANDED.load(Ordering::Relaxed);
    let top = ladder.bands()[max_band];
    crate::logf!("BEAM: ladder up — camera {}×{} @ {} fps, ceiling {}×{} until the links measure, starting {}×{} at {} fps, {} kbit/s", sw, sh, cam_fps, top.0, top.1, bw, bh, enc_fps, plan.bitrate / 1000);
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("beam-send".into())
        .spawn(move || {
            // THE RANK: a beam is live on this device for exactly as long as this thread runs, and bulk transfers give way for that long (network::rank).
            let _rank = crate::network::rank::realtime_begin();
            let mut frame_no = first_frame;
            let mut rate_acc = 0u32;
            let mut sent_bytes = 0u64;
            let mut self_rgb = Vec::new();
            let mut coded = h264::I420::new(bw as usize, bh as usize);
            // The encoder's clock: each frame is stamped with when the camera handed it over, counted from the encoder's start, so rate control sees the frames' real spacing.
            let mut enc_t0 = std::time::Instant::now();
            let mut watch = LinkWatch::new();
            // Camera frames since the self-view last refreshed (it refreshes every other camera frame: the camera as it is, whatever the ladder sends), and how far its copy is shrunk.
            let mut self_gap = 0u32;
            let self_k = self_view_shrink(sw);
            // This camera's turns (Settings → Beam), sent now and again on every change — a change bumps the settings generation, read on each frame's edge.
            let cam = camera_name().unwrap_or_default();
            let mut turns = super::beam_settings::turns_for(&cam);
            let mut turns_seen = super::beam_settings::generation();
            // The stats window: camera frames, frames sent and their bytes since it opened; it closes after a nominal second's worth of camera frames (an edge on the count — the clock only measures how long that took).
            let (mut win_t, mut win_cam, mut win_sent, mut win_bytes) = (std::time::Instant::now(), 0u64, 0u64, 0u64);
            tx.send_orient(turns);
            while !stop2.load(Ordering::Relaxed) {
                if win_cam >= cam_fps as u64 {
                    let ms = win_t.elapsed().as_millis() as u64;
                    *SEND_STATS.lock().unwrap() = Some(SendStats { cam_w: sw, cam_h: sh, cam_fps: per_second(win_cam, ms), sent_w: coded.w, sent_h: coded.h, sent_fps: per_second(win_sent, ms), kbps: per_second(win_bytes * 8, ms) / 1000 });
                    (win_t, win_cam, win_sent, win_bytes) = (std::time::Instant::now(), 0, 0, 0);
                }
                // The ladder moves on edges: a new verdict from any link and the ceiling the links have measured (`steer`), a tap on Settings → Beams; `|` so both run.
                let takes = tx.takes();
                if follow_settings(&mut ladder, &mut seen) | steer(&mut ladder, &mut watch, &takes) {
                    let next = ladder.plan();
                    // As at the start: the plan's rate, or a slower camera's own.
                    let next_fps = next.fps.min(cam_fps);
                    if next.band != plan.band {
                        // A band crossing: the one place the coded size — and so a keyframe — changes.
                        let (nw, nh) = (next.w, next.h);
                        match h264::Encoder::new(nw as usize, nh as usize, next_fps, next.bitrate) {
                            Ok(e) => {
                                enc = e;
                                enc_t0 = std::time::Instant::now();
                                enc_fps = next_fps;
                                coded = h264::I420::new(nw as usize, nh as usize);
                                info.width = nw;
                                info.height = nh;
                                info.fps = enc_fps as u8;
                                tx.send_info(&info);
                                crate::logf!("BEAM: ladder → band {}×{} (soft {} half-stops, {} fps, {} kbit/s) — a keyframe for the new size", nw, nh, next.soft, enc_fps, next.bitrate / 1000);
                            }
                            Err(e) => crate::logf!("BEAM: ladder band change failed: {}", e),
                        }
                    } else {
                        if next.bitrate != plan.bitrate {
                            enc.set_bitrate(next.bitrate);
                        }
                        // The frame-rate lever moves the encoder's rate with it, live: no keyframe, and the next self-description says so.
                        if next_fps != enc_fps {
                            enc_fps = next_fps;
                            enc.set_fps(enc_fps);
                            info.fps = enc_fps as u8;
                        }
                    }
                    plan = next;
                }
                // The plan's frame rate (the ladder's lever below the smallest band, and the user's cap): keep plan.fps of every cam_fps camera frames, spread evenly — 24 of 30 keeps four in five (free in H.264, no keyframe). A camera slower than the plan keeps every frame.
                rate_acc += enc_fps;
                let keep = rate_acc >= cam_fps;
                if keep {
                    rate_acc -= cam_fps;
                }
                let show = self_gap >= 1;
                // NOTHING CONVERTED THAT NOTHING USES (2026-11-11, a 1080p group beam that went out at 7.5 fps of a planned 30): a camera frame neither encoded nor due for the self-view goes by undecoded — half of a 60 fps Motion-JPEG camera's frames at a 30 fps plan.
                if !keep && !show {
                    if !source.skip() {
                        crate::log("BEAM: camera gone — sender stops");
                        break;
                    }
                    win_cam += 1;
                    self_gap += 1;
                    continue;
                }
                let Some((frame, gains)) = source.next() else {
                    crate::log("BEAM: camera gone — sender stops");
                    break;
                };
                let at_ms = enc_t0.elapsed().as_millis() as u64;
                win_cam += 1;
                // Calibrate: the frame goes to the scanner when one is wanted (one atomic otherwise); a calibration that landed for this camera recolours the self-view and the beam's self-description at once.
                super::beam_calibrate::offer_frame(&frame);
                let now_landed = super::beam_calibrate::LANDED.load(Ordering::Relaxed);
                if now_landed != landed {
                    landed = now_landed;
                    if let Some((e, c)) = super::beam_calibrate::calibration_for(&cam_name) {
                        self_conv.set_characterization(Characterization::from_entry(&e));
                        info.colour = colour_bytes_cal(&e, Some(c));
                        tx.send_info(&info);
                        crate::logf!("BEAM: {} calibrated — the beam's colour follows", cam_name);
                    }
                }
                if super::beam_settings::generation() != turns_seen {
                    turns_seen = super::beam_settings::generation();
                    let t = super::beam_settings::turns_for(&cam);
                    if t != turns {
                        turns = t;
                        tx.send_orient(turns);
                        crate::logf!("BEAM: {} turned {} quarter turn(s)", cam, turns);
                    }
                }
                tx.follow();
                if show {
                    // The inset is small: the frame is shrunk by whole blocks first, a sixteenth of the colour conversion for a 1080p camera.
                    post_self(&self_conv, &frame.shrunk(self_k), gains, frame_no, &mut self_rgb, turns, true);
                    self_gap = 0;
                } else {
                    self_gap += 1;
                }
                if !keep {
                    continue;
                }
                if frame_no % beam::INFO_EVERY_FRAMES == 0 {
                    tx.send_info(&info);
                    tx.send_orient(turns);
                }
                // A receiver asked for a picture to start from (a late join, a reference it lost): this very frame is the keyframe, one for every ask that landed since the last frame.
                if beam::take_key_ask() {
                    enc.force_keyframe();
                    crate::logf!("BEAM: a receiver asked for a picture to start from — frame {} is a keyframe", frame_no);
                }
                // The camera frame → the coded size at the ladder's softness (crop to the coded aspect, shrink, grow); a frame already the coded size at full detail goes to the encoder as it is.
                let encoded = if (frame.w, frame.h) == (coded.w, coded.h) && plan.soft == 0 {
                    enc.encode_at(&frame, at_ms)
                } else {
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
                    enc.encode_at(&coded, at_ms)
                };
                match encoded {
                    Ok(au) if !au.is_empty() => {
                        // Every IDR answers every ask made before it, whatever caused it (the stream's start, a band change, an ask).
                        if h264::has_idr(&au) {
                            beam::key_answered(frame_no);
                        }
                        sent_bytes += au.len() as u64;
                        win_sent += 1;
                        win_bytes += au.len() as u64;
                        tx.send_frame(frame_no, gains, &au);
                    }
                    Ok(_) => {}
                    Err(e) => crate::logf!("BEAM: encode: {}", e),
                }
                frame_no = frame_no.wrapping_add(1);
                if frame_no % (cam_fps * 10) == 0 {
                    let (sent, fail) = tx.counts();
                    crate::logf!("BEAM: sent {} frames, {} KB, {} datagrams ({} refused); ladder {}×{} soft {} @ {} fps", frame_no, sent_bytes / 1024, sent, fail, coded.w, coded.h, plan.soft, enc_fps);
                }
            }
            *SEND_STATS.lock().unwrap() = None;
            clear_self();
            super::beam_calibrate::set_desktop_camera(None);
            tx.park(key, frame_no);
        })
        .map_err(|e| format!("beam-send thread: {e}"))?;
    Ok(Sender { stop, thread: Some(thread) })
}

/// The ENCODED-frame feed (Android: Kotlin's Camera2 + MediaCodec push access units here): the way out, the beam's self-description, the next frame number, and the stream's parameter sets as last seen in it. Installed by [`start_encoded_sender`], read by [`push_encoded`].
struct EncodedFeed {
    out: Outbound,
    info: Info,
    frame_no: u32,
    /// SPS and PPS with their start codes: the phone's encoder puts them ahead of its first access unit (and every CONFIG_EVERY after, PhotonBeam.kt), and an ask puts them ahead of the next one.
    param_sets: Vec<u8>,
}

static ENCODED_TX: Mutex<Option<EncodedFeed>> = Mutex::new(None);
/// The key of the 1:1 transport the feed sends on (see PAIR_TX); `None` for a group beam, whose fan stays with the group wave.
static ENCODED_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// Start a sender that is fed encoded frames from outside (the phone). Returns the handle whose drop clears the feed.
pub fn start_encoded_sender(target: Target, info: Info) -> Result<Sender, String> {
    let (mut out, first_frame, key) = Outbound::for_target(&target)?;
    // The encoder's first access unit carries the parameter sets, so nothing an earlier beam was asked is owed.
    beam::reset_key_asks();
    out.send_info(&info);
    *ENCODED_KEY.lock().unwrap() = key;
    *ENCODED_TX.lock().unwrap() = Some(EncodedFeed { out, info, frame_no: first_frame, param_sets: Vec::new() });
    Ok(Sender { stop: Arc::new(AtomicBool::new(false)), thread: None })
}

/// One encoded frame from the platform encoder. `gains` are what the ISP applied (Q12), `au` the H.264 access unit. Returns false when no encoded sender is up.
pub fn push_encoded(gains: Gains, au: &[u8]) -> bool {
    let mut g = ENCODED_TX.lock().unwrap();
    let Some(feed) = g.as_mut() else { return false };
    feed.out.follow();
    let n = feed.frame_no;
    let o = android_turns(ANDROID_ORIENT.load(Ordering::Relaxed));
    let changed = ANDROID_ORIENT_SENT.swap(o, Ordering::Relaxed) != o;
    if n % beam::INFO_EVERY_FRAMES == 0 {
        feed.out.send_info(&feed.info);
    }
    // The rotation goes out the moment it changes, and with every self-description for a receiver that joined late.
    if changed || n % beam::INFO_EVERY_FRAMES == 0 {
        feed.out.send_orient(o);
    }
    // A PICTURE TO START FROM, the phone's way (Nick 2026-11-11: the rolling refresh starts transparent and "chunks in like a rolling shutter, no network spike"): never an IDR.
    // A cold decoder needs only the parameter sets to begin, and the refresh paints the picture in; so a unit that carries them answers every ask before it, and an ask puts them ahead of the next unit that does not.
    let sets = h264::param_sets(au);
    if !sets.is_empty() {
        feed.param_sets = sets;
        beam::key_answered(n);
        feed.out.send_frame(n, gains, au);
    } else if !feed.param_sets.is_empty() && beam::take_key_ask() {
        let mut with = feed.param_sets.clone();
        with.extend_from_slice(au);
        beam::key_answered(n);
        crate::logf!("BEAM: a receiver asked for a picture to start from — frame {} carries the parameter sets; the rolling refresh paints the rest", n);
        feed.out.send_frame(n, gains, &with);
    } else {
        feed.out.send_frame(n, gains, au);
    }
    feed.frame_no = n.wrapping_add(1);
    true
}

/// Is an encoded-frame sender up (the phone's camera pipeline asks before encoding anything)?
pub fn encoded_sender_live() -> bool {
    ENCODED_TX.lock().unwrap().is_some()
}

// ───────────────────────── the phone ─────────────────────────

/// The Beam button armed a send on the phone: the wave secret and our side, waiting for Kotlin's pipeline-up edge (`android_started`). Cleared by `android_stop`.
static ANDROID_ARMED: Mutex<Option<Target>> = Mutex::new(None);
/// What Kotlin's camera pipeline last reported when it came up (the pipeline-up edge's arguments): a preview holds them so the sender can start on the same open camera when the wave goes live.
// Read only by `android_arm` (the phone); the desktop compiles the pipeline-up edge but never arms a phone send.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Clone)]
struct PipelineUp {
    w: usize,
    h: usize,
    fps: u32,
    maker: Option<[f32; 9]>,
    straight: bool,
    curve: bool,
    hdr: bool,
    camera: String,
    camera_name: String,
}
static ANDROID_PIPELINE: Mutex<Option<PipelineUp>> = Mutex::new(None);
/// The phone's encoded sender handle (the UI thread cannot own it: the pipeline-up edge arrives on the service thread).
static ANDROID_SENDER: Mutex<Option<Sender>> = Mutex::new(None);

/// Arm a phone send for the active wave and ask Kotlin to open the camera + encoder. Returns false when the service bridge is down.
#[cfg(target_os = "android")]
pub fn android_arm(target: Target) -> bool {
    *ANDROID_ARMED.lock().unwrap() = Some(target);
    // A preview already holds the camera: start the sender on it — Kotlin's next frame gets a plan and builds its encoder, whose first frame is the keyframe a new stream needs.
    let up = ANDROID_PIPELINE.lock().unwrap().clone();
    match up {
        Some(p) => match android_started(p.w, p.h, p.fps, p.maker, p.straight, p.curve, p.hdr, &p.camera, &p.camera_name) {
            Ok(()) => true,
            Err(e) => {
                crate::logf!("{}", e);
                false
            }
        },
        None => crate::platform::jni_android::wave_service_void("startBeamCapture"),
    }
}

/// The pipeline-up edge from Kotlin: build the beam's self-description from what the camera is, and start the encoded sender the button armed. `camera` names the lens ("1", "0/3" — logical id / physical id), the key a calibration is kept under.
/// A CAMERA SWITCH mid-beam (Nick 2026-10-14: "tap and switch between the front and back cam, or the other three back cams… cycles thru all when I tap on my preview") arrives as this same edge while the sender is live: the transport stays — a new BeamTx would restart its key chain — and the ladder rebuilds from the new capture keeping the link's detail; the new size, colour and orientation go out at once, and the encoder's restart opens with the keyframe the new size needs.
pub fn android_started(w: usize, h: usize, fps: u32, maker: Option<[f32; 9]>, straight: bool, curve: bool, hdr: bool, camera: &str, camera_name: &str) -> Result<(), String> {
    *ANDROID_CAMERA_NAME.lock().unwrap() = Some(camera_name.to_string());
    *ANDROID_PIPELINE.lock().unwrap() = Some(PipelineUp { w, h, fps, maker, straight, curve, hdr, camera: camera.to_string(), camera_name: camera_name.to_string() });
    let armed = *ANDROID_ARMED.lock().unwrap();
    // A calibration made earlier this run for THIS lens (beam_calibrate.rs) outranks the maker's matrix — on the straight path only, where the frames are the colour it was solved from.
    let on_straight = straight && curve && !hdr;
    super::beam_calibrate::set_camera(camera, on_straight);
    let calibrated = super::beam_calibrate::calibration().filter(|_| on_straight);
    let entry = if hdr { hdr_entry() } else { calibrated.as_ref().map_or_else(|| phone_entry(maker, straight, curve), |(e, _)| e.clone()) };
    *ANDROID_SELF_CONV.lock().unwrap() = Some(Converter::new(Characterization::from_entry(&entry)));
    // THE PREVIEW (a beam placed, still ringing): the self-view runs and nothing is sent — no ladder, so Kotlin plans no frame and builds no encoder until the wave is live.
    let Some(target) = armed else {
        crate::logf!("BEAM: phone preview up — camera {}, {}×{} @ {} fps; nothing is sent until the wave is live", camera, w, h, fps);
        return Ok(());
    };
    let colour = colour_bytes_cal(&entry, calibrated.map(|(_, c)| c));
    // THE LADDER (beam_ladder.rs): bands from the capture itself (Kotlin picked it: ≤ ~5 MP, ≤ 30 fps, least cropped), the ceiling per `ceiling`.
    let bands = super::beam_ladder::bands_for(w, h);
    let mut lg = ANDROID_LADDER.lock().unwrap();
    // A new beam measures its links from nothing; a camera switch keeps what the links have shown (the network did not change with the lens).
    if lg.is_none() {
        NET_W.store(CONSERVATIVE_W as u32, Ordering::Relaxed);
    }
    let max_band = ceiling(&bands);
    let top = bands[max_band];
    if let (Some(PhoneLadder { ladder, last_kept, .. }), Some(feed)) = (lg.as_mut(), ENCODED_TX.lock().unwrap().as_mut()) {
        ladder.rebuild(bands, max_band);
        *last_kept = 0;
        let p = ladder.plan();
        feed.info.width = p.w;
        feed.info.height = p.h;
        feed.info.fps = fps.min(255) as u8;
        feed.info.colour = colour;
        feed.out.send_info(&feed.info);
        ANDROID_ORIENT_SENT.store(0xFF, Ordering::Relaxed);
        crate::logf!("BEAM: camera switched to {} — capture {}×{} @ {} fps, ceiling {}×{}, now {}×{} (detail kept)", camera, w, h, fps, top.0, top.1, p.w, p.h);
        return Ok(());
    }
    let (ladder, seen) = new_ladder(bands);
    let p = ladder.plan();
    crate::logf!("BEAM: phone ladder up — camera {}, capture {}×{} @ {} fps, ceiling {}×{} until the links measure, starting {}×{}", camera, w, h, fps, top.0, top.1, p.w, p.h);
    *lg = Some(PhoneLadder { ladder, last_kept: 0, seen, watch: LinkWatch::new() });
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
    *ANDROID_PIPELINE.lock().unwrap() = None;
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
    if let Some(feed) = ENCODED_TX.lock().unwrap().as_mut() {
        feed.info.colour = colour_bytes_cal(entry, Some(cal));
        feed.out.send_info(&feed.info);
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
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        crate::platform::camera_v4l2::open(desktop_camera_pick().as_deref()).map(|c| Box::new(c) as Box<dyn FrameSource>)
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::camera_avf::open(desktop_camera_pick().as_deref()).map(|c| Box::new(c) as Box<dyn FrameSource>)
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
        drop(tx.send(started));
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
        let cam = camera_name().unwrap_or_default();
        crate::logf!("BEAM: preview {}×{} from {}", w, h, cam);
        // The preview calibrates like the sender (Settings → Beam, a beam ringing out): this camera registered, its frames offered, a landed calibration recolouring the self-view.
        let base = source.characterization();
        super::beam_calibrate::set_desktop_camera(Some((&cam, base.transfer)));
        let entry = super::beam_calibrate::calibration_for(&cam).map_or(base, |(e, _)| e);
        let mut conv = Converter::new(Characterization::from_entry(&entry));
        let mut landed = super::beam_calibrate::LANDED.load(Ordering::Relaxed);
        let mut rgb = Vec::new();
        let mut n = 0u32;
        while !stop2.load(Ordering::Relaxed) {
            let Some((frame, gains)) = source.next() else {
                crate::log("BEAM: preview — camera gone");
                super::beam_calibrate::set_desktop_camera(None);
                clear_self();
                return None;
            };
            super::beam_calibrate::offer_frame(&frame);
            let now_landed = super::beam_calibrate::LANDED.load(Ordering::Relaxed);
            if now_landed != landed {
                landed = now_landed;
                if let Some((e, _)) = super::beam_calibrate::calibration_for(&cam) {
                    conv.set_characterization(Characterization::from_entry(&e));
                }
            }
            n = n.wrapping_add(1);
            // Every other camera frame, as the sender's self-view does.
            if n % 2 == 0 {
                post_self(&conv, &frame, gains, n, &mut rgb, super::beam_settings::turns_for(&cam), true);
            }
        }
        Some(source)
    });
    match spawned {
        Ok(thread) => *slot = Some(Preview { stop, thread }),
        Err(e) => crate::logf!("BEAM: no preview thread: {}", e),
    }
}

/// Is a preview showing the camera now (Settings → Beam's Preview button, a beam ringing out)?
#[cfg(not(target_os = "android"))]
pub fn preview_running() -> bool {
    PREVIEW.lock().unwrap().as_ref().is_some_and(|p| !p.thread.is_finished())
}
#[cfg(target_os = "android")]
pub fn preview_running() -> bool {
    ANDROID_PIPELINE.lock().unwrap().is_some() && !android_sending()
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
        super::beam_calibrate::set_desktop_camera(None);
    });
}

/// The camera a running preview holds, for the sender (a worker thread: the join waits at most one camera frame, or out a camera still opening).
#[cfg(not(target_os = "android"))]
fn take_preview_source() -> Option<Box<dyn FrameSource>> {
    let p = PREVIEW.lock().unwrap().take()?;
    p.stop.store(true, Ordering::Relaxed);
    p.thread.join().ok().flatten()
}

/// A phone previews thru Kotlin's own capture pipeline: started here with nothing armed, it shows the self-view and sends nothing (see `android_started`); stopped here unless a send took it over.
#[cfg(target_os = "android")]
pub fn preview_start() {
    if !crate::platform::jni_android::wave_service_void("startBeamCapture") {
        crate::log("BEAM: preview — the wave service did not start the capture");
    }
}
#[cfg(target_os = "android")]
pub fn preview_stop() {
    if !android_sending() {
        android_stop();
    }
}

/// The stopping that runs off the UI thread, in order: each retirement joins the one before it, and a camera open waits for the last.
static RETIRING: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

fn retire_with(f: impl FnOnce() + Send + 'static) {
    let prev = RETIRING.lock().unwrap().take();
    let spawned = std::thread::Builder::new().name("beam-retire".into()).spawn(move || {
        if let Some(p) = prev {
            // A panicked retirement's Err is the panic the hook already logged; the join is for the ordering.
            drop(p.join());
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
#[cfg(not(target_os = "android"))]
fn await_retired() {
    let h = RETIRING.lock().unwrap().take();
    if let Some(h) = h {
        // A panicked retirement's Err is the panic the hook already logged; the join is for the ordering.
        drop(h.join());
    }
}

/// Can this device SEND a beam at all: a codec and a camera path.
pub fn can_send() -> bool {
    if cfg!(target_os = "android") {
        return true;
    }
    h264::AVAILABLE && (cfg!(target_os = "linux") || cfg!(target_os = "macos"))
}

/// Can this device place or answer a BEAM right now (Nick 2026-10-31: "if a beam is incoming and you don't have a cam you can wave back"): it can send a beam at all, the camera grant is not refused, and on a desktop a camera is listed.
/// The desktop list is read from the cache only (the render never opens a device): a cache not filled yet counts as a camera, and a worker fills it once.
pub fn camera_usable() -> bool {
    if !can_send() || crate::platform::grants::status(crate::platform::grants::Need::Camera) == crate::platform::mic_permission::MicAccess::Denied {
        return false;
    }
    #[cfg(not(target_os = "android"))]
    {
        let listed = DESKTOP_CAMS.lock().unwrap().as_ref().map(|l| !l.is_empty());
        if listed.is_none() {
            static ASKED: AtomicBool = AtomicBool::new(false);
            if !ASKED.swap(true, Ordering::Relaxed) {
                crate::log_err!(std::thread::Builder::new().name("camera-list".into()).spawn(reenumerate_desktop_cameras), "BEAM: spawning the camera-list thread");
            }
        }
        listed.unwrap_or(true)
    }
    #[cfg(target_os = "android")]
    {
        true
    }
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
        if !crate::platform::jni_android::wave_service_void("askCameraEarly") {
            crate::log("BEAM: the wave service did not take the early camera ask");
        }
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

    /// One link's reading as its engine would publish it.
    fn take(slot: usize, seq: u32, congested: bool, clean: bool, queue_ms: u32, bytes: u64) -> LinkTake {
        LinkTake { slot, now: crate::wave::LinkNow { epoch: 0, seq, congested, clean, rtt_ms: 40, queue_ms }, bytes }
    }

    /// The bytes that make a one-second verdict interval carry `bps`.
    fn bytes_for(bps: u32) -> u64 {
        bps as u64 / 8
    }

    /// THE CEILING FROM THE LINKS (Nick 2026-11-11): conservative until a link has measured anything, then one band past what it has carried cleanly, the weakest link setting it — and a second carried under a queue earns nothing.
    #[test]
    fn the_ceiling_opens_band_by_band_as_the_weakest_link_carries() {
        use crate::wave::beam_ladder::{bands_for, bitrate_for, top_within, Ladder};
        let bands = bands_for(1920, 1080);
        assert_eq!(bands, vec![(120, 68), (240, 134), (480, 270), (960, 540), (1920, 1080)]);
        let ladder = Ladder::new(bands.clone(), 4);
        let top = |b: usize| bitrate_for(bands[b].0 as u32, bands[b].1 as u32, 0, 30, 100);
        let mut w = LinkWatch::new();
        // Nothing measured: the conservative width (960 for a 1920 capture), whatever the capture could do.
        assert_eq!(w.net_top(&ladder), top_within(&bands, CONSERVATIVE_W));
        assert_eq!(w.net_top(&ladder), 3);
        // First sight of a link that has spoken: still nothing measured.
        assert_eq!(w.step(0, &[take(0, 7, false, true, 2, 0)]), None);
        assert_eq!(w.net_top(&ladder), 3);
        // A clean second carrying the top of band 1 opens band 2 and no further.
        let mut bytes = bytes_for(top(1));
        assert_eq!(w.step(1000, &[take(0, 8, false, true, 2, bytes)]), Some((1, false, true)));
        assert_eq!(w.net_top(&ladder), 2, "measured now, and below the conservative start");
        // Carrying the top of band 2 opens band 3; carrying band 3's opens the capture.
        bytes += bytes_for(top(2));
        w.step(2000, &[take(0, 9, false, true, 2, bytes)]);
        assert_eq!(w.net_top(&ladder), 3);
        bytes += bytes_for(top(3));
        w.step(3000, &[take(0, 10, false, true, 2, bytes)]);
        assert_eq!(w.net_top(&ladder), 4);
        // An encoder a fifth under its plan still opens the next band; one half under does not.
        let mut u = LinkWatch::new();
        u.step(0, &[take(0, 1, false, true, 2, 0)]);
        u.step(1000, &[take(0, 2, false, true, 2, bytes_for(top(2) / 5 * 4))]);
        assert_eq!(u.net_top(&ladder), 3);
        let mut h = LinkWatch::new();
        h.step(0, &[take(0, 1, false, true, 2, 0)]);
        h.step(1000, &[take(0, 2, false, true, 2, bytes_for(top(2) / 2))]);
        assert_eq!(h.net_top(&ladder), 2);
        // A clean second over a standing queue (Jeff's 71 ms on 2026-11-11) earns nothing: still unmeasured, the conservative ceiling.
        let mut q = LinkWatch::new();
        q.step(0, &[take(0, 1, false, true, 71, 0)]);
        q.step(1000, &[take(0, 2, false, true, 71, bytes_for(top(4)))]);
        assert_eq!(q.net_top(&ladder), 3);
        // Two links: the mesh sends every frame on each, so the weaker one's top binds.
        let mut two = LinkWatch::new();
        two.step(0, &[take(1, 1, false, true, 2, 0), take(2, 1, false, true, 2, 0)]);
        two.step(1000, &[take(1, 2, false, true, 2, bytes_for(top(3))), take(2, 2, false, true, 2, bytes_for(top(1)))]);
        assert_eq!(two.net_top(&ladder), 2);
        // A congested second caps what a link is credited with at the rate it could not take.
        two.step(2000, &[take(1, 3, true, false, 2, bytes_for(top(3)) + bytes_for(top(0))), take(2, 2, false, true, 2, bytes_for(top(1)))]);
        assert_eq!(link_top(&ladder, two.links[0].carried_bps), link_top(&ladder, top(0) as u64));
        // The weak link leaves: the other one alone sets it.
        let mut gone = LinkWatch::new();
        gone.step(0, &[take(1, 1, false, true, 2, 0), take(2, 1, false, true, 2, 0)]);
        gone.step(1000, &[take(1, 2, false, true, 2, bytes_for(top(3))), take(2, 2, false, true, 2, bytes_for(top(1)))]);
        gone.step(1100, &[take(1, 2, false, true, 2, bytes_for(top(3)))]);
        assert_eq!(gone.net_top(&ladder), 4);
    }

    /// CONGESTION ON EVERY LINK (Nick 2026-11-11): any sustained link congested steps the beam down at once; a clean verdict needs every sustained link heard clean, so two links climb no faster than one; a link's first verdict (its start) is not counted.
    #[test]
    fn every_sustained_link_is_heard() {
        let mut w = LinkWatch::new();
        assert_eq!(w.step(0, &[take(1, 1, false, true, 2, 0), take(2, 1, true, false, 2, 0)]), None, "first sight: neither link counts yet");
        // Link 1 speaks clean; link 2 has not spoken again, and is not sustained yet, so link 1 alone makes the round.
        assert_eq!(w.step(1000, &[take(1, 2, false, true, 2, 1000), take(2, 1, true, false, 2, 0)]), Some((1, false, true)));
        // Link 2's second verdict is congested: down at once, whatever link 1 says.
        assert_eq!(w.step(1300, &[take(1, 2, false, true, 2, 1000), take(2, 2, true, false, 2, 500)]), Some((2, true, false)));
        // Link 1 clean again: the round waits for link 2.
        assert_eq!(w.step(2000, &[take(1, 3, false, true, 2, 2000), take(2, 2, true, false, 2, 500)]), None);
        // Link 2 clean too: one clean verdict for the pair.
        assert_eq!(w.step(2300, &[take(1, 3, false, true, 2, 2000), take(2, 3, false, true, 2, 1000)]), Some((3, false, true)));
        // A round where one link was neither clean nor congested holds (not clean).
        w.step(3000, &[take(1, 4, false, true, 2, 3000), take(2, 3, false, true, 2, 1000)]);
        assert_eq!(w.step(3300, &[take(1, 4, false, true, 2, 3000), take(2, 4, false, false, 2, 1500)]), Some((4, false, false)));
        // A link whose engine ended (a new epoch in its slot) starts over: not counted until sustained again.
        let mut fresh = take(2, 1, true, false, 2, 0);
        fresh.now.epoch = 1;
        assert_eq!(w.step(4000, &[take(1, 5, false, true, 2, 4000), fresh]), Some((5, false, true)), "the new link's congested first verdict does not count");
        // The same sequence read twice is one verdict.
        assert_eq!(w.step(4100, &[take(1, 5, false, true, 2, 4000), fresh]), None);
    }

    /// A receiver asks on its first stuck frame, again only after KEY_ASK_AGAIN more, and a clean picture ends the need.
    #[test]
    fn a_stuck_receiver_asks_once_then_again_only_if_still_stuck() {
        let mut n = KeyNeed::default();
        assert!(n.stuck(), "the first frame it cannot picture asks at once");
        for _ in 1..KEY_ASK_AGAIN {
            assert!(!n.stuck());
        }
        assert!(n.stuck(), "still stuck a whole interval later: the answer was lost, ask again");
        n.pictured();
        assert!(n.stuck(), "a new need after a picture asks at once");
    }

    /// The self-view shrinks a big camera by whole powers of two to no narrower than its target, and leaves a small one alone.
    #[test]
    fn the_self_view_shrinks_by_powers_of_two() {
        assert_eq!(self_view_shrink(1920), 4);
        assert_eq!(self_view_shrink(1280), 4);
        assert_eq!(self_view_shrink(640), 2);
        assert_eq!(self_view_shrink(320), 1);
    }

    #[test]
    fn bitrate_scales_with_area_inside_the_ladder() {
        assert_eq!(bitrate_for(640, 480), 600_000);
        assert_eq!(bitrate_for(320, 240), 150_000);
        assert_eq!(bitrate_for(1280, 720), 1_800_000);
    }
}
