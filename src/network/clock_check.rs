//! One-shot wall-clock sanity check via nunc-time multi-source consensus.
//!
//! Photon stamps every eagle-time on the LOCAL clock (the braid, message ordering, avatar newer-wins all need a monotonic, unique-per-tick local stamp — nunc's ~seconds latency and ±confidence interval would break that). nunc is used ONLY to ask, out of band, "is this device's wall clock telling the truth?" If the consensus says we're off by more than a threshold, the UI raises an amber "clock off" banner — a warning, never a silent correction. Open source relies on everyone being honest; we surface the anomaly loudly instead of trusting or overriding it.
//!
//! It runs once a few seconds after attest, and again whenever the jump detector below notices the wall clock diverging from the monotonic clock by more than the re-check slack (an NTP step,
//! a long sleep, or an adversary moving the clock after boot). The jump is only the cheap TRIGGER;
//! nunc is the arbiter that decides whether the banner shows.

#[cfg(not(target_os = "android"))]
use std::sync::Arc;
use std::time::{Instant, SystemTime};

#[cfg(not(target_os = "android"))]
use crate::ui::PhotonEvent;
#[cfg(not(target_os = "android"))]
use fluor::host::WakeSender;

/// The wake handle the worker uses to nudge the event loop after it posts a result. Matches the shape the inline avatar / clutch workers use (`self.event_proxy.clone()`): an `Arc<dyn WakeSender>` on desktop, nothing on Android (its redraws come thru the JNI/Choreographer path).
#[cfg(not(target_os = "android"))]
pub type ClockWake = Option<Arc<dyn WakeSender<PhotonEvent>>>;
#[cfg(target_os = "android")]
pub type ClockWake = Option<()>;

/// Outcome of one consensus query against the system clock.
#[derive(Debug, Clone)]
pub enum ClockCheckResult {
    /// Consensus reached. `offset_secs` = consensus_time − system_time (positive: system clock is BEHIND true time; negative: AHEAD). `confidence_secs` is the consensus half-width.
    Ok {
        offset_secs: i64,
        confidence_secs: u64,
        sources_used: usize,
        sources_queried: usize,
    },
    /// The consensus query failed (offline, every source unreachable, etc). Not an anomaly — we simply couldn't verify, so the UI leaves the banner in its prior state.
    Unavailable(String),
}

/// Spawn the background clock check. Mirrors the avatar / clutch worker shape: own thread, own current-thread tokio runtime (nunc is async), result back over an `mpsc` channel, then wake the event loop so the next frame drains it. Never blocks the UI thread.
///
/// Desktop-only: nunc-time isn't a dependency on Android OR Redox (its roughtime source pulls `ring`, which needs the NDK to cross-compile and doesn't build for Redox — see Cargo.toml's target-gated deps), and the clock check is a desktop-host affordance. The call sites in `photon_app` are `cfg(not(android))`-gated; on Redox they reach the same-signature stub below so they compile without a nunc link.
#[cfg(not(any(target_os = "android", target_os = "redox")))]
pub fn spawn_clock_check(
    tx: std::sync::mpsc::Sender<ClockCheckResult>,
    #[allow(unused_variables)] event_proxy: ClockWake,
) {
    std::thread::spawn(move || {
        // Sample the system clock as close as possible to the consensus query so the offset reflects the same instant on both clocks.
        let system_at_query = SystemTime::now();

        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(ClockCheckResult::Unavailable(format!("runtime: {}", e)));
                return;
            }
        };

        let result = runtime.block_on(async {
            // Fast mode: smaller source set, quicker to consensus — plenty for a sanity check.
            // The package is `nunc-time` but its lib crate name is `nunc`, so the path is `nunc::`.
            match nunc::query(nunc::Mode::Fast).await {
                Ok(t) => {
                    let consensus = t.timestamp();
                    // offset = consensus − system, in whole seconds, sign preserved.
                    let offset_secs = match consensus.duration_since(system_at_query) {
                        Ok(d) => d.as_secs() as i64, // consensus ahead → system behind → positive
                        Err(e) => -(e.duration().as_secs() as i64), // system ahead → negative
                    };
                    ClockCheckResult::Ok {
                        offset_secs,
                        confidence_secs: t.confidence().as_secs(),
                        sources_used: t.sources_used,
                        sources_queried: t.sources_queried,
                    }
                }
                Err(e) => ClockCheckResult::Unavailable(e.to_string()),
            }
        });

        let _ = tx.send(result);

        #[cfg(not(target_os = "android"))]
        if let Some(proxy) = event_proxy.as_ref() {
            let _ = proxy.send(PhotonEvent::NetworkUpdate);
        }
    });
}

/// Redox stub: `nunc-time` isn't linked here (its roughtime source pulls `ring`, which doesn't cross-compile to Redox), so there's no consensus source to query. Report Unavailable so the caller's channel still receives a result and the UI leaves the clock banner in its prior state — the real check runs only where nunc is a dependency.
#[cfg(target_os = "redox")]
pub fn spawn_clock_check(
    tx: std::sync::mpsc::Sender<ClockCheckResult>,
    #[allow(unused_variables)] event_proxy: ClockWake,
) {
    let _ = tx.send(ClockCheckResult::Unavailable(
        "nunc-time not available on this platform".to_string(),
    ));
}

/// Detects gross, unexplained jumps in the wall clock by comparing it to the monotonic clock.
///
/// `Instant` (monotonic) cannot be set or run backward; `SystemTime` (wall clock) can be stepped by NTP, by suspend/resume, or by an adversary. If the wall clock advances much more (or less) than the monotonic clock did over the same span, the wall clock jumped. We can't tell a benign jump (NTP step, laptop slept) from a malicious one HERE — that's nunc's job. This is only the cheap trigger that says "something moved, go re-verify."
pub struct ClockJumpDetector {
    mono_baseline: Instant,
    wall_baseline: SystemTime,
    /// Skew beyond this (in seconds) counts as a jump worth re-verifying.
    slack_secs: u64,
}

impl ClockJumpDetector {
    /// `slack_secs` is the unexplained divergence that triggers a re-check (~3600 = one hour: loose enough to ignore ordinary NTP steps and short sleeps, tight enough to catch a day-scale set or a long suspend).
    pub fn new(slack_secs: u64) -> Self {
        Self {
            mono_baseline: Instant::now(),
            wall_baseline: SystemTime::now(),
            slack_secs,
        }
    }

    /// Returns true if the wall clock has diverged from the monotonic clock by more than the slack since the last baseline. On a true return it also re-baselines, so a single jump fires once (the caller turns that one `true` into one re-check), not every tick thereafter.
    pub fn check_and_reset(&mut self) -> bool {
        let mono_elapsed = self.mono_baseline.elapsed().as_secs();
        let wall_elapsed = match SystemTime::now().duration_since(self.wall_baseline) {
            Ok(d) => d.as_secs() as i64,
            // Wall clock went BACKWARD past the baseline — unambiguously a jump.
            Err(e) => -(e.duration().as_secs() as i64),
        };

        let divergence = (wall_elapsed - mono_elapsed as i64).unsigned_abs();
        let jumped = divergence > self.slack_secs;
        if jumped {
            self.mono_baseline = Instant::now();
            self.wall_baseline = SystemTime::now();
        }
        jumped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_detector_reports_no_jump() {
        // No time has passed and both clocks agree → no jump.
        let mut d = ClockJumpDetector::new(3600);
        assert!(!d.check_and_reset());
    }

    #[test]
    fn offset_sign_convention_is_consensus_minus_system() {
        // Document the sign convention the UI relies on: positive offset ⇒ system clock is BEHIND.
        // (Constructed directly; the worker computes the same way from real SystemTimes.)
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2000);
        let earlier = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000);
        // consensus = later, system = earlier → system behind → positive.
        let offset = later.duration_since(earlier).unwrap().as_secs() as i64;
        assert_eq!(offset, 1000);
    }
}
