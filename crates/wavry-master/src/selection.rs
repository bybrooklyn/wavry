//! Relay Selection Logic
//!
//! Implements the Weighted Random Selection algorithm defined in `docs/WAVRY_RELAY_SELECTION.md`.
//!
//! # Algorithm Overview
//!
//! 1. **Scoring**: Each candidate is assigned a score (0-100) based on metrics (success rate,
//!    latency, feedback, etc.) and its current state (ACTIVE, PROBATION, etc.).
//! 2. **Filtering**: Candidates with 0 score (or invalid states) are filtered out.
//! 3. **Weighting**: Scores are shifted to ensure positive weights: `weight = score - min_score + 10`.
//!    PROBATION relays get a 20% exploration bonus.
//! 4. **Selection**: A weighted random choice is made to select the final relay.
//!
//! This ensures high-quality relays are preferred while maintaining diversity and allowing new
//! relays to prove themselves.

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RelayState {
    #[default]
    New,
    Probation,
    Active,
    Degraded,
    Draining,
    Quarantined,
    Banned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayMetrics {
    pub success_rate: f32,           // 0.0 - 1.0
    pub handshake_timeout_rate: f32, // 0.0 - 1.0
    pub avg_duration_score: f32,     // 0.0 - 1.0
    pub feedback_score: f32,         // 0.0 - 100.0
    pub probe_rtt_score: f32,        // 0.0 - 100.0
    pub probe_loss_score: f32,       // 0.0 - 1.0
    pub capacity_score: f32,         // 0.0 - 1.0
}

impl Default for RelayMetrics {
    fn default() -> Self {
        Self {
            success_rate: 1.0,
            handshake_timeout_rate: 0.0,
            avg_duration_score: 1.0,
            feedback_score: 50.0,
            probe_rtt_score: 100.0,
            probe_loss_score: 1.0,
            capacity_score: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelayCandidate {
    pub _id: String,
    pub endpoints: Vec<String>,
    pub state: RelayState,
    pub metrics: RelayMetrics,
    pub region: Option<String>,
    pub asn: Option<u32>,
    pub load_pct: f32,
    pub last_seen: SystemTime,
}

pub fn calculate_relay_score(relay: &RelayCandidate) -> f32 {
    let m = &relay.metrics;

    // Weighted combination per spec
    let success_score = m.success_rate * 100.0;
    let handshake_score = (1.0 - m.handshake_timeout_rate) * 100.0;
    let duration_score = m.avg_duration_score * 100.0;
    let feedback_score = m.feedback_score;
    let rtt_score = m.probe_rtt_score;
    let loss_score = m.probe_loss_score * 100.0;

    // Blend live load and probe-based capacity score.
    let load_capacity = (1.0 - (relay.load_pct / 100.0).clamp(0.0, 1.0)) * 100.0;
    let metric_capacity = (m.capacity_score.clamp(0.0, 1.0)) * 100.0;
    let capacity_score = load_capacity * 0.7 + metric_capacity * 0.3;

    let mut raw_score = success_score * 0.25
        + handshake_score * 0.15
        + duration_score * 0.10
        + feedback_score * 0.20
        + rtt_score * 0.15
        + loss_score * 0.10
        + capacity_score * 0.05;

    // Penalize stale relays as heartbeat freshness decays.
    if let Ok(age) = SystemTime::now().duration_since(relay.last_seen) {
        let freshness_multiplier = match age.as_secs() {
            0..=30 => 1.0,
            31..=90 => 0.9,
            91..=180 => 0.65,
            181..=300 => 0.4,
            _ => 0.2,
        };
        raw_score *= freshness_multiplier;
    }

    let state_multiplier = match relay.state {
        RelayState::New => 0.25,
        RelayState::Probation => 0.65,
        RelayState::Active => 1.0,
        RelayState::Degraded => 0.4,
        RelayState::Draining => 0.0,
        RelayState::Quarantined => 0.0,
        RelayState::Banned => 0.0,
    };

    raw_score * state_multiplier
}

pub fn select_relay(candidates: &[RelayCandidate]) -> Option<&RelayCandidate> {
    if candidates.is_empty() {
        return None;
    }

    let scored_candidates: Vec<(&RelayCandidate, f32)> = candidates
        .iter()
        .map(|r| (r, calculate_relay_score(r)))
        .filter(|(_, score)| *score > 0.0)
        .collect();

    if scored_candidates.is_empty() {
        // Fallback: pick one with highest availability if all scores are 0 (e.g. all PROBATION/NEW)
        // or just pick any ACTIVE/PROBATION/DEGRADED even if score is low.
        return candidates.iter().find(|r| {
            matches!(
                r.state,
                RelayState::Active | RelayState::Probation | RelayState::Degraded
            )
        });
    }

    let min_score = scored_candidates
        .iter()
        .map(|(_, s)| *s)
        .fold(f32::INFINITY, f32::min);

    // Shift weights: score - min_score + 10
    let weights: Vec<f32> = scored_candidates
        .iter()
        .map(|(r, s)| {
            let mut w = s - min_score + 10.0;
            if r.state == RelayState::Probation {
                w *= 1.2; // Exploration bonus
            }
            w
        })
        .collect();

    let total_weight: f32 = weights.iter().sum();
    if total_weight <= 0.0 {
        return Some(scored_candidates.first().unwrap().0);
    }

    let mut rng = rand::thread_rng();
    let r = rng.gen::<f32>() * total_weight;

    let mut cumulative = 0.0;
    for (i, weight) in weights.iter().enumerate() {
        cumulative += weight;
        if r <= cumulative {
            return Some(scored_candidates[i].0);
        }
    }

    Some(scored_candidates.last().unwrap().0)
}

/// Simple heuristic for distance between two regions.
fn region_distance(r1: &str, r2: &str) -> u32 {
    if r1 == r2 {
        return 0;
    }
    // Same continent (first part of string) = 1, different = 5
    let p1 = r1.split('-').next().unwrap_or("");
    let p2 = r2.split('-').next().unwrap_or("");
    if p1 == p2 {
        1
    } else {
        5
    }
}

/// Filter and sort candidates by geographic proximity to both peers.
/// Also ensures ASN diversity (max 2 relays per ASN).
pub fn filter_by_geography(
    candidates: Vec<RelayCandidate>,
    client_region: Option<&str>,
    server_region: Option<&str>,
    max_candidates: usize,
) -> Vec<RelayCandidate> {
    if client_region.is_none() && server_region.is_none() {
        return candidates;
    }

    let mut sorted = candidates;
    sorted.sort_by_key(|r| {
        let r_region = r.region.as_deref().unwrap_or("unknown");
        let d1 = client_region
            .map(|cr| region_distance(r_region, cr))
            .unwrap_or(2);
        let d2 = server_region
            .map(|sr| region_distance(r_region, sr))
            .unwrap_or(2);
        d1 + d2
    });

    let mut result = Vec::new();
    let mut seen_asns = HashMap::new();

    for r in sorted {
        if result.len() >= max_candidates {
            break;
        }

        let asn = r.asn.unwrap_or(0);
        let count = seen_asns.entry(asn).or_insert(0);
        if *count < 2 {
            result.push(r);
            *count += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]

    fn test_score_active_perfect() {
        let r = RelayCandidate {
            _id: "test".into(),

            endpoints: vec![],

            state: RelayState::Active,

            metrics: RelayMetrics::default(), // All perfect

            region: None,

            asn: None,

            load_pct: 0.0,

            last_seen: SystemTime::now(),
        };

        // Expect score ~100

        // success=100*0.25=25

        // handshake=100*0.15=15

        // duration=100*0.10=10

        // feedback=50*0.20=10 (default is 50)

        // rtt=100*0.15=15

        // loss=100*0.10=10

        // capacity=100*0.05=5 (load_pct 0 -> 100)

        // Sum = 90.0

        assert_eq!(calculate_relay_score(&r), 90.0);
    }

    #[test]

    fn test_selection_distribution() {
        let r1 = RelayCandidate {
            _id: "r1".into(),

            state: RelayState::Active,

            metrics: RelayMetrics::default(), // Score 90

            endpoints: vec![],

            region: None,

            asn: None,

            load_pct: 0.0,

            last_seen: SystemTime::now(),
        };

        let mut r2 = r1.clone();

        r2._id = "r2".into();

        r2.state = RelayState::Degraded; // Multiplier 0.3 -> Score 27

        let candidates = vec![r1, r2];

        let mut r1_count = 0;

        for _ in 0..1000 {
            let selected = select_relay(&candidates).unwrap();

            if selected._id == "r1" {
                r1_count += 1;
            }
        }

        // r1 should be selected much more often

        assert!(r1_count > 800);
    }

    #[test]

    fn test_geo_filtering() {
        let r_us = RelayCandidate {
            _id: "us".into(),

            state: RelayState::Active,

            metrics: RelayMetrics::default(),

            region: Some("us-east-1".into()),

            asn: Some(100),

            endpoints: vec![],

            load_pct: 0.0,

            last_seen: SystemTime::now(),
        };

        let r_eu = RelayCandidate {
            _id: "eu".into(),

            state: RelayState::Active,

            metrics: RelayMetrics::default(),

            region: Some("eu-west-1".into()),

            asn: Some(200),

            endpoints: vec![],

            load_pct: 0.0,

            last_seen: SystemTime::now(),
        };

        let candidates = vec![r_us.clone(), r_eu.clone()];

        // Client in US should get US relay first

        let filtered = filter_by_geography(candidates.clone(), Some("us-west-2"), None, 10);

        assert_eq!(filtered[0]._id, "us");

        // Client in EU should get EU relay first

        let filtered = filter_by_geography(candidates.clone(), Some("eu-central-1"), None, 10);

        assert_eq!(filtered[0]._id, "eu");
    }

    #[test]
    fn test_draining_relay_is_never_selected() {
        let healthy = RelayCandidate {
            _id: "active".into(),
            endpoints: vec![],
            state: RelayState::Active,
            metrics: RelayMetrics::default(),
            region: None,
            asn: None,
            load_pct: 0.0,
            last_seen: SystemTime::now(),
        };
        let draining = RelayCandidate {
            _id: "drain".into(),
            endpoints: vec![],
            state: RelayState::Draining,
            metrics: RelayMetrics::default(),
            region: None,
            asn: None,
            load_pct: 0.0,
            last_seen: SystemTime::now(),
        };

        for _ in 0..100 {
            let pool = vec![healthy.clone(), draining.clone()];
            let selected = select_relay(&pool).expect("a relay should be selected");
            assert_eq!(selected._id, "active");
        }
    }
}
