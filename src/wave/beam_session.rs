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
}

static PICTURE: Mutex<Option<Picture>> = Mutex::new(None);
/// Bumped on every new picture: the paint compares it with what it last drew.
pub static PICTURE_GEN: AtomicU64 = AtomicU64::new(0);

/// The newest picture the peer's beam produced, if a beam is flowing.
pub fn picture() -> Option<Picture> {
    PICTURE.lock().unwrap().clone()
}

/// THE SELF-VIEW (2026-10-10, Nick: "when you choose beam, still no video shows"): what this device's camera is sending, converted thru the same chain the receiver runs — so the inset shows the colour the far side gets, tungsten-yellow included.
static SELF_PICTURE: Mutex<Option<Picture>> = Mutex::new(None);

pub fn self_picture() -> Option<Picture> {
    SELF_PICTURE.lock().unwrap().clone()
}

fn post_self(conv: &Converter, frame: &h264::I420, gains: Gains, n: u32, rgb: &mut Vec<u8>) {
    conv.convert(frame, gains, n, rgb);
    *SELF_PICTURE.lock().unwrap() = Some(Picture { w: frame.w, h: frame.h, frame_no: n, rgb: rgb.clone() });
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
        post_self(conv, &f, Gains::NONE, 0, &mut rgb);
    }
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
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        *PICTURE.lock().unwrap() = None;
        PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Start receiving the peer's beam for a wave. `we_are_origin` picks the chain: the peer sends in the other direction.
pub fn start_receiver(wave_secret: &[u8; 32], we_are_origin: bool) -> Receiver {
    let dir = if we_are_origin { Direction::AnswerToOrigin } else { Direction::OriginToAnswer };
    let rx = beam::start_rx(wave_secret, dir);
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("beam-decode".into())
        .spawn(move || decode_loop(stop2))
        .expect("beam-decode thread");
    Receiver { _rx: rx, stop, thread: Some(thread) }
}

fn decode_loop(stop: Arc<AtomicBool>) {
    let mut decoder: Option<h264::Decoder> = None;
    let mut conv = Converter::new(Characterization::IDENTITY_GAMMA2);
    let mut rgb = Vec::new();
    let mut decoded = 0u64;
    let mut failed = 0u64;
    let mut last_info: Option<Info> = None;
    while !stop.load(Ordering::Relaxed) {
        let frames = beam::take_frames_wait(std::time::Duration::from_millis(250));
        if frames.is_empty() {
            continue;
        }
        // The self-description edge: a changed characterization reconfigures the colour chain.
        if let Some(info) = beam::current_info() {
            if last_info.as_ref() != Some(&info) {
                if let Some(chr) = colour_from_bytes(&info.colour) {
                    conv.set_characterization(chr);
                }
                crate::logf!("BEAM: peer beam {}×{} @ {} fps, {}, {} colour bytes", info.width, info.height, info.fps, info.codec.name(), info.colour.len());
                last_info = Some(info);
            }
        }
        // Only the NEWEST completed frame is worth decoding when several queued: a late picture is no picture. (H.264 P-frames reference their predecessors, so skipping one costs a reference; the decoder conceals and the next keyframe or refresh stripe repairs it — the price of never falling behind.)
        let Some(frame) = frames.into_iter().last() else { continue };
        if decoder.is_none() {
            match h264::Decoder::new() {
                Ok(d) => decoder = Some(d),
                Err(e) => {
                    crate::logf!("BEAM: {}", e);
                    // No codec on this platform: nothing to show, but keep draining so the mailbox never fills.
                    continue;
                }
            }
        }
        let Some(dec) = decoder.as_mut() else { continue };
        match dec.decode(&frame.bytes) {
            Ok(Some(pic)) => {
                conv.convert(&pic, frame.gains, frame.frame_no, &mut rgb);
                *PICTURE.lock().unwrap() = Some(Picture { w: pic.w, h: pic.h, frame_no: frame.frame_no, rgb: rgb.clone() });
                PICTURE_GEN.fetch_add(1, Ordering::Relaxed);
                decoded += 1;
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
    crate::logf!("BEAM: decode thread down — {} pictures, {} failures", decoded, failed);
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
        *ENCODED_TX.lock().unwrap() = None;
    }
}

/// Start sending a camera's frames (desktop): capture → H.264 → the beam track, toward the wave's current peer address (re-read every frame, so a redirect is followed).
pub fn start_sender(wave_secret: &[u8; 32], we_are_origin: bool, mut source: Box<dyn FrameSource>, bitrate_bps: u32) -> Result<Sender, String> {
    let dir = if we_are_origin { Direction::OriginToAnswer } else { Direction::AnswerToOrigin };
    let (w, h) = source.dimensions();
    let fps = source.fps();
    let mut enc = h264::Encoder::new(w, h, fps, bitrate_bps)?;
    let Some(peer) = super::wave_tx_addr() else {
        return Err("BEAM: no peer address yet — the wave's first authenticated packet sets it".into());
    };
    let mut tx = BeamTx::new(wave_secret, dir, peer);
    let self_conv = Converter::new(Characterization::from_entry(&source.characterization()));
    let colour = colour_bytes(&source.characterization());
    let info = Info { width: w as u16, height: h as u16, fps: fps as u8, codec: Codec::H264, colour };
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("beam-send".into())
        .spawn(move || {
            let mut frame_no = 0u32;
            let mut sent_bytes = 0u64;
            let mut self_rgb = Vec::new();
            while !stop2.load(Ordering::Relaxed) {
                let Some((frame, gains)) = source.next() else {
                    crate::log("BEAM: camera gone — sender stops");
                    break;
                };
                if let Some(addr) = super::wave_tx_addr() {
                    if addr != tx.peer() {
                        tx.set_peer(addr);
                    }
                }
                if frame_no % beam::INFO_EVERY_FRAMES == 0 {
                    tx.send_info(&info);
                }
                // The self-view, every other frame: the camera thru the receiver's own chain.
                if frame_no % 2 == 0 {
                    post_self(&self_conv, &frame, gains, frame_no, &mut self_rgb);
                }
                match enc.encode(&frame) {
                    Ok(au) if !au.is_empty() => {
                        sent_bytes += au.len() as u64;
                        tx.send_frame(frame_no, gains, &au);
                    }
                    Ok(_) => {}
                    Err(e) => crate::logf!("BEAM: encode: {}", e),
                }
                frame_no = frame_no.wrapping_add(1);
                if frame_no % (fps.max(1) * 10) == 0 {
                    crate::logf!("BEAM: sent {} frames, {} KB, {} datagrams ({} refused)", frame_no, sent_bytes / 1024, tx.sent, tx.send_fail);
                }
            }
            clear_self();
        })
        .map_err(|e| format!("beam-send thread: {e}"))?;
    Ok(Sender { stop, thread: Some(thread) })
}

/// The ENCODED-frame feed (Android: Kotlin's Camera2 + MediaCodec push access units here). Installed by [`start_encoded_sender`], read by [`push_encoded`].
static ENCODED_TX: Mutex<Option<(BeamTx, Info, u32)>> = Mutex::new(None);

/// Start a sender that is fed encoded frames from outside (the phone). Returns the handle whose drop clears the feed.
pub fn start_encoded_sender(wave_secret: &[u8; 32], we_are_origin: bool, info: Info) -> Result<Sender, String> {
    let dir = if we_are_origin { Direction::OriginToAnswer } else { Direction::AnswerToOrigin };
    let Some(peer) = super::wave_tx_addr() else {
        return Err("BEAM: no peer address yet".into());
    };
    let mut tx = BeamTx::new(wave_secret, dir, peer);
    tx.send_info(&info);
    *ENCODED_TX.lock().unwrap() = Some((tx, info, 0));
    Ok(Sender { stop: Arc::new(AtomicBool::new(false)), thread: None })
}

/// One encoded frame from the platform encoder. `gains` are what the ISP applied (Q12), `au` the H.264 access unit. Returns false when no encoded sender is up.
pub fn push_encoded(gains: Gains, au: &[u8]) -> bool {
    let mut g = ENCODED_TX.lock().unwrap();
    let Some((tx, info, frame_no)) = g.as_mut() else { return false };
    if let Some(addr) = super::wave_tx_addr() {
        if addr != tx.peer() {
            tx.set_peer(addr);
        }
    }
    if *frame_no % beam::INFO_EVERY_FRAMES == 0 {
        tx.send_info(info);
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
static ANDROID_ARMED: Mutex<Option<([u8; 32], bool)>> = Mutex::new(None);
/// The phone's encoded sender handle (the UI thread cannot own it: the pipeline-up edge arrives on the service thread).
static ANDROID_SENDER: Mutex<Option<Sender>> = Mutex::new(None);

/// Arm a phone send for the active wave and ask Kotlin to open the camera + encoder. Returns false when the service bridge is down.
#[cfg(target_os = "android")]
pub fn android_arm(wave_secret: [u8; 32], we_are_origin: bool) -> bool {
    *ANDROID_ARMED.lock().unwrap() = Some((wave_secret, we_are_origin));
    crate::platform::jni_android::wave_service_void("startBeamCapture")
}

/// The pipeline-up edge from Kotlin: build the beam's self-description from what the camera is, and start the encoded sender the button armed.
pub fn android_started(w: usize, h: usize, fps: u32, maker: Option<[f32; 9]>, straight: bool) -> Result<(), String> {
    let Some((secret, origin)) = *ANDROID_ARMED.lock().unwrap() else {
        return Err("BEAM: pipeline up with nothing armed".into());
    };
    let entry = phone_entry(maker, straight);
    *ANDROID_SELF_CONV.lock().unwrap() = Some(Converter::new(Characterization::from_entry(&entry)));
    let info = Info { width: w as u16, height: h as u16, fps: fps as u8, codec: Codec::H264, colour: colour_bytes(&entry) };
    let s = start_encoded_sender(&secret, origin, info)?;
    *ANDROID_SENDER.lock().unwrap() = Some(s);
    Ok(())
}

/// Stop a phone send: the sender handle, the arm, and Kotlin's pipeline.
pub fn android_stop() {
    *ANDROID_SENDER.lock().unwrap() = None;
    *ANDROID_ARMED.lock().unwrap() = None;
    *ANDROID_SELF_CONV.lock().unwrap() = None;
    clear_self();
    #[cfg(target_os = "android")]
    crate::platform::jni_android::wave_service_void("stopBeamCapture");
}

pub fn android_sending() -> bool {
    ANDROID_ARMED.lock().unwrap().is_some()
}

/// The phone's characterization (docs/beams.md): with the maker's XYZ→camera matrix AND the straight-thru request in force, `absolute` / `model` / `gamma2` with camera→VSF-RGB = XYZ→VSF-RGB × inv(XYZ→camera); otherwise `creative` / `assumed` / `gamma2` (the HAL white-balanced and matrixed toward sRGB under our γ2 curve), the sRGB→VSF-RGB matrix.
pub fn phone_entry(maker: Option<[f32; 9]>, straight: bool) -> vsf::spectral_image::ProfileEntry {
    if let (Some(m), true) = (maker, straight) {
        if let Some(inv) = invert3(&m) {
            return vsf::spectral_image::ProfileEntry {
                matrix: mat_mul3(&vsf::colour::XYZ2VSF_RGB, &inv),
                source: "dng_colormatrix2".into(),
                class: vsf::spectral_image::IdtClass::Absolute,
                tier: vsf::spectral_image::ProfileTier::Model,
                illuminant: 21, // D65 (EXIF LightSource), the daylight set
                transfer: vsf::spectral_image::Transfer::Gamma2,
            };
        }
    }
    vsf::spectral_image::ProfileEntry {
        matrix: vsf::colour::SRGB2VSF_RGB,
        source: "assumed_srgb".into(),
        class: vsf::spectral_image::IdtClass::Creative,
        tier: vsf::spectral_image::ProfileTier::Assumed,
        illuminant: 0,
        transfer: vsf::spectral_image::Transfer::Gamma2,
    }
}

fn invert3(m: &[f32; 9]) -> Option<[f32; 9]> {
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

fn mat_mul3(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
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
    let profile = vsf::spectral_image::ColourProfile {
        target: "vsf_rgb".into(),
        entries: vec![entry.clone()],
        dng_colormatrix: [None, None],
        patches: None,
        cal: None,
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

/// The entry for a camera we know nothing about (a UVC webcam, a Mac): `creative` / `assumed` / `srgb`, the sRGB→VSF-RGB matrix.
pub fn assumed_srgb_entry() -> vsf::spectral_image::ProfileEntry {
    vsf::spectral_image::ProfileEntry {
        matrix: vsf::colour::SRGB2VSF_RGB,
        source: "assumed_srgb".into(),
        class: vsf::spectral_image::IdtClass::Creative,
        tier: vsf::spectral_image::ProfileTier::Assumed,
        illuminant: 0,
        transfer: vsf::spectral_image::Transfer::Srgb,
    }
}

/// The desktop's camera, if the platform has one we can open: Linux V4L2 today.
pub fn open_desktop_camera() -> Result<Box<dyn FrameSource>, String> {
    #[cfg(target_os = "linux")]
    {
        crate::platform::camera_v4l2::open().map(|c| Box::new(c) as Box<dyn FrameSource>)
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::camera_avf::open().map(|c| Box::new(c) as Box<dyn FrameSource>)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err("BEAM: no camera path on this platform yet (docs/beams.md stage 5)".into())
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

/// The sender's bitrate for a frame size, from the ladder in docs/beams.md: ~600 kbit/s at 640×480, scaled by area.
pub fn bitrate_for(w: usize, h: usize) -> u32 {
    let area = (w * h) as u64;
    ((600_000u64 * area) / (640 * 480)).clamp(150_000, 2_000_000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colour_entry_round_trips_as_vsf() {
        let e = assumed_srgb_entry();
        let bytes = colour_bytes(&e);
        assert!(!bytes.is_empty());
        let chr = colour_from_bytes(&bytes).expect("reads back");
        assert_eq!(chr.transfer, super::super::beam_colour::Transfer::Srgb);
        assert_eq!(chr.matrix, vsf::colour::SRGB2VSF_RGB);
        assert!(colour_from_bytes(&[]).is_none());
        assert!(colour_from_bytes(b"not a vsf document").is_none());
    }

    #[test]
    fn the_phone_entry_is_absolute_only_with_a_matrix_and_a_straight_request() {
        let e = phone_entry(None, true);
        assert_eq!(e.class, vsf::spectral_image::IdtClass::Creative);
        let e = phone_entry(Some([1., 0., 0., 0., 1., 0., 0., 0., 1.]), false);
        assert_eq!(e.class, vsf::spectral_image::IdtClass::Creative);
        let e = phone_entry(Some([2., 0., 0., 0., 2., 0., 0., 0., 2.]), true);
        assert_eq!(e.class, vsf::spectral_image::IdtClass::Absolute);
        // XYZ→camera = 2·I, so camera→VSF = XYZ2VSF × ½·I.
        for i in 0..9 {
            assert!((e.matrix[i] - vsf::colour::XYZ2VSF_RGB[i] * 0.5).abs() < 1e-6);
        }
        assert!(invert3(&[0.; 9]).is_none());
    }

    #[test]
    fn bitrate_scales_with_area_inside_the_ladder() {
        assert_eq!(bitrate_for(640, 480), 600_000);
        assert_eq!(bitrate_for(320, 240), 150_000);
        assert_eq!(bitrate_for(1280, 720), 1_800_000);
    }
}
