//! SETTINGS → BEAMS (docs/beams.md "Settings", Nick 2026-10-26): what this device asks of the beams it sends — HDR (`g2t`), a size cap, a frame-rate cap, the bits each pixel gets, a bitrate cap, and what congestion sheds first.
//! The settings page writes them on a tap; each sender reads them thru one generation counter on its own frame edge, so a change lands live: the ladder retunes and its ceiling moves at once, and the HDR capture follows the next camera start.

use super::beam_ladder::Tuning;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

/// One device's beam settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeamSettings {
    /// Send HDR: capture 10-bit HLG where the camera offers it and put `g2t` codes on the wire, light above white kept (only on a device whose cameras can, [`hdr_capable`]).
    pub hdr: bool,
    /// The widest coded size the ladder may climb to; `None` = the link decides (the whole capture on a LAN path, 1280 wide off it, two bands lower on a metered network).
    pub size_cap: Option<u32>,
    pub tuning: Tuning,
}

impl BeamSettings {
    pub const DEFAULT: BeamSettings = BeamSettings { hdr: false, size_cap: None, tuning: Tuning::DEFAULT };
}

/// The choices the page offers, largest first.
pub const SIZE_CAPS: [u32; 4] = [1920, 1280, 640, 320];
pub const FPS_CAPS: [u32; 3] = [30, 24, 15];
/// Bits per effective pixel per frame, in thousandths: low, standard, high.
pub const QUALITIES: [u32; 3] = [60, 100, 180];
pub const BITRATE_CAPS: [u32; 4] = [4_000_000, 2_000_000, 1_000_000, 500_000];

static CURRENT: Mutex<BeamSettings> = Mutex::new(BeamSettings::DEFAULT);
static GENERATION: AtomicU32 = AtomicU32::new(0);
static HDR_CAPABLE: AtomicBool = AtomicBool::new(false);

pub fn current() -> BeamSettings {
    *CURRENT.lock().unwrap()
}

/// Bumped by every change: a sender holding an older number retunes on its next frame.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// Replace the settings; a no-op when nothing changed.
pub fn set(s: BeamSettings) {
    let mut g = CURRENT.lock().unwrap();
    if *g != s {
        *g = s;
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Can this device's cameras send HDR? The phone says so at startup from its cameras' 10-bit HLG profiles; a desktop camera (V4L2, AVFoundation) is 8-bit SDR, so it stays false and the page shows HDR greyed out.
pub fn hdr_capable() -> bool {
    HDR_CAPABLE.load(Ordering::Relaxed)
}

pub fn set_hdr_capable(capable: bool) {
    HDR_CAPABLE.store(capable, Ordering::Relaxed);
}

/// Should the next camera start capture HDR?
pub fn hdr_wanted() -> bool {
    hdr_capable() && current().hdr
}
