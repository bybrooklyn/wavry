//! Encoder pooling and GPU memory management for long streaming sessions.
//!
//! This module provides strategies for managing encoder lifecycle and GPU memory
//! across codec changes, quality adaptations, and extended streaming sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Configuration for encoder pooling.
#[derive(Debug, Clone)]
pub struct EncoderPoolConfig {
    /// Maximum encoders per configuration (for adaptive bitrate switching)
    pub max_encoders_per_config: usize,

    /// Enable statistics tracking
    pub track_stats: bool,
}

impl Default for EncoderPoolConfig {
    fn default() -> Self {
        Self {
            max_encoders_per_config: 2,
            track_stats: true,
        }
    }
}

/// Statistics about encoder pool usage.
#[derive(Debug, Clone, Default)]
pub struct EncoderPoolStats {
    pub total_created: u64,
    pub reuses: u64,
    pub active_encoders: usize,
    pub idle_encoders: usize,
}

/// Represents a single encoder configuration that can be pooled.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EncoderConfig {
    pub codec: u32,  // Codec type (H.264, HEVC, AV1)
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: u32,
    pub fps: u32,
}

/// Wrapper around an encoder with lifecycle tracking.
#[derive(Debug, Clone)]
pub struct PooledEncoder {
    pub id: u64,
    pub config: EncoderConfig,
    pub frames_encoded: u64,
    pub is_healthy: bool,
}

impl PooledEncoder {
    pub fn new(id: u64, config: EncoderConfig) -> Self {
        Self {
            id,
            config,
            frames_encoded: 0,
            is_healthy: true,
        }
    }

    /// Mark encoder as unhealthy if it produces corrupted frames or errors.
    pub fn mark_unhealthy(&mut self) {
        self.is_healthy = false;
    }

    /// Check if encoder should be reconfigured before reuse.
    pub fn needs_reconfiguration(&self, new_config: &EncoderConfig) -> bool {
        &self.config != new_config
    }
}

/// Reference frame tracking for managed reference frame release.
#[derive(Debug, Clone)]
pub struct ReferenceFrame {
    pub frame_id: u64,
    pub is_keyframe: bool,
    pub size_mb: u32,
}

/// Manages reference frames held by the encoder to prevent unbounded growth.
#[derive(Debug)]
pub struct ReferenceFrameManager {
    pub references: Vec<ReferenceFrame>,
    pub max_references: usize,
}

impl ReferenceFrameManager {
    pub fn new(max_references: usize) -> Self {
        Self {
            references: Vec::new(),
            max_references,
        }
    }

    /// Update manager with new frame and manage old references.
    pub fn update(&mut self, new_frame_id: u64, is_keyframe: bool, size_mb: u32) {
        // New keyframe means B-frame references can be released
        if is_keyframe {
            self.references.retain(|rf| rf.is_keyframe);
        }

        // Remove oldest references if over limit
        while self.references.len() >= self.max_references {
            if !self.references.is_empty() {
                self.references.remove(0);
            }
        }

        self.references.push(ReferenceFrame {
            frame_id: new_frame_id,
            is_keyframe,
            size_mb,
        });
    }

    pub fn total_memory_mb(&self) -> u32 {
        self.references.iter().map(|rf| rf.size_mb).sum()
    }

    pub fn reference_count(&self) -> usize {
        self.references.len()
    }
}

/// Staging buffer for CPU-GPU data transfer with explicit lifecycle.
#[derive(Debug, Clone)]
pub struct StagingBuffer {
    pub id: u64,
    pub size_bytes: u32,
    pub last_used_frame_id: u64,
}

/// Manages staging buffers with reclaim timeout.
#[derive(Debug)]
pub struct StagingBufferPool {
    pub buffers: HashMap<u64, StagingBuffer>,
    pub max_total_bytes: u32,
    pub reclaim_timeout_frames: u64,
    pub current_frame_id: u64,
}

impl StagingBufferPool {
    pub fn new(max_total_bytes: u32, reclaim_timeout_frames: u64) -> Self {
        Self {
            buffers: HashMap::new(),
            max_total_bytes,
            reclaim_timeout_frames,
            current_frame_id: 0,
        }
    }

    /// Register a staging buffer for tracking.
    pub fn register(&mut self, size_bytes: u32) -> u64 {
        let id = self.buffers.len() as u64;
        self.buffers.insert(
            id,
            StagingBuffer {
                id,
                size_bytes,
                last_used_frame_id: self.current_frame_id,
            },
        );
        id
    }

    /// Mark a buffer as used.
    pub fn use_buffer(&mut self, id: u64) {
        if let Some(buf) = self.buffers.get_mut(&id) {
            buf.last_used_frame_id = self.current_frame_id;
        }
    }

    /// Advance frame counter and prune stale buffers.
    pub fn advance_frame(&mut self) {
        self.current_frame_id += 1;

        let timeout = self.reclaim_timeout_frames;
        let current = self.current_frame_id;

        // Mark old buffers for reclaim
        let to_remove: Vec<u64> = self
            .buffers
            .iter()
            .filter(|(_, buf)| current.saturating_sub(buf.last_used_frame_id) > timeout)
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            self.buffers.remove(&id);
        }
    }

    pub fn total_memory_bytes(&self) -> u32 {
        self.buffers.values().map(|b| b.size_bytes).sum()
    }

    pub fn memory_pressure(&self) -> MemoryPressure {
        let used = self.total_memory_bytes();

        if used > self.max_total_bytes {
            MemoryPressure::Critical
        } else if used > (self.max_total_bytes * 3 / 4) {
            MemoryPressure::High
        } else if used > (self.max_total_bytes / 2) {
            MemoryPressure::Medium
        } else {
            MemoryPressure::Low
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    Critical,  // >100% of max
    High,      // >75% of max
    Medium,    // >50% of max
    Low,       // <50% of max
}

/// Pool of reusable encoders for adaptive bitrate scenarios.
pub struct EncoderPool {
    config: EncoderPoolConfig,
    idle_encoders: HashMap<EncoderConfig, Vec<PooledEncoder>>,
    active_encoders: HashMap<u64, PooledEncoder>,
    next_encoder_id: u64,
    stats: Arc<Mutex<EncoderPoolStats>>,
}

impl EncoderPool {
    pub fn new(config: EncoderPoolConfig) -> Self {
        Self {
            config,
            idle_encoders: HashMap::new(),
            active_encoders: HashMap::new(),
            next_encoder_id: 0,
            stats: Arc::new(Mutex::new(EncoderPoolStats::default())),
        }
    }

    /// Acquire an encoder matching the configuration, or create a new one.
    pub fn acquire(&mut self, config: EncoderConfig) -> PooledEncoder {
        // Try to reuse existing encoder with same config
        if let Some(idle_list) = self.idle_encoders.get_mut(&config) {
            if let Some(encoder) = idle_list.pop() {
                if let Ok(mut stats) = self.stats.lock() {
                    stats.reuses += 1;
                    stats.active_encoders = self.active_encoders.len() + 1;
                    stats.idle_encoders = self.get_total_idle();
                }
                self.active_encoders.insert(encoder.id, encoder.clone());
                return encoder;
            }
        }

        // Create new if under limit
        let active_count = self.active_encoders.len();
        let idle_count = self.idle_encoders.get(&config).map(|v| v.len()).unwrap_or(0);

        if active_count + idle_count < self.config.max_encoders_per_config {
            let encoder = PooledEncoder::new(self.next_encoder_id, config);
            self.next_encoder_id += 1;

            if let Ok(mut stats) = self.stats.lock() {
                stats.total_created += 1;
                stats.active_encoders = self.active_encoders.len() + 1;
            }

            self.active_encoders.insert(encoder.id, encoder.clone());
            return encoder;
        }

        // Fallback: return dummy encoder (should not happen in production)
        let encoder = PooledEncoder::new(self.next_encoder_id, config);
        self.next_encoder_id += 1;
        encoder
    }

    /// Release an encoder back to the pool for reuse.
    pub fn release(&mut self, encoder: PooledEncoder) {
        // Don't pool unhealthy encoders
        if !encoder.is_healthy {
            self.active_encoders.remove(&encoder.id);
            // Drop encoder
            if let Ok(mut stats) = self.stats.lock() {
                stats.active_encoders = self.active_encoders.len();
            }
            return;
        }

        // Return to idle pool if there's space
        self.active_encoders.remove(&encoder.id);

        let config = encoder.config.clone();
        let idle_list = self.idle_encoders.entry(config).or_default();

        if idle_list.len() < self.config.max_encoders_per_config {
            idle_list.push(encoder);
        } else {
            // Drop excess encoders
        }

        if let Ok(mut stats) = self.stats.lock() {
            stats.active_encoders = self.active_encoders.len();
            stats.idle_encoders = self.get_total_idle();
        }
    }

    fn get_total_idle(&self) -> usize {
        self.idle_encoders.values().map(|v| v.len()).sum()
    }

    pub fn stats(&self) -> EncoderPoolStats {
        self.stats
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn active_count(&self) -> usize {
        self.active_encoders.len()
    }

    pub fn idle_count(&self) -> usize {
        self.get_total_idle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_config_equality() {
        let config1 = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };
        let config2 = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };
        assert_eq!(config1, config2);
    }

    #[test]
    fn test_pooled_encoder_creation() {
        let config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };
        let encoder = PooledEncoder::new(1, config.clone());

        assert_eq!(encoder.id, 1);
        assert_eq!(encoder.config, config);
        assert_eq!(encoder.frames_encoded, 0);
        assert!(encoder.is_healthy);
    }

    #[test]
    fn test_pooled_encoder_health() {
        let config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };
        let mut encoder = PooledEncoder::new(1, config);

        assert!(encoder.is_healthy);
        encoder.mark_unhealthy();
        assert!(!encoder.is_healthy);
    }

    #[test]
    fn test_reference_frame_manager_keyframe() {
        let mut manager = ReferenceFrameManager::new(3);

        manager.update(0, true, 10);   // Keyframe
        manager.update(1, false, 5);   // B-frame reference
        manager.update(2, false, 5);   // B-frame reference

        assert_eq!(manager.reference_count(), 3);

        // New keyframe should keep only keyframes
        manager.update(3, true, 10);
        assert!(manager.references.iter().all(|rf| rf.is_keyframe || rf.frame_id == 3));
    }

    #[test]
    fn test_reference_frame_manager_limit() {
        let mut manager = ReferenceFrameManager::new(2);

        manager.update(0, true, 10);
        manager.update(1, true, 10);
        manager.update(2, true, 10);

        assert_eq!(manager.reference_count(), 2);
    }

    #[test]
    fn test_reference_frame_total_memory() {
        let mut manager = ReferenceFrameManager::new(5);

        manager.update(0, true, 10);
        manager.update(1, false, 5);
        manager.update(2, false, 5);

        assert_eq!(manager.total_memory_mb(), 20);
    }

    #[test]
    fn test_staging_buffer_pool_register() {
        let mut pool = StagingBufferPool::new(1000, 100);

        let _id1 = pool.register(100);
        let _id2 = pool.register(200);

        assert_eq!(pool.buffers.len(), 2);
        assert_eq!(pool.total_memory_bytes(), 300);
    }

    #[test]
    fn test_staging_buffer_pool_memory_pressure() {
        let mut pool = StagingBufferPool::new(1000, 100);

        pool.register(200);
        assert_eq!(pool.memory_pressure(), MemoryPressure::Low);

        pool.register(400);
        assert_eq!(pool.memory_pressure(), MemoryPressure::Medium);

        pool.register(400);
        assert_eq!(pool.memory_pressure(), MemoryPressure::High);
    }

    #[test]
    fn test_staging_buffer_pool_prune() {
        let mut pool = StagingBufferPool::new(1000, 10);

        let id1 = pool.register(100);
        pool.use_buffer(id1);
        // id1 is used above, just not afterwards

        pool.current_frame_id = 20;  // Advance beyond timeout
        pool.advance_frame();

        assert!(pool.buffers.is_empty());
    }

    #[test]
    fn test_encoder_pool_acquire() {
        let config = EncoderPoolConfig {
            max_encoders_per_config: 2,
            track_stats: true,
        };
        let mut pool = EncoderPool::new(config);

        let encoder_config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };

        let _encoder = pool.acquire(encoder_config);
        assert_eq!(pool.active_count(), 1);
        assert_eq!(pool.idle_count(), 0);
    }

    #[test]
    fn test_encoder_pool_reuse() {
        let config = EncoderPoolConfig {
            max_encoders_per_config: 2,
            track_stats: true,
        };
        let mut pool = EncoderPool::new(config);

        let encoder_config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };

        let encoder1 = pool.acquire(encoder_config.clone());
        let id1 = encoder1.id;
        pool.release(encoder1);

        let encoder2 = pool.acquire(encoder_config);
        let id2 = encoder2.id;

        // Should be reused (same ID)
        assert_eq!(id1, id2);
        assert_eq!(pool.idle_count(), 0);
    }

    #[test]
    fn test_encoder_pool_max_size() {
        let config = EncoderPoolConfig {
            max_encoders_per_config: 2,
            track_stats: true,
        };
        let mut pool = EncoderPool::new(config);

        let encoder_config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };

        let enc1 = pool.acquire(encoder_config.clone());
        let enc2 = pool.acquire(encoder_config.clone());
        let enc3 = pool.acquire(encoder_config.clone());

        // Third encoder should not be pooled (only 2 max)
        pool.release(enc1);
        pool.release(enc2);
        pool.release(enc3);

        assert_eq!(pool.idle_count(), 2);
    }

    #[test]
    fn test_encoder_pool_healthy_only() {
        let config = EncoderPoolConfig {
            max_encoders_per_config: 2,
            track_stats: true,
        };
        let mut pool = EncoderPool::new(config);

        let encoder_config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };

        let mut encoder = pool.acquire(encoder_config.clone());
        encoder.mark_unhealthy();
        pool.release(encoder);

        // Unhealthy encoder should not be pooled
        assert_eq!(pool.idle_count(), 0);
    }

    #[test]
    fn test_encoder_pool_stats() {
        let config = EncoderPoolConfig {
            max_encoders_per_config: 2,
            track_stats: true,
        };
        let mut pool = EncoderPool::new(config);

        let encoder_config = EncoderConfig {
            codec: 0,
            width: 1920,
            height: 1080,
            bitrate_kbps: 5000,
            fps: 60,
        };

        let encoder1 = pool.acquire(encoder_config.clone());
        pool.release(encoder1);

        let encoder2 = pool.acquire(encoder_config);
        pool.release(encoder2);

        let stats = pool.stats();
        assert_eq!(stats.total_created, 1);
        assert_eq!(stats.reuses, 1);
    }
}
