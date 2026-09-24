//! The "measure now" ritual (the Wave settings page, Nick 2026-09-16): open the audio session for a few seconds, listen to the person say a sentence, and post the same voiced/quiet measurement a wave's teardown posts — so a mic the app has never heard speak gets a real seed before its first wave, and a stale profile can be replaced on demand.
//!
//! The arithmetic is the engine's, verbatim (engine.rs TX block): frame means at 24-bit, the slow-rise/fast-fall room tracker, the RELATIVE voiced gate at 3× the tracker, the fine floor as the min-statistic. Nothing renders, so the far-quiet gate is moot.

use std::sync::mpsc::{channel, Receiver};

/// What the ritual heard: this mic's voiced level in coarse units, its fine floor, and how many voiced frames it rested on (0 = nobody spoke — only the floor was posted).
#[derive(Clone, Debug)]
pub struct MeasureResult {
    pub mic_id: String,
    pub voiced: u32,
    pub floor: f32,
    pub voiced_frames: u32,
}

/// Start listening for `secs` seconds on a worker; the result lands on the receiver (the UI drains it on its tick — the page shows "listening…" until then). None when the audio session refuses to open.
pub fn start(secs: u64) -> Option<Receiver<MeasureResult>> {
    let gen = crate::platform::audio::start_owned()?;
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("wave-measure".into())
        .spawn(move || {
            let mic_id = crate::platform::audio::mic_id();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            let mut raw_floor_q8: i64 = i64::MAX;
            let mut noise_est_q8: i64 = 0;
            let mut voiced_sum_q8: i64 = 0;
            let mut voiced_frames: i64 = 0;
            // Let the streams settle before the first frame counts.
            std::thread::sleep(std::time::Duration::from_millis(300));
            crate::platform::audio::captured_frames();
            while std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(50));
                for (_, frame24) in crate::platform::audio::captured_frames() {
                    let mean_q8 = frame24.iter().map(|s| s.unsigned_abs() as i64).sum::<i64>() / frame24.len().max(1) as i64;
                    if mean_q8 > 0 && mean_q8 < raw_floor_q8 {
                        raw_floor_q8 = mean_q8;
                    }
                    if mean_q8 > 0 {
                        if noise_est_q8 == 0 {
                            noise_est_q8 = mean_q8;
                        } else if mean_q8 > noise_est_q8 {
                            noise_est_q8 += ((mean_q8 - noise_est_q8) >> 10).max(1);
                        } else {
                            noise_est_q8 = mean_q8 + ((noise_est_q8 - mean_q8) >> 2);
                        }
                    }
                    if noise_est_q8 > 0 && mean_q8 > (noise_est_q8 * 3).max(512) {
                        voiced_sum_q8 += mean_q8;
                        voiced_frames += 1;
                    }
                }
            }
            crate::platform::audio::stop_owned(gen);
            let floor = if raw_floor_q8 == i64::MAX { 0.0 } else { raw_floor_q8 as f32 / 256.0 };
            // The same evidence bar as a wave: 3 s of voiced speech posts a full profile; less posts the floor alone (voiced 0 = the floor-only sentinel).
            let full = voiced_frames >= 600;
            let voiced_coarse = if voiced_frames > 0 { (voiced_sum_q8 / voiced_frames) as f32 / 256.0 } else { 0.0 };
            if full || floor > 0.0 {
                super::calibrate::post_learned(vec![super::calibrate::LearnedResult {
                    result: super::calibrate::CalResult::Voice(super::calibrate::VoiceProfile {
                        voiced: if full { voiced_coarse } else { 0.0 },
                        floor,
                        mic_id: mic_id.clone(),
                    }),
                    windows: (voiced_frames.max(200) / 200) as u32,
                    solid: voiced_frames >= 2400,
                }]);
            }
            crate::logf!(
                "CAL: measure now — mic \"{}\" voiced {} floor {} over {} voiced frames{}",
                mic_id,
                format!("{voiced_coarse:.0}"),
                format!("{floor:.2}"),
                voiced_frames,
                if full { "" } else { " (under 3 s of speech — floor-only post)" }
            );
            let _ = tx.send(MeasureResult { mic_id, voiced: voiced_coarse.round() as u32, floor, voiced_frames: voiced_frames as u32 });
        })
        .is_ok();
    if !spawned {
        crate::platform::audio::stop_owned(gen);
        return None;
    }
    Some(rx)
}
