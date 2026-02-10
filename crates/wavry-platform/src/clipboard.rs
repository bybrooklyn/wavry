use crate::Clipboard;
use anyhow::{anyhow, Result};

pub struct ArboardClipboard {
    #[cfg(not(target_os = "android"))]
    inner: arboard::Clipboard,
}

impl ArboardClipboard {
    pub fn new() -> Result<Self> {
        #[cfg(not(target_os = "android"))]
        {
            let inner = arboard::Clipboard::new()
                .map_err(|e| anyhow!("Failed to initialize arboard: {}", e))?;
            Ok(Self { inner })
        }
        #[cfg(target_os = "android")]
        {
            Err(anyhow!("Clipboard not supported on Android"))
        }
    }
}

impl Clipboard for ArboardClipboard {
    fn get_text(&mut self) -> Result<Option<String>> {
        #[cfg(not(target_os = "android"))]
        {
            match self.inner.get_text() {
                Ok(text) => Ok(Some(text)),
                Err(arboard::Error::ContentNotAvailable) => Ok(None),
                Err(e) => Err(anyhow!("Clipboard get_text failed: {}", e)),
            }
        }
        #[cfg(target_os = "android")]
        Ok(None)
    }

    fn set_text(&mut self, text: String) -> Result<()> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner
                .set_text(text)
                .map_err(|e| anyhow!("Clipboard set_text failed: {}", e))
        }
        #[cfg(target_os = "android")]
        {
            let _ = text;
            Ok(())
        }
    }
}
