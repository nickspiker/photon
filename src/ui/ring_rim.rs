//! The incoming-wave circle (Nick 2026-09-04): ONE perfect circle behind the avatar, alive thru its transform — digest-keyed waveforms drive its x/y offset, a rotation decouples those from the axes (plain sine/cosine rotation of the offset vector), a fourth scales it, a fifth breathes its opacity. The circle strictly moves, scales, and fades; it is never deformed (the Fourier-rim experiment retired 2026-09-04 — an interpolated mess in the field).
//!
//! Every waveform is a pair of sines at digest-drawn incommensurate frequencies, DELIBERATELY ABOVE the display refresh (Nick 2026-09-04: "the phase should be higher than the refresh so it jutters around" — 256× the first cut's crawl): each frame samples an unrelated point of the waveform, so the circle jitters alive instead of drifting imperceptibly. The same digest still greets you with the same jitter (pure function of digest + now, no stored animation state).
//!
//! LESSON kept from the retired rim: phases fold mod 2π in f64 BEFORE the f32 sin. Absolute eagle seconds are ~10⁸ — through f32 the time step quantizes to whole seconds and sin's range reduction turns to noise; the motion froze and teleported instead of drifting.

use std::f64::consts::TAU;

/// One waveform: two sines, unit output. Frequencies in Hz, phases in radians.
#[derive(Clone, Copy, Debug)]
struct Wave {
    f1: f64,
    p1: f64,
    f2: f64,
    p2: f64,
}

impl Wave {
    /// Digest-drawn: primary frequency in `lo..hi` Hz, secondary at an irrational-ish multiple with its own phase — beats, never a visible loop.
    fn for_channel(name: &str, digest: &[u8; 32], lo: f64, hi: f64) -> Self {
        let u = |suffix: &str| chirp::channel_unit(&format!("orbit {name} {suffix}"), digest);
        let f1 = lo + u("f1") * (hi - lo);
        Self {
            f1,
            p1: u("p1") * TAU,
            f2: f1 * (1.5 + u("fr") * 0.8),
            p2: u("p2") * TAU,
        }
    }

    /// Sample at absolute seconds. Phase folds in f64 (see the module doc's lesson); output in −1..1.
    fn at(&self, t: f64) -> f32 {
        let a = (self.f1 * TAU * t + self.p1).rem_euclid(TAU) as f32;
        let b = (self.f2 * TAU * t + self.p2).rem_euclid(TAU) as f32;
        (a.sin() + 0.5 * b.sin()) / 1.5
    }
}

/// The five channels of the circle's motion, derived from the relationship digest.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    x: Wave,
    y: Wave,
    scale: Wave,
    opacity: Wave,
    /// Steady spin rate for the offset rotation, rad/s, digest-signed — the decoupling Nick asked for: the (x, y) pair swings thru every direction instead of tracing an axis-aligned figure.
    spin: f64,
    spin_phase: f64,
}

/// What one moment of the orbit looks like, in normalized units the caller scales to its layout.
#[derive(Clone, Copy, Debug)]
pub struct OrbitSample {
    /// Offset of the circle's center, each in −1..1 (caller multiplies by its max excursion).
    pub dx: f32,
    pub dy: f32,
    /// Radius scale factor in −1..1 (caller maps to its scale range).
    pub scale: f32,
    /// Opacity in 0..1 (caller maps to its alpha range).
    pub opacity: f32,
}

/// Derive the motion from the relationship digest — same channel discipline as the audio: named draws, deterministic, one identity.
pub fn orbit_for(digest: &[u8; 32]) -> Orbit {
    let u = |name: &str| chirp::channel_unit(name, digest);
    Orbit {
        x: Wave::for_channel("x", digest, 12.8, 46.0),
        y: Wave::for_channel("y", digest, 12.8, 46.0),
        scale: Wave::for_channel("scale", digest, 10.2, 30.7),
        opacity: Wave::for_channel("opacity", digest, 15.4, 38.4),
        spin: (12.8 + u("orbit spin rate") * 38.4) * if u("orbit spin dir") < 0.5 { -1.0 } else { 1.0 },
        spin_phase: u("orbit spin phase") * TAU,
    }
}

/// Sample the orbit at absolute seconds: raw x/y waves, rotated by the spin (the matrix multiply — cos/sin of one angle), plus scale and opacity.
pub fn sample(o: &Orbit, t: f64) -> OrbitSample {
    let (x, y) = (o.x.at(t), o.y.at(t));
    let rho = (o.spin * t + o.spin_phase).rem_euclid(TAU) as f32;
    let (s, c) = rho.sin_cos();
    OrbitSample {
        dx: x * c - y * s,
        dy: x * s + y * c,
        scale: o.scale.at(t),
        opacity: (o.opacity.at(t) + 1.0) * 0.5,
    }
}
