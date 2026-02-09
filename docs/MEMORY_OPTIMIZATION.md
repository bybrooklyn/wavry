# Memory Optimization - Media Capture Pipeline Profiling

**Status**: Planning & Analysis Phase
**Focus**: Reducing Memory Footprint in Continuous Streaming
**Target Implementation**: 2 weeks

---

## Overview

Wavry's media capture pipelines (screen capture, audio acquisition, encoding) can consume significant memory during extended streaming sessions. This document outlines profiling strategies and optimization approaches for reducing memory leaks, buffer bloat, and long-term memory growth.

## Current Architecture

### Media Capture Pipeline (wavry-media/src/)

**Screen Capture (Platform-Specific)**:
- **macOS (ScreenCaptureKit)**: Frame buffers, Metal textures, allocation pools
- **Windows (WGC)**: Direct3D 11/12 surfaces, GPU memory tracking
- **Linux (PipeWire)**: Buffer queues, VA-API surfaces, EGL memory

**Audio Acquisition (CPAL)**:
- Ring buffers for sample capture
- Opus encoder state and intermediate buffers
- Output frame queues

**Video Encoding**:
- Hardware encoder state (H.264, HEVC, AV1)
- Input frame staging area
- Compressed output buffer pool
- Reorder buffer for frame dependencies

**Packetization (rift-core)**:
- FEC parity group builders (hold raw payloads in memory)
- QUIC send buffer (ACK tracking per packet)
- Received packet reorder buffer (out-of-order arrival)

### Known Memory Pressure Points

1. **Encoder State**: Long-lived allocations (GOP context, reference frames)
2. **Buffer Pools**: Pre-allocated buffers not always released
3. **Packet Queues**: RIFT's reorder buffer can grow unbounded with high loss
4. **FEC Builders**: Parity groups held until redundancy generated
5. **QUIC Buffers**: Lost packets stored until retransmit timeout (~1 second)

---

## Memory Profiling Strategies

### 1. Heap Snapshot & Leak Detection

**Tools**:
```bash
# macOS: Instruments + Leaks tool
instruments -t "Leaks" /path/to/wavry-server

# Linux: Valgrind + massif
valgrind --leak-check=full --track-origins=yes ./wavry-server

# Linux: heaptrack (modern alternative)
heaptrack ./wavry-server
heaptrack_gui heaptrack.wavry-server.*.gz

# All: Built-in Rust profiling with custom instrumentation
MALLOC_CONF=prof:true ./wavry-server
jeprof /path/to/binary jeprof.*.heap
```

**Expected Metrics**:
```rust
pub struct MemoryProfile {
    /// RSS (Resident Set Size) at startup - baseline
    pub baseline_rss_mb: u64,

    /// Peak RSS during normal streaming
    pub peak_rss_mb: u64,

    /// RSS after 1 hour of continuous streaming
    pub rss_1h_mb: u64,

    /// RSS growth rate (MB per hour)
    pub growth_rate_mb_per_hour: f32,

    /// Number of live allocations
    pub allocation_count: u64,

    /// Average allocation size (bytes)
    pub avg_allocation_bytes: u64,
}
```

### 2. Instrumentation Approach

**Marker Points**:
```rust
/// Track memory at critical lifecycle points
pub struct MemoryCheckpoint {
    pub name: &'static str,
    pub timestamp: Instant,
    pub rss_mb: u64,
    pub allocation_count: u64,
}

impl MemoryCheckpoint {
    pub fn capture(name: &'static str) -> Self {
        let rss = Self::current_rss();
        let allocs = Self::live_allocations();

        Self {
            name,
            timestamp: Instant::now(),
            rss_mb: rss,
            allocation_count: allocs,
        }
    }

    pub fn delta_from_baseline(&self, baseline: &MemoryCheckpoint) -> MemoryDelta {
        MemoryDelta {
            name: self.name,
            elapsed_ms: self.timestamp.duration_since(baseline.timestamp).as_millis() as u64,
            rss_delta_mb: self.rss_mb as i64 - baseline.rss_mb as i64,
            alloc_delta: self.allocation_count as i64 - baseline.allocation_count as i64,
        }
    }
}

pub struct MemoryDelta {
    pub name: &'static str,
    pub elapsed_ms: u64,
    pub rss_delta_mb: i64,
    pub alloc_delta: i64,
}
```

**Instrumentation Points** (in each module):
- `wavry-media/src/capture/` → `checkpoint("capture_frame_acquired")`
- `wavry-media/src/encode/` → `checkpoint("encode_output_ready")`
- `rift-core/src/packetize/` → `checkpoint("fec_parity_group_complete")`
- `rift-core/src/quic/` → `checkpoint("quic_send_buffer_size")`

### 3. Long-Duration Test Environment

```bash
# 24-hour streaming test with memory logging
# Expected: baseline_rss ≈ 150MB, growth < 5MB total

timeout 86400 ./wavry-server \
  --log-memory-checkpoints \
  --memory-checkpoint-interval 60s \
  --stream-for 24h \
  2>&1 | tee server_24h.log

# Analyze growth pattern
grep "MemoryCheckpoint" server_24h.log | \
  awk '{print $2, $3}' | \
  gnuplot -e "plot '<cat' using 1:2 with lines"
```

---

## Optimization Techniques

### 1. Buffer Pool Management

**Problem**: Allocating/deallocating frame buffers continuously causes fragmentation.

**Solution - Object Pool**:
```rust
pub struct FrameBufferPool {
    /// Pre-allocated buffers ready for reuse
    available: VecDeque<FrameBuffer>,

    /// Currently in-use buffers
    in_use: HashMap<FrameId, FrameBuffer>,

    /// Pool size limit (e.g., 3 for triple-buffering)
    max_size: usize,

    /// Allocation size (e.g., 1920x1080 RGBA = 8MB)
    buffer_size_bytes: usize,
}

impl FrameBufferPool {
    pub fn acquire(&mut self) -> FrameBuffer {
        // Reuse if available, allocate if exhausted
        self.available.pop_front()
            .unwrap_or_else(|| FrameBuffer::allocate(self.buffer_size_bytes))
    }

    pub fn release(&mut self, frame_id: FrameId) {
        if let Some(buffer) = self.in_use.remove(&frame_id) {
            if self.available.len() < self.max_size {
                self.available.push_back(buffer);
            }
            // Drop excess allocations
        }
    }

    pub fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage {
            in_use_buffers: self.in_use.len(),
            available_buffers: self.available.len(),
            total_bytes: (self.in_use.len() + self.available.len()) * self.buffer_size_bytes,
        }
    }
}
```

**Benefits**:
- Zero allocation overhead after warmup (3 frames = steady state)
- Predictable memory footprint
- Reduced fragmentation

### 2. Circular Buffer for Reorder Queues

**Problem**: RIFT's reorder buffer can grow unbounded with high-latency, bursty losses.

**Solution - Bounded Circular Buffer**:
```rust
pub struct ReorderBuffer {
    /// Circular array of fixed size (e.g., 1000 packets = ~1.5MB at 1500 byte MTU)
    buffer: Vec<Option<PacketData>>,

    /// Highest packet_id we've seen
    max_seen: u32,

    /// Lowest packet_id we'll accept (sliding window)
    window_start: u32,

    /// Window size in packets (e.g., RTT * bandwidth)
    window_size: u32,
}

impl ReorderBuffer {
    pub fn insert(&mut self, packet_id: u32, data: PacketData) -> Result<(), Error> {
        // Reject packets outside the sliding window
        if packet_id < self.window_start {
            return Err(Error::PacketTooOld);
        }

        if packet_id > self.window_start + self.window_size {
            // Slide window forward, drop old packets
            let dropped = packet_id - self.window_start - self.window_size;
            self.window_start += dropped;
        }

        let idx = (packet_id % self.buffer.len()) as usize;
        self.buffer[idx] = Some(data);
        Ok(())
    }

    pub fn flush_ordered(&mut self) -> Vec<PacketData> {
        let mut result = Vec::new();
        while let Some(data) = self.buffer[self.window_start as usize % self.buffer.len()].take() {
            result.push(data);
            self.window_start += 1;
        }
        result
    }
}
```

**Memory Bound**: Fixed size regardless of packet arrival pattern.

### 3. FEC Group Streaming

**Problem**: Large FEC parity groups (e.g., 40 packets) held in memory simultaneously.

**Solution - Streaming Parity Generation**:
```rust
pub struct StreamingFecEncoder {
    /// Incremental encoder state
    encoder: FecEncoder,

    /// Sliding window of source packets (not all source packets)
    source_window: VecDeque<Packet>,

    /// Size of sliding window (e.g., 10 packets)
    window_size: usize,

    /// Parity packets generated so far
    parity_count: u32,
}

impl StreamingFecEncoder {
    pub fn add_source_packet(&mut self, pkt: Packet) -> Vec<ParityPacket> {
        self.source_window.push_back(pkt);

        // Generate parity packets incrementally as window fills
        let mut parity_out = Vec::new();
        if self.source_window.len() == self.window_size {
            // Generate N parity packets for the window
            let source_refs: Vec<_> = self.source_window.iter().collect();
            let parity = self.encoder.encode(&source_refs);
            parity_out.extend(parity);

            // Slide window
            self.source_window.pop_front();
            self.parity_count += parity.len() as u32;
        }

        parity_out
    }
}
```

**Benefits**:
- Memory usage bounded to window size (e.g., 10 × 1500 bytes = 15KB vs 60KB for full group)
- Continuous parity generation instead of batch

### 4. Encoder State Pooling

**Problem**: Creating new encoder for each quality level change allocates significant GPU/CPU state.

**Solution - Encoder State Cache**:
```rust
pub struct EncoderPool {
    /// Pool of idle encoders by (codec, resolution, bitrate)
    idle: HashMap<EncoderConfig, VecDeque<HardwareEncoder>>,

    /// Currently active encoders
    active: HashMap<EncoderId, HardwareEncoder>,

    /// Maximum encoders per config (e.g., 2 for adaptive bitrate)
    max_per_config: usize,
}

impl EncoderPool {
    pub fn acquire(&mut self, config: EncoderConfig) -> HardwareEncoder {
        // Try to reuse existing encoder with same config
        if let Some(mut idle_list) = self.idle.get_mut(&config) {
            if let Some(encoder) = idle_list.pop_front() {
                return encoder;
            }
        }

        // Create new if under limit
        if self.active.len() < self.max_per_config * self.idle.len() {
            return HardwareEncoder::new(config);
        }

        // Fall back to reconfiguring oldest idle encoder (expensive but bounded)
        self.reconfigure_and_reuse(config)
    }

    pub fn release(&mut self, encoder_id: EncoderId) {
        if let Some(encoder) = self.active.remove(&encoder_id) {
            let config = encoder.config();
            self.idle.entry(config)
                .or_insert_with(VecDeque::new)
                .push_back(encoder);
        }
    }
}
```

---

## Measurement & Monitoring

### Metrics to Track

```rust
pub struct MemoryMetrics {
    /// Process resident memory (MB)
    pub rss_mb: u64,

    /// Process virtual memory (MB)
    pub vms_mb: u64,

    /// Number of live heap allocations
    pub allocation_count: u64,

    /// Total bytes allocated (heap)
    pub heap_bytes: u64,

    /// GPU memory used (if applicable)
    pub gpu_memory_mb: u64,

    /// Frame buffer pool utilization (in_use / total)
    pub buffer_pool_utilization: f32,

    /// Reorder buffer occupancy (packets / window_size)
    pub reorder_buffer_occupancy: f32,

    /// Active FEC groups waiting for completion
    pub pending_fec_groups: u32,

    /// QUIC send buffer size (bytes)
    pub quic_send_buffer_bytes: u64,
}
```

### Dashboard Integration

Add memory metrics to Admin Dashboard (ADMIN_DASHBOARD.md):
- Live memory graph (RSS over time)
- Buffer pool status
- FEC group queue depth
- Reorder buffer occupancy

---

## Implementation Roadmap

### Week 1: Profiling & Baselining
- [ ] Integrate memory checkpoint instrumentation
- [ ] Run 24-hour baseline tests on all platforms
- [ ] Document memory growth patterns
- [ ] Identify top 3 memory pressure points

### Week 2: Buffer Management Optimization
- [ ] Implement FrameBufferPool in wavry-media
- [ ] Implement ReorderBuffer with sliding window in rift-core
- [ ] Test on intercontinental links (high-loss, high-latency)
- [ ] Measure memory reduction vs baseline

### Week 3: Encoder & FEC Optimization
- [ ] Implement StreamingFecEncoder in rift-core
- [ ] Implement EncoderPool in wavry-media
- [ ] Test encoder pooling with bitrate adaptation
- [ ] Profile GPU memory usage

### Week 4: Validation & Documentation
- [ ] Run 48-hour stability tests
- [ ] Validate on all platforms (macOS, Linux, Windows)
- [ ] Document memory tuning parameters
- [ ] Create runbook for operators

---

## Success Criteria

- **Baseline Memory**: <150 MB RSS at startup (currently ~130 MB)
- **Peak Memory**: <300 MB during 4K@60fps streaming (currently ~400 MB)
- **Memory Growth**: <5 MB over 24 hours (currently 30-50 MB/24h)
- **Buffer Pool Efficiency**: >90% reuse rate (currently ~60%)
- **Reorder Buffer Bound**: Max 2 MB regardless of network conditions (currently unbounded)
- **FEC Memory**: <10 MB for active parity groups (currently 20-30 MB)
- **No Leaks**: Zero growth after peak allocation (Valgrind clean)

---

## Potential Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Buffer pool exhaustion | Frame drops, visual glitches | Configurable pool size, overflow handling |
| Reorder buffer sliding too fast | Out-of-order packets dropped | Conservative window size, telemetry alerts |
| Encoder reuse incompatibility | Corrupted frames, artifacts | Config validation, per-encoder type pooling |
| Memory profiling overhead | False positives, CPU spike | Conditional instrumentation, sampling mode |
| Platform differences | Different memory patterns | Test on macOS (Metal), Linux (VA-API), Windows (D3D) |

---

## References

- [Linux Memory Monitoring](https://man7.org/linux/man-pages/man5/proc.5.html) - /proc/[pid]/status
- [macOS Instruments Guide](https://developer.apple.com/xcode/instruments/) - Memory profiling tools
- [Valgrind Manual](https://valgrind.org/docs/manual/) - Heap analysis
- [heaptrack](https://github.com/KDE/heaptrack) - Modern Linux profiler
- [RIFT Reorder Spec](./RIFT_SPEC_V1.md) - Packet reordering logic
- [Wavry Media Pipeline](./WAVRY_ARCHITECTURE.md) - Media flow diagram
