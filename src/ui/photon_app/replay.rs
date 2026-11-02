//! THE REPLAYED WAVE SCREEN (Nick 2026-11-01: "The UI for playing back a wave isn't super great in the molecule … make it uniform somehow" → "Wave screen replayed is good"): a kept recording plays on a screen of its own, one tile per voice whatever the size — two people or twenty — each person's face ringed by their own voice as the playback reaches it (the live wave's band powers, `LiveBands`), a scrub bar and Play/Pause in the dock, a tap on a face to leave that voice out of the mix. Opened from the wave card's Play; Back returns to the conversation and the recording keeps playing on the card.
//!
//! It is drawn over the conversation the way the image viewer is (render.rs, the viewer branch): the conversation body is skipped, the area's hit stamps are swallowed inside the frame's damage, and the controls win their rects as they draw.

use super::*;

/// Hit slots under `replay_base`: Back, Play/Pause, the scrub bar, the area swallow, then one per voice tile.
const SLOT_BACK: HitId = 0;
const SLOT_PLAY: HitId = 1;
const SLOT_BAR: HitId = 2;
const SLOT_AREA: HitId = 3;
const SLOT_TILES: HitId = 8;
/// The most voice tiles that take a tap — the playback's mute mask is 64 bits.
const MAX_TILES: usize = 1 << 6;
/// The slot span reserved in init.
pub(super) const REPLAY_HIT_IDS: HitId = SLOT_TILES + MAX_TILES as HitId;

/// One voice of the recording, in channel order.
pub(crate) struct ReplayVoice {
    /// Their party (zero = a channel the recording does not name).
    pub party: [u8; 32],
    pub name: String,
    /// The name's colour — theirs as the live wave screen draws it.
    pub tint: u32,
}

/// The replay screen's state while it is open.
pub(crate) struct WaveReplay {
    /// The recording (the playback's hash).
    pub hash: [u8; 32],
    pub title: String,
    pub voices: Vec<ReplayVoice>,
    /// Voices left out of the mix, kept here so a pause and a resume (a new playback) keep them out.
    pub muted: Vec<bool>,
    /// Where Pause left the playhead (archive slots); `None` while playing.
    pub paused_at: Option<usize>,
    /// The recording's length in archive slots, from the playback once it ran.
    pub total: usize,
    /// Each voice's face at `face_diam` (scaled before the paint borrows the app).
    pub faces: Vec<Vec<u8>>,
    pub face_diam: usize,
    /// The scrub bar's left and right edges on screen, for the tap-to-seek.
    pub bar: (f32, f32),
}

/// Where everything sits: the Back pill, the title line, the stage the tiles divide, the bar and the Play pill.
pub(super) struct ReplayGeom {
    pub back: fluor::region::Region,
    pub title_y: f32,
    pub stage: (usize, usize, usize, usize),
    pub bar: (f32, f32, f32, f32),
    pub play: fluor::region::Region,
    pub time_y: f32,
}

/// The screen's layout under `area_top` (the chrome strip, or the status bar on Android), in `unit`s.
pub(super) fn replay_geom(buf_w: usize, buf_h: usize, unit: f32, area_top: f32) -> ReplayGeom {
    let pad = unit.max(crate::ui::safe_side_px() as f32);
    let pill_h = unit * 0.62 * 1.6 * 1.4 * 0.9;
    let w = buf_w as f32;
    let h = buf_h as f32;
    let top = area_top + unit * 0.4;
    let back = fluor::region::Region::new(pad, top, ((w - pad * 2.0) * 0.28).max(unit * 0.62 * 3.0), pill_h);
    // The dock: the Play pill at the foot, the bar and its times above it.
    let play_w = ((w - pad * 2.0) * 0.36).max(unit * 3.0);
    let play = fluor::region::Region::new((w - play_w) * 0.5, h - unit * 0.6 - pill_h, play_w, pill_h);
    let bar_h = unit * 0.25;
    let bar_y = play.y - unit * 1.4;
    let time_y = bar_y - unit * 0.5;
    let stage_top = back.y + back.h + unit * 1.2;
    let stage_bottom = time_y - unit * 0.8;
    // A window shorter than the bands leaves a stage of nothing; the tiles then draw nothing, never a negative height.
    let stage_h = if stage_bottom > stage_top { stage_bottom - stage_top } else { 0.0 };
    ReplayGeom { back, title_y: back.y + back.h * 0.5, stage: (pad as usize, stage_top as usize, (w - pad * 2.0) as usize, stage_h as usize), bar: (pad, w - pad, bar_y, bar_h), play, time_y }
}

impl PhotonApp {
    /// The wave card's Play: open the replay screen on this recording and start it from the top (or keep it going if it already plays).
    pub(super) fn open_replay(&mut self, hash: [u8; 32]) {
        if self.wave_playback_hash != Some(hash) {
            self.play_recording_from(hash, 0);
            if self.wave_playback.is_none() {
                return; // refused (a live wave owns the audio) — the toast says so
            }
        }
        let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)).unwrap_or_default();
        let conv = self.active_conversation;
        // The recording's people, as its row names them; a recording kept before rows named them reads as us and, in a pair, the other person.
        let named: Vec<[u8; 32]> = conv
            .and_then(|id| self.conv_by_id(id))
            .and_then(|v| v.messages.iter().find_map(|m| m.file.as_ref().filter(|f| f.hash == hash).map(|f| f.parties.clone())))
            .unwrap_or_default();
        let gid = self.active_molecule().map(|gi| self.molecule_rosters[gi].0);
        let parties = if !named.is_empty() {
            named
        } else {
            match self.active_contact().filter(|_| gid.is_none()) {
                Some(ci) => vec![us, self.contacts[ci].handle_hash],
                None => vec![us],
            }
        };
        let nchan = self.wave_playback.as_ref().map_or(parties.len(), |p| p.nchan.max(1));
        let mut voices: Vec<ReplayVoice> = Vec::with_capacity(nchan);
        for k in 0..nchan {
            let party = parties.get(k).copied().unwrap_or([0u8; 32]);
            let (name, tint) = if party == us {
                (tr(Msg::WaveYou).into_owned(), *theme::STATUS_TEXT_COLOUR)
            } else if party == [0u8; 32] {
                (tr(Msg::ReplayVoice(k + 1)).into_owned(), *theme::LABEL_COLOUR)
            } else {
                let name = match self.contacts.iter().find(|c| !c.is_sibling && c.handle_hash == party) {
                    Some(c) => c.display_name(),
                    None => gid.map(|g| self.member_granted_name(&g, &party)).unwrap_or_default(),
                };
                (name, party_colour(&relationship_digest(&party, &us)))
            };
            voices.push(ReplayVoice { party, name, tint });
        }
        let title = match (gid, self.active_contact()) {
            (Some(g), _) => self.molecule_rosters.iter().find(|(id, _)| *id == g).map(|(_, r)| r.display_title()).unwrap_or_default(),
            (None, Some(ci)) => self.contacts[ci].display_name(),
            _ => String::new(),
        };
        let total = self.wave_playback.as_ref().map_or(0, |p| p.total);
        let n = voices.len();
        self.wave_replay = Some(WaveReplay { hash, title, voices, muted: vec![false; n], paused_at: None, total, faces: Vec::new(), face_diam: 0, bar: (0.0, 0.0) });
        crate::logf!("WAVE: replaying {}… — {} voice(s)", hex::encode(&hash[..4]), n);
        self.scene_dirty = true;
    }

    /// Back: the screen goes; the recording keeps playing on the card.
    pub(super) fn close_replay(&mut self) -> bool {
        let was = self.wave_replay.take().is_some();
        if was {
            self.scene_dirty = true;
        }
        was
    }

    /// Keep every voice's face scaled to `diameter`, before the paint borrows `self` (a face is scaled once per layout).
    pub(super) fn refresh_replay_faces(&mut self, diameter: usize) {
        let Some(rp) = self.wave_replay.as_ref() else {
            return;
        };
        if rp.face_diam == diameter && rp.faces.len() == rp.voices.len() {
            return;
        }
        let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        let proof = self.session.as_ref().map(|s| s.handle_proof).unwrap_or_default();
        let faces: Vec<Vec<u8>> = rp
            .voices
            .iter()
            .map(|v| {
                if Some(v.party) == us {
                    return match self.device_avatar_pixels.as_ref() {
                        Some(base) => crate::ui::avatar_render::update_avatar_scaled(base, crate::ui::avatar::AVATAR_SIZE, diameter),
                        None => gradient_avatar_rgb(proof_gradient_seed(&proof), diameter),
                    };
                }
                match self.contacts.iter().find(|c| !c.is_sibling && c.handle_hash == v.party) {
                    Some(c) => match c.avatar_pixels.as_ref() {
                        Some(base) => crate::ui::avatar_render::update_avatar_scaled(base, crate::ui::avatar::AVATAR_SIZE, diameter),
                        None => gradient_avatar_rgb(proof_gradient_seed(&c.handle_proof), diameter),
                    },
                    None => gradient_avatar_rgb(proof_gradient_seed(&v.party), diameter),
                }
            })
            .collect();
        if let Some(rp) = self.wave_replay.as_mut() {
            rp.faces = faces;
            rp.face_diam = diameter;
        }
    }

    /// A press on the replay screen. Returns whether it was the screen's.
    pub(super) fn on_replay_hit(&mut self, hit_id: HitId, x: f32) -> bool {
        let base = self.replay_base;
        if self.wave_replay.is_none() || base == HIT_NONE || hit_id < base || hit_id >= base + REPLAY_HIT_IDS {
            return false;
        }
        match hit_id - base {
            SLOT_BACK => {
                self.close_replay();
            }
            SLOT_PLAY => self.toggle_replay_play(),
            SLOT_BAR => {
                let Some(rp) = self.wave_replay.as_ref() else { return true };
                let (x0, x1) = rp.bar;
                let total = self.wave_playback.as_ref().map_or(rp.total, |p| p.total);
                // The tap's place along the bar, as a share of the recording (a bar of no width has no place — the tap is the start).
                let frac = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
                let slot = (frac * total as f32) as usize;
                let hash = rp.hash;
                if let Some(rp) = self.wave_replay.as_mut() {
                    rp.paused_at = None;
                }
                self.play_recording_from(hash, slot);
                self.apply_replay_mutes();
            }
            k if k >= SLOT_TILES => {
                let v = (k - SLOT_TILES) as usize;
                if let Some(rp) = self.wave_replay.as_mut() {
                    if let Some(m) = rp.muted.get_mut(v) {
                        *m = !*m;
                    }
                }
                self.apply_replay_mutes();
            }
            _ => {}
        }
        self.scene_dirty = true;
        true
    }

    /// Play/Pause: pausing remembers the playhead and lets the playback go; playing resumes from it (or from the top once the recording ran out).
    fn toggle_replay_play(&mut self) {
        let Some(hash) = self.wave_replay.as_ref().map(|r| r.hash) else { return };
        let playing = self.wave_playback_hash == Some(hash) && self.wave_playback.is_some();
        if playing {
            let at = self.wave_playback.as_ref().map(|p| p.position());
            self.wave_playback = None; // drop = stop
            self.wave_playback_hash = None;
            if let Some(rp) = self.wave_replay.as_mut() {
                rp.paused_at = at;
            }
        } else {
            let from = self.wave_replay.as_ref().and_then(|r| r.paused_at.filter(|&a| a < r.total)).unwrap_or(0);
            if let Some(rp) = self.wave_replay.as_mut() {
                rp.paused_at = None;
            }
            self.play_recording_from(hash, from);
            self.apply_replay_mutes();
        }
    }

    /// The screen's mutes onto the running playback (a new playback starts with every voice in).
    fn apply_replay_mutes(&mut self) {
        let (Some(rp), Some(pb)) = (self.wave_replay.as_ref(), self.wave_playback.as_ref()) else { return };
        if self.wave_playback_hash != Some(rp.hash) {
            return;
        }
        for (k, m) in rp.muted.iter().enumerate() {
            pb.set_muted(k, *m);
        }
        if rp.total == 0 {
            let total = pb.total;
            if let Some(rp) = self.wave_replay.as_mut() {
                rp.total = total;
            }
        }
    }
}

/// Paint the replay screen (inside the chrome borrow: everything it reads is passed in). Topmost first — controls and names before faces, faces before rings — so each wins exactly its own pixels.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_replay(
    rp: &mut WaveReplay,
    pb: Option<&crate::wave::playback::PlaybackHandle>,
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    unit: f32,
    area_top: f32,
    base: HitId,
    pressed: HitId,
    damage: fluor::canvas::PixelRect,
) {
    let g = replay_geom(buf_w, buf_h, unit, area_top);
    // The area's stale stamps go (inside the damage only — a control re-stamps only the pixels it paints, the dead-Leave class).
    restamp_hit_rect(hit_map, buf_w, buf_h, damage.x0 as isize, (area_top as isize).max(damage.y0 as isize), damage.x1 as isize, damage.y1 as isize, base + SLOT_AREA);
    draw_stub_pill_filled(canvas, text, hit_map, buf_w, buf_h, g.back, &tr(Msg::ViewerBack), base + SLOT_BACK, pressed, true, None, "Oxanium");
    text.draw_text_center(canvas, &rp.title, buf_w as f32 * 0.5, g.title_y, &TextStyle::new(unit * 0.72, *theme::CONTACT_NAME_COLOUR).weight(600).font("Oxanium"), None, None);

    // The dock: Play/Pause, the times, the bar (fill before track — first paint wins).
    let playing = pb.is_some();
    let total = pb.map_or(rp.total, |p| p.total);
    let pos = pb.map_or(rp.paused_at.unwrap_or(0), |p| p.position());
    let label = if playing { tr(Msg::PausePill) } else { tr(Msg::PlayPill) };
    draw_stub_pill_filled(canvas, text, hit_map, buf_w, buf_h, g.play, &label, base + SLOT_PLAY, pressed, true, Some(*theme::PILL_GREEN), "Oxanium");
    // Archive slots are 10 ms: a hundred to the second.
    let secs = |slots: usize| (slots / 100) as i64;
    let small = TextStyle::new(unit * 0.5, *theme::LABEL_COLOUR).weight(500).font("Oxanium");
    let (bx0, bx1, by, bh) = g.bar;
    text.draw_text_left(canvas, &super::wave_ui::fmt_duration_secs(secs(pos)), bx0, g.time_y, &small, None, None);
    let total_s = super::wave_ui::fmt_duration_secs(secs(total));
    let tw = text.measure_text(&total_s, &small);
    text.draw_text_left(canvas, &total_s, bx1 - tw, g.time_y, &small, None, None);
    let frac = if total > 0 { pos as f32 / total as f32 } else { 0.0 };
    let bw = bx1 - bx0;
    paint::fill_rect(canvas, bx0 as isize, by as isize, (bw * frac) as isize, bh as isize, *theme::PROGRESS_FILL, None, None);
    paint::fill_rect(canvas, bx0 as isize, by as isize, bw as isize, bh as isize, *theme::PROGRESS_TRACK, None, None);
    // The bar takes a tap a unit tall, centred on it — a quarter-unit line is too thin a target.
    restamp_hit_rect(hit_map, buf_w, buf_h, bx0 as isize, (by + bh * 0.5 - unit * 0.5) as isize, bx1 as isize, (by + bh * 0.5 + unit * 0.5) as isize, base + SLOT_BAR);
    rp.bar = (bx0, bx1);

    // The stage: one tile per voice, the live group screen's grid.
    let tiles = super::wave_field::face_tiles(g.stage, rp.voices.len(), unit);
    let now_fno = vsf::grid::eagle_to_sample(crate::network::time_base::now_osc()).div_euclid(crate::wave::align::FRAME);
    for (k, (v, ((tx, ty, tw, th), (fx, fy, fr)))) in rp.voices.iter().zip(tiles).enumerate() {
        let muted = rp.muted.get(k).copied().unwrap_or(false);
        let name_style = TextStyle::new(unit * 0.55, if muted { *theme::LABEL_COLOUR } else { v.tint });
        text.draw_text_center(canvas, &v.name, fx, fy + fr + unit * 0.75, &name_style, None, None);
        if muted {
            text.draw_text_center(canvas, &tr(Msg::WaveUnmute), fx, fy + fr + unit * 1.4, &TextStyle::new(unit * 0.45, *theme::LABEL_COLOUR), None, None);
        }
        if let Some(px) = rp.faces.get(k) {
            crate::ui::avatar_render::draw_avatar(canvas, fx, fy, fr, px, rp.face_diam, None);
        }
        // The ring: this voice's band powers in the frame just played, coloured as a live wave colours it; a muted voice stays dark.
        if playing && !muted {
            let p = pb.map(|p| p.frame(k)).unwrap_or_default();
            let ring = super::wave_field::env_ring_colour(Some(crate::wave::live::FrameEnv { fno: now_fno, p }));
            paint::draw_circle(canvas, fx, fy, fr + super::ring_thickness(fr), ring, None);
        }
        if k < MAX_TILES {
            restamp_hit_rect(hit_map, buf_w, buf_h, tx as isize, ty as isize, (tx + tw) as isize, (ty + th) as isize, base + SLOT_TILES + k as HitId);
        }
    }
}
