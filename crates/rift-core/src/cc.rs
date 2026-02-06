use std::time::{Duration, Instant};
use tracing::{debug, info};

/// States for the DELTA Congestion Control algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaState {
    /// Delay is flat or decreasing.
    Stable,
    /// Delay is consistently increasing.
    Rising,
    /// Critical delay threshold breached.
    Congested,
}

/// Configuration for DELTA Congestion Control.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeltaConfig {
    /// Target queuing delay in microseconds (T_limit). Default 15ms.
    pub target_delay_us: u64,
    /// EWMA alpha for RTT smoothing. Default 0.125.
    pub alpha: f64,
    /// Back-off factor on congestion (Beta). Default 0.85.
    pub beta: f64,
    /// Additive increase step in kbps. Default 500kbps.
    pub increase_kbps: u32,
    /// Minimum bitrate in kbps.
    pub min_bitrate_kbps: u32,
    /// Maximum bitrate in kbps.
    pub max_bitrate_kbps: u32,
    /// Persistence factor for state transitions. Default 3.
    pub k_persistence: usize,
    /// Slope noise floor (Epsilon) in microseconds. Default 100us.
    pub epsilon_us: f64,
}

impl Default for DeltaConfig {
    fn default() -> Self {
        Self {
            target_delay_us: 20_000,
            alpha: 0.125,
            beta: 0.85,
            increase_kbps: 500,
            min_bitrate_kbps: 2_000,
            max_bitrate_kbps: 50_000,
            k_persistence: 5,
            epsilon_us: 100.0,
        }
    }
}

/// Implementation of DELTA (Differential Latency Estimation and Tuning Algorithm).
pub struct DeltaCC {
    config: DeltaConfig,

    // Signals
    rtt_smooth_us: f64,
    rtt_min_us: u64,
    last_d_q_us: f64,

    // State machine
    state: DeltaState,
    rising_count: usize,
    stable_count: usize,
    congested_start: Option<Instant>,

    // Windowed minimum tracking
    window_samples: Vec<(Instant, u64)>,
    window_duration: Duration,

    // Outputs
    current_bitrate_kbps: u32,
    current_fps: u32,
    fec_ratio: f32, // 0.0 to 1.0
}

impl DeltaCC {
    pub fn new(config: DeltaConfig, initial_bitrate: u32, initial_fps: u32) -> Self {
        Self {
            config,
            rtt_smooth_us: 0.0,
            rtt_min_us: u64::MAX,
            last_d_q_us: 0.0,
            state: DeltaState::Stable,
            rising_count: 0,
            stable_count: 0,
            congested_start: None,
            window_samples: Vec::new(),
            window_duration: Duration::from_secs(10),
            current_bitrate_kbps: initial_bitrate,
            current_fps: initial_fps,
            fec_ratio: 0.05, // Start with 5% baseline
        }
    }

    /// Process a new RTT sample and update congestion state and parameters.
    /// Jitter is used to preemptively adjust FEC before packet loss occurs.
    pub fn on_rtt_sample(&mut self, rtt_us: u64, packet_loss: f32, jitter_us: u32) {
        let now = Instant::now();

        // 1. Update RTT Min Window
        self.update_rtt_min(now, rtt_us);

        // 2. Update EWMA Smooth RTT
        if self.rtt_smooth_us == 0.0 {
            self.rtt_smooth_us = rtt_us as f64;
        } else {
            self.rtt_smooth_us = (1.0 - self.config.alpha) * self.rtt_smooth_us
                + self.config.alpha * (rtt_us as f64);
        }

        // 3. Compute Queue Delay and Slope
        let d_q = (self.rtt_smooth_us - self.rtt_min_us as f64).max(0.0);
        let delta_q = d_q - self.last_d_q_us;
        self.last_d_q_us = d_q;

        // 4. Dynamic Slope Noise Floor (Epsilon)
        let epsilon = if self.rtt_smooth_us > 0.0 {
            self.rtt_smooth_us * 0.05
        } else {
            self.config.epsilon_us
        };

        // 5. State Transitions
        self.update_state(now, d_q, delta_q, epsilon);

        // 5. Update Control Params
        self.update_params(now, d_q, packet_loss, jitter_us);
    }

    fn update_rtt_min(&mut self, now: Instant, rtt_us: u64) {
        // Prune old samples
        self.window_samples
            .retain(|(t, _)| now.duration_since(*t) < self.window_duration);

        // Add new sample
        self.window_samples.push((now, rtt_us));

        // Update current minimum
        self.rtt_min_us = self
            .window_samples
            .iter()
            .map(|(_, rtt)| *rtt)
            .min()
            .unwrap_or(u64::MAX);
    }

    fn update_state(&mut self, now: Instant, d_q: f64, delta_q: f64, epsilon: f64) {
        if d_q > self.config.target_delay_us as f64 {
            if self.state != DeltaState::Congested {
                info!(
                    "DELTA: Transition to CONGESTED (Delay: {:.1}ms)",
                    d_q / 1000.0
                );
                self.state = DeltaState::Congested;
                self.congested_start = Some(now);
            }
            self.rising_count = 0;
            self.stable_count = 0;
        } else if delta_q > epsilon {
            self.rising_count += 1;
            if self.rising_count >= self.config.k_persistence && self.state == DeltaState::Stable {
                info!(
                    "DELTA: Transition to RISING (Slope: {:.1}us, Epsilon: {:.1}us)",
                    delta_q, epsilon
                );
                self.state = DeltaState::Rising;
            }
            self.stable_count = 0;
        } else if delta_q <= 0.0 {
            self.stable_count += 1;
            if self.stable_count >= self.config.k_persistence && self.state != DeltaState::Stable {
                info!("DELTA: Transition to STABLE (Delay: {:.1}ms)", d_q / 1000.0);
                self.state = DeltaState::Stable;
                self.congested_start = None;
            }
            self.rising_count = 0;
        }
    }

    fn update_params(&mut self, now: Instant, d_q: f64, packet_loss: f32, jitter_us: u32) {
        // Preemptive FEC adjustment based on jitter
        // High jitter (>10ms) indicates network instability, increase FEC before loss occurs
        if jitter_us > 10_000 {
            self.fec_ratio = (self.fec_ratio + 0.02).min(0.25);
            debug!(
                "DELTA: High jitter {}us - FEC increased to {:.0}%",
                jitter_us,
                self.fec_ratio * 100.0
            );
        } else if jitter_us > 5_000 {
            self.fec_ratio = (self.fec_ratio + 0.01).min(0.20);
        }

        match self.state {
            DeltaState::Stable => {
                // Additive Increase: R = R + Step * (1 - Dq/Tlimit)
                let gain = (1.0 - (d_q / self.config.target_delay_us as f64)).max(0.0);
                let increase = (self.config.increase_kbps as f64 * gain) as u32;
                self.current_bitrate_kbps =
                    (self.current_bitrate_kbps + increase).min(self.config.max_bitrate_kbps);

                // Gradually decay FEC ratio back to 5% baseline (only if jitter is low)
                if jitter_us < 5_000 {
                    self.fec_ratio = (self.fec_ratio - 0.001).max(0.05);
                }
            }
            DeltaState::Rising => {
                // Hold bitrate and observe
                debug!(
                    "DELTA: RISING - Bitrate held at {}kbps",
                    self.current_bitrate_kbps
                );
            }
            DeltaState::Congested => {
                // Multiplicative Decrease: R = R * Beta
                self.current_bitrate_kbps =
                    (self.current_bitrate_kbps as f64 * self.config.beta) as u32;
                self.current_bitrate_kbps =
                    self.current_bitrate_kbps.max(self.config.min_bitrate_kbps);

                // Sustained congestion leads to FPS step-down
                if let Some(start) = self.congested_start {
                    if now.duration_since(start) > Duration::from_secs(1) {
                        self.step_down_fps();
                        // Reset timer after step-down to avoid triple-dropping
                        self.congested_start = Some(now);
                    }
                }

                // Increase FEC only if loss is actually observed during congestion
                if packet_loss > 0.01 {
                    self.fec_ratio = (self.fec_ratio * 1.5).min(0.5);
                }
            }
        }
    }

    fn step_down_fps(&mut self) {
        let next_fps = match self.current_fps {
            144 | 120 => 90,
            90 => 60,
            60 => 45,
            45 => 30,
            f if f > 60 => 60,
            _ => self.current_fps, // Don't drop below 30 if possible
        };
        if next_fps != self.current_fps {
            info!(
                "DELTA: Stepping down FPS: {} -> {}",
                self.current_fps, next_fps
            );
            self.current_fps = next_fps;
        }
    }

    pub fn state(&self) -> DeltaState {
        self.state
    }

    pub fn target_bitrate_kbps(&self) -> u32 {
        self.current_bitrate_kbps
    }

    pub fn target_fps(&self) -> u32 {
        self.current_fps
    }

    pub fn fec_ratio(&self) -> f32 {
        self.fec_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_rtt_min_tracking() {
        let mut cc = DeltaCC::new(DeltaConfig::default(), 10000, 60);

        cc.on_rtt_sample(5000, 0.0, 0);
        assert_eq!(cc.rtt_min_us, 5000);

        cc.on_rtt_sample(4000, 0.0, 0);
        assert_eq!(cc.rtt_min_us, 4000);

        cc.on_rtt_sample(6000, 0.0, 0);
        assert_eq!(cc.rtt_min_us, 4000);
    }

    #[test]
    fn test_delta_transition_rising() {
        let config = DeltaConfig {
            alpha: 1.0, // Disable smoothing for easy testing
            epsilon_us: 100.0,
            k_persistence: 5,
            ..DeltaConfig::default()
        };
        let mut cc = DeltaCC::new(config, 10000, 60);
        cc.on_rtt_sample(5000, 0.0, 0); // Baseline

        cc.on_rtt_sample(5200, 0.0, 0); // rising_count = 1
        cc.on_rtt_sample(5400, 0.0, 0); // rising_count = 2
        cc.on_rtt_sample(5600, 0.0, 0); // rising_count = 3
        cc.on_rtt_sample(5800, 0.0, 0); // rising_count = 4
        assert_eq!(cc.state, DeltaState::Stable);

        cc.on_rtt_sample(6000, 0.0, 0); // rising_count = 5 -> RISING
        assert_eq!(cc.state, DeltaState::Rising);
    }

    #[test]
    fn test_delta_dynamic_epsilon() {
        let config = DeltaConfig {
            alpha: 1.0,
            epsilon_us: 100.0,
            k_persistence: 1,
            ..DeltaConfig::default()
        };
        let mut cc = DeltaCC::new(config, 10000, 60);
        cc.on_rtt_sample(5000, 0.0, 0); // Baseline

        // With 10ms jitter, epsilon = 5000us.
        // A slope of 1000us should NOT trigger RISING.
        cc.on_rtt_sample(6000, 0.0, 10000);
        assert_eq!(cc.state, DeltaState::Stable);

        // A slope of 6000us SHOULD trigger RISING because it exceeds 5000us.
        cc.on_rtt_sample(12000, 0.0, 10000);
        assert_eq!(cc.state, DeltaState::Rising);
    }

    #[test]
    fn test_delta_transition_congested() {
        let config = DeltaConfig {
            alpha: 1.0,
            target_delay_us: 10000,
            ..DeltaConfig::default()
        };
        let mut cc = DeltaCC::new(config, 10000, 60);
        cc.on_rtt_sample(5000, 0.0, 0); // Baseline

        // Breach 10ms target delay
        cc.on_rtt_sample(16000, 0.0, 0); // 16ms - 5ms = 11ms > 10ms
        assert_eq!(cc.state, DeltaState::Congested);
    }

    #[test]
    fn test_delta_bitrate_adjustment() {
        let config = DeltaConfig {
            alpha: 1.0,
            increase_kbps: 1000,
            beta: 0.5,
            target_delay_us: 10000,
            ..DeltaConfig::default()
        };
        let mut cc = DeltaCC::new(config, 10000, 60);

        // Baseline: RTT smooth init + first additive increase
        cc.on_rtt_sample(5000, 0.0, 0);
        assert_eq!(cc.target_bitrate_kbps(), 11000);

        // Stable: second additive increase
        cc.on_rtt_sample(5000, 0.0, 0);
        assert_eq!(cc.target_bitrate_kbps(), 12000);

        // Congested: half (beta 0.5)
        cc.on_rtt_sample(20000, 0.0, 0); // 15ms queue delay > 10ms target
        assert_eq!(cc.state, DeltaState::Congested);
        assert_eq!(cc.target_bitrate_kbps(), 6000); // 12000 * 0.5
    }

    #[test]
    fn test_delta_jitter_fec_adjustment() {
        let mut cc = DeltaCC::new(DeltaConfig::default(), 10000, 60);

        // Baseline FEC is 5%
        assert_eq!(cc.fec_ratio, 0.05);

        // High jitter (15ms) should increase FEC
        cc.on_rtt_sample(5000, 0.0, 15000);
        assert!(cc.fec_ratio > 0.05);

        // Low jitter (2ms) should allow FEC to decay
        let high_fec = cc.fec_ratio;
        cc.on_rtt_sample(5000, 0.0, 2000);
        assert!(cc.fec_ratio < high_fec);
    }
}
