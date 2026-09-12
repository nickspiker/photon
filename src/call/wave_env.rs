//! wave.env — the shared waveform envelope (Nick 2026-09-12: "3 channel u8 normalized tensor VSF").
//!
//! One per party per wave: at keep, the transcode's pyramid reduces our RAW-MIC channel to three power tracks — `x²`, `(n₀−n₁)²`, `((n₀+n₁)−(n₂+n₃))²` — each normalized to its own peak and STOCHASTICALLY quantized to u8 (the dither carries sub-LSB signal into the render's per-pixel averages). The file is a plain VSF section holding the metadata and an 8-bit `[3, bins]` tensor, stored as a content-addressed blob and pushed at wave end — a few hundred KB, so the far card colours in long before the multi-MB audio replicates. A wave shorter than [`crate::call::record::ENV_MIN_SHARE_SAMPLES`] ships none: the receiver derives the envelope from the audio it fetches anyway (the same fallback covers the far channel before its blob lands, and every pre-exchange recording).
//! The row that carries it is an ordinary attachment row named [`WAVE_ENV_NAME`] referencing the wave row (`RefKind::Wave`), so replication, fetch, tombstones and the fold-into-the-card all ride the existing machinery.

use std::sync::Arc;

/// The attachment-row filename that marks an envelope blob (never rendered as a bubble — it folds into the wave card like the recording row does).
pub const WAVE_ENV_NAME: &str = "wave.env";

/// A parsed envelope: three planar u8 tracks and the scales to make them absolute again.
pub struct WaveEnv {
    pub bins: usize,
    pub sample_rate: u32,
    pub samples_per_bin: u32,
    /// Per-component peak MEAN POWER relative to full scale (peak = the u8 255 of that track).
    pub peaks: [f32; 3],
    /// Planar `[3][bins]`: power, first-difference power, second-level detail power.
    pub data: Arc<Vec<u8>>,
}

impl WaveEnv {
    /// One track's absolute mean power (full-scale-relative) at a bin.
    #[inline]
    pub fn power(&self, comp: usize, bin: usize) -> f32 {
        self.data[comp * self.bins + bin] as f32 / 255.0 * self.peaks[comp]
    }
}

/// Serialize an envelope to VSF bytes.
pub fn write(sample_rate: u32, samples_per_bin: u32, bins: usize, peaks: [f32; 3], data: &[u8]) -> Vec<u8> {
    let tensor = vsf::BitPackedTensor::pack(8, vec![3, bins], data);
    let fields = vec![
        ("rate".to_string(), vsf::VsfType::u(sample_rate as usize, false)),
        ("spb".to_string(), vsf::VsfType::u(samples_per_bin as usize, false)),
        ("bins".to_string(), vsf::VsfType::u(bins, false)),
        ("peak".to_string(), vsf::VsfType::t_f5(vsf::types::tensor::Tensor::new(vec![3], peaks.to_vec()))),
        ("env".to_string(), vsf::VsfType::p(tensor)),
    ];
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section("wave_env", fields)
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
    let peaks_v: Vec<f32> = match field("peak")? {
        vsf::VsfType::t_f5(t) => t.data.clone(),
        _ => return None,
    };
    let data: Vec<u8> = match field("env")? {
        vsf::VsfType::p(t) => t.unpack_u8(),
        _ => return None,
    };
    if peaks_v.len() != 3 || bins == 0 || data.len() != 3 * bins || samples_per_bin == 0 {
        return None;
    }
    Some(WaveEnv {
        bins,
        sample_rate,
        samples_per_bin,
        peaks: [peaks_v[0], peaks_v[1], peaks_v[2]],
        data: Arc::new(data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_env_round_trips() {
        let bins = 70_000usize;
        let data: Vec<u8> = (0..3 * bins).map(|i| (i * 37 % 251) as u8).collect();
        let bytes = write(48_000, 32, bins, [0.25, 0.001, 0.0004], &data);
        assert!(!bytes.is_empty());
        let e = read(&bytes).expect("parse back");
        assert_eq!((e.bins, e.sample_rate, e.samples_per_bin), (bins, 48_000, 32));
        assert_eq!(*e.data, data);
        assert!((e.peaks[0] - 0.25).abs() < 1e-6);
        assert!((e.power(0, 0) - (data[0] as f32 / 255.0 * 0.25)).abs() < 1e-9);
    }

    #[test]
    fn junk_reads_none() {
        assert!(read(b"not a vsf file at all").is_none());
        assert!(read(&[]).is_none());
    }
}
