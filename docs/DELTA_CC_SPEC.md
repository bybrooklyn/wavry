# DELTA Congestion Control Specification v1.1.0

**DELTA**: Differential Latency Estimation and Tuning Algorithm.

DELTA is a delay-based, trend-driven congestion control algorithm optimized for ultra-low-latency interactive streaming (RIFT). It prioritizes latency stability over raw throughput by reacting to queuing trends before packet loss occurs.

---

## 1. Core Principles

1.  **Delay as Primary Signal**: Use queuing delay trends ($\Delta D_q$) to detect congestion early.
2.  **Trend-Driven**: React to the *slope* of latency using damped persistence to ignore jitter.
3.  **Fast Decrease, Slow Recovery**: Multiplicative reduction on congestion, additive increase during stability.
4.  **Baseline Awareness**: Maintain an accurate minimum RTT baseline using a sliding window.

---

## 2. Observed Signals

| Signal | Description | Formula / Logic |
| :--- | :--- | :--- |
| $RTT_{sample}$ | Raw RTT from current packet exchange. | - |
| $RTT_{min}$ | Minimum RTT observed in a 10s sliding window. | $\min(RTT_{samples})$ |
| $RTT_{smooth}$ | EWMA filtered RTT to remove high-frequency jitter. | $(1-\alpha) \cdot RTT_{smooth} + \alpha \cdot RTT_{sample}$ |
| $D_q$ | **Queue Delay Estimate**: Current excess delay. | $RTT_{smooth} - RTT_{min}$ |
| $\Delta D_q$ | **Queue Delay Slope**: Trend of queuing delay. | $D_q - D_{q, prev}$ |

### 2.1 Baseline Management
The $RTT_{min}$ MUST be updated continuously. If the sliding window (10s) expires without a new minimum, the $RTT_{min}$ MAY be allowed to increase to the smallest value in the current window to account for path changes.

---

## 3. Control State Machine

### 3.1 States
- **STABLE**: Delay is flat or decreasing. Network is cleared.
- **RISING**: Delay is consistently increasing. Queues are filling.
- **CONGESTED**: Delay or slope has crossed critical safety thresholds.

### 3.2 State Transition & Persistence Rules
To prevent oscillating due to network jitter, transitions require **$k$ consecutive samples** of a trend.

| From | To | Trigger | Persistence ($k$) |
| :--- | :--- | :--- | :--- |
| Any | **CONGESTED** | $D_q > T_{limit}$ | 1 (Immediate) |
| **STABLE** | **RISING** | $\Delta D_q > \epsilon$ | 3 samples |
| **RISING** | **STABLE** | $\Delta D_q \le 0$ | 3 samples |
| **CONGESTED**| **STABLE** | $D_q < T_{threshold}$ AND $\Delta D_q \le 0$ | 5 samples |

---

## 4. Control Actions

### 4.1 Target Bitrate ($R$)
- **STABLE**: Additive increase. $R_{next} = R + Increase \cdot (1 - \frac{D_q}{T_{limit}})$.
- **RISING**: Hold. Maintain current bitrate to observe if the trend stabilizes.
- **CONGESTED**: Multiplicative decrease. $R_{next} = R \cdot \beta$ (e.g., $\beta = 0.85$).

### 4.2 Target FPS ($F$)
- If session stays in **CONGESTED** for $> 1.0s$, reduce $F_{target}$ by one step (e.g., 60 $\to$ 45 $\to$ 30).
- Recovery: Increase $F_{target}$ only after 5.0s of continuous **STABLE** state.

### 4.3 FEC Redundancy ($\rho$)
- **Trigger**: Only adjust if packet loss is observed while in the **CONGESTED** state.
- **Action**: Increase $\rho$ by 1.5x (up to a max of 50%) to mitigate tail-drops.
- **Recovery**: Gradually decay $\rho$ toward baseline during **STABLE** periods.

---

## 5. Pseudocode

```rust
// DELTA Control Loop Logic
fn process_sample(sample: RttSample) {
    // 1. Update Signals
    update_rtt_min_window(sample.rtt);
    rtt_smooth = alpha * sample.rtt + (1-alpha) * rtt_smooth;
    let d_q = rtt_smooth - rtt_min;
    let delta_q = d_q - last_d_q;

    // 2. Evaluate State with Persistence
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

---

## 6. Tunable Constants

- **$T_{limit}$**: Latency budget (e.g., 15ms). Safety ceiling for queuing delay.
- **$\epsilon$**: Slope noise floor. Prevents reacting to tiny RTT fluctuations.
- **$\alpha$**: EWMA weight (e.g., 0.125). Balanced between responsiveness and smoothing.
- **$k_{rising}$**: Persistence factor. Higher values make the algorithm "sturdier" against jitter but slower to react.
- **$\beta$**: Back-off factor. Determines the depth of the bitrate cut on congestion.
