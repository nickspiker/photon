//! A KEPT BEAM ON REPLAY (Nick 2026-11-01, the replayed wave screen "with video too"): a `beam.video` document (wave/beam_keep.rs) decoded in step with its recording's audio — each track's access units fed to its own H.264 decoder up to the moment the playback has reached, the newest picture per track handed to the screen. The audio is the clock: the screen tells the worker where the playhead is (`set_moment`), and a move backwards re-decodes that track from its start (a beam carries no keyframes to seek to — intra refresh rebuilds the picture as it plays).
//!
//! The blob read and every decode run on the worker thread; the screen only reads the newest pictures.

use crate::wave::beam_colour::{rotate_px, rotate_rgb, Characterization, Converter};
use crate::wave::beam_keep::TrackEvent;
use crate::wave::beam_session::Picture;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

/// A move backwards of more than this (half a second, in Eagle oscillations) re-decodes the track from its start; less is the playhead's own jitter.
const BACK_SEEK: i64 = (vsf::OSCILLATIONS_PER_SECOND / 2) as i64;

/// The running decode of one kept beam.
pub struct ReplayVideo {
    stop: Arc<AtomicBool>,
    /// The moment the audio has reached (Eagle oscillations); `i64::MIN` until the screen says.
    target: Arc<AtomicI64>,
    /// Each track's person and newest picture, in the document's order (empty until the worker has read it).
    pictures: Arc<Mutex<Vec<([u8; 32], Option<Picture>)>>>,
}

impl ReplayVideo {
    /// Load the kept beam `hash` and start decoding it on a worker — nothing is read on the calling thread.
    pub fn start(identity_seed: [u8; 32], hash: [u8; 32]) -> Option<ReplayVideo> {
        if !crate::wave::h264::AVAILABLE {
            return None; // no decoder on this platform: the replay shows faces
        }
        let stop = Arc::new(AtomicBool::new(false));
        let target = Arc::new(AtomicI64::new(i64::MIN));
        let pictures: Arc<Mutex<Vec<([u8; 32], Option<Picture>)>>> = Arc::new(Mutex::new(Vec::new()));
        let (s, t, p) = (stop.clone(), target.clone(), pictures.clone());
        std::thread::Builder::new()
            .name("replay-video".into())
            .spawn(move || run(identity_seed, hash, &s, &t, &p))
            .ok()?;
        Some(ReplayVideo { stop, target, pictures })
    }

    /// Where the audio's playhead is now.
    pub fn set_moment(&self, osc: i64) {
        self.target.store(osc, Ordering::Relaxed);
    }

    /// The newest picture of `party`'s camera, if the kept beam has one by now.
    pub fn picture_of(&self, party: &[u8; 32]) -> Option<Picture> {
        self.pictures.lock().unwrap().iter().find(|(p, _)| p == party).and_then(|(_, pic)| pic.clone())
    }
}

impl Drop for ReplayVideo {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// One track's decode state.
struct TrackState {
    decoder: Option<crate::wave::h264::Decoder>,
    conv: Converter,
    rot: u8,
    /// The next event to feed, and the time of the last one fed.
    at: usize,
    last: i64,
}

impl TrackState {
    fn fresh() -> Self {
        TrackState { decoder: None, conv: Converter::new(Characterization::IDENTITY_GAMMA2), rot: 0, at: 0, last: i64::MIN }
    }
}

fn run(seed: [u8; 32], hash: [u8; 32], stop: &AtomicBool, target: &AtomicI64, pictures: &Mutex<Vec<([u8; 32], Option<Picture>)>>) {
    let Some(bytes) = crate::storage::blob_load(&seed, &hash) else {
        crate::logf!("BEAM: replay — the kept beam {}… is not here", hex::encode(&hash[..4]));
        return;
    };
    let Some(tracks) = crate::wave::beam_keep::read(&bytes) else {
        crate::logf!("BEAM: replay — the kept beam {}… did not read", hex::encode(&hash[..4]));
        return;
    };
    drop(bytes);
    *pictures.lock().unwrap() = tracks.iter().map(|t| (t.party, None)).collect();
    let mut states: Vec<TrackState> = tracks.iter().map(|_| TrackState::fresh()).collect();
    let mut rgb = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let now = target.load(Ordering::Relaxed);
        if now != i64::MIN {
            for (k, (tr, st)) in tracks.iter().zip(states.iter_mut()).enumerate() {
                // The playhead went back: this track starts over (no keyframe to seek to).
                if st.last != i64::MIN && now < st.last - BACK_SEEK {
                    *st = TrackState::fresh();
                }
                let mut shown: Option<(crate::wave::h264::I420, crate::wave::beam::Gains, u32)> = None;
                while let Some((at, ev)) = tr.events.get(st.at) {
                    if *at > now {
                        break;
                    }
                    st.at += 1;
                    st.last = *at;
                    match ev {
                        TrackEvent::Info(i) => {
                            if let Some(chr) = crate::wave::beam_session::colour_from_bytes(&i.colour) {
                                st.conv.set_characterization(chr);
                            }
                        }
                        TrackEvent::Orient(q) => st.rot = *q,
                        TrackEvent::Frame { frame_no, gains, au } => {
                            if st.decoder.is_none() {
                                st.decoder = crate::wave::h264::Decoder::new().ok();
                            }
                            if let Some(dec) = st.decoder.as_mut() {
                                if let Ok(Some(pic)) = dec.decode(au) {
                                    shown = Some((pic, *gains, *frame_no));
                                }
                            }
                        }
                    }
                }
                if let Some((pic, gains, n)) = shown {
                    st.conv.convert(&pic, gains, n, &mut rgb);
                    let (out, w, h) = rotate_rgb(&rgb, pic.w, pic.h, st.rot);
                    let holes = pic.unpainted_mask().map(|m| rotate_px(&m, pic.w / 2, pic.h / 2, st.rot, 1).0);
                    if let Some(slot) = pictures.lock().unwrap().get_mut(k) {
                        slot.1 = Some(Picture { w, h, frame_no: n, rgb: out, holes });
                    }
                }
            }
        }
        // The worker's poll on the playhead (the playback worker's own cadence) — a worker-thread measurement, never UI timing.
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}
