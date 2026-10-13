//! THE BEAM LADDER (Nick 2026-10-13): how big, how sharp and how fast a beam is, steered by the wave engine's once-a-second link verdict — one controller for the whole link, video the elastic part of it.
//!
//! Five CODED sizes, each double the last: 160×90, 320×180, 640×360, 1280×720, 2560×1440. An H.264 stream can only change its coded size at a keyframe, so within a band the size stays put and the DETAIL moves instead: the camera frame is shrunk to the effective size and grown back to the coded size before the encoder (beam_scale.rs), so the encoder sees a softer picture and spends fewer bits — no keyframe, no new parameter sets, any decoder plays it.
//!
//! Detail is counted in HALF-STOPS of effective width above 160: `d = 2·log2(width/160)`. Band `b` (coded width 160·2^b) covers `d ∈ [2b−4, 2b]`: from its own size down to a quarter of its width, two stops. Adjacent bands OVERLAP by one stop (`[2b−4, 2b−2]`), and the ladder changes band only on leaving the current band's range: down when `d` falls below its bottom, up when `d` passes its top. Inside the overlap it stays where it is — so a link teetering on an edge sits in the LOWER band (Nick: "it doesn't jump back and forth if the quality is teetering on an edge and instead sticks in the lower"), and the only keyframes are real band crossings.
//!
//! Below the smallest band (`d < 0`) the lever is FRAME RATE, which costs nothing to change: 30 fps at `d = 0`, halving per stop, floored at 5.
//!
//! The verdict (wave/mod.rs `LINK_VERDICT`, published by the audio engine each second): CONGESTED → down one half-stop at once; CLEAN → up one half-stop after [`CLIMB_CLEAN_VERDICTS`] in a row; anything else holds. Edges, not timers: the ladder moves only when a new verdict lands.

/// The coded sizes, smallest first.
pub const BANDS: [(u16, u16); 5] = [(160, 90), (320, 180), (640, 360), (1280, 720), (2560, 1440)];
/// Clean verdicts in a row before a climb of one half-stop.
pub const CLIMB_CLEAN_VERDICTS: u32 = 3;
/// The lowest detail: two stops of frame rate below the smallest band.
pub const D_MIN: i32 = -4;

/// What the encoder should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Index into [`BANDS`] — the coded size.
    pub band: usize,
    /// Softness: half-stops of effective width below the coded width (0 = full detail, 4 = a quarter of the width).
    pub soft: u8,
    pub fps: u32,
    pub bitrate: u32,
}

#[derive(Debug, Clone)]
pub struct Ladder {
    band: usize,
    d: i32,
    max_band: usize,
    clean_run: u32,
    last_seq: u32,
    pub band_changes: u32,
}

impl Ladder {
    /// Start at full detail of the middle band (640×360), or the highest the caller allows if that is lower.
    pub fn new(max_band: usize) -> Self {
        let max_band = max_band.min(BANDS.len() - 1);
        let band = 2usize.min(max_band);
        Self { band, d: 2 * band as i32, max_band, clean_run: 0, last_seq: 0, band_changes: 0 }
    }

    /// The ceiling can move (a metered network, a LAN path appearing): a lower ceiling takes effect at once.
    pub fn set_max_band(&mut self, max_band: usize) {
        self.max_band = max_band.min(BANDS.len() - 1);
        if self.d > 2 * self.max_band as i32 {
            self.d = 2 * self.max_band as i32;
        }
        self.settle_band();
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
        self.d = self.d.clamp(D_MIN, 2 * self.max_band as i32);
        self.settle_band();
        self.plan() != before
    }

    /// Change band only on leaving the current band's range — the one-stop overlap is where it holds.
    fn settle_band(&mut self) {
        let start = self.band;
        while self.band < self.max_band && self.d > 2 * self.band as i32 {
            self.band += 1;
        }
        while self.band > 0 && self.d < 2 * self.band as i32 - 4 {
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
        // Softness counts only detail at or above the smallest band's width; below it (d < 0) the lever is frame rate alone — never a picture narrower than 160.
        let soft = (2 * self.band as i32 - self.d.max(0)).clamp(0, 4) as u8;
        // Below the smallest band, frame rate: 30 at d = 0, halving per stop (two half-steps), floored at 5.
        let fps = if self.d >= 0 { 30 } else { ((30.0 * 2f64.powf(self.d as f64 / 2.0)).round() as u32).max(5) };
        let (w, h) = BANDS[self.band];
        Plan { band: self.band, soft, fps, bitrate: bitrate_for(w as u32, h as u32, soft, fps) }
    }
}

/// The bits a plan needs: ~0.1 bit per EFFECTIVE pixel per frame (decent H.264 at a real-time preset), never below the coded size's per-macroblock floor (each 16×16 block costs a few bits however soft the picture — the reason softness alone cannot carry a big coded size to a tiny bitrate).
pub fn bitrate_for(w: u32, h: u32, soft: u8, fps: u32) -> u32 {
    let scale = 2f64.powf(-(soft as f64) / 2.0);
    let eff = (w as f64 * scale) * (h as f64 * scale);
    let content = eff * fps as f64 * 0.1;
    let blocks = ((w + 15) / 16 * ((h + 15) / 16)) as f64;
    let floor = blocks * 3.0 * fps as f64;
    content.max(floor).clamp(24_000.0, 16_000_000.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_full_detail_of_the_middle_band() {
        let l = Ladder::new(4);
        assert_eq!(l.plan(), Plan { band: 2, soft: 0, fps: 30, bitrate: bitrate_for(640, 360, 0, 30) });
        assert_eq!(Ladder::new(1).plan().band, 1, "a lower ceiling starts lower");
    }

    #[test]
    fn congestion_softens_first_and_crosses_a_band_only_below_its_bottom() {
        let mut l = Ladder::new(4);
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
        let mut l = Ladder::new(4);
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
        let mut l = Ladder::new(3);
        let mut seq = 0u32;
        for _ in 0..200 {
            seq += 1;
            l.verdict(seq, false, true);
        }
        assert_eq!(l.plan(), Plan { band: 3, soft: 0, fps: 30, bitrate: bitrate_for(1280, 720, 0, 30) });
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
        assert!(bitrate_for(1280, 720, 4, 30) > bitrate_for(320, 180, 0, 30));
    }
}
