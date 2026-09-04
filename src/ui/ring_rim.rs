//! The incoming-wave circle (Nick 2026-09-04): ONE perfect circle behind the avatar, alive thru its transform — digest-keyed jitter drives its x/y offset, a rotation decouples those from the axes (plain sine/cosine rotation of the offset vector), a fourth channel scales it, a fifth breathes its opacity. The circle strictly moves, scales, and fades; it is never deformed (the Fourier-rim experiment retired 2026-09-04 — an interpolated mess in the field).
//!
//! The motion is HASH JITTER, not waveforms (Nick 2026-09-04 "drop it in"): the 256× sine cut already ran every channel above the display refresh, so nothing of a sine's shape survived aliasing — it was five expensive random number generators with a value distribution clustered at the extremes. Now each frame folds eagle time into a per-channel blake3 draw: truly uniform, refresh-rate-invariant character, cheaper than five sins. Replayability holds — the same digest at the same wall-clock instant shows the same frame of motion (pure function of digest + now, no stored animation state).
//!
//! Frames quantize on a 1/128 s lattice: fast enough that every refresh lands a fresh draw (the jitter Nick asked for), coarse enough that a 60 Hz and a 120 Hz panel walk the same sequence rather than the faster one seeing twice the frames.

/// Jitter lattice: draws change this many times per second. Above any common refresh interval's worth of perceptual smoothing, below double-sampling on 120 Hz panels.
const TICKS_PER_SEC: f64 = 128.0;

/// The circle's motion source — just the digest; every draw is named off it per tick.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    digest: [u8; 32],
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

/// The motion is the digest — same channel discipline as the audio: named draws, deterministic, one identity.
pub fn orbit_for(digest: &[u8; 32]) -> Orbit {
    Orbit { digest: *digest }
}

/// One uniform draw in 0..1 for this tick's channel: blake3(digest ‖ channel ‖ tick), first 8 bytes as a fraction. Deterministic in (digest, tick) — replayable, machine-independent.
fn draw(digest: &[u8; 32], channel: u8, tick: i64) -> f64 {
    let mut buf = [0u8; 41];
    buf[..32].copy_from_slice(digest);
    buf[32] = channel;
    buf[33..].copy_from_slice(&tick.to_le_bytes());
    let h = blake3::hash(&buf);
    let mut b = [0u8; 8];
    b.copy_from_slice(&h.as_bytes()[..8]);
    u64::from_le_bytes(b) as f64 / u64::MAX as f64
}

/// Sample the orbit at absolute seconds: fresh uniform draws per 1/128 s tick — x/y offset rotated by a per-tick angle (the matrix multiply — cos/sin of one angle, so the jitter isn't axis-coupled), plus scale and opacity.
pub fn sample(o: &Orbit, t: f64) -> OrbitSample {
    let tick = (t * TICKS_PER_SEC) as i64;
    let unit = |ch: u8| draw(&o.digest, ch, tick) as f32;
    let (x, y) = (unit(0) * 2.0 - 1.0, unit(1) * 2.0 - 1.0);
    let rho = unit(2) * std::f32::consts::TAU;
    let (s, c) = rho.sin_cos();
    OrbitSample {
        dx: x * c - y * s,
        dy: x * s + y * c,
        scale: unit(3) * 2.0 - 1.0,
        opacity: unit(4),
    }
}
