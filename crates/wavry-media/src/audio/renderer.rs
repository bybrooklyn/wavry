use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, Stream, StreamConfig, SupportedBufferSize};
#[cfg(feature = "opus-support")]
use opus::{Channels, Decoder as OpusDecoder};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use super::{
    AUDIO_MAX_BUFFER_SAMPLES, OPUS_CHANNELS, OPUS_FRAME_SAMPLES,
    OPUS_SAMPLE_RATE,
};
#[cfg(feature = "opus-support")]
use super::OPUS_MAX_FRAME_SAMPLES;
use crate::Renderer;

pub struct CpalAudioRenderer {
    _stream: Stream,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    #[cfg(feature = "opus-support")]
    decoder: OpusDecoder,
    channels: usize,
    #[cfg(feature = "opus-support")]
    decode_buf: Vec<i16>,
}

unsafe impl Send for CpalAudioRenderer {}

impl CpalAudioRenderer {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No audio output device available"))?;

        let (config, sample_format) = select_output_config(&device)?;
        let channels = config.channels as usize;

        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(
            AUDIO_MAX_BUFFER_SAMPLES,
        )));
        let buffer_clone = buffer.clone();

        let err_fn = |err| {
            log::error!("cpal audio stream error: {}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => build_stream_f32(&device, &config, buffer_clone, err_fn)?,
            SampleFormat::I16 => build_stream_i16(&device, &config, buffer_clone, err_fn)?,
            SampleFormat::U16 => build_stream_u16(&device, &config, buffer_clone, err_fn)?,
            _ => return Err(anyhow!("Unsupported audio sample format")),
        };

        stream.play()?;

        #[cfg(feature = "opus-support")]
        {
            let decoder = OpusDecoder::new(OPUS_SAMPLE_RATE, Channels::Stereo)
                .map_err(|e| anyhow!("Opus decoder init failed: {}", e))?;

            Ok(Self {
                _stream: stream,
                buffer,
                decoder,
                channels,
                decode_buf: vec![0i16; OPUS_MAX_FRAME_SAMPLES * channels],
            })
        }
        #[cfg(not(feature = "opus-support"))]
        {
            Ok(Self {
                _stream: stream,
                buffer,
                channels,
            })
        }
    }

    #[cfg(feature = "opus-support")]
    pub fn push(&mut self, payload: &[u8]) -> Result<()> {
        let decoded = self
            .decoder
            .decode(payload, &mut self.decode_buf, false)
            .map_err(|e| anyhow!("Opus decode failed: {}", e))?;
        let decoded_samples = decoded * self.channels;

        let mut guard = match self.buffer.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        for sample in self.decode_buf.iter().take(decoded_samples) {
            let sample = *sample as f32 / i16::MAX as f32;
            guard.push_back(sample);
        }

        while guard.len() > AUDIO_MAX_BUFFER_SAMPLES {
            guard.pop_front();
        }

        Ok(())
    }

    #[cfg(not(feature = "opus-support"))]
    pub fn push(&mut self, _payload: &[u8]) -> Result<()> {
        // Opus disabled, no-op or implement alternate decoder
        Ok(())
    }
}

impl Renderer for CpalAudioRenderer {
    fn render(&mut self, payload: &[u8], _timestamp_us: u64) -> Result<()> {
        self.push(payload)
    }
}

fn select_output_config(device: &cpal::Device) -> Result<(StreamConfig, SampleFormat)> {
    let mut chosen: Option<(StreamConfig, SampleFormat)> = None;
    let configs = device
        .supported_output_configs()
        .map_err(|e| anyhow!("Audio configs error: {}", e))?;

    for cfg in configs {
        if cfg.channels() != OPUS_CHANNELS as u16 {
            continue;
        }
        let min = cfg.min_sample_rate().0;
        let max = cfg.max_sample_rate().0;
        if min <= OPUS_SAMPLE_RATE && max >= OPUS_SAMPLE_RATE {
            let mut config = cfg.with_sample_rate(SampleRate(OPUS_SAMPLE_RATE)).config();
            if let SupportedBufferSize::Range { min, max } = cfg.buffer_size() {
                let desired = OPUS_FRAME_SAMPLES as u32;
                let buffer = desired.clamp(*min, *max);
                config.buffer_size = BufferSize::Fixed(buffer);
            }
            chosen = Some((config, cfg.sample_format()));
            break;
        }
    }

    if let Some(cfg) = chosen {
        return Ok(cfg);
    }

    let fallback = device
        .default_output_config()
        .map_err(|e| anyhow!("Default audio config error: {}", e))?;
    Ok((fallback.config(), fallback.sample_format()))
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
            let mut guard = match buffer.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            for sample in data.iter_mut() {
                *sample = guard.pop_front().unwrap_or(0.0);
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let stream = device.build_output_stream(
        config,
        move |data: &mut [i16], _| {
            let mut guard = match buffer.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            for sample in data.iter_mut() {
                let value = guard.pop_front().unwrap_or(0.0);
                *sample = f32_to_i16(value);
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

fn build_stream_u16(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let stream = device.build_output_stream(
        config,
        move |data: &mut [u16], _| {
            let mut guard = match buffer.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            for sample in data.iter_mut() {
                let value = guard.pop_front().unwrap_or(0.0);
                *sample = f32_to_u16(value);
            }
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

pub(crate) fn f32_to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn f32_to_u16(value: f32) -> u16 {
    let scaled = value.clamp(-1.0, 1.0) * 0.5 + 0.5;
    (scaled * u16::MAX as f32) as u16
}
