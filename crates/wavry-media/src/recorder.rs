use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use mp4::{
    AacConfig, AudioObjectType, AvcConfig, ChannelConfig, HevcConfig, MediaConfig, Mp4Config,
    Mp4Writer, SampleFreqIndex, TrackConfig,
};

use crate::{Codec, Resolution};

#[derive(Debug, Clone)]
pub enum Quality {
    High,        // Preserve original bitrate
    Standard,    // 75% of original
    Low,         // 50% of original
    Custom(u32), // Specific bitrate in kbps
}

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub enabled: bool,
    pub output_dir: PathBuf,
    pub filename_prefix: String,
    pub max_file_size_mb: u32,
    pub quality: Quality,
    pub split_on_codec_change: bool,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_dir: PathBuf::from("recordings"),
            filename_prefix: String::from("wavry-capture"),
            max_file_size_mb: 2048,
            quality: Quality::Standard,
            split_on_codec_change: true,
        }
    }
}

pub struct VideoRecorder {
    config: RecorderConfig,
    writer: Option<Mp4Writer<BufWriter<File>>>,
    video_track_id: u32,
    audio_track_id: u32,
    frame_count: u64,
    audio_sample_count: u64,
    start_time: Instant,
    current_codec: Option<Codec>,
    resolution: Option<Resolution>,
    fps: u16,
}

impl VideoRecorder {
    pub fn new(config: RecorderConfig) -> Result<Self> {
        if !config.output_dir.exists() {
            std::fs::create_dir_all(&config.output_dir)?;
        }

        Ok(Self {
            config,
            writer: None,
            video_track_id: 0,
            audio_track_id: 0,
            frame_count: 0,
            audio_sample_count: 0,
            start_time: Instant::now(),
            current_codec: None,
            resolution: None,
            fps: 60,
        })
    }

    pub fn is_initialized(&self) -> bool {
        self.writer.is_some()
    }

    fn init_writer(&mut self, codec: Codec, res: Resolution, fps: u16) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let filename = format!(
            "{}-{}-{:?}x{:?}-{:?}.mp4",
            self.config.filename_prefix, timestamp, res.width, res.height, codec
        );
        let path = self.config.output_dir.join(filename);
        let file = File::create(path)?;
        let writer = BufWriter::new(file);

        let config = Mp4Config {
            major_brand: str::parse("isom").unwrap(),
            minor_version: 512,
            compatible_brands: vec![
                str::parse("isom").unwrap(),
                str::parse("iso2").unwrap(),
                str::parse("avc1").unwrap(),
                str::parse("mp41").unwrap(),
            ],
            timescale: 1000,
        };

        let mut mp4_writer = Mp4Writer::write_start(writer, &config)?;

        // Video Track
        let media_conf = match codec {
            Codec::H264 => MediaConfig::AvcConfig(AvcConfig {
                width: res.width,
                height: res.height,
                seq_param_set: vec![],
                pic_param_set: vec![],
            }),
            Codec::Hevc => MediaConfig::HevcConfig(HevcConfig {
                width: res.width,
                height: res.height,
            }),
            Codec::Av1 => {
                return Err(anyhow!(
                    "AV1 recording is not yet supported by the MP4 muxer"
                ));
            }
        };

        let video_track_config = TrackConfig {
            track_type: mp4::TrackType::Video,
            timescale: 1000,
            language: String::from("und"),
            media_conf,
        };

        mp4_writer.add_track(&video_track_config)?;
        self.video_track_id = 1;

        // Audio Track (AAC)
        let audio_track_config = TrackConfig {
            track_type: mp4::TrackType::Audio,
            timescale: 48000,
            language: String::from("und"),
            media_conf: MediaConfig::AacConfig(AacConfig {
                bitrate: 128000,
                profile: AudioObjectType::AacLowComplexity,
                freq_index: SampleFreqIndex::Freq48000,
                chan_conf: ChannelConfig::Stereo,
            }),
        };

        mp4_writer.add_track(&audio_track_config)?;
        self.audio_track_id = 2;

        self.writer = Some(mp4_writer);
        self.current_codec = Some(codec);
        self.resolution = Some(res);
        self.fps = fps;
        self.start_time = Instant::now();
        self.frame_count = 0;
        self.audio_sample_count = 0;

        Ok(())
    }

    pub fn write_frame(
        &mut self,
        data: &[u8],
        keyframe: bool,
        codec: Codec,
        res: Resolution,
        fps: u16,
    ) -> Result<()> {
        if self.writer.is_none()
            || (self.config.split_on_codec_change
                && (self.current_codec != Some(codec) || self.resolution != Some(res)))
        {
            if self.writer.is_some() {
                self.finalize()?;
            }
            if let Err(e) = self.init_writer(codec, res, fps) {
                log::warn!("Failed to initialize recorder: {}", e);
                return Ok(());
            }
        }

        if let Some(ref mut writer) = self.writer {
            let duration = 1000 / self.fps as u64;
            let sample = mp4::Mp4Sample {
                start_time: self.frame_count * duration,
                duration: duration as u32,
                rendering_offset: 0,
                is_sync: keyframe,
                bytes: data.to_vec().into(),
            };

            writer.write_sample(self.video_track_id, &sample)?;
            self.frame_count += 1;
        }

        Ok(())
    }

    pub fn write_audio(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            let start_time = (timestamp_us * 48) / 1000;
            let sample = mp4::Mp4Sample {
                start_time,
                duration: 960, // 20ms at 48kHz
                rendering_offset: 0,
                is_sync: true,
                bytes: payload.to_vec().into(),
            };

            writer.write_sample(self.audio_track_id, &sample)?;
            self.audio_sample_count += 1;
        }
        Ok(())
    }

    pub fn finalize(&mut self) -> Result<()> {
        if let Some(mut writer) = self.writer.take() {
            writer.write_end()?;
        }
        Ok(())
    }
}

impl Drop for VideoRecorder {
    fn drop(&mut self) {
        let _ = self.finalize();
    }
}
