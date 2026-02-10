use crate::Clipboard;
use anyhow::{anyhow, Result};
use arboard::Clipboard as Arboard;

pub struct ArboardClipboard {
    inner: Arboard,
}

impl ArboardClipboard {
    pub fn new() -> Result<Self> {
        let inner = Arboard::new().map_err(|e| anyhow!("Failed to initialize arboard: {}", e))?;
        Ok(Self { inner })
    }
}

impl Clipboard for ArboardClipboard {
    fn get_text(&mut self) -> Result<Option<String>> {
        match self.inner.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(anyhow!("Clipboard get_text failed: {}", e)),
        }
    }

    fn set_text(&mut self, text: String) -> Result<()> {
        self.inner
            .set_text(text)
            .map_err(|e| anyhow!("Clipboard set_text failed: {}", e))
    }
}
