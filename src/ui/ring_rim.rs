//! The ring rim (Nick 2026-09-03): the incoming-wave pulse replaced by ONE continuous contour around the avatar whose radius undulates — a Fourier ring, r(θ,t) = R₀ + Σ Aᵢ·sin(kᵢθ + φᵢ + ωᵢt).
//!
//! The harmonics are not invented: they are the contact's BELL CASTING (`chirp::casting_from_hash`) mapped to angle — each audio partial's frequency ratio becomes an angular harmonic index, its amplitude the deformation weight. A sparse glassy chime rings as a slow two-lobe swell; a bright ten-partial bell as a busy shimmering rim. Colour from the digest, ring from the digest, motion from the digest — one identity, three senses.
//!
//! Integer harmonic indices make closure free (the contour meets itself at θ = 2π by construction — no wraparound seam, the pain a noise field would bring). k = 1 is skipped on principle: it translates the circle instead of deforming it. Total deformation is capped star-shaped (Σ|Aᵢ| ≤ 0.18·R₀) so the contour never self-intersects and never touches the avatar. Drift speeds are digest-drawn, slow, sign-alternating, and mutually incommensurate — the pattern evolves without ever visibly looping, beating partials in angle.
//!
//! Pure function of (digest, now): no stored animation state, the same wave always greets you from the same person. No expansion — the drift supplies the life, the ring sound supplies the urgency (the three radiating discs retired here).

/// One angular harmonic: an audio partial mapped to the rim.
#[derive(Clone, Copy, Debug)]
pub struct Term {
    /// Angular harmonic index (2..=9) — the partial's frequency ratio, rounded and clamped.
    pub k: f32,
    /// Deformation weight, normalized so all terms sum to 1 (scaled by the cap × R₀ at draw).
    pub weight: f32,
    /// Initial phase, digest-drawn.
    pub phase: f32,
    /// Drift speed in rad/s — slow, sign-alternating by term order.
    pub speed: f32,
}

/// Star-shape cap: Σ deformation ≤ this fraction of R₀ — one radius per angle, never self-intersecting, never touching the avatar.
const DEFORM_CAP: f32 = 0.18;
/// Angular resolution of the per-frame radius table. 1024 entries ≈ 0.35°/step — beyond what a soft band can show.
const LUT_N: usize = 1024;

/// Map the contact's bell casting onto rim harmonics. Deterministic in the digest; call once per ring and cache.
pub fn terms_for(digest: &[u8; 32]) -> Vec<Term> {
    let casting = chirp::Chirp::casting_from_hash(*digest);
    let total: f64 = casting.iter().map(|(_, a)| a).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    casting
        .iter()
        .enumerate()
        .map(|(i, &(ratio, amp))| {
            // The partial's frequency ratio IS the angular frequency: ratio 2 → two lobes, ratio 5 → five. Sub-harmonics (the bell's 0.5 hum) and the prime land at k=2 — the gentlest deformation a circle can carry.
            let k = (ratio.round() as f32).clamp(2.0, 9.0);
            let phase = (chirp::channel_unit(&format!("rim phase {i}"), digest) * std::f64::consts::TAU) as f32;
            // 0.1..0.5 rad/s, sign alternating by order — incommensurate drifts, the pattern never visibly loops.
            let mag = (0.1 + chirp::channel_unit(&format!("rim speed {i}"), digest) * 0.4) as f32;
            let speed = if i % 2 == 0 { mag } else { -mag };
            Term { k, weight: (amp / total) as f32, phase, speed }
        })
        .collect()
}

/// Paint the rim: a soft translucent band centered on r(θ,t), same alpha-under composition as `paint::draw_circle`. `r0` = base radius, `half_w` = the band's half-width (soft falloff both sides), `t_secs` = absolute seconds (eagle osc / rate), `colour` = α+darkness packed (the α byte is the PEAK band alpha).
pub fn draw(
    canvas: &mut fluor::canvas::Canvas,
    cx: f32,
    cy: f32,
    r0: f32,
    half_w: f32,
    terms: &[Term],
    t_secs: f64,
    colour: u32,
) {
    use fluor::pixel::Blend;
    if terms.is_empty() || r0 <= 0.0 || half_w <= 0.0 {
        return;
    }
    let deform = DEFORM_CAP * r0;
    // Per-frame radius table over θ ∈ [0, 2π): the sin sum runs LUT_N × terms times here, once — pixels below just index it.
    let mut lut = [0f32; LUT_N];
    let (mut r_min, mut r_max) = (f32::MAX, 0f32);
    for (j, slot) in lut.iter_mut().enumerate() {
        let theta = j as f32 / LUT_N as f32 * std::f32::consts::TAU;
        let mut r = r0;
        for term in terms {
            r += deform
                * term.weight
                * (term.k * theta + term.phase + term.speed * t_secs as f32).sin();
        }
        *slot = r;
        r_min = r_min.min(r);
        r_max = r_max.max(r);
    }
    let (inner, outer) = (r_min - half_w, r_max + half_w);
    let (inner2, outer2) = ((inner.max(0.0)) * (inner.max(0.0)), outer * outer);

    let width = canvas.width;
    let height = canvas.height;
    let x_start = ((cx - outer) as i32).max(0) as usize;
    let x_end = (((cx + outer + 1.0) as i32).max(0) as usize).min(width);
    let y_start = ((cy - outer) as i32).max(0) as usize;
    let y_end = (((cy + outer + 1.0) as i32).max(0) as usize).min(height);
    if x_start >= x_end || y_start >= y_end {
        return;
    }
    canvas
        .damage
        .add_bounds(x_start, y_start, x_end, y_end);

    let peak_alpha = ((colour >> 24) & 0xFF) as f32;
    let dark = colour & 0x00FF_FFFF;
    let inv_w = 1.0 / half_w;
    let idx_per_rad = LUT_N as f32 / std::f32::consts::TAU;

    for py in y_start..y_end {
        let dy = (py as f32 + 0.5) - cy;
        let dy2 = dy * dy;
        let row = &mut canvas.pixels[py * width..(py + 1) * width];
        for px in x_start..x_end {
            let dx = (px as f32 + 0.5) - cx;
            let dist2 = dx * dx + dy2;
            // The annulus reject does the heavy lifting: only pixels near the band pay for the sqrt + atan2.
            if dist2 < inner2 || dist2 > outer2 {
                continue;
            }
            let dist = dist2.sqrt();
            let theta = dy.atan2(dx);
            let j = ((theta + std::f32::consts::TAU) * idx_per_rad) as usize % LUT_N;
            let delta = (dist - lut[j]).abs();
            if delta >= half_w {
                continue;
            }
            // Smooth falloff both sides: t² reads as a field, not a stroked line.
            let t = 1.0 - delta * inv_w;
            let a = (peak_alpha * t * t) as u32;
            if a > 0 {
                row[px] = row[px].under((a << 24) | dark, fluor::BlendMode::Normal);
            }
        }
    }
}
