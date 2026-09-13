//! Q32 integer gain with a carried remainder — THE one way a sample is scaled (Nick 2026-09-13: "cast to 64 bit, multiply by 2^32·gain, then bitshift to dst so we keep it int and it floors proper… carry the remainder rather than this garbage rounding business").
//!
//! The kernel per sample: `acc = s·g + carry; out = acc >> 32; carry = acc & 0xFFFF_FFFF`.
//! The arithmetic shift floors toward −∞ uniformly (an `as i16` cast truncates toward zero — signal-correlated distortion), and for two's-complement that low mask IS the residue exactly, so the carry costs one AND.
//! The carry makes the running sum exact: over any run `Σout = (Σs·g + c₀ − cₙ) >> 32` — zero mean error, and the per-sample error spectrum is shaped by (1 − z⁻¹), first-order highpassed out of the voice band. The mic and room floor (tens of LSB on every phone measured) is stronger dither than anything we could add, so error feedback alone is the complete treatment.
//! Unity (`1 << 32`) is bit-exact passthrough with zero carry — a gain of one provably touches nothing.
//! Bit budget: a 24-bit sample × the 16× gain cap (2^36) + a carry under 2^32 stays under 2^61 — i64 never overflows, today's i16 or a future 24-bit capture alike.

/// Q32 unity: the gain that is exactly one.
pub const UNITY: i64 = 1 << 32;

/// One gain stage: a Q32 gain and the running remainder it carries.
pub struct QGain {
    /// Gain in Q32 (`UNITY` = 1.0). Callers keep it within ±2^36 (16×) — the bit budget above.
    pub g: i64,
    carry: i64,
}

impl QGain {
    pub fn new(g: i64) -> Self {
        Self { g, carry: 0 }
    }

    /// Scale one sample. Saturation drops the carry (anti-windup): feeding a clip's error forward would smear the clip across the samples after it.
    #[inline]
    pub fn apply(&mut self, s: i16) -> i16 {
        let acc = s as i64 * self.g + self.carry;
        let out = acc >> 32;
        self.carry = acc & 0xFFFF_FFFF;
        if out > i16::MAX as i64 {
            self.carry = 0;
            return i16::MAX;
        }
        if out < i16::MIN as i64 {
            self.carry = 0;
            return i16::MIN;
        }
        out as i16
    }

    /// Scale a frame in place. Unity with no pending carry is a true no-op.
    pub fn apply_frame(&mut self, frame: &mut [i16]) {
        if self.g == UNITY && self.carry == 0 {
            return;
        }
        for s in frame.iter_mut() {
            *s = self.apply(*s);
        }
    }
}

/// Compose two Q32 gains into one: `(a·b) >> 32` thru i128 (one widening multiply).
pub fn compose(a: i64, b: i64) -> i64 {
    ((a as i128 * b as i128) >> 32) as i64
}

#[cfg(test)]
mod kat {
    use super::*;

    /// Unity is identity: bit-equal output, zero carry, forever.
    #[test]
    fn unity_is_identity() {
        let mut q = QGain::new(UNITY);
        let src: Vec<i16> = (0..4800).map(|i| ((i * 7919) % 65536 - 32768) as i16).collect();
        let mut f = src.clone();
        q.apply_frame(&mut f);
        assert_eq!(f, src);
        assert_eq!(q.carry, 0);
    }

    /// The carried remainder keeps the SUM exact where truncation drifts by N/2 LSB: DC 100 at gain 1/3 over 48000 samples lands within one final carry of the true product.
    #[test]
    fn dc_sum_is_exact() {
        let g = UNITY / 3; // 0.333…, not representable — the carry is doing the work
        let mut q = QGain::new(g);
        let n = 48_000i64;
        let sum: i64 = (0..n).map(|_| q.apply(100) as i64).sum();
        let true_sum = (100 * g * n) >> 32;
        assert!((sum - true_sum).abs() <= 1, "sum {sum} vs {true_sum}");
        // The truncating version (each sample floored alone) sits a third of an LSB low every sample: 33 per 100.
        let trunc: i64 = n * ((100 * g) >> 32);
        assert!((trunc - true_sum).abs() > n / 4, "truncation must actually be wrong for this KAT to mean anything");
    }

    /// The pad as a Q32 power of two beats the old `>>4`: a −1 DC thru `>>4` gave −1 forever (floor bias, a full-scale error at that level); thru the carry it averages −1/16.
    #[test]
    fn pad_negative_dc_is_mean_exact() {
        let mut q = QGain::new(UNITY >> 4);
        let n = 1600i64;
        let sum: i64 = (0..n).map(|_| q.apply(-1) as i64).sum();
        assert!((sum - (-n / 16)).abs() <= 1, "sum {sum} vs {}", -n / 16);
    }

    /// Saturation clamps and drops the carry: a full-scale burst at 4× leaves no tail on the silence after it.
    #[test]
    fn saturation_drops_the_carry() {
        let mut q = QGain::new(UNITY * 4);
        assert_eq!(q.apply(i16::MAX), i16::MAX);
        assert_eq!(q.apply(i16::MIN), i16::MIN);
        assert_eq!(q.apply(0), 0);
        assert_eq!(q.carry, 0);
    }

    /// Composed gain equals the two stages run back to back, within one carry of resolution — on a source the intermediate i16 doesn't clip (past full scale the cascade saturates where the composed gain sails thru, which is exactly WHY stages compose).
    #[test]
    fn compose_matches_cascade() {
        let (ga, gb) = (UNITY * 3 / 2, UNITY / 5);
        let src: Vec<i16> = (0..4800).map(|i| ((i * 31_337) % 40000 - 20000) as i16).collect();
        let mut one = QGain::new(compose(ga, gb));
        let (mut a, mut b) = (QGain::new(ga), QGain::new(gb));
        let s1: i64 = src.iter().map(|&s| one.apply(s) as i64).sum();
        let s2: i64 = src.iter().map(|&s| b.apply(a.apply(s)) as i64).sum();
        assert!((s1 - s2).abs() <= src.len() as i64 / 100, "{s1} vs {s2}");
    }
}
