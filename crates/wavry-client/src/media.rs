use std::collections::{HashMap, VecDeque, BTreeSet};
use rift_core::{VideoChunk, FecPacket};
use crate::helpers::now_us;
use tracing::debug;

pub const FRAME_TIMEOUT_US: u64 = 50_000;
pub const MAX_FEC_CACHE: usize = 256;
pub const JITTER_GROW_THRESHOLD_US: f64 = 2_000.0;
pub const JITTER_SHRINK_THRESHOLD_US: f64 = 500.0;
pub const JITTER_MAX_BUFFER_US: u64 = 10_000;
pub const NACK_WINDOW_SIZE: u64 = 128;

pub struct FrameAssembler {
    timeout_us: u64,
    frames: HashMap<u64, FrameBuffer>,
}

pub struct FrameBuffer {
    pub first_seen_us: u64,
    pub timestamp_us: u64,
    #[allow(dead_code)]
    pub keyframe: bool,
    pub chunk_count: u32,
    pub chunks: Vec<Option<Vec<u8>>>,
}

pub struct AssembledFrame {
    pub frame_id: u64,
    pub timestamp_us: u64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

impl FrameAssembler {
    pub fn new(timeout_us: u64) -> Self {
        Self {
            timeout_us,
            frames: HashMap::new(),
        }
    }

    pub fn push(&mut self, chunk: VideoChunk) -> Option<AssembledFrame> {
        let now = now_us();
        self.frames
            .retain(|_, frame| now.saturating_sub(frame.first_seen_us) < self.timeout_us);

        let entry = self
            .frames
            .entry(chunk.frame_id)
            .or_insert_with(|| FrameBuffer {
                first_seen_us: now,
                timestamp_us: chunk.timestamp_us,
                keyframe: chunk.keyframe,
                chunk_count: chunk.chunk_count,
                chunks: vec![None; chunk.chunk_count as usize],
            });

        if chunk.chunk_index < entry.chunk_count {
            entry.chunks[chunk.chunk_index as usize] = Some(chunk.payload);
        }

        if entry.chunks.iter().all(|c| c.is_some()) {
            let mut assembled = Vec::new();
            for part in entry.chunks.iter_mut() {
                if let Some(bytes) = part.take() {
                    assembled.extend_from_slice(&bytes);
                }
            }
            let timestamp_us = entry.timestamp_us;
            let keyframe = entry.keyframe;
            let frame_id = chunk.frame_id;
            self.frames.remove(&chunk.frame_id);
            return Some(AssembledFrame {
                frame_id,
                timestamp_us,
                keyframe,
                data: assembled,
            });
        }
        None
    }
}

pub struct FecCache {
    packets: HashMap<u64, Vec<u8>>,
}

impl FecCache {
    pub fn new() -> Self {
        Self {
            packets: HashMap::new(),
        }
    }

    pub fn insert(&mut self, packet_id: u64, data: Vec<u8>) {
        if self.packets.len() >= MAX_FEC_CACHE {
            if let Some(min_id) = self.packets.keys().min().copied() {
                self.packets.remove(&min_id);
            }
        }
        self.packets.insert(packet_id, data);
    }

    pub fn try_recover(&self, fec: &FecPacket) -> Option<Vec<u8>> {
        let mut missing_id = None;
        let mut recovered_payload = fec.payload.clone();
        let mut present_count = 0;

        for offset in 0..(fec.shard_count - 1) {
            let pid = fec.first_packet_id + offset as u64;
            if let Some(p) = self.packets.get(&pid) {
                // XOR in the present packets
                for (i, b) in p.iter().enumerate() {
                    if i < recovered_payload.len() {
                        recovered_payload[i] ^= b;
                    }
                }
                present_count += 1;
            } else {
                if missing_id.is_some() {
                    // More than one missing, can't recover
                    return None;
                }
                missing_id = Some(pid);
            }
        }

        if present_count == (fec.shard_count - 2) {
            // Exactly one missing, we've XORed everything else into the parity
            if let Some(id) = missing_id {
                debug!("FEC: Recovered packet {}", id);
            }
            Some(recovered_payload)
        } else {
            None
        }
    }
}

pub struct ArrivalJitter {
    last_arrival_us: Option<u64>,
    ia_avg_us: f64,
    jitter_us: f64,
}

impl ArrivalJitter {
    pub fn new() -> Self {
        Self {
            last_arrival_us: None,
            ia_avg_us: 0.0,
            jitter_us: 0.0,
        }
    }

    pub fn on_arrival(&mut self, arrival_us: u64) {
        if let Some(last) = self.last_arrival_us {
            let ia = arrival_us.saturating_sub(last) as f64;
            if self.ia_avg_us == 0.0 {
                self.ia_avg_us = ia;
            } else {
                self.ia_avg_us += (ia - self.ia_avg_us) / 16.0;
            }
            let deviation = (ia - self.ia_avg_us).abs();
            self.jitter_us += (deviation - self.jitter_us) / 16.0;
        }
        self.last_arrival_us = Some(arrival_us);
    }

    pub fn jitter_us(&self) -> u32 {
        self.jitter_us.max(0.0) as u32
    }

    pub fn jitter_us_f64(&self) -> f64 {
        self.jitter_us.max(0.0)
    }
}

pub struct RttTracker {
    smooth_us: f64,
}

impl RttTracker {
    pub fn new() -> Self {
        Self { smooth_us: 0.0 }
    }

    pub fn on_sample(&mut self, rtt_us: u64) -> f64 {
        if self.smooth_us == 0.0 {
            self.smooth_us = rtt_us as f64;
        } else {
            self.smooth_us = 0.875 * self.smooth_us + 0.125 * (rtt_us as f64);
        }
        self.smooth_us
    }
}

pub struct NackWindow {
    window: u64,
    highest: Option<u64>,
    received: BTreeSet<u64>,
    missing: BTreeSet<u64>,
}

impl NackWindow {
    pub fn new(window: u64) -> Self {
        Self {
            window,
            highest: None,
            received: BTreeSet::new(),
            missing: BTreeSet::new(),
        }
    }

    pub fn on_packet(&mut self, packet_id: u64) -> Vec<u64> {
        let mut newly_missing = Vec::new();
        if let Some(highest) = self.highest {
            if packet_id > highest + 1 {
                let gap_start = highest + 1;
                let gap_end = packet_id - 1;
                let min_start = packet_id.saturating_sub(self.window).max(gap_start);
                for id in min_start..=gap_end {
                    if !self.received.contains(&id) && !self.missing.contains(&id) {
                        self.missing.insert(id);
                        newly_missing.push(id);
                    }
                }
                self.highest = Some(packet_id);
            } else if packet_id > highest {
                self.highest = Some(packet_id);
            }
        } else {
            self.highest = Some(packet_id);
        }

        self.received.insert(packet_id);
        self.missing.remove(&packet_id);
        self.evict_old();
        newly_missing
    }

    fn evict_old(&mut self) {
        if let Some(highest) = self.highest {
            let cutoff = highest.saturating_sub(self.window);
            self.received = self.received.split_off(&cutoff);
            self.missing = self.missing.split_off(&cutoff);
        }
    }
}

pub struct JitterBuffer {
    target_delay_us: u64,
    queue: VecDeque<BufferedFrame>,
}

pub struct BufferedFrame {
    pub arrival_us: u64,
    pub frame: AssembledFrame,
}

impl JitterBuffer {
    pub fn new() -> Self {
        Self {
            target_delay_us: 0,
            queue: VecDeque::new(),
        }
    }

    pub fn update(&mut self, jitter_us: f64) {
        if jitter_us > JITTER_GROW_THRESHOLD_US {
            self.target_delay_us = (self.target_delay_us + 1_000).min(JITTER_MAX_BUFFER_US);
        } else if jitter_us < JITTER_SHRINK_THRESHOLD_US {
            self.target_delay_us = self.target_delay_us.saturating_sub(500);
        }
    }

    pub fn push(&mut self, frame: AssembledFrame, arrival_us: u64) {
        self.queue.push_back(BufferedFrame { arrival_us, frame });
    }

    pub fn pop_ready(&mut self, now_us: u64) -> Option<AssembledFrame> {
        if let Some(front) = self.queue.front() {
            if now_us.saturating_sub(front.arrival_us) >= self.target_delay_us {
                return self.queue.pop_front().map(|f| f.frame);
            }
        }
        None
    }
}
