use crate::audio::renderer::CpalAudioRenderer;
use crate::Renderer;
use anyhow::Result;

pub struct MacAudioRenderer {
    inner: CpalAudioRenderer,
}

unsafe impl Send for MacAudioRenderer {}

impl MacAudioRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: CpalAudioRenderer::new()?,
        })
    }

    pub fn push(&mut self, payload: &[u8]) -> Result<()> {
        self.inner.push(payload)
    }
}

impl Renderer for MacAudioRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        self.inner.render(payload, timestamp_us)
    }
}
