# Wavry Backlog Roadmap (v0.0.3+)

**Status**: Updated After v0.0.3-rc1 Implementation
**Current Version**: v0.0.3-rc1
**Next Release**: v0.0.4

---

## Feature Priority Matrix

| Feature | Complexity | Impact | Dependencies | Est. Hours | Status |
|---------|-----------|--------|--------------|-----------|--------|
| **Recording** | Medium | High | wavry-media | 6-8 | ✅ Shipped (v0.0.3-rc1) |
| **Clipboard Sync** | Low | Medium | signal channel | 4-6 | ✅ Shipped (v0.0.3-rc1) |
| **File Transfer** | High | High | RIFT protocol | 12-16 | 🟡 MVP Implemented |
| **Audio Routing** | Medium | Medium | wavry-media | 6-8 | 🟡 Phase 2 (mic parity) |
| **Input Mapping** | Low | Low | config | 3-4 | ✅ Shipped (v0.0.3-rc1) |

---

## v0.0.3 Delivered + v0.4 Carry-Over Features

### 🎬 1. Recording - LOCAL MP4 RECORDING

**What**: Save streaming sessions to MP4 files
**Where**: Client-side and/or server-side
**Why**: Audit, training, content creation, local archive
**Time**: 6-8 hours
**Status**: ✅ Shipped in v0.0.3-rc1

**Key Features**:
- Configurable quality (High/Standard/Low)
- Automatic metadata (duration, resolution, codec)
- Separate audio/video tracks
- File rotation on codec change
- CLI arguments + environment variables

**Design**: See [RECORDING_DESIGN.md](RECORDING_DESIGN.md)

**Implementation Order**:
1. RecorderConfig + VideoRecorder struct
2. MP4 muxing (video track only)
3. Audio track + synchronization
4. Server & client integration
5. Quality presets & testing

---

### 📋 2. Clipboard Sync - BIDIRECTIONAL CLIPBOARD SHARING

**What**: Share clipboard between host and client
**Where**: Input control channel (signal)
**Why**: Seamless workflow, copy/paste between machines
**Time**: 4-6 hours
**Status**: ✅ Shipped in v0.0.3-rc1

**Protocol**:
```protobuf
message ClipboardData {
    string content = 1;
    int64 timestamp_us = 2;
}

// Add to InputMessage:
oneof event {
    ...
    ClipboardData clipboard = 7;
}
```

**Architecture**:
```
Host Clipboard ←→ [signal channel] ←→ Client Clipboard
     ↓ (monitor)                          ↓ (monitor)
   [Clipboard Stack]                    [Clipboard Stack]
```

**Implementation**:
1. Add `ClipboardData` message to protobuf
2. Create `ClipboardMonitor` (watch system clipboard)
3. Server-side integration (monitor host, inject client)
4. Client-side integration (monitor client, inject host)
5. Rate limiting (avoid spam)
6. Size limits (10MB max)

**Challenges**:
- Platform-specific clipboard APIs (pasteboard on macOS, WinAPI on Windows, Xclip on Linux)
- Avoid infinite loops (host → client → host)
- Handle binary data (images, files)

---

### 📁 3. File Transfer - SECURE FILE EXCHANGE

**What**: Transfer files between host and client
**Where**: New RIFT media message type
**Why**: Application deployment, document sharing, data transfer
**Time**: 12-16 hours
**Status**: ✅ v0.0.4 hardening complete (resume/cancel controls + fair-share scheduling)

**Protocol**:
```protobuf
message FileHeader {
    string filename = 1;
    uint64 file_size = 2;
    string checksum = 3;     // SHA256
    int64 timestamp = 4;
    uint32 permissions = 5;
}

message FileChunk {
    uint64 file_id = 1;
    uint32 chunk_index = 2;
    uint32 total_chunks = 3;
    bytes data = 4;
}

message FileStatus {
    uint64 file_id = 1;
    enum Status { PENDING, IN_PROGRESS, COMPLETE, ERROR }
    Status status = 2;
    string error = 3;
}

// Add to ControlMessage or new message type
```

**Architecture**:
```
┌─────────────┐
│  File A.zip │
└──────┬──────┘
       │ (split into 64KB chunks)
       │
   ┌───▼──────────────────────┐
   │ Encrypted + FEC          │
   └───┬──────────────────────┘
       │
   ┌───▼────────────────────────┐
   │ Packetized over RIFT       │
   └───┬────────────────────────┘
       │ (Network)
       │
   ┌───▼────────────────────────┐
   │ Depacketize + FEC Recover  │
   └───┬────────────────────────┘
       │
   ┌───▼──────────────────────┐
   │ Reassemble + Verify SHA  │
   └───┬──────────────────────┘
       │
   ┌───▼──────────────────────┐
   │  File A.zip (verified)   │
   └──────────────────────────┘
```

**Implementation**:
1. Add FileHeader/FileChunk messages to RIFT proto
2. Create `FileTransferManager` (send side)
3. Create `FileReceiver` (receive side)
4. Chunking + checksum validation
5. Progress tracking & cancellation
6. Bandwidth limiting (don't starve video)
7. Integration with server/client main loop

**Challenges**:
- Large file handling (memory efficient)
- Bandwidth fairness with video
- Cross-platform file permissions
- Resume on disconnect
- Malicious file rejection

**Security**:
- Validate filename (no path traversal)
- Verify checksum (prevent corruption)
- Size limits (1GB per file default)
- Quarantine suspicious files

---

### 🔊 4. AUDIO ROUTING - PER-APP AUDIO CAPTURE

**What**: Capture and route audio from specific applications
**Where**: wavry-media + platform modules
**Why**: Game audio, selective streaming, audio conferencing
**Time**: 6-8 hours
**Status**: 🟡 Phase 2 in progress (`--audio-source` + system mix + microphone parity + Linux app routing; Windows app parity pending)

**Architecture**:
```
┌──────────────────┐
│ Application A    │
│ (Audio Out)      │
└────────┬─────────┘
         │
    ┌────▼──────────────────────┐
    │ Audio Router               │
    │ (select streams)           │
    └────┬───────────────────────┘
         │ (mixed or selected)
    ┌────▼──────────┐
    │ Encoder       │
    │ (Opus/AAC)    │
    └────┬──────────┘
         │
    ┌────▼──────────┐
    │ Network       │
    │ (RIFT)        │
    └───────────────┘
```

**Configuration**:
```rust
pub enum AudioSource {
    SystemMix,           // All system audio
    Application(String), // Specific app (by name)
    Microphone,          // Input device
    Desktop,             // Desktop audio only
}

pub struct AudioConfig {
    pub source: AudioSource,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate_kbps: u32,
}
```

**Implementation**:
1. **macOS**: Core Audio with device enumeration
2. **Linux**: PulseAudio/ALSA source selection
3. **Windows**: WASAPI with app enumeration
4. Audio source selector in config
5. Level metering + visualization
6. Tests for format conversion

**Challenges**:
- Platform audio APIs differ significantly
- App enumeration (Windows VID, Linux process names)
- Latency (audio lag vs video)
- Mixing multiple sources efficiently

---

### ⌨️ 5. INPUT MAPPING - CUSTOM INPUT PROFILES

**What**: Remap input devices (e.g., Xbox controller → PlayStation layout)
**Where**: wavry-platform + client config
**Why**: Consistency across platforms, accessibility, game-specific layouts
**Time**: 3-4 hours
**Status**: ✅ Shipped in v0.0.3-rc1

**Configuration**:
```rust
pub struct InputMapping {
    pub name: String,
    pub gamepad_layout: GamepadLayout,  // Xbox / PlayStation / Switch
    pub deadzone: f32,
    pub sensitivity: f32,
    pub button_remaps: HashMap<u32, u32>,
}

pub enum GamepadLayout {
    XboxStandard,      // A/B/X/Y
    PlayStationStandard, // △/○/□/×
    NintendoStandard,   // A/B/X/Y (different positions)
}
```

**Use Cases**:
- Cross-platform game testing
- Accessibility remapping
- Per-game profiles
- Emulation layouts

**Implementation**:
1. Input mapping config parsing
2. Gamepad button remapping
3. Analog stick deadzone tuning
4. Sensitivity scaling
5. Profile manager (save/load)

---

## Implementation Roadmap

### Phase A: Recording (v0.0.3)
```timeline
Week 1: Design ✅ → Implementation
  Day 1-2: Core recorder struct + MP4 muxing
  Day 3: Audio track + sync
  Day 4: Server/client integration
  Day 5: Testing + polish

Tests: 10-15 new (MP4 format, recording workflow)
Docs: RECORDING_DESIGN.md (done), update README
```

### Phase B: Clipboard + Input Mapping (v0.0.3)
```timeline
Week 2:
  Day 1-2: Clipboard monitor + protocol
  Day 3: Server/client clipboard sync
  Day 4: Input mapping + profiles
  Day 5: Testing + polish

Tests: 5-8 new (clipboard, input mapping)
```

### Phase C: Audio Routing (v0.0.4)
```timeline
Week 3-4: Platform-specific audio APIs
  Day 1-3: macOS Core Audio implementation
  Day 4-5: Linux PulseAudio implementation
  Day 6-7: Windows WASAPI implementation
  Day 8: Integration + testing

Tests: 10+ new (audio source selection, format conversion)
Docs: AUDIO_ROUTING.md
```

### Phase D: File Transfer (v0.0.4+)
```timeline
Week 5-6: Complex protocol work
  Day 1-2: RIFT message definitions
  Day 3-4: File chunking + checksums
  Day 5-6: Manager/receiver implementation
  Day 7-8: Integration + bandwidth fairness
  Day 9: Testing (large files, corruption, resume)

Tests: 15+ new (file ops, network simulation)
Docs: FILE_TRANSFER.md
```

---

## Dependencies & Prerequisites

### Recording
- ✅ `mp4` crate available
- ✅ `wavry-media` (EncodedFrame available)
- ✅ No protocol changes needed

### Clipboard Sync
- ⚠️ Platform clipboard libraries needed
  - macOS: `cocoa` crate (optional, use system calls)
  - Linux: `x11-clipboard` or `wl-clipboard`
  - Windows: `clipboard-win` crate
- ✅ Protocol extension (minor)
- ✅ Control channel exists

### File Transfer
- ✅ RIFT message types implemented (`FileHeader`, `FileChunk`, `FileStatus`)
- ✅ Media channel extension implemented
- ✅ Crypto already in place
- ✅ FEC can be reused

### Audio Routing
- 🟡 Audio source routing selector implemented (`--audio-source`)
- 🟡 System mix capture integrated in host streaming path
- ✅ Microphone capture path integrated on macOS/Linux/Windows (with runtime fallback safety)
- 🟡 Linux app-specific capture route integrated (`app:<name>` via Pulse sink-input matching + fallback)
- 🔴 Windows app-specific capture (process loopback) still pending
- ✅ Audio encoding infrastructure exists
- ⚠️ May need Opus codec updates

### Input Mapping
- ✅ Config system exists
- ✅ Input path available
- No protocol changes needed

---

## Success Metrics

### Recording
- [ ] 5MB+ MP4 files created
- [ ] Playable in standard players
- [ ] Video + audio synchronized
- [ ] <5% CPU overhead
- [ ] Zero frame loss during recording

### Clipboard
- [ ] Bidirectional text sync
- [ ] <200ms latency
- [ ] 10MB max size enforced
- [ ] Loop prevention working

### File Transfer
- [ ] 1GB file transfer in <2 minutes (local)
- [ ] Checksum validation passes
- [ ] Bandwidth limited (don't starve video)
- [ ] Resume on disconnect

### Audio Routing
- [ ] App-specific capture working
- [ ] Quality: 128kbps Opus
- [ ] <50ms latency
- [ ] Cross-platform consistent

### Input Mapping
- [ ] Remap persists across sessions
- [ ] Sensitivity/deadzone applied
- [ ] Profile switching seamless

---

## Risk Assessment

| Feature | Risk | Mitigation |
|---------|------|-----------|
| Recording | File I/O contention | Async writes, buffering |
| Clipboard | Infinite loop | Loop detection, source tracking |
| File Transfer | Large memory use | Streaming, chunking |
| Audio Routing | Audio latency | Buffering strategy, calibration |
| Input Mapping | Deadlock on input | Non-blocking handler |

---

## Questions for Planning

1. **Recording**: Should we support HLS/DASH (streaming format) or just MP4?
2. **Clipboard**: Binary data (images) or text-only first?
3. **File Transfer**: Resume capability required or MVP without it?
4. **Audio**: Native app enumeration or fallback to system mix?
5. **Input Mapping**: Built-in profiles or community profiles?

---

## Next Steps

1. ✅ Design completed (this document)
2. ✅ v0.0.3 features implemented (recording, clipboard, input mapping)
3. ✅ File transfer MVP integrated across protocol + client/server
4. ✅ Audio routing phase 1 integrated (`--audio-source` + forwarding)
5. ✅ v0.4 hardening: transfer resume/cancel + congestion-aware fairness
6. 🟡 v0.4 hardening: Windows per-app routing parity
7. ⏳ Release v0.0.4

---

See individual design documents for detailed specifications:
- [RECORDING_DESIGN.md](RECORDING_DESIGN.md) - Complete
- [CLIPBOARD_DESIGN.md](CLIPBOARD_DESIGN.md) - Complete
- [FILE_TRANSFER_DESIGN.md](FILE_TRANSFER_DESIGN.md) - Complete
- [AUDIO_ROUTING_DESIGN.md](AUDIO_ROUTING_DESIGN.md) - Complete
- [INPUT_MAPPING_DESIGN.md](INPUT_MAPPING_DESIGN.md) - Complete
