//! PT Adaptive Windowing and RTT Estimation
//!
//! Implements blast-256 congestion control:
//! - Initial blast: send up to INITIAL_BLAST packets immediately
//! - Send ratio: send multiple packets per ACK (default 2.0)
//! - Loss adaptation: adjust ratio based on observed loss rate
//! - No artificial window cap - naturally fills available BDP
//!
//! This is NOT TCP - we intentionally overshoot to saturate the link,
//! then clean up gaps in sweep cycles after all data is sent.

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

/// Initial blast size - send this many packets immediately
pub const INITIAL_BLAST: u32 = 256;

/// Blast-256 window controller
///
/// Implements aggressive link saturation:
/// - Initial blast: send INITIAL_BLAST packets immediately (no slow start)
/// - Send ratio: for each ACK, send floor(send_ratio) new packets
/// - Loss adaptation: rolling EMA updated per-ACK
/// - No artificial max_window - BDP naturally limits in-flight
///
/// Philosophy: saturate first, clean up gaps later
pub struct WindowController {
    /// Send ratio - packets to send per ACK received (always > 1.0)
    send_ratio: f32,
    /// Rolling loss rate EMA (0.0 to 1.0)
    loss_rate: f32,
    /// Whether we're still in initial blast phase
    in_blast_phase: bool,
    /// Packets remaining in initial blast
    blast_remaining: u32,
    /// Fractional packet accumulator (for non-integer ratios)
    fractional_accum: f32,
}

impl WindowController {
    /// Create new window controller
    pub fn new() -> Self {
        Self {
            send_ratio: 2.0, // Start aggressive: 2 packets per ACK
            loss_rate: 0.0,
            in_blast_phase: true,
            blast_remaining: INITIAL_BLAST,
            fractional_accum: 0.0,
        }
    }

    /// Get current window size (for compatibility with FlightTracker)
    /// In blast phase, return blast_remaining
    /// After blast, this is effectively unlimited (we use send_ratio instead)
    pub fn window(&self) -> u32 {
        if self.in_blast_phase {
            self.blast_remaining.max(1)
        } else {
            // After blast, allow large in-flight count
            // Real limit is send_ratio controlling new sends
            65536
        }
    }

    /// Get number of packets to send for this ACK
    /// Returns 0 if we shouldn't send (during sweep phase)
    pub fn packets_per_ack(&mut self) -> u32 {
        if self.in_blast_phase {
            return 0; // Blast phase doesn't use per-ACK sending
        }

        // Add ratio to accumulator
        self.fractional_accum += self.send_ratio;

        // Extract integer part
        let to_send = self.fractional_accum as u32;
        self.fractional_accum -= to_send as f32;

        to_send
    }

    /// Called on successful ACK - update rolling loss rate and adapt ratio
    pub fn on_ack(&mut self) {
        // EMA update: successful ACK = 0 loss for this sample
        // α = 0.02 gives ~50 packet smoothing window
        self.loss_rate = 0.98 * self.loss_rate;

        // Adapt ratio based on current loss rate
        if self.loss_rate > 0.10 {
            // >10% loss - back off
            self.send_ratio = (self.send_ratio * 0.995).max(1.1);
        } else if self.loss_rate < 0.01 {
            // <1% loss - push harder
            self.send_ratio = (self.send_ratio * 1.001).min(4.0);
        }
        // 1-10% loss - hold steady
    }

    /// Called on packet loss (timeout or NAK)
    pub fn on_loss(&mut self) {
        // EMA update: loss = 1.0 for this sample
        self.loss_rate = 0.98 * self.loss_rate + 0.02;

        // Immediate backoff on loss
        self.send_ratio = (self.send_ratio * 0.95).max(1.1);
    }

    /// Consume one blast packet (call when sending during blast phase)
    pub fn consume_blast(&mut self) {
        if self.blast_remaining > 0 {
            self.blast_remaining -= 1;
            if self.blast_remaining == 0 {
                self.in_blast_phase = false;
            }
        }
    }

    /// Check if we're in initial blast phase
    pub fn in_blast_phase(&self) -> bool {
        self.in_blast_phase
    }

    /// Check if we're in slow start phase (compatibility - always false for blast)
    pub fn in_slow_start(&self) -> bool {
        self.in_blast_phase
    }

    /// Get current send ratio (for stats/logging)
    pub fn send_ratio(&self) -> f32 {
        self.send_ratio
    }

    /// Get current loss rate (for stats/logging)
    pub fn loss_rate(&self) -> f32 {
        self.loss_rate
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
    fn test_window_blast_phase() {
        let mut window = WindowController::new();

        // Should start in blast phase with INITIAL_BLAST remaining
        assert!(window.in_blast_phase());
        assert_eq!(window.window(), INITIAL_BLAST);

        // Consume some blast
        for _ in 0..10 {
            window.consume_blast();
        }
        assert!(window.in_blast_phase());
        assert_eq!(window.window(), INITIAL_BLAST - 10);

        // Consume all remaining
        for _ in 0..(INITIAL_BLAST - 10) {
            window.consume_blast();
        }
        assert!(!window.in_blast_phase());
    }

    #[test]
    fn test_window_send_ratio() {
        let mut window = WindowController::new();

        // Exit blast phase
        for _ in 0..INITIAL_BLAST {
            window.consume_blast();
        }
        assert!(!window.in_blast_phase());

        // Default ratio is 2.0, so should send 2 packets per ACK
        let to_send = window.packets_per_ack();
        assert_eq!(to_send, 2);
    }

    #[test]
    fn test_window_loss_backoff() {
        let mut window = WindowController::new();

        // Exit blast phase
        for _ in 0..INITIAL_BLAST {
            window.consume_blast();
        }

        let initial_ratio = window.send_ratio();

        // Loss should reduce ratio
        window.on_loss();
        assert!(window.send_ratio() < initial_ratio);
        assert!(window.send_ratio() >= 1.1); // Never below 1.1
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
