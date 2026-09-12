//! wave.env — the shared waveform envelope (Nick 2026-09-12: "3 channel u8 normalized tensor VSF").
//!
//! One per party per wave: the pyramid reduces our CLEAN archive channel to four power tracks — total `x²` plus the three tuned voice bands (red ≤240 Hz, green bell ~2.4 kHz, blue shelf ≥7.7 kHz; see record.rs ENV_COMPONENTS) — each normalized to its own peak and STOCHASTICALLY quantized to u8 (the dither carries sub-LSB signal into the render's per-pixel averages). The file is a plain VSF section holding the metadata and an 8-bit `[4, bins]` tensor, stored as a content-addressed blob and pushed at wave end — a few hundred KB, so the far card colours in long before the multi-MB audio replicates. A wave shorter than [`crate::call::record::ENV_MIN_SHARE_SAMPLES`] ships none: the receiver derives the envelope from the audio it fetches anyway (the same fallback covers the far channel before its blob lands, and every pre-exchange recording).
//! The row that carries it is an ordinary attachment row named [`WAVE_ENV_NAME`] referencing the wave row (`RefKind::Wave`), so replication, fetch, tombstones and the fold-into-the-card all ride the existing machinery.

use std::sync::Arc;

/// The attachment-row filename that marks an envelope blob (never rendered as a bubble — it folds into the wave card like the recording row does).
pub const WAVE_ENV_NAME: &str = "wave.env";

/// A parsed envelope: three planar u8 tracks and the scales to make them absolute again.
pub struct WaveEnv {
    pub bins: usize,
    pub sample_rate: u32,
    pub samples_per_bin: u32,
    /// Per-component peak MEAN POWER relative to full scale, Q48 fixed point — exact integers at rest, never a float (Nick: and scaling is multiplication + shifts, never division: a byte decodes as `byte × peak_q48 >> 8` in Q48, the ÷256 a shift, the ÷2^48 a constant multiply at the float boundary).
    pub peak_q48: [u64; 4],
    /// Planar `[4][bins]`: total power, red, green, blue band powers; a byte spans 1/256 of the peak (256, not 255 — integer maths floors).
    pub data: Arc<Vec<u8>>,
}

impl WaveEnv {
    /// One u8 step of a track as full-scale-relative power (f32 at the display boundary only): `peak_q48 × 2^-56` = peak ÷ 256 ÷ 2^48 — two constant multiplies, no division.
    #[inline]
    pub fn lsb(&self, comp: usize) -> f32 {
        self.peak_q48[comp] as f32 * (1.0 / (1u64 << 48) as f32 / 256.0)
    }
    /// One track's absolute mean power (full-scale-relative) at a bin.
    #[inline]
    pub fn power(&self, comp: usize, bin: usize) -> f32 {
        self.data[comp * self.bins + bin] as f32 * self.lsb(comp)
    }
}

/// Serialize an envelope to VSF bytes. `pk` is ONE field with three values (never decimal-indexed names), each an exact Q48 integer.
pub fn write(sample_rate: u32, samples_per_bin: u32, bins: usize, peak_q48: [u64; 4], data: &[u8]) -> Vec<u8> {
    let tensor = vsf::BitPackedTensor::pack(8, vec![4, bins], data);
    let mut section = vsf::VsfSection::new("wave_env");
    section.add_field_multi("rate", vec![vsf::VsfType::u(sample_rate as usize, false)]);
    section.add_field_multi("spb", vec![vsf::VsfType::u(samples_per_bin as usize, false)]);
    section.add_field_multi("bins", vec![vsf::VsfType::u(bins, false)]);
    section.add_field_multi("pk", peak_q48.iter().map(|&q| vsf::VsfType::u(q as usize, false)).collect());
    section.add_field_multi("env", vec![vsf::VsfType::p(tensor)]);
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section_direct(section)
        .build()
        .unwrap_or_default()
}

/// Parse an envelope blob. `None` on anything malformed — a bad blob just means the card derives from audio instead.
pub fn read(bytes: &[u8]) -> Option<WaveEnv> {
    // The blob crosses a trust boundary (it arrives from the peer), so the verified read runs first: provenance self-consistency + structure before any field is believed. Content authenticity rides the blob's content hash like every attachment.
    let (header, header_end) = vsf::verification::read_verified(bytes, None).ok()?;
    // primary_section, never a bare body parse — the section name lives in the header TOC (the documented trap).
    let section = header.primary_section(bytes, header_end).ok()?;
    if section.name != "wave_env" {
        return None;
    }
    let field = |n: &str| section.fields.iter().find(|f| f.name == n).and_then(|f| f.values.first());
    // Width-agnostic integer reads (the ek parse-death trap): writers auto-size, readers widen.
    let sample_rate = field("rate")?.as_u64()? as u32;
    let samples_per_bin = field("spb")?.as_u64()? as u32;
    let bins = field("bins")?.as_u64()? as usize;
    let pk_field = section.fields.iter().find(|f| f.name == "pk")?;
    let pk: Vec<u64> = pk_field.values.iter().filter_map(|v| v.as_u64()).collect();
    let data: Vec<u8> = match field("env")? {
        vsf::VsfType::p(t) => t.unpack_u8(),
        _ => return None,
    };
    if pk.len() != 4 || bins == 0 || data.len() != 4 * bins || samples_per_bin == 0 {
        return None;
    }
    Some(WaveEnv {
        bins,
        sample_rate,
        samples_per_bin,
        peak_q48: [pk[0], pk[1], pk[2], pk[3]],
        data: Arc::new(data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_env_round_trips() {
        let bins = 70_000usize;
        let data: Vec<u8> = (0..4 * bins).map(|i| (i * 37 % 251) as u8).collect();
        let pk = [1u64 << 46, 12_345_678_901, 42, 7];
        let bytes = write(48_000, 32, bins, pk, &data);
        assert!(!bytes.is_empty());
        let e = read(&bytes).expect("parse back");
        assert_eq!((e.bins, e.sample_rate, e.samples_per_bin), (bins, 48_000, 32));
        assert_eq!(*e.data, data);
        assert_eq!(e.peak_q48, pk, "peaks are exact integers at rest");
        assert!((e.power(0, 0) - data[0] as f32 * e.lsb(0)).abs() < 1e-12);
    }

    #[test]
    fn junk_reads_none() {
        assert!(read(b"not a vsf file at all").is_none());
        assert!(read(&[]).is_none());
    }
}
