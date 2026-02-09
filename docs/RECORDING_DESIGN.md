# Recording Feature Design

**Status**: Design Phase (v0.0.3 candidate)
**Priority**: Medium (after v0.0.2 validation)
**Scope**: Local MP4 recording with configurable quality

---

## Overview

Enable Wavry clients and servers to record streaming sessions to disk as MP4 files with:
- Separate video and audio tracks
- Configurable quality (bitrate, codec selection)
- Automatic compression
- Metadata embedding (timestamp, duration, resolution)

---

## Use Cases

1. **Server-Side Recording**: Host records all streaming sessions (audit, training)
2. **Client-Side Recording**: Client records received stream for local playback
3. **Hybrid**: Both endpoints record independently (full capture)

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│ Existing Streaming Pipeline                     │
└──────────────────┬──────────────────────────────┘
                   │
         ┌─────────┴─────────┐
         │                   │
    ┌────▼─────┐         ┌──▼──────┐
    │ Recorder  │         │Streaming│
    │ (New)     │         │ (Existing)
    └────┬─────┘         └──┬──────┘
         │                  │
    ┌────▼──────────────────▼────┐
    │ MP4 Muxer (libav)           │
    └────┬─────────────────────────┘
         │
    ┌────▼─────────┐
    │ MP4 File     │
    │ (.mp4)       │
    └──────────────┘
```

---

## Components

### 1. RecorderConfig
```rust
pub struct RecorderConfig {
    pub enabled: bool,
    pub output_dir: PathBuf,
    pub filename: String,  // {timestamp}, {codec}, {resolution} placeholders
    pub max_file_size_mb: u32,
    pub quality: Quality,
    pub split_on_codec_change: bool,
}

pub enum Quality {
    High,      // Preserve original bitrate
    Standard,  // 75% of original
    Low,       // 50% of original
    Custom(u32), // Specific bitrate in kbps
}
```

### 2. VideoRecorder (New Struct)

**Location**: `crates/wavry-media/src/recorder.rs`

```rust
pub struct VideoRecorder {
    config: RecorderConfig,
    output_file: File,
    muxer: MP4Muxer,
    frame_count: u64,
    start_time: Instant,
    current_codec: Codec,
    resolution: Resolution,
}

impl VideoRecorder {
    pub fn new(config: RecorderConfig) -> Result<Self>;
    pub fn write_frame(&mut self, frame: &EncodedFrame) -> Result<()>;
    pub fn write_audio(&mut self, packet: &AudioPacket) -> Result<()>;
    pub fn finalize(&mut self) -> Result<()>;
    pub fn get_stats(&self) -> RecorderStats;
}
```

### 3. AudioRecorder (New Struct)

```rust
pub struct AudioRecorder {
    config: RecorderConfig,
    sample_rate: u32,
    channels: u8,
    buffer: Vec<i16>,
}

impl AudioRecorder {
    pub fn new(config: RecorderConfig, sample_rate: u32, channels: u8) -> Result<Self>;
    pub fn write_samples(&mut self, samples: &[i16]) -> Result<()>;
    pub fn finalize(&mut self) -> Result<()>;
}
```

### 4. MP4Muxer Abstraction

**Dependency**: `mp4` crate (lightweight MP4 writer)

```rust
pub trait MP4Muxer {
    fn add_video_track(
        &mut self,
        codec: Codec,
        width: u16,
        height: u16,
        fps: u16,
    ) -> Result<TrackId>;

    fn add_audio_track(
        &mut self,
        sample_rate: u32,
        channels: u8,
    ) -> Result<TrackId>;

    fn write_video_sample(
        &mut self,
        track_id: TrackId,
        sample: &[u8],
        timestamp: u64,
        is_keyframe: bool,
    ) -> Result<()>;

    fn write_audio_sample(
        &mut self,
        track_id: TrackId,
        samples: &[i16],
        timestamp: u64,
    ) -> Result<()>;

    fn finalize(&mut self) -> Result<()>;
}

pub struct SimpleMP4Muxer {
    writer: BufWriter<File>,
    tracks: HashMap<TrackId, TrackInfo>,
}
```

---

## Integration Points

### Server-Side Recording (wavry-server)

Add to main streaming loop:

```rust
let recorder = if config.recording.enabled {
    Some(VideoRecorder::new(config.recording.clone())?)
} else {
    None
};

// In encode loop:
if let Some(ref mut recorder) = recorder {
    recorder.write_frame(&encoded_frame)?;
    if let Some(ref audio) = audio_packet {
        recorder.write_audio(audio)?;
    }
}

// On shutdown:
if let Some(mut recorder) = recorder {
    recorder.finalize()?;
}
```

### Client-Side Recording (wavry-client)

Add to decode loop:

```rust
let recorder = if config.recording.enabled {
    Some(VideoRecorder::new(config.recording.clone())?)
} else {
    None
};

// In decode loop:
if let Some(ref mut recorder) = recorder {
    recorder.write_frame(&encoded_frame)?;
}
```

---

## File Format

### MP4 Structure
```
MP4
├── ftyp (File Type Box)
├── moov (Movie Box)
│   ├── mvhd (Movie Header)
│   ├── trak (Video Track)
│   │   ├── tkhd (Track Header)
│   │   ├── edts (Edit List)
│   │   └── mdia (Media)
│   │       ├── mdhd (Media Header)
│   │       ├── hdlr (Handler)
│   │       └── minf (Media Information)
│   │           ├── vmhd (Video Media Header)
│   │           ├── dinf (Data Information)
│   │           └── stbl (Sample Table)
│   └── trak (Audio Track)
│       └── [similar structure]
└── mdat (Media Data)
    ├── Video Samples
    └── Audio Samples
```

### Metadata
- **Duration**: Calculated from frame timestamps
- **Creation Time**: System timestamp
- **Codec**: Embedded in stbl
- **Resolution/FPS**: Embedded in trak

---

## Configuration

### CLI Arguments
```bash
# Start with recording enabled
cargo run --bin wavry-server -- \
  --record \
  --record-dir /tmp/recordings \
  --record-quality high \
  --record-split-on-codec-change

# OR from client
cargo run --bin wavry-client -- \
  --record \
  --record-dir ~/Wavry\ Recordings
```

### Environment Variables
```bash
WAVRY_RECORD=true
WAVRY_RECORD_DIR=/var/lib/wavry/recordings
WAVRY_RECORD_QUALITY=standard
WAVRY_RECORD_MAX_SIZE_MB=2048
```

---

## Quality Settings

| Quality | Video Bitrate | Audio Bitrate | Use Case |
|---------|---------------|---------------|----------|
| High | 100% original | Lossless (128kbps) | Archive, analysis |
| Standard | 75% original | 96kbps AAC | Most use cases |
| Low | 50% original | 64kbps AAC | Long recordings, storage |
| Custom(N) | N kbps | Fixed 96kbps | Specific needs |

---

## Performance Considerations

### Memory
- **Buffering**: Encoded frames already in memory (no extra copy)
- **Peak**: ~50MB per minute at 1080p60 H.264
- **Strategy**: Streaming writes, no in-memory accumulation

### Disk I/O
- **Sequential writes**: One write per frame + periodic audio batch
- **Rate**: ~300-800 KB/s at 1080p60
- **Format**: Efficient MP4 (single-pass writing)

### CPU
- **Muxing overhead**: <5% CPU (negligible)
- **Compression**: Handled by encoder (not recorder's job)
- **Parallel**: Non-blocking relative to streaming

---

## Implementation Phases

### Phase 1: Core Recording (2-3 hours)
- [ ] RecorderConfig struct
- [ ] VideoRecorder implementation
- [ ] Basic MP4 muxing (single video track)
- [ ] Server-side integration
- [ ] Tests: file creation, frame writing, finalization

### Phase 2: Audio & Metadata (2 hours)
- [ ] AudioRecorder implementation
- [ ] Multi-track MP4 (video + audio)
- [ ] Metadata embedding (duration, resolution, creation time)
- [ ] Tests: audio synchronization, metadata validation

### Phase 3: Client-Side & Polish (2 hours)
- [ ] Client-side recording integration
- [ ] Quality presets & configuration
- [ ] Filename templating ({timestamp}, {codec}, etc.)
- [ ] File rotation/splitting on codec change
- [ ] Documentation & examples

### Phase 4: Advanced (Future)
- [ ] HLS/DASH segmented recording
- [ ] Hardware-accelerated encoding for MP4 (if applicable)
- [ ] Automated file management (retention, archival)
- [ ] Recording analytics dashboard

---

## Dependencies

### Required
- `mp4` crate - Lightweight MP4 writer
- Existing: `wavry-media` (codecs), `rift-core` (frames)

### Optional
- `ffmpeg-sys` - Advanced muxing (future)
- `rayon` - Parallel encoding (future)

### Cargo.toml Addition
```toml
[dependencies]
mp4 = "0.13"  # MP4 file format writer
```

---

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_recorder_config_validation() { }

#[test]
fn test_video_recorder_frame_write() { }

#[test]
fn test_audio_recorder_sample_write() { }

#[test]
fn test_mp4_muxer_track_creation() { }
```

### Integration Tests
```rust
#[tokio::test]
async fn test_full_recording_workflow() {
    // 1. Create recorder
    // 2. Write 100 video frames
    // 3. Write audio samples
    // 4. Finalize
    // 5. Verify MP4 structure
}

#[test]
fn test_recorded_mp4_playable() {
    // Write MP4, verify with ffmpeg/mediainfo
}

#[test]
fn test_quality_presets() {
    // Test High/Standard/Low -> correct bitrate
}
```

### Manual Testing
```bash
# Record and verify
cargo run --bin wavry-server -- --record --record-quality high
# Wait 10 seconds
# Check output.mp4 with: ffplay, mediainfo, or ffprobe

ffprobe recordings/wavry-*.mp4
```

---

## Security & Privacy

- **File Permissions**: 0600 (user only)
- **Metadata**: No PII stored beyond stream resolution/codec
- **Encryption**: Optional (encrypt at rest, separate concern)
- **Audit**: Log file creation, rotation, deletion
- **Compliance**: Document retention policy implications

---

## Success Criteria

- ✅ MP4 files created successfully
- ✅ Video and audio synchronized
- ✅ File playable in VLC, QuickTime, ffplay
- ✅ Performance overhead <5% CPU
- ✅ Disk I/O sustainable (no buffering)
- ✅ 150+ tests passing (existing + new)
- ✅ Documentation complete
- ✅ Quality presets working

---

## References

- MP4 Format: [ISO/IEC 14496-12](https://en.wikipedia.org/wiki/MPEG-4_Part_12)
- H.264 in MP4: [RFC 6381](https://tools.ietf.org/html/rfc6381)
- Rust MP4 Crate: [mp4 on crates.io](https://crates.io/crates/mp4)
