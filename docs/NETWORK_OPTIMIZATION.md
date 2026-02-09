# Network Optimization - QUIC Congestion Control Tuning

**Status**: Planning & Analysis Phase
**Focus**: High-Latency Network Optimization
**Target Implementation**: 2 weeks

---

## Overview

QUIC's built-in congestion control works well for typical networks but can be suboptimal for high-latency, variable-bandwidth scenarios common in remote streaming. This document outlines tuning strategies for improving RIFT's DELTA congestion control when operating over QUIC.

## Current Architecture

### DELTA Congestion Control (rift-core/src/cc.rs)

DELTA (Differential Latency Estimation and Tuning Algorithm) is Wavry's custom congestion control algorithm that:

1. **Tracks one-way queuing delay** via RTT smoothing
2. **Identifies trends**: STABLE → RISING → CONGESTED states
3. **Adjusts bitrate** based on delay slopes, not throughput
4. **Manages FEC redundancy** dynamically based on network stability

### QUIC Integration

QUIC provides:
- Built-in congestion control (Cubic/AIMD-like)
- Loss detection and recovery
- ACK-based feedback
- Congestion window management

**Current Gap**: DELTA operates independently of QUIC's CC, potentially causing conflicts

---

## High-Latency Tuning Strategies

### 1. RTT Baseline Calibration

**Problem**: High-latency links (satellite, intercontinental) have large baseline RTTs.

**Solution**:
```rust
// Measure baseline RTT during handshake
pub struct LatencyProfile {
    pub base_rtt_ms: u64,
    pub rtt_std_dev: f32,
    pub percentile_p95_rtt: u64,
    pub link_type: LinkType,  // Satellite, Intercontinental, etc.
}

pub enum LinkType {
    Local,           // <10ms baseline
    Regional,        // 10-50ms baseline
    Intercontinental, // 50-150ms baseline
    Satellite,       // 150-500ms baseline
}

impl LatencyProfile {
    pub fn estimate_from_samples(rtts: &[u64]) -> Self {
        // Calculate baseline and adapt DELTA parameters accordingly
    }
}
```

**Implementation**:
- Collect first 100 RTT samples after handshake
- Classify link type automatically
- Adjust `rtt_threshold` and transition margins

### 2. Congestion Detection Refinement

**Problem**: DELTA's congestion detection is delay-slope based, but high-latency links may have different characteristics.

**Solution - Hybrid Detection**:
```rust
pub struct CongestionDetector {
    // Original delay-slope based detection
    rising_threshold_ms: f32,

    // Add loss-rate detection for high-latency scenarios
    loss_rate_threshold: f32,  // e.g., 0.1% = congested

    // Time constants adapted to link latency
    rtt_filter_window_ms: u64,

    // Hybrid scoring (0-100)
    delay_score: f32,   // 0-50 points
    loss_score: f32,    // 0-50 points
}

impl CongestionDetector {
    pub fn is_congested(&self) -> bool {
        self.delay_score + self.loss_score > 50.0
    }
}
```

**Benefits**:
- Loss rate becomes primary indicator in lossy high-latency links
- Delay-slope still detects queuing on stable links
- Prevents false positives from natural jitter

### 3. Bitrate Adjustment Curves

**Problem**: Linear bitrate adjustments may be too aggressive or slow on high-latency links.

**Solution - Adaptive Curves**:
```rust
pub enum AdjustmentStrategy {
    Conservative,  // ±5% per second, for satellite
    Moderate,      // ±10% per second, for intercontinental
    Aggressive,    // ±20% per second, for local
}

impl AdjustmentStrategy {
    pub fn adjust_bitrate(
        &self,
        current_kbps: u32,
        target_adjustment_pct: f32,
    ) -> u32 {
        let max_change = match self {
            Conservative => 5,
            Moderate => 10,
            Aggressive => 20,
        };

        let clamped = target_adjustment_pct.clamp(-max_change as f32, max_change as f32);
        ((current_kbps as f32 * (100.0 + clamped)) / 100.0) as u32
    }
}
```

### 4. FEC Redundancy Optimization

**Problem**: Fixed FEC overhead may be wasteful on stable links, insufficient on lossy ones.

**Solution - Dynamic Overhead**:
```rust
pub struct FecController {
    pub base_redundancy_pct: u32,  // 10-20%
    pub current_redundancy_pct: u32,
}

impl FecController {
    pub fn update(&mut self, recent_loss_rate: f32, link_stability: f32) {
        let loss_factor = (recent_loss_rate * 1000.0) as u32;  // Scale up
        let stability_factor = ((1.0 - link_stability) * 10.0) as u32;

        self.current_redundancy_pct =
            (self.base_redundancy_pct + loss_factor + stability_factor).min(50);
    }
}
```

---

## Measurement & Profiling

### Metrics to Track

```rust
pub struct NetworkMetrics {
    pub rtt_min_ms: u64,
    pub rtt_max_ms: u64,
    pub rtt_avg_ms: u64,
    pub rtt_std_dev: f32,

    pub packets_sent: u64,
    pub packets_lost: u64,
    pub loss_rate_pct: f32,

    pub bitrate_kbps_sent: u32,
    pub bitrate_kbps_capacity: u32,

    pub fec_overhead_pct: u32,
    pub fec_recovery_rate: f32,

    pub delta_state: String,  // "STABLE", "RISING", "CONGESTED"
    pub congestion_events: u32,
}
```

### Profiling Environment

Create synthetic test scenarios:
```bash
# High-latency (satellite), stable
tc qdisc add dev eth0 root netem delay 250ms

# Intercontinental, variable
tc qdisc add dev eth0 root netem delay 100ms ±20ms

# Local, with loss
tc qdisc add dev eth0 root netem delay 5ms loss 0.5%

# Collect metrics for 5+ minutes per scenario
```

---

## Implementation Roadmap

### Week 1: Analysis & Measurement
- [ ] Implement `LatencyProfile` detection in handshake
- [ ] Add comprehensive metrics collection
- [ ] Profile on synthetic high-latency networks
- [ ] Document baseline behavior

### Week 2: DELTA Tuning
- [ ] Implement hybrid congestion detection (delay + loss)
- [ ] Tune RTT thresholds for different link types
- [ ] Implement adaptive bitrate adjustment curves
- [ ] Test on real intercontinental connections

### Week 3: FEC & QUIC Integration
- [ ] Dynamic FEC redundancy controller
- [ ] QUIC congestion window coordination
- [ ] Test interaction between DELTA and QUIC CC
- [ ] Measure CPU impact of tuning

### Week 4: Validation & Documentation
- [ ] Run longevity tests (24-hour streams)
- [ ] Validate on all target platforms
- [ ] Document tuning parameters for operators
- [ ] Create deployment guide

---

## Success Criteria

- **High-latency (250ms) bitrate**: ±10% variance (currently ±30%)
- **Congestion recovery time**: <5 seconds (currently 10-15s)
- **FEC efficiency**: 95%+ recovery on lossy links (currently 80%)
- **CPU overhead**: <1% increase for tuning logic
- **Compatibility**: No impact on low-latency networks

---

## Potential Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Over-aggressive tuning | Bitrate swings | Conservative curves by default, operator tuning |
| QUIC CC conflicts | Unpredictable behavior | Coordinate window growth, test thoroughly |
| Platform variability | Different results | Test on macOS, Linux, Windows in cloud |
| Backward compatibility | Breaking changes | Feature flag, gradual rollout |

---

## References

- [QUIC RFC 9000](https://datatracker.ietf.org/doc/html/rfc9000) - Connection Migration & Congestion Control
- [DELTA Spec](./DELTA_CC_SPEC.md) - Wavry's congestion control algorithm
- [RIFT Spec](./RIFT_SPEC_V1.md) - Protocol details

