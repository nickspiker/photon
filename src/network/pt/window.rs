//! PT RTT estimation, the shared path pace, and in-flight tracking
//!
//! - One send window per path ([`PathPace`]), shared by every large transfer on it and steered by queueing delay: it settles where the path's queue holds [`PACE_TARGET`], halves on loss, and yields to a queue anyone else builds.
//! - RTT samples follow Karn's rule: a retransmitted packet's ACK is never timed, since it cannot say which copy it answers.

use std::time::{Duration, Instant};

/// RTT Estimator using TCP-style exponential moving average
///
/// SRTT = (1-α)*SRTT + α*R  where α=0.125 RTTVAR = (1-β)*RTTVAR + β*|SRTT-R|  where β=0.25 RTO = SRTT + 4*RTTVAR
pub struct RTTEstimator {
    /// Smoothed RTT
    smoothed_rtt: Duration,
    /// RTT variance
    rtt_variance: Duration,
    /// Retransmission timeout
    rto: Duration,
    /// Whether we've received any samples
    initialized: bool,
}

impl RTTEstimator {
    /// Create new estimator with initial guess
    pub fn new() -> Self {
        Self {
            smoothed_rtt: Duration::from_millis(100),
            rtt_variance: Duration::from_millis(50),
            rto: Duration::from_millis(300),
            initialized: false,
        }
    }

    /// Update RTT estimate with new sample
    pub fn update(&mut self, sample: Duration) {
        const ALPHA: f64 = 0.125;
        const BETA: f64 = 0.25;

        if !self.initialized {
            // First sample: initialize directly
            self.smoothed_rtt = sample;
            self.rtt_variance = sample / 2;
            self.initialized = true;
        } else {
            // Compute difference
            let diff = if sample > self.smoothed_rtt {
                sample - self.smoothed_rtt
            } else {
                self.smoothed_rtt - sample
            };

            // Update variance
            self.rtt_variance = Duration::from_secs_f64(
                (1.0 - BETA) * self.rtt_variance.as_secs_f64() + BETA * diff.as_secs_f64(),
            );

            // Update smoothed RTT
            self.smoothed_rtt = Duration::from_secs_f64(
                (1.0 - ALPHA) * self.smoothed_rtt.as_secs_f64() + ALPHA * sample.as_secs_f64(),
            );
        }

        // Compute RTO = SRTT + 4*RTTVAR
        self.rto = self.smoothed_rtt + self.rtt_variance * 4;

        // Clamp RTO between 100ms and 10s
        self.rto = self
            .rto
            // WHY/PROOF: the retransmit timeout's floor and ceiling are the protocol's (the RFC 6298 shape): a sub-100 ms RTO retransmits into a queue that has not drained, an unbounded one waits out a transient forever — the bound is the design, not a guard.
            .clamp(Duration::from_millis(100), Duration::from_secs(10));
    }

    /// Get current RTO (retransmission timeout)
    pub fn rto(&self) -> Duration {
        self.rto
    }

    /// Get smoothed RTT
    pub fn srtt(&self) -> Duration {
        self.smoothed_rtt
    }

    /// Back off RTO on timeout (exponential backoff)
    pub fn backoff(&mut self) {
        self.rto = (self.rto * 2).min(Duration::from_secs(10));
    }
}

impl Default for RTTEstimator {
    fn default() -> Self {
        Self::new()
    }
}

/// The queueing delay a path's window settles at: the RTT the pace aims for is the path's smallest RTT plus this.
pub const PACE_TARGET: Duration = Duration::from_millis(1 << 5);

/// A path's opening window, as log2 packets (16).
pub const PACE_INITIAL_LOG2: f32 = 4.;

/// THE PATH PACE (Nick 2026-10-29, "delay based please"): ONE send window per path, shared by every large transfer on it, steered by queueing delay.
/// It replaces the per-transfer blast (256 packets fired at once, then a send ratio of 1.1-4 packets per ACK moved by loss alone): on 2026-10-29 23:01 Nick's MacBook served Josh 149 chunks of 258 packets, thirteen at a time, so each chunk went out as one blast; the uplink held a standing queue (RTT 40 ms → 70-115 ms with no loss to react to), and the log submit's one HTTPS connection to FGTW could not get a request thru.
/// Each timed ACK moves the window by its share of the factor (base + TARGET) / rtt: `in_flight` ACKs come back per round trip, so each takes `1 / in_flight` of the step in log2, and one round trip multiplies the window by the whole factor.
/// A sample above base + TARGET shrinks the window, one below grows it, so the path's queue settles at TARGET; a queue someone else builds reads exactly like one we built, so bulk transfers yield to other traffic instead of crowding it out.
/// Loss halves the window, once per stretch without an ACK, so an outage costs one halving rather than one per timed-out packet.
pub struct PathPace {
    /// The window in packets, as log2: the multiplicative steps are additions here, and the window stays above zero however far it shrinks.
    log2_win: f32,
    /// The smallest RTT seen on this path since its pace opened; None until a sample arrives.
    /// The manager closes a pace when its path goes idle, so our own queue has drained by the next transfer and its first ACK measures the base afresh (a route that changed under the same address cannot leave a stale base crushing the window).
    base_rtt: Option<Duration>,
    /// A loss has halved the window and no ACK has arrived since: a further loss is the same stretch of trouble and does not halve again.
    halved: bool,
}

impl PathPace {
    pub fn new() -> Self {
        Self { log2_win: PACE_INITIAL_LOG2, base_rtt: None, halved: false }
    }

    /// The window in packets (fractional: below one, the empty-path rule in [`PathPace::admits`] still lets one packet out).
    pub fn window(&self) -> f32 {
        self.log2_win.exp2()
    }

    /// May one more new packet go out on a path with `in_flight` packets outstanding?
    /// An empty path always admits one: with nothing in flight no ACK can come back to open the window, so that packet is the ACK clock's seed.
    pub fn admits(&self, in_flight: usize) -> bool {
        in_flight == 0 || (in_flight as f32) < self.window()
    }

    /// One timed ACK: `sample` its round trip, `in_flight` the path's packets outstanding when it arrived, the acked one counted, so it is at least one.
    /// The window grows only while it binds (at least half of it in flight): a path the application leaves underused learns nothing about how much more it could carry, and a window grown there would dump one burst on the next transfer.
    pub fn on_rtt(&mut self, sample: Duration, in_flight: usize) {
        let base = match self.base_rtt {
            Some(b) if b <= sample => b,
            _ => sample,
        };
        self.base_rtt = Some(base);
        self.halved = false;
        // A network round trip is never zero on a nanosecond clock, so the quotient is finite.
        let step = ((base + PACE_TARGET).as_secs_f32() / sample.as_secs_f32()).log2() / in_flight as f32;
        if step < 0. || 2. * in_flight as f32 >= self.window() {
            self.log2_win += step;
        }
    }

    /// A loss (a retransmit timeout, a NAK, the receiver's SlowDown): halve, once per stretch without an ACK.
    pub fn on_loss(&mut self) {
        if !self.halved {
            self.log2_win -= 1.;
            self.halved = true;
        }
    }

    /// The smallest RTT seen on this path since its pace opened, for the log.
    pub fn base_rtt(&self) -> Option<Duration> {
        self.base_rtt
    }
}

impl Default for PathPace {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks in-flight packets for timeout detection, and times their ACKs.
pub struct FlightTracker {
    /// Packets currently in flight: (sequence, send time, sent more than once).
    in_flight: Vec<(u32, Instant, bool)>,
}

impl FlightTracker {
    pub fn new() -> Self {
        Self { in_flight: Vec::new() }
    }

    /// Record a packet's first send.
    pub fn sent(&mut self, sequence: u32) {
        self.in_flight.push((sequence, Instant::now(), false));
    }

    /// Record a retransmit: in flight again, but its ACK is never timed (Karn's rule — an ACK for a packet sent twice may answer either copy, and timing it from the second send under-reads the RTT, which would drag the path's base RTT below the truth for good).
    pub fn resent(&mut self, sequence: u32) {
        self.in_flight.push((sequence, Instant::now(), true));
    }

    /// Record an ACK: the RTT sample when the packet was in flight and sent once, None otherwise.
    pub fn acked(&mut self, sequence: u32) -> Option<Duration> {
        let pos = self.in_flight.iter().position(|(s, _, _)| *s == sequence)?;
        let (_, send_time, resent) = self.in_flight.remove(pos);
        (!resent).then(|| send_time.elapsed())
    }

    /// Take the sequences whose send is older than `timeout` out of flight.
    pub fn timed_out(&mut self, timeout: Duration) -> Vec<u32> {
        let now = Instant::now();
        let mut timed_out = Vec::new();
        self.in_flight.retain(|(seq, send_time, _)| {
            if now.duration_since(*send_time) >= timeout {
                timed_out.push(*seq);
                false
            } else {
                true
            }
        });
        timed_out
    }

    /// Number of packets currently in flight
    pub fn count(&self) -> usize {
        self.in_flight.len()
    }

    /// Clear all in-flight tracking (on abort/reset)
    pub fn clear(&mut self) {
        self.in_flight.clear();
    }
}

impl Default for FlightTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtt_estimator_initial() {
        let mut rtt = RTTEstimator::new();

        // First sample initializes directly
        rtt.update(Duration::from_millis(50));
        assert_eq!(rtt.srtt(), Duration::from_millis(50));
    }

    #[test]
    fn test_rtt_estimator_convergence() {
        let mut rtt = RTTEstimator::new();

        // Feed consistent samples
        for _ in 0..20 {
            rtt.update(Duration::from_millis(100));
        }

        // Should converge close to 100ms
        let srtt = rtt.srtt().as_millis();
        assert!(srtt >= 90 && srtt <= 110, "SRTT was {}ms", srtt);
    }

    /// One round trip of ACKs at a full window, `rtt` each, the sender refilling the window after every ACK as the manager's pump does.
    fn round(p: &mut PathPace, rtt: Duration) {
        let n = p.window().ceil() as usize;
        for _ in 0..n {
            p.on_rtt(rtt, n);
        }
    }

    /// The Mac's link: base 40 ms. A clear path grows the window; a queue past the target (the 2026-10-29 70-115 ms) shrinks it; the target itself holds it.
    #[test]
    fn the_pace_settles_where_the_queue_holds_the_target() {
        let base = Duration::from_millis(40);
        let mut p = PathPace::new();
        let w0 = p.window();
        round(&mut p, base);
        assert!(p.window() > w0, "a clear path grows the window");
        let grown = p.window();
        round(&mut p, Duration::from_millis(100));
        assert!(p.window() < grown, "a standing queue shrinks it");
        let held = p.window();
        round(&mut p, base + PACE_TARGET);
        assert!((p.window() - held).abs() < held / 64., "at base + target it holds ({} → {})", held, p.window());
    }

    /// An outage costs one halving however many packets time out; the next ACK re-arms it.
    #[test]
    fn loss_halves_once_per_stretch_without_an_ack() {
        let mut p = PathPace::new();
        let w = p.window();
        for _ in 0..8 {
            p.on_loss();
        }
        assert_eq!(p.window(), w / 2.);
        p.on_rtt(Duration::from_millis(40), 1);
        let after = p.window();
        p.on_loss();
        assert_eq!(p.window(), after / 2.);
    }

    /// An empty path admits one packet however small the window, and a full one admits none.
    #[test]
    fn an_empty_path_always_admits_the_seed() {
        let mut p = PathPace::new();
        for _ in 0..8 {
            p.on_loss();
            p.on_rtt(Duration::from_secs(1), 1);
        }
        assert!(p.window() < 1.);
        assert!(p.admits(0));
        assert!(!p.admits(1));
    }

    /// An underused path (two in flight against sixteen) never grows its window; a queue still shrinks it.
    #[test]
    fn an_underused_window_does_not_grow() {
        let mut p = PathPace::new();
        let w = p.window();
        p.on_rtt(Duration::from_millis(40), 2);
        p.on_rtt(Duration::from_millis(40), 1);
        assert_eq!(p.window(), w);
        p.on_rtt(Duration::from_millis(200), 2);
        assert!(p.window() < w);
    }

    #[test]
    fn test_flight_tracker() {
        let mut tracker = FlightTracker::new();
        tracker.sent(0);
        tracker.sent(1);
        tracker.sent(2);
        assert_eq!(tracker.count(), 3);
        assert!(tracker.acked(1).is_some());
        assert_eq!(tracker.count(), 2);
        assert!(tracker.acked(99).is_none());
    }

    /// A retransmitted packet leaves flight on its ACK but gives no RTT sample.
    #[test]
    fn a_resent_packet_is_never_timed() {
        let mut tracker = FlightTracker::new();
        tracker.resent(4);
        assert!(tracker.acked(4).is_none());
        assert_eq!(tracker.count(), 0);
    }
}
