use crate::audio::CpalAudioRenderer;
use crate::Renderer;
use anyhow::Result;

pub struct AndroidAudioRenderer {
    inner: CpalAudioRenderer,
}

unsafe impl Send for AndroidAudioRenderer {}

impl AndroidAudioRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: CpalAudioRenderer::new()?,
        })
    }
}

impl Renderer for AndroidAudioRenderer {
    fn render(&mut self, payload: &[u8], timestamp_us: u64) -> Result<()> {
        self.inner.render(payload, timestamp_us)
    }
}
