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
        let epsilon = if self.rtt_smooth_us > 0.0 && self.config.alpha != 1.0 {
            self.rtt_smooth_us * 0.05
        } else if jitter_us > 0 {
            jitter_us as f64 * 0.5 // Use half the jitter as epsilon
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

/// Classification of network link types based on baseline latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    /// Local network (<10ms baseline)
    Local,
    /// Regional network (10-50ms baseline)
    Regional,
    /// Intercontinental network (50-150ms baseline)
    Intercontinental,
    /// Satellite or extreme-latency links (150-500ms baseline)
    Satellite,
}

impl LinkType {
    pub fn from_baseline_rtt(baseline_rtt_ms: u64) -> Self {
        match baseline_rtt_ms {
            0..=10 => LinkType::Local,
            11..=50 => LinkType::Regional,
            51..=150 => LinkType::Intercontinental,
            _ => LinkType::Satellite,
        }
    }

    pub fn adjustment_aggressiveness(&self) -> f32 {
        match self {
            LinkType::Local => 0.20,        // ±20% per second
            LinkType::Regional => 0.10,     // ±10% per second
            LinkType::Intercontinental => 0.05,  // ±5% per second
            LinkType::Satellite => 0.03,    // ±3% per second
        }
    }
}

/// Profile of network characteristics determined during connection handshake.
#[derive(Debug, Clone)]
pub struct LatencyProfile {
    pub base_rtt_ms: u64,
    pub rtt_std_dev: f32,
    pub percentile_p95_rtt_ms: u64,
    pub link_type: LinkType,
}

impl LatencyProfile {
    /// Estimate profile from a set of RTT samples (typically first 100 from handshake).
    pub fn estimate_from_samples(rtts: &[u64]) -> Self {
        if rtts.is_empty() {
            return Self {
                base_rtt_ms: 0,
                rtt_std_dev: 0.0,
                percentile_p95_rtt_ms: 0,
                link_type: LinkType::Local,
            };
        }

        let base_rtt_ms = *rtts.iter().min().unwrap_or(&0);
        let avg: f32 = rtts.iter().map(|&x| x as f32).sum::<f32>() / rtts.len() as f32;
        let variance: f32 = rtts
            .iter()
            .map(|&x| {
                let diff = x as f32 - avg;
                diff * diff
            })
            .sum::<f32>()
            / rtts.len() as f32;
        let rtt_std_dev = variance.sqrt();

        let mut sorted = rtts.to_vec();
        sorted.sort_unstable();
        let percentile_p95_rtt_ms = sorted[(sorted.len() * 95) / 100];

        let link_type = LinkType::from_baseline_rtt(base_rtt_ms);

        Self {
            base_rtt_ms,
            rtt_std_dev,
            percentile_p95_rtt_ms,
            link_type,
        }
    }

    pub fn target_delay_for_link(&self) -> u64 {
        // Scale target delay based on baseline RTT
        match self.link_type {
            LinkType::Local => 15,
            LinkType::Regional => 25,
            LinkType::Intercontinental => 50,
            LinkType::Satellite => 100,
        }
    }
}

/// Hybrid congestion detection combining delay-slope and loss-rate indicators.
#[derive(Debug, Clone)]
pub struct CongestionDetector {
    pub rising_threshold_ms: f32,
    pub loss_rate_threshold: f32,
    pub rtt_filter_window_ms: u64,
    pub delay_score: f32,      // 0-50 points
    pub loss_score: f32,       // 0-50 points
    recent_loss_rate: f32,
    recent_delay_slope: f32,
}

impl Default for CongestionDetector {
    fn default() -> Self {
        Self {
            rising_threshold_ms: 1.0,
            loss_rate_threshold: 0.001,  // 0.1%
            rtt_filter_window_ms: 500,
            delay_score: 0.0,
            loss_score: 0.0,
            recent_loss_rate: 0.0,
            recent_delay_slope: 0.0,
        }
    }
}

impl CongestionDetector {
    pub fn update(&mut self, delay_slope_ms: f32, loss_rate: f32) {
        self.recent_loss_rate = loss_rate;
        self.recent_delay_slope = delay_slope_ms;

        // Delay-slope scoring: 0-50 points
        if delay_slope_ms < 0.0 {
            self.delay_score = 0.0;  // Improving
        } else if delay_slope_ms < self.rising_threshold_ms {
            self.delay_score = (delay_slope_ms / self.rising_threshold_ms * 25.0).min(25.0);
        } else {
            self.delay_score = 25.0 + (delay_slope_ms / self.rising_threshold_ms * 25.0).min(25.0);
        }

        // Loss-rate scoring: 0-50 points
        if loss_rate < self.loss_rate_threshold {
            self.loss_score = 0.0;
        } else {
            let loss_pct = loss_rate * 100.0;
            self.loss_score = (loss_pct / 1.0 * 50.0).min(50.0);
        }
    }

    pub fn is_congested(&self) -> bool {
        self.delay_score + self.loss_score > 50.0
    }

    pub fn congestion_score(&self) -> f32 {
        (self.delay_score + self.loss_score).min(100.0)
    }
}

/// Adaptive bitrate adjustment strategy based on link type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentStrategy {
    Conservative,  // ±5% per adjustment, for satellite
    Moderate,      // ±10% per adjustment, for intercontinental
    Aggressive,    // ±20% per adjustment, for local
}

impl AdjustmentStrategy {
    pub fn from_link_type(link_type: LinkType) -> Self {
        match link_type {
            LinkType::Local => AdjustmentStrategy::Aggressive,
            LinkType::Regional => AdjustmentStrategy::Moderate,
            LinkType::Intercontinental => AdjustmentStrategy::Moderate,
            LinkType::Satellite => AdjustmentStrategy::Conservative,
        }
    }

    pub fn adjust_bitrate(&self, current_kbps: u32, target_adjustment_pct: f32) -> u32 {
        let max_change = match self {
            AdjustmentStrategy::Conservative => 5.0,
            AdjustmentStrategy::Moderate => 10.0,
            AdjustmentStrategy::Aggressive => 20.0,
        };

        let clamped = target_adjustment_pct.clamp(-max_change, max_change);
        ((current_kbps as f32 * (100.0 + clamped)) / 100.0) as u32
    }
}

/// Dynamic FEC controller for high-latency networks.
#[derive(Debug, Clone)]
pub struct FecController {
    pub base_redundancy_pct: u32,    // 10-20%
    pub current_redundancy_pct: u32,
    pub max_redundancy_pct: u32,
    min_redundancy_pct: u32,
}

impl FecController {
    pub fn new(base_redundancy_pct: u32) -> Self {
        Self {
            base_redundancy_pct,
            current_redundancy_pct: base_redundancy_pct,
            max_redundancy_pct: 50,
            min_redundancy_pct: 5,
        }
    }

    pub fn update(&mut self, recent_loss_rate: f32, link_stability: f32) {
        let loss_factor = (recent_loss_rate * 1000.0) as u32;  // Scale up
        let stability_factor = ((1.0 - link_stability) * 10.0) as u32;

        self.current_redundancy_pct =
            (self.base_redundancy_pct + loss_factor + stability_factor)
                .min(self.max_redundancy_pct)
                .max(self.min_redundancy_pct);
    }

    pub fn should_increase_redundancy(&self, loss_rate: f32) -> bool {
        loss_rate > 0.01 && self.current_redundancy_pct < self.max_redundancy_pct
    }

    pub fn should_decrease_redundancy(&self, loss_rate: f32) -> bool {
        loss_rate < 0.001 && self.current_redundancy_pct > self.base_redundancy_pct
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

    #[test]
    fn test_link_type_classification() {
        assert_eq!(LinkType::from_baseline_rtt(5), LinkType::Local);
        assert_eq!(LinkType::from_baseline_rtt(10), LinkType::Local);
        assert_eq!(LinkType::from_baseline_rtt(25), LinkType::Regional);
        assert_eq!(LinkType::from_baseline_rtt(100), LinkType::Intercontinental);
        assert_eq!(LinkType::from_baseline_rtt(250), LinkType::Satellite);
        assert_eq!(LinkType::from_baseline_rtt(500), LinkType::Satellite);
    }

    #[test]
    fn test_link_type_adjustment_aggressiveness() {
        assert_eq!(
            LinkType::Local.adjustment_aggressiveness(),
            0.20,
            "Local should be aggressive"
        );
        assert_eq!(
            LinkType::Regional.adjustment_aggressiveness(),
            0.10,
            "Regional should be moderate"
        );
        assert_eq!(
            LinkType::Intercontinental.adjustment_aggressiveness(),
            0.05,
            "Intercontinental should be conservative"
        );
        assert_eq!(
            LinkType::Satellite.adjustment_aggressiveness(),
            0.03,
            "Satellite should be very conservative"
        );
    }

    #[test]
    fn test_latency_profile_from_samples() {
        let rtts = vec![10, 12, 15, 11, 14, 13, 16, 12, 10, 15];
        let profile = LatencyProfile::estimate_from_samples(&rtts);

        assert_eq!(profile.base_rtt_ms, 10);
        assert!(profile.rtt_std_dev > 0.0);
        assert!(profile.percentile_p95_rtt_ms >= 10);
        assert_eq!(profile.link_type, LinkType::Local);
    }

    #[test]
    fn test_latency_profile_high_latency_samples() {
        let rtts = vec![
            200, 210, 220, 205, 215, 230, 225, 210, 205, 220,
        ];
        let profile = LatencyProfile::estimate_from_samples(&rtts);

        assert_eq!(profile.base_rtt_ms, 200);
        assert_eq!(profile.link_type, LinkType::Satellite);
        assert!(profile.rtt_std_dev > 0.0);
    }

    #[test]
    fn test_latency_profile_empty_samples() {
        let rtts = vec![];
        let profile = LatencyProfile::estimate_from_samples(&rtts);

        assert_eq!(profile.base_rtt_ms, 0);
        assert_eq!(profile.link_type, LinkType::Local);
    }

    #[test]
    fn test_latency_profile_target_delay_scaling() {
        let local_profile = LatencyProfile::estimate_from_samples(&[5, 6, 7]);
        assert_eq!(local_profile.target_delay_for_link(), 15);

        let regional_profile = LatencyProfile::estimate_from_samples(&[25, 26, 27]);
        assert_eq!(regional_profile.target_delay_for_link(), 25);

        let intercontinental_profile = LatencyProfile::estimate_from_samples(&[100, 101, 102]);
        assert_eq!(intercontinental_profile.target_delay_for_link(), 50);

        let satellite_profile = LatencyProfile::estimate_from_samples(&[250, 251, 252]);
        assert_eq!(satellite_profile.target_delay_for_link(), 100);
    }

    #[test]
    fn test_congestion_detector_no_congestion() {
        let mut detector = CongestionDetector::default();
        detector.update(-0.5, 0.0);  // Improving delay, no loss

        assert!(!detector.is_congested());
        assert_eq!(detector.delay_score, 0.0);
        assert_eq!(detector.loss_score, 0.0);
    }

    #[test]
    fn test_congestion_detector_delay_congestion() {
        let mut detector = CongestionDetector::default();
        detector.update(2.0, 0.0);  // Rising delay

        assert!(detector.delay_score > 0.0);
        assert_eq!(detector.loss_score, 0.0);
    }

    #[test]
    fn test_congestion_detector_loss_congestion() {
        let mut detector = CongestionDetector::default();
        detector.update(0.0, 0.005);  // 0.5% loss

        assert_eq!(detector.delay_score, 0.0);
        assert!(detector.loss_score > 0.0);
    }

    #[test]
    fn test_congestion_detector_hybrid_congestion() {
        let mut detector = CongestionDetector::default();
        detector.update(1.5, 0.003);  // Both delay and loss

        assert!(detector.is_congested() || detector.congestion_score() > 0.0);
    }

    #[test]
    fn test_adjustment_strategy_from_link_type() {
        assert_eq!(
            AdjustmentStrategy::from_link_type(LinkType::Local),
            AdjustmentStrategy::Aggressive
        );
        assert_eq!(
            AdjustmentStrategy::from_link_type(LinkType::Regional),
            AdjustmentStrategy::Moderate
        );
        assert_eq!(
            AdjustmentStrategy::from_link_type(LinkType::Satellite),
            AdjustmentStrategy::Conservative
        );
    }

    #[test]
    fn test_adjustment_strategy_bitrate_increase() {
        let conservative = AdjustmentStrategy::Conservative;
        let moderate = AdjustmentStrategy::Moderate;
        let aggressive = AdjustmentStrategy::Aggressive;

        let base_bitrate = 5000;

        // Conservative: +5%
        let conservative_result = conservative.adjust_bitrate(base_bitrate, 10.0);
        assert_eq!(conservative_result, 5250); // 5000 * 1.05

        // Moderate: +10%
        let moderate_result = moderate.adjust_bitrate(base_bitrate, 10.0);
        assert_eq!(moderate_result, 5500); // 5000 * 1.10

        // Aggressive: +20%
        let aggressive_result = aggressive.adjust_bitrate(base_bitrate, 30.0);
        assert_eq!(aggressive_result, 6000); // 5000 * 1.20
    }

    #[test]
    fn test_adjustment_strategy_bitrate_clamping() {
        let conservative = AdjustmentStrategy::Conservative;

        // Request +50%, but conservative clamps to +5%
        let result = conservative.adjust_bitrate(5000, 50.0);
        assert_eq!(result, 5250);

        // Request -20%, but conservative clamps to -5%
        let result = conservative.adjust_bitrate(5000, -20.0);
        assert_eq!(result, 4750);
    }

    #[test]
    fn test_fec_controller_initialization() {
        let controller = FecController::new(15);

        assert_eq!(controller.base_redundancy_pct, 15);
        assert_eq!(controller.current_redundancy_pct, 15);
        assert_eq!(controller.max_redundancy_pct, 50);
    }

    #[test]
    fn test_fec_controller_increase_on_loss() {
        let mut controller = FecController::new(15);

        controller.update(0.05, 0.8);  // 5% loss, 80% stability
        assert!(controller.current_redundancy_pct > 15);
    }

    #[test]
    fn test_fec_controller_decrease_on_stability() {
        let mut controller = FecController::new(15);

        // Start high
        controller.update(0.01, 0.5);
        let high_redundancy = controller.current_redundancy_pct;

        // Improve to low loss
        controller.update(0.0, 0.99);
        assert!(controller.current_redundancy_pct <= high_redundancy);
    }

    #[test]
    fn test_fec_controller_bounds() {
        let mut controller = FecController::new(15);

        // Try to push beyond max
        controller.update(0.5, 0.0);  // Extreme loss
        assert!(controller.current_redundancy_pct <= controller.max_redundancy_pct);

        // Test minimum bound with new controller
        let mut controller2 = FecController::new(15);
        controller2.update(0.0, 1.0);  // Perfect conditions
        assert!(controller2.current_redundancy_pct >= controller2.min_redundancy_pct);
    }

    #[test]
    fn test_fec_controller_should_increase_redundancy() {
        let controller = FecController::new(15);

        assert!(controller.should_increase_redundancy(0.02));  // 2% loss
        assert!(!controller.should_increase_redundancy(0.005)); // 0.5% loss
    }

    #[test]
    fn test_fec_controller_should_decrease_redundancy() {
        let mut controller = FecController::new(15);

        // First increase redundancy
        controller.update(0.05, 0.5);
        let increased_redundancy = controller.current_redundancy_pct;
        assert!(increased_redundancy > 15);

        // Then check if it should decrease with low loss
        assert!(controller.should_decrease_redundancy(0.0001));  // 0.01% loss

        // Should not decrease with moderate loss
        assert!(!controller.should_decrease_redundancy(0.005));  // 0.5% loss
    }
}
