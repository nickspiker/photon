//! PLTP Adaptive Windowing and RTT Estimation
//!
//! Implements TCP-like congestion control:
//! - Slow start: exponential growth until loss or ssthresh
//! - Congestion avoidance: linear growth after ssthresh
//! - RTT estimation: exponential moving average with variance tracking

use std::time::{Duration, Instant};

/// RTT Estimator using TCP-style exponential moving average
///
/// SRTT = (1-α)*SRTT + α*R  where α=0.125
/// RTTVAR = (1-β)*RTTVAR + β*|SRTT-R|  where β=0.25
/// RTO = SRTT + 4*RTTVAR
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

/// Adaptive window controller
///
/// Implements TCP-like congestion control:
/// - Slow start: double window on each ACK until ssthresh or loss
/// - Congestion avoidance: add 1/window on each ACK (linear growth)
/// - On loss: set ssthresh = window/2, window = 1, enter slow start
pub struct WindowController {
    /// Current congestion window size (in packets)
    window_size: u32,
    /// Slow start threshold
    ssthresh: u32,
    /// Maximum window size
    max_window: u32,
    /// Number of ACKs received in current RTT (for congestion avoidance)
    ack_count: u32,
}

impl WindowController {
    /// Create new window controller
    pub fn new() -> Self {
        Self {
            window_size: 1,
            ssthresh: 64,
            max_window: 256,
            ack_count: 0,
        }
    }

    /// Get current window size
    pub fn window(&self) -> u32 {
        self.window_size
    }

    /// Called on successful ACK
    pub fn on_ack(&mut self) {
        if self.window_size < self.ssthresh {
            // Slow start: exponential growth
            self.window_size = self.window_size.saturating_add(1);
        } else {
            // Congestion avoidance: linear growth (add 1 per RTT)
            self.ack_count += 1;
            if self.ack_count >= self.window_size {
                self.window_size = self.window_size.saturating_add(1);
                self.ack_count = 0;
            }
        }

        // Cap at max window
        self.window_size = self.window_size.min(self.max_window);
    }

    /// Called on packet loss (timeout or NAK)
    pub fn on_loss(&mut self) {
        // Multiplicative decrease
        self.ssthresh = (self.window_size / 2).max(2);
        self.window_size = 1;
        self.ack_count = 0;
    }

    /// Called when receiver signals buffer pressure (buffer_pct > 75%)
    pub fn on_buffer_pressure(&mut self) {
        // Gentle slowdown - halve window but don't reset to 1
        self.window_size = (self.window_size / 2).max(1);
        self.ack_count = 0;
    }

    /// Check if we're in slow start phase
    pub fn in_slow_start(&self) -> bool {
        self.window_size < self.ssthresh
    }
}

impl Default for WindowController {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks in-flight packets for timeout detection
pub struct FlightTracker {
    /// Packets currently in flight: (sequence, send_time)
    in_flight: Vec<(u32, Instant)>,
}

impl FlightTracker {
    pub fn new() -> Self {
        Self {
            in_flight: Vec::new(),
        }
    }

    /// Record packet sent
    pub fn sent(&mut self, sequence: u32) {
        self.in_flight.push((sequence, Instant::now()));
    }

    /// Record ACK received, returns RTT sample if found
    pub fn acked(&mut self, sequence: u32) -> Option<Duration> {
        if let Some(pos) = self.in_flight.iter().position(|(s, _)| *s == sequence) {
            let (_, send_time) = self.in_flight.remove(pos);
            Some(send_time.elapsed())
        } else {
            None
        }
    }

    /// Get sequences that have timed out
    pub fn timed_out(&mut self, timeout: Duration) -> Vec<u32> {
        let now = Instant::now();
        let mut timed_out = Vec::new();

        self.in_flight.retain(|(seq, send_time)| {
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

    /// Check if we can send more (window not full)
    pub fn can_send(&self, window: u32) -> bool {
        (self.in_flight.len() as u32) < window
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

    #[test]
    fn test_window_slow_start() {
        let mut window = WindowController::new();

        assert_eq!(window.window(), 1);
        assert!(window.in_slow_start());

        // Slow start: should grow by 1 per ACK (exponential when all ACKs come)
        window.on_ack();
        assert_eq!(window.window(), 2);

        window.on_ack();
        assert_eq!(window.window(), 3);
    }

    #[test]
    fn test_window_loss_recovery() {
        let mut window = WindowController::new();

        // Grow window
        for _ in 0..20 {
            window.on_ack();
        }
        let before_loss = window.window();
        assert!(before_loss > 10);

        // Loss event
        window.on_loss();
        assert_eq!(window.window(), 1);
        assert!(window.in_slow_start());

        // ssthresh should be half of previous window
        // We can verify by growing back - slow start should end at ssthresh
    }

    #[test]
    fn test_flight_tracker() {
        let mut tracker = FlightTracker::new();

        tracker.sent(0);
        tracker.sent(1);
        tracker.sent(2);

        assert_eq!(tracker.count(), 3);
        assert!(!tracker.can_send(3));
        assert!(tracker.can_send(4));

        // ACK packet 1
        let rtt = tracker.acked(1);
        assert!(rtt.is_some());
        assert_eq!(tracker.count(), 2);

        // ACK unknown packet
        assert!(tracker.acked(99).is_none());
    }
}
