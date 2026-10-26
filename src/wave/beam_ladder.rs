//! THE BEAM LADDER (Nick 2026-10-13): how big, how sharp and how fast a beam is, steered by the wave engine's once-a-second link verdict — one controller for the whole link, video the elastic part of it.
//!
//! The CODED sizes come from the CAPTURE (Nick 2026-10-14: "sampling what we have available and working with it… cap the framerate at 30 and the megapixels at 5 ish… ideally uncropped"): the top band IS the camera's frame, at whatever aspect the camera gives, and each band below is an exact half of the one above — the sensor's own picture binned 2:1, down to [`MIN_BAND_WIDTH`], at most [`MAX_BANDS`] of them ([`bands_for`]). A 2560×1920 capture gives 160×120, 320×240, 640×480, 1280×960, 2560×1920; a 1280×720 webcam 160×90 … 1280×720. A camera switch mid-beam rebuilds the bands from the new capture and keeps the link's detail ([`Ladder::rebuild`]). An H.264 stream can only change its coded size at a keyframe, so within a band the size stays put and the DETAIL moves instead: the camera frame is shrunk to the effective size and grown back to the coded size before the encoder (beam_scale.rs), so the encoder sees a softer picture and spends fewer bits — no keyframe, no new parameter sets, any decoder plays it.
//!
//! Detail is counted in HALF-STOPS of effective width above the smallest band: `d = 2·log2(width/smallest)`. Band `b` (coded width 160·2^b) covers `d ∈ [2b−4, 2b]`: from its own size down to a quarter of its width, two stops. Adjacent bands OVERLAP by one stop (`[2b−4, 2b−2]`), and the ladder changes band only on leaving the current band's range: down when `d` falls below its bottom, up when `d` passes its top. Inside the overlap it stays where it is — so a link teetering on an edge sits in the LOWER band (Nick: "it doesn't jump back and forth if the quality is teetering on an edge and instead sticks in the lower"), and the only keyframes are real band crossings.
//!
//! Below the smallest band (`d < 0`) the lever is FRAME RATE, which costs nothing to change: 30 fps at `d = 0`, halving per stop, floored at 5.
//!
//! The verdict (wave/mod.rs `LINK_VERDICT`, published by the audio engine each second): CONGESTED → down one half-stop at once; CLEAN → up one half-stop after [`CLIMB_CLEAN_VERDICTS`] in a row; anything else holds. Edges, not timers: the ladder moves only when a new verdict lands.

/// The smallest band is at least this wide.
pub const MIN_BAND_WIDTH: usize = 120;
/// At most this many bands below and including the capture.
pub const MAX_BANDS: usize = 5;

/// The coded sizes for a capture, smallest first: the capture itself, then halves (even dimensions) down to [`MIN_BAND_WIDTH`].
pub fn bands_for(w: usize, h: usize) -> Vec<(u16, u16)> {
    // WHY/PROOF: the capture size is the camera's (OS) — the u16 casts below would wrap a size past 65535, and H.264 codes no plane under 2 or past 8190 a side.
    let (tw, th) = ((w & !1).clamp(2, 8190), (h & !1).clamp(2, 8190));
    let mut v = vec![(tw as u16, th as u16)];
    for k in 1..MAX_BANDS {
        let bw = (tw >> k) & !1;
        if bw < MIN_BAND_WIDTH {
            break;
        }
        let bh = (((th * bw + tw / 2) / tw) & !1).max(2);
        v.push((bw as u16, bh as u16));
    }
    v.reverse();
    v
}

/// The highest band no wider than `max_w` (the smallest band when none is).
pub fn top_within(bands: &[(u16, u16)], max_w: usize) -> usize {
    bands.iter().rposition(|&(w, _)| w as usize <= max_w).unwrap_or(0)
}
/// Clean verdicts in a row before a climb of one half-stop.
pub const CLIMB_CLEAN_VERDICTS: u32 = 3;
/// The lowest detail: two stops of frame rate below the smallest band.
pub const D_MIN: i32 = -4;

/// The user's beam settings as the ladder reads them (Settings → Beams, beam_settings.rs): a frame-rate ceiling, the bits each effective pixel gets, an optional bitrate ceiling, and which lever congestion pulls first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    pub fps_cap: u32,
    /// Bits per effective pixel per frame, in thousandths (100 = 0.1, the default).
    pub bpp_milli: u32,
    /// No plan spends more than this (bit/s): the ladder's top rung becomes the highest whose bitrate fits.
    pub bitrate_cap: Option<u32>,
    /// Congestion sheds FRAME RATE first ([`FPS_FIRST`] half-stops, to a quarter of the cap) and only then detail; false sheds detail first (the default: a beam stays smooth).
    pub keep_detail: bool,
}

impl Tuning {
    pub const DEFAULT: Tuning = Tuning { fps_cap: 30, bpp_milli: 100, bitrate_cap: None, keep_detail: false };
}

/// The half-stops of frame rate a keep-detail ladder spends before it touches detail: two stops, a quarter of the cap.
pub const FPS_FIRST: i32 = 4;
/// No beam runs slower than this.
pub const FPS_FLOOR: u32 = 5;

/// What the encoder should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Index into the ladder's bands — the coded size, `w`×`h`.
    pub band: usize,
    pub w: u16,
    pub h: u16,
    /// Softness: half-stops of effective width below the coded width (0 = full detail, 4 = a quarter of the width).
    pub soft: u8,
    pub fps: u32,
    pub bitrate: u32,
}

#[derive(Debug, Clone)]
pub struct Ladder {
    bands: Vec<(u16, u16)>,
    band: usize,
    d: i32,
    max_band: usize,
    clean_run: u32,
    last_seq: u32,
    pub band_changes: u32,
    tuning: Tuning,
}

impl Ladder {
    /// Start at full detail of the third band (640 wide for a 2560 capture), or the highest the caller allows if that is lower.
    pub fn new(bands: Vec<(u16, u16)>, max_band: usize) -> Self {
        let bands = if bands.is_empty() { vec![(160, 120)] } else { bands };
        let max_band = max_band.min(bands.len() - 1);
        let band = 2usize.min(max_band);
        Self { bands, band, d: 2 * band as i32, max_band, clean_run: 0, last_seq: 0, band_changes: 0, tuning: Tuning::DEFAULT }
    }

    /// A new capture mid-beam (a camera switch): new bands and ceiling, the link's detail kept — clamped into the new range and settled into a band. Counts as a band change when the coded size moves.
    pub fn rebuild(&mut self, bands: Vec<(u16, u16)>, max_band: usize) {
        if bands.is_empty() {
            return;
        }
        let before = self.bands[self.band];
        self.bands = bands;
        self.max_band = max_band.min(self.bands.len() - 1);
        self.band = self.band.min(self.max_band);
        // WHY/PROOF: the ladder's rungs, D_MIN up to the ceiling's top half-stop — the new capture can have fewer bands, and a detail past them names no band.
        self.d = self.d.clamp(D_MIN, self.top());
        let start = self.band_changes;
        self.settle_band();
        if self.band_changes == start && self.bands[self.band] != before {
            self.band_changes += 1;
        }
    }

    pub fn bands(&self) -> &[(u16, u16)] {
        &self.bands
    }

    /// The ceiling can move (a metered network, a LAN path appearing): a lower ceiling takes effect at once.
    pub fn set_max_band(&mut self, max_band: usize) {
        self.max_band = max_band.min(self.bands.len() - 1);
        self.d = self.d.min(self.top());
        self.settle_band();
    }

    /// New settings (Settings → Beams), live: a change of shed order keeps the picture's detail (a keep-detail ladder below its top runs [`FPS_FIRST`] half-stops slow — that is its trade), a lower ceiling or a tighter bitrate cap takes effect at once.
    pub fn set_tuning(&mut self, t: Tuning) {
        if t.keep_detail != self.tuning.keep_detail {
            let (detail, slow) = self.split(self.d);
            let full = 2 * self.max_band as i32;
            self.d = if t.keep_detail && detail >= full { full + FPS_FIRST - slow.min(FPS_FIRST) } else { detail };
        }
        self.tuning = t;
        self.d = self.d.clamp(D_MIN, self.top());
        self.settle_band();
    }

    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    /// The highest rung: full detail of the ceiling band (plus the frame-rate rungs of a keep-detail ladder), lowered until the plan fits the bitrate cap.
    fn top(&self) -> i32 {
        let mut top = 2 * self.max_band as i32 + if self.tuning.keep_detail { FPS_FIRST } else { 0 };
        if let Some(cap) = self.tuning.bitrate_cap {
            while top > D_MIN && self.raw_plan(top, Self::band_for(self.split(top).0, self.max_band)).bitrate > cap {
                top -= 1;
            }
        }
        top
    }

    /// The lowest band whose range holds a detail (the overlap's lower band), within the ceiling.
    fn band_for(detail: i32, max_band: usize) -> usize {
        // WHY/PROOF: detail runs D_MIN..=2·max_band; a negative detail is the smallest band, and (detail + 1) / 2 is at most max_band.
        ((detail.max(0) + 1) / 2).min(max_band as i32) as usize
    }

    /// A rung → (detail in half-stops above the smallest band, half-stops of frame rate below the cap).
    fn split(&self, d: i32) -> (i32, i32) {
        let full = 2 * self.max_band as i32;
        if !self.tuning.keep_detail {
            (d, (-d).max(0))
        } else if d > full {
            (full, full + FPS_FIRST - d)
        } else {
            (d, FPS_FIRST + (-d).max(0))
        }
    }

    /// Feed the link verdict (`seq` increments once per verdict; a repeat is ignored). Returns true when the plan changed.
    pub fn verdict(&mut self, seq: u32, congested: bool, clean: bool) -> bool {
        if seq == self.last_seq {
            return false;
        }
        self.last_seq = seq;
        let before = self.plan();
        if congested {
            self.d -= 1;
            self.clean_run = 0;
        } else if clean {
            self.clean_run += 1;
            if self.clean_run >= CLIMB_CLEAN_VERDICTS {
                self.d += 1;
                self.clean_run = 0;
            }
        } else {
            self.clean_run = 0;
        }
        // WHY/PROOF: the verdicts are the LINK's, and a congested run or a long clean one walks `d` without end — held to the ladder's rungs so a recovery climbs from the floor, not from a debt of stops below it.
        self.d = self.d.clamp(D_MIN, self.top());
        self.settle_band();
        self.plan() != before
    }

    /// Change band only on leaving the current band's range — the one-stop overlap is where it holds.
    fn settle_band(&mut self) {
        let start = self.band;
        let detail = self.split(self.d).0;
        while self.band < self.max_band && detail > 2 * self.band as i32 {
            self.band += 1;
        }
        while self.band > 0 && detail < 2 * self.band as i32 - 4 {
            self.band -= 1;
        }
        if self.band > self.max_band {
            self.band = self.max_band;
        }
        if self.band != start {
            self.band_changes += 1;
        }
    }

    pub fn detail(&self) -> i32 {
        self.d
    }

    pub fn plan(&self) -> Plan {
        self.plan_at(self.d, self.band)
    }

    fn plan_at(&self, d: i32, band: usize) -> Plan {
        let mut p = self.raw_plan(d, band);
        if let Some(c) = self.tuning.bitrate_cap {
            // The floor rung may still cost more than a tiny cap: the cap wins, the picture goes blocky rather than over budget.
            p.bitrate = p.bitrate.min(c.max(24_000));
        }
        p
    }

    /// A rung's plan at what it costs, before the bitrate cap.
    fn raw_plan(&self, d: i32, band: usize) -> Plan {
        let (detail, slow) = self.split(d);
        // Softness counts only detail at or above the smallest band's width; below it (detail < 0) the lever is frame rate alone — never a picture narrower than 160.
        // WHY/PROOF: softness is designed for 0-4 half-stops (the resampler's range) and an int→int `as u8` WRAPS — a band picked above the detail would ask for more, a negative is none.
        let soft = (2 * band as i32 - detail.max(0)).clamp(0, 4) as u8;
        // Frame rate: the cap, halving per stop (two half-steps) of `slow`, floored.
        let cap = self.tuning.fps_cap.max(FPS_FLOOR);
        let fps = if slow == 0 { cap } else { ((cap as f64 * 2f64.powf(-slow as f64 / 2.0)).round() as u32).max(FPS_FLOOR) };
        let (w, h) = self.bands[band];
        Plan { band, w, h, soft, fps, bitrate: bitrate_for(w as u32, h as u32, soft, fps, self.tuning.bpp_milli) }
    }
}

/// The bits a plan needs: `bpp_milli` thousandths of a bit per EFFECTIVE pixel per frame (100, ~0.1 bit, is decent H.264 at a real-time preset), never below the coded size's per-macroblock floor (each 16×16 block costs a few bits however soft the picture — the reason softness alone cannot carry a big coded size to a tiny bitrate).
pub fn bitrate_for(w: u32, h: u32, soft: u8, fps: u32, bpp_milli: u32) -> u32 {
    let scale = 2f64.powf(-(soft as f64) / 2.0);
    let eff = (w as f64 * scale) * (h as f64 * scale);
    let content = eff * fps as f64 * bpp_milli as f64 / 1000.0;
    let blocks = ((w + 15) / 16 * ((h + 15) / 16)) as f64;
    let floor = blocks * 3.0 * fps as f64;
    // WHY/PROOF: the size comes from the camera (OS) — the encoder's working range, 24 kbit/s to 16 Mbit/s, holds a tiny or enormous capture to a rate the encoder accepts and the link can carry.
    content.max(floor).clamp(24_000.0, 16_000_000.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 16:9 ladder a 2560×1440 capture makes — the old fixed bands.
    fn wide() -> Vec<(u16, u16)> {
        bands_for(2560, 1440)
    }

    #[test]
    fn bands_are_the_capture_halved_at_its_own_aspect() {
        assert_eq!(wide(), vec![(160, 90), (320, 180), (640, 360), (1280, 720), (2560, 1440)]);
        assert_eq!(bands_for(2560, 1920), vec![(160, 120), (320, 240), (640, 480), (1280, 960), (2560, 1920)]);
        assert_eq!(bands_for(1280, 720), vec![(160, 90), (320, 180), (640, 360), (1280, 720)], "stops above 120 wide");
        assert_eq!(bands_for(1720, 1224).last(), Some(&(1720, 1224)));
        assert!(bands_for(1720, 1224).iter().all(|&(w, h)| w % 2 == 0 && h % 2 == 0 && w as usize >= MIN_BAND_WIDTH));
        assert_eq!(top_within(&bands_for(2560, 1920), 1280), 3);
    }

    #[test]
    fn a_camera_switch_keeps_the_detail() {
        let mut l = Ladder::new(wide(), 4);
        let d = l.detail();
        l.rebuild(bands_for(2560, 1920), 4);
        assert_eq!((l.detail(), l.plan().w, l.plan().h), (d, 640, 480), "same rung, the new camera's aspect");
        assert_eq!(l.band_changes, 1, "the coded size moved — the encoder restarts");
        l.rebuild(bands_for(1280, 720), 3);
        assert!(l.plan().band <= 3);
    }

    #[test]
    fn starts_at_full_detail_of_the_middle_band() {
        let l = Ladder::new(wide(), 4);
        assert_eq!(l.plan(), Plan { band: 2, w: 640, h: 360, soft: 0, fps: 30, bitrate: bitrate_for(640, 360, 0, 30, 100) });
        assert_eq!(Ladder::new(wide(), 1).plan().band, 1, "a lower ceiling starts lower");
    }

    #[test]
    fn congestion_softens_first_and_crosses_a_band_only_below_its_bottom() {
        let mut l = Ladder::new(wide(), 4);
        let mut seq = 0;
        let mut step = |l: &mut Ladder, c: bool, k: bool| {
            seq += 1;
            l.verdict(seq, c, k);
        };
        // Band 2 runs d 4 → 0; four congested verdicts soften it to a quarter width without leaving the band.
        for _ in 0..4 {
            step(&mut l, true, false);
        }
        assert_eq!((l.plan().band, l.plan().soft, l.detail()), (2, 4, 0));
        assert_eq!(l.band_changes, 0);
        // One more: below band 2's bottom → band 1, where d = −1 sits a half-stop under its full-detail top… and frame rate starts to fall (d < 0).
        step(&mut l, true, false);
        assert_eq!(l.plan().band, 1);
        assert_eq!(l.band_changes, 1);
    }

    #[test]
    fn teetering_in_the_overlap_sticks_in_the_lower_band() {
        let mut l = Ladder::new(wide(), 4);
        let mut seq = 0u32;
        // Drive down into band 1 (d = −1), then climb back up into the overlap [0, 2] and wobble there.
        for _ in 0..5 {
            seq += 1;
            l.verdict(seq, true, false);
        }
        assert_eq!(l.plan().band, 1);
        // Climb to band 1's TOP (d = 2), the upper edge of the overlap with band 2: three climbs of three clean verdicts.
        for _ in 0..9 {
            seq += 1;
            l.verdict(seq, false, true);
        }
        assert_eq!((l.plan().band, l.detail()), (1, 2));
        let changes = l.band_changes;
        // Now teeter on that edge: one congested verdict, then three clean — d swings 2 → 1 → 2, never past 2.
        for i in 0..60 {
            seq += 1;
            let congested = i % 4 == 0;
            l.verdict(seq, congested, !congested);
            assert!(l.detail() <= 2, "never past band 1's top, so never back up to band 2");
            assert_eq!(l.plan().band, 1, "inside the overlap it holds the lower band");
        }
        assert_eq!(l.band_changes, changes, "no band flapping while teetering");
    }

    #[test]
    fn a_clean_link_climbs_to_the_ceiling_and_frame_rate_is_the_bottom_lever() {
        let mut l = Ladder::new(wide(), 3);
        let mut seq = 0u32;
        for _ in 0..200 {
            seq += 1;
            l.verdict(seq, false, true);
        }
        assert_eq!(l.plan(), Plan { band: 3, w: 1280, h: 720, soft: 0, fps: 30, bitrate: bitrate_for(1280, 720, 0, 30, 100) });
        for _ in 0..200 {
            seq += 1;
            l.verdict(seq, true, false);
        }
        let p = l.plan();
        assert_eq!((p.band, p.soft), (0, 0), "the smallest band at full detail");
        assert!(p.fps <= 8 && p.fps >= 5, "two stops of frame rate below it, floored at 5, got {}", p.fps);
        // A repeated verdict sequence number is not a new verdict.
        let before = l.plan();
        assert!(!l.verdict(seq, false, true));
        assert_eq!(l.plan(), before);
    }

    #[test]
    fn the_macroblock_floor_binds_a_big_coded_size() {
        // 1280×720 softened two stops still costs more than 320×180 at full detail — why bands, not softness alone, carry the bottom.
        assert!(bitrate_for(1280, 720, 4, 30, 100) > bitrate_for(320, 180, 0, 30, 100));
    }
    #[test]
    fn the_frame_rate_cap_and_quality_shape_every_plan() {
        let mut l = Ladder::new(wide(), 4);
        l.set_tuning(Tuning { fps_cap: 15, bpp_milli: 200, ..Tuning::DEFAULT });
        let p = l.plan();
        assert_eq!(p.fps, 15, "the cap is the top frame rate");
        assert_eq!(p.bitrate, bitrate_for(640, 360, 0, 15, 200), "twice the bits per pixel");
        assert!(bitrate_for(640, 360, 0, 15, 200) > bitrate_for(640, 360, 0, 15, 100));
    }

    #[test]
    fn a_bitrate_cap_lowers_the_top_rung() {
        let mut l = Ladder::new(wide(), 4);
        let cap = bitrate_for(640, 360, 0, 30, 100);
        l.set_tuning(Tuning { bitrate_cap: Some(cap), ..Tuning::DEFAULT });
        let mut seq = 0u32;
        for _ in 0..200 {
            seq += 1;
            l.verdict(seq, false, true);
        }
        let p = l.plan();
        assert!(p.bitrate <= cap, "never over the cap: {} > {}", p.bitrate, cap);
        assert!(p.band <= 3 && (p.w, p.soft) != (2560, 0), "a clean link climbs only as far as the cap allows");
    }

    #[test]
    fn keep_detail_sheds_frame_rate_first() {
        let mut l = Ladder::new(wide(), 4);
        l.set_tuning(Tuning { keep_detail: true, ..Tuning::DEFAULT });
        let start = l.plan();
        assert_eq!((start.band, start.soft, start.fps), (2, 0, 8), "the change of shed order keeps the detail; below the top a keep-detail ladder runs two stops slow");
        let mut seq = 0u32;
        for _ in 0..200 {
            seq += 1;
            l.verdict(seq, false, true);
        }
        assert_eq!((l.plan().band, l.plan().soft, l.plan().fps), (4, 0, 30), "a clean link reaches the top: full detail, full rate");
        for _ in 0..4 {
            seq += 1;
            l.verdict(seq, true, false);
        }
        let p = l.plan();
        assert_eq!((p.band, p.soft), (4, 0), "four congested verdicts: detail untouched");
        assert!(p.fps <= 8, "two stops of frame rate gone first, got {}", p.fps);
        seq += 1;
        l.verdict(seq, true, false);
        assert_eq!(l.plan().soft, 1, "then detail");
        // Back to the default order: the same picture, the frame rate restored.
        l.set_tuning(Tuning::DEFAULT);
        assert_eq!((l.plan().band, l.plan().soft, l.plan().fps), (4, 1, 30));
    }
}
