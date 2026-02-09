//! Memory-efficient buffer pool management for frame capture and encoding pipelines.
//!
//! This module provides object pooling strategies to reduce allocator pressure and fragmentation
//! during continuous streaming sessions, particularly on high-latency networks where buffers
//! are held for extended periods.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Configuration for the frame buffer pool.
#[derive(Debug, Clone)]
pub struct FrameBufferPoolConfig {
    /// Maximum number of buffers to keep in the pool (e.g., 3 for triple-buffering)
    pub max_pool_size: usize,

    /// Size of each buffer in bytes (e.g., 1920 * 1080 * 4 for RGBA)
    pub buffer_size_bytes: usize,

    /// Enable statistics tracking (slight performance overhead)
    pub track_stats: bool,
}

impl Default for FrameBufferPoolConfig {
    fn default() -> Self {
        Self {
            max_pool_size: 3,
            buffer_size_bytes: 1920 * 1080 * 4,  // 1080p RGBA
            track_stats: true,
        }
    }
}

/// Statistics about buffer pool usage.
#[derive(Debug, Clone, Default)]
pub struct FrameBufferPoolStats {
    pub allocations: u64,
    pub reallocations: u64,
    pub reuses: u64,
    pub in_use_count: usize,
    pub available_count: usize,
}

/// A frame buffer that can be reused across multiple frame captures.
#[derive(Debug, Clone)]
pub struct FrameBuffer {
    id: u64,
    data: Vec<u8>,
    capacity: usize,
}

impl FrameBuffer {
    fn new(id: u64, capacity: usize) -> Self {
        let data = vec![0u8; capacity];
        Self { id, data, capacity }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Clear the buffer (set all bytes to 0) for reuse.
    pub fn reset(&mut self) {
        self.data.fill(0);
    }
}

/// Memory-efficient pool of reusable frame buffers.
pub struct FrameBufferPool {
    config: FrameBufferPoolConfig,
    available: VecDeque<FrameBuffer>,
    in_use: Vec<u64>,  // Track which buffer IDs are in use
    next_buffer_id: u64,
    stats: Arc<Mutex<FrameBufferPoolStats>>,
}

impl FrameBufferPool {
    pub fn new(config: FrameBufferPoolConfig) -> Self {
        Self {
            config,
            available: VecDeque::new(),
            in_use: Vec::new(),
            next_buffer_id: 0,
            stats: Arc::new(Mutex::new(FrameBufferPoolStats::default())),
        }
    }

    /// Acquire a buffer from the pool, or allocate a new one if pool is empty.
    pub fn acquire(&mut self) -> FrameBuffer {
        let buffer = if let Some(mut buf) = self.available.pop_front() {
            // Reuse existing buffer
            buf.reset();
            if let Ok(mut stats) = self.stats.lock() {
                stats.reuses += 1;
                stats.in_use_count += 1;
                stats.available_count = self.available.len();
            }
            buf
        } else {
            // Allocate new buffer if pool is exhausted
            let buf = FrameBuffer::new(self.next_buffer_id, self.config.buffer_size_bytes);
            self.next_buffer_id += 1;

            if let Ok(mut stats) = self.stats.lock() {
                stats.allocations += 1;
                stats.in_use_count += 1;
            }
            buf
        };

        self.in_use.push(buffer.id());
        buffer
    }

    /// Release a buffer back to the pool for reuse.
    pub fn release(&mut self, mut buffer: FrameBuffer) {
        // Remove from in-use tracking
        if let Some(pos) = self.in_use.iter().position(|&id| id == buffer.id()) {
            self.in_use.remove(pos);
        }

        // Return to pool if there's space, otherwise drop
        if self.available.len() < self.config.max_pool_size {
            buffer.reset();
            self.available.push_back(buffer);
        }
        // else: buffer is dropped and deallocated

        if let Ok(mut stats) = self.stats.lock() {
            stats.in_use_count = self.in_use.len();
            stats.available_count = self.available.len();
        }
    }

    /// Get current statistics about pool usage.
    pub fn stats(&self) -> FrameBufferPoolStats {
        self.stats
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Calculate total memory usage of the pool.
    pub fn memory_usage_bytes(&self) -> usize {
        let total_buffers = self.available.len() + self.in_use.len();
        total_buffers * self.config.buffer_size_bytes
    }

    /// Get the number of available buffers ready for reuse.
    pub fn available_count(&self) -> usize {
        self.available.len()
    }

    /// Get the number of buffers currently in use.
    pub fn in_use_count(&self) -> usize {
        self.in_use.len()
    }
}

/// Circular reorder buffer for managing out-of-order packet arrivals with bounded memory.
#[derive(Debug)]
pub struct ReorderBuffer {
    buffer: Vec<Option<Vec<u8>>>,
    max_seen: u32,
    window_start: u32,
    window_size: u32,
    buffer_size_bytes: usize,
}

impl ReorderBuffer {
    /// Create a new reorder buffer with capacity for `window_size` packets.
    ///
    /// # Arguments
    /// * `window_size` - Maximum number of packets to hold (e.g., 1000 packets)
    pub fn new(window_size: u32) -> Self {
        let capacity = window_size as usize;
        let buffer = vec![None; capacity];

        Self {
            buffer,
            max_seen: 0,
            window_start: 0,
            window_size,
            buffer_size_bytes: 0,
        }
    }

    /// Insert a packet into the buffer at its sequence number position.
    ///
    /// Returns `Err` if the packet is outside the sliding window (too old).
    pub fn insert(&mut self, packet_id: u32, data: Vec<u8>) -> Result<(), ReorderError> {
        // Reject packets outside the sliding window
        if packet_id < self.window_start {
            return Err(ReorderError::PacketTooOld);
        }

        // Slide window forward if packet is beyond our window
        if packet_id > self.window_start + self.window_size {
            let advance = packet_id - self.window_start - self.window_size;
            self.slide_window(advance);
        }

        // Calculate circular index
        let idx = (packet_id % self.window_size) as usize;

        // Track memory usage
        let data_len = data.len();
        self.buffer[idx] = Some(data);
        self.buffer_size_bytes += data_len;
        self.max_seen = self.max_seen.max(packet_id);

        Ok(())
    }

    /// Get and remove a packet from the buffer if it exists.
    pub fn get(&mut self, packet_id: u32) -> Option<Vec<u8>> {
        if packet_id < self.window_start || packet_id >= self.window_start + self.window_size {
            return None;
        }

        let idx = (packet_id % self.window_size) as usize;
        if let Some(data) = self.buffer[idx].take() {
            self.buffer_size_bytes = self.buffer_size_bytes.saturating_sub(data.len());
            Some(data)
        } else {
            None
        }
    }

    /// Flush all consecutive packets starting from window_start.
    pub fn flush_ordered(&mut self) -> Vec<Vec<u8>> {
        let mut result = Vec::new();

        loop {
            let idx = (self.window_start % self.window_size) as usize;
            if let Some(data) = self.buffer[idx].take() {
                self.buffer_size_bytes = self.buffer_size_bytes.saturating_sub(data.len());
                result.push(data);
                self.window_start += 1;
            } else {
                break;
            }
        }

        result
    }

    /// Slide the window forward by dropping old packets.
    fn slide_window(&mut self, count: u32) {
        for _ in 0..count.min(self.window_size) {
            let idx = (self.window_start % self.window_size) as usize;
            if let Some(data) = self.buffer[idx].take() {
                self.buffer_size_bytes = self.buffer_size_bytes.saturating_sub(data.len());
            }
            self.window_start += 1;
        }
    }

    /// Get the current memory usage of the buffer.
    pub fn memory_usage_bytes(&self) -> usize {
        self.buffer_size_bytes
    }

    /// Get the number of packets currently in the buffer.
    pub fn packet_count(&self) -> usize {
        self.buffer.iter().filter(|p| p.is_some()).count()
    }

    /// Get buffer occupancy ratio (current packets / window size).
    pub fn occupancy_ratio(&self) -> f32 {
        self.packet_count() as f32 / self.window_size as f32
    }

    pub fn window_start(&self) -> u32 {
        self.window_start
    }

    pub fn max_seen(&self) -> u32 {
        self.max_seen
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderError {
    PacketTooOld,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_buffer_pool_acquire_and_release() {
        let config = FrameBufferPoolConfig {
            max_pool_size: 2,
            buffer_size_bytes: 1024,
            track_stats: true,
        };
        let mut pool = FrameBufferPool::new(config);

        let buf1 = pool.acquire();
        assert_eq!(buf1.capacity(), 1024);
        assert_eq!(pool.in_use_count(), 1);
        assert_eq!(pool.available_count(), 0);

        pool.release(buf1);
        assert_eq!(pool.in_use_count(), 0);
        assert_eq!(pool.available_count(), 1);
    }

    #[test]
    fn test_frame_buffer_pool_reuse() {
        let config = FrameBufferPoolConfig {
            max_pool_size: 2,
            buffer_size_bytes: 1024,
            track_stats: true,
        };
        let mut pool = FrameBufferPool::new(config);

        let buf1 = pool.acquire();
        let id1 = buf1.id();
        pool.release(buf1);

        let buf2 = pool.acquire();
        let id2 = buf2.id();

        // Should be reused (same ID)
        assert_eq!(id1, id2);

        let stats = pool.stats();
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.reuses, 1);
    }

    #[test]
    fn test_frame_buffer_pool_max_size() {
        let config = FrameBufferPoolConfig {
            max_pool_size: 2,
            buffer_size_bytes: 1024,
            track_stats: true,
        };
        let mut pool = FrameBufferPool::new(config);

        let buf1 = pool.acquire();
        let buf2 = pool.acquire();
        let buf3 = pool.acquire();

        pool.release(buf1);
        pool.release(buf2);
        pool.release(buf3);

        // Should only keep 2 in pool
        assert_eq!(pool.available_count(), 2);
    }

    #[test]
    fn test_frame_buffer_pool_memory_usage() {
        let config = FrameBufferPoolConfig {
            max_pool_size: 3,
            buffer_size_bytes: 1024,
            track_stats: true,
        };
        let mut pool = FrameBufferPool::new(config);

        let buf1 = pool.acquire();
        let buf2 = pool.acquire();

        // 2 buffers in use: 2 * 1024 = 2048 bytes
        assert_eq!(pool.memory_usage_bytes(), 2048);

        pool.release(buf1);

        // 1 in use + 1 in pool: still 2 * 1024 = 2048 bytes
        assert_eq!(pool.memory_usage_bytes(), 2048);

        pool.release(buf2);

        // 2 available in pool: 2 * 1024 = 2048 bytes
        assert_eq!(pool.memory_usage_bytes(), 2048);
    }

    #[test]
    fn test_reorder_buffer_sequential() {
        let mut buf = ReorderBuffer::new(100);

        buf.insert(0, vec![1]).unwrap();
        buf.insert(1, vec![2]).unwrap();
        buf.insert(2, vec![3]).unwrap();

        let flushed = buf.flush_ordered();
        assert_eq!(flushed.len(), 3);
    }

    #[test]
    fn test_reorder_buffer_out_of_order() {
        let mut buf = ReorderBuffer::new(100);

        buf.insert(2, vec![3]).unwrap();
        buf.insert(0, vec![1]).unwrap();
        buf.insert(1, vec![2]).unwrap();

        let flushed = buf.flush_ordered();
        assert_eq!(flushed.len(), 3);
        assert_eq!(flushed[0][0], 1);
        assert_eq!(flushed[1][0], 2);
        assert_eq!(flushed[2][0], 3);
    }

    #[test]
    fn test_reorder_buffer_too_old() {
        let mut buf = ReorderBuffer::new(10);

        buf.insert(5, vec![5]).unwrap();
        buf.window_start = 10;  // Simulate sliding window forward

        let result = buf.insert(5, vec![5]);
        assert_eq!(result, Err(ReorderError::PacketTooOld));
    }

    #[test]
    fn test_reorder_buffer_window_slide() {
        let mut buf = ReorderBuffer::new(10);

        buf.insert(0, vec![0]).unwrap();
        buf.insert(5, vec![5]).unwrap();
        buf.insert(15, vec![15]).unwrap();  // Should slide window

        assert!(buf.window_start() >= 5);
    }

    #[test]
    fn test_reorder_buffer_occupancy() {
        let mut buf = ReorderBuffer::new(100);

        buf.insert(0, vec![0]).unwrap();
        buf.insert(1, vec![1]).unwrap();

        let occupancy = buf.occupancy_ratio();
        assert_eq!(occupancy, 2.0 / 100.0);
    }

    #[test]
    fn test_reorder_buffer_memory_tracking() {
        let mut buf = ReorderBuffer::new(100);

        buf.insert(0, vec![0u8; 1000]).unwrap();
        buf.insert(1, vec![0u8; 500]).unwrap();

        assert_eq!(buf.memory_usage_bytes(), 1500);

        buf.flush_ordered();
        assert_eq!(buf.memory_usage_bytes(), 0);
    }

    #[test]
    fn test_reorder_buffer_get() {
        let mut buf = ReorderBuffer::new(100);

        buf.insert(0, vec![42]).unwrap();
        buf.insert(1, vec![99]).unwrap();

        assert_eq!(buf.get(0), Some(vec![42]));
        assert_eq!(buf.get(1), Some(vec![99]));
        assert_eq!(buf.get(1), None);  // Already removed
    }
}
