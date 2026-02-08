# DELTA Congestion Control Specification v1.1.0

**DELTA**: Differential Latency Estimation and Tuning Algorithm  
**Status:** Implemented  
**Last Updated:** 2026-02-07

DELTA is a delay-based, trend-driven congestion control algorithm optimized for ultra-low-latency interactive streaming (RIFT). It prioritizes latency stability over raw throughput by reacting to queuing trends before packet loss occurs.

---

## Table of Contents

1. [Core Principles](#1-core-principles)
2. [Observed Signals](#2-observed-signals)
3. [Control State Machine](#3-control-state-machine)
4. [Control Actions](#4-control-actions)
5. [Implementation](#5-implementation)
6. [Tunable Constants](#6-tunable-constants)
7. [Related Documents](#7-related-documents)

---

## 1. Core Principles

1. **Delay as Primary Signal**: Use queuing delay trends ($\Delta D_q$) to detect congestion early
2. **Trend-Driven**: React to the *slope* of latency using damped persistence to ignore jitter
3. **Fast Decrease, Slow Recovery**: Multiplicative reduction on congestion, additive increase during stability
4. **Baseline Awareness**: Maintain an accurate minimum RTT baseline using a sliding window

---

## 2. Observed Signals

| Signal | Description | Formula |
|:-------|:------------|:--------|
| $RTT_{sample}$ | Raw RTT from current packet exchange | — |
| $RTT_{min}$ | Minimum RTT observed in a 10s sliding window | $\min(RTT_{samples})$ |
| $RTT_{smooth}$ | EWMA filtered RTT to remove high-frequency jitter | $(1-\alpha) \cdot RTT_{smooth} + \alpha \cdot RTT_{sample}$ |
| $D_q$ | **Queue Delay Estimate**: Current excess delay | $RTT_{smooth} - RTT_{min}$ |
| $\Delta D_q$ | **Queue Delay Slope**: Trend of queuing delay | $D_q - D_{q, prev}$ |

### 2.1 Baseline Management

The $RTT_{min}$ MUST be updated continuously. If the sliding window (10s) expires without a new minimum, the $RTT_{min}$ MAY be allowed to increase to the smallest value in the current window to account for path changes.

---

## 3. Control State Machine

### 3.1 States

| State | Description | Entry Condition |
|:------|:------------|:----------------|
| **STABLE** | Delay is flat or decreasing. Network is cleared. | $\Delta D_q \le 0$ for $k$ samples |
| **RISING** | Delay is consistently increasing. Queues are filling. | $\Delta D_q > \epsilon$ for $k$ samples |
| **CONGESTED** | Delay or slope has crossed critical safety thresholds. | $D_q > T_{limit}$ |

### 3.2 State Transition & Persistence Rules

To prevent oscillating due to network jitter, transitions require **$k$ consecutive samples** of a trend.

| From | To | Trigger | Persistence ($k$) |
|:-----|:---|:--------|:------------------|
| Any | **CONGESTED** | $D_q > T_{limit}$ | 1 (Immediate) |
| **STABLE** | **RISING** | $\Delta D_q > \epsilon$ | 3 samples |
| **RISING** | **STABLE** | $\Delta D_q \le 0$ | 3 samples |
| **CONGESTED** | **STABLE** | $D_q < T_{threshold}$ AND $\Delta D_q \le 0$ | 5 samples |

**State Transition Diagram:**

```
                    ┌─────────────┐
        ┌──────────►│   STABLE    │◄─────────┐
        │           │             │          │
   ΔDq ≤ 0 (3x)     └─────────────┘     ΔDq > ε (3x)
        │                                    │
        │           ┌─────────────┐          │
        └──────────►│   RISING    │──────────┘
                    └─────────────┘
                           │
                    Dq > Tlimit
                           ▼
                    ┌─────────────┐
                    │ CONGESTED   │
                    └─────────────┘
```

---

## 4. Control Actions

### 4.1 Target Bitrate ($R$)

| State | Action | Formula |
|:------|:-------|:--------|
| **STABLE** | Additive increase | $R_{next} = R + Increase \cdot (1 - \frac{D_q}{T_{limit}})$ |
| **RISING** | Hold | Maintain current bitrate to observe if the trend stabilizes |
| **CONGESTED** | Multiplicative decrease | $R_{next} = R \cdot \beta$ (e.g., $\beta = 0.85$) |

### 4.2 Target FPS ($F$)

- If session stays in **CONGESTED** for $> 1.0s$, reduce $F_{target}$ by one step (e.g., 60 $\to$ 45 $\to$ 30)
- Recovery: Increase $F_{target}$ only after 5.0s of continuous **STABLE** state

### 4.3 FEC Redundancy ($\rho$)

- **Trigger**: Only adjust if packet loss is observed while in the **CONGESTED** state
- **Action**: Increase $\rho$ by 1.5x (up to a max of 50%) to mitigate tail-drops
- **Recovery**: Gradually decay $\rho$ toward baseline during **STABLE** periods

---

## 5. Implementation

### 5.1 Pseudocode

```rust
// DELTA Control Loop Logic
fn process_sample(sample: RttSample) {
    // 1. Update Signals
    update_rtt_min_window(sample.rtt);
    rtt_smooth = alpha * sample.rtt + (1-alpha) * rtt_smooth;
    let d_q = rtt_smooth - rtt_min;
    let delta_q = d_q - last_d_q;

    // 2. Evaluate State with Persistence
    let epsilon = rtt_smooth * 0.05;
    if d_q > T_LIMIT {
        set_state(CONGESTED);
        congested_start_time = now();
    } else if delta_q > EPSILON {
        rising_count += 1;
        if rising_count >= K_RISING { set_state(RISING); }
    } else if delta_q <= 0 {
        stable_count += 1;
        if stable_count >= K_STABLE { set_state(STABLE); }
    }

    // 3. Apply Actions
    match state {
        CONGESTED => {
            target_bitrate *= BETA;
            if now() - congested_start_time > 1s {
                target_fps = step_down_fps(target_fps);
            }
            if sample.packet_loss > 0 {
                fec_ratio = min(MAX_FEC, fec_ratio * 1.5);
            }
        }
        RISING => {
            // Hold and wait for signal to flip or breach limit
        }
        STABLE => {
            let gain = (1.0 - d_q / T_LIMIT).max(0.0);
            target_bitrate += (ADDITIVE_STEP * gain);
            decay_fec_ratio();
        }
    }
}
```

### 5.2 Integration with RIFT

DELTA runs in the `rift-core` crate and operates on RTT samples collected from:
- Ping/Pong control messages
- Acknowledgment of reliable control packets
- RTCP-like feedback from media receiver

The controller outputs:
- `target_bitrate`: Fed to the encoder rate control
- `target_fps`: Used to adjust frame pacing
- `fec_ratio`: Passed to the FEC encoder

---

## 6. Tunable Constants

| Constant | Symbol | Default | Description |
|:---------|:-------|:--------|:------------|
| Latency Budget | $T_{limit}$ | 15 ms | Safety ceiling for queuing delay |
| Slope Noise Floor | $\epsilon$ | $RTT_{smooth} \cdot 0.05$ | Scales with smoothed RTT to adapt across LAN/WAN |
| EWMA Weight | $\alpha$ | 0.125 | Balances responsiveness vs. smoothing |
| Rising Persistence | $k_{rising}$ | 3 samples | Makes algorithm "sturdier" against jitter |
| Back-off Factor | $\beta$ | 0.85 | Determines depth of bitrate cut on congestion |
| Additive Step | — | 50 kbps | Bitrate increase per stable interval |
| Max FEC Ratio | — | 0.50 | Maximum redundancy (50%) |

---

## 7. Related Documents

- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) - RIFT protocol specification
- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - System architecture overview
- [WAVRY_TESTING.md](WAVRY_TESTING.md) - Testing and validation runbooks
