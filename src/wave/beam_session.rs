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
    let colour = colour_bytes(&source.characterization());
    let info = Info { width: w as u16, height: h as u16, fps: fps as u8, codec: Codec::H264, colour };
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("beam-send".into())
        .spawn(move || {
            let mut frame_no = 0u32;
            let mut sent_bytes = 0u64;
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
    // `decode` + `primary_section`, never a bare section parse: the section NAME lives in the header TOC (vsf-toc-section-name-trap).
    let (header, header_end) = vsf::file_format::VsfHeader::decode(bytes).ok()?;
    let section = header.primary_section(bytes, header_end).ok()?;
    let profile = vsf::spectral_image::profile_from_fields(&section.fields).ok()?;
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
    #[cfg(not(target_os = "linux"))]
    {
        Err("BEAM: no camera path on this platform yet (docs/beams.md stage 5)".into())
    }
}

/// Can this device SEND a beam at all: a codec and a camera path.
pub fn can_send() -> bool {
    if cfg!(target_os = "android") {
        return true;
    }
    h264::AVAILABLE && cfg!(target_os = "linux")
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
    fn bitrate_scales_with_area_inside_the_ladder() {
        assert_eq!(bitrate_for(640, 480), 600_000);
        assert_eq!(bitrate_for(320, 240), 150_000);
        assert_eq!(bitrate_for(1280, 720), 1_800_000);
    }
}
