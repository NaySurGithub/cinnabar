//! Bounded native clipboard adapter for chat and text fields.

#[cfg(target_os = "android")]
mod android;

use std::sync::Arc;

use ui::ChatClipboard;

#[derive(Default)]
pub struct PlatformClipboard;

#[derive(Debug, thiserror::Error)]
pub enum PlatformClipboardError {
    #[error("platform clipboard failed: {0}")]
    #[cfg(not(target_os = "android"))]
    Platform(#[from] arboard::Error),
    #[error("platform clipboard failed: {0}")]
    #[cfg(target_os = "android")]
    Platform(String),
    #[error("clipboard text exceeds the {maximum}-byte chat insertion bound")]
    TooLong { maximum: usize },
}

impl ChatClipboard for PlatformClipboard {
    type Error = PlatformClipboardError;

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, Self::Error> {
        #[cfg(not(target_os = "android"))]
        let text = Some(arboard::Clipboard::new()?.get_text()?);
        #[cfg(target_os = "android")]
        let text = android::read_text().map_err(PlatformClipboardError::Platform)?;
        let Some(text) = text else {
            return Ok(None);
        };
        if text.len() > maximum_bytes {
            return Err(PlatformClipboardError::TooLong {
                maximum: maximum_bytes,
            });
        }
        Ok(Some(Arc::from(text)))
    }
}

impl PlatformClipboard {
    pub fn write_text(&mut self, text: String) -> Result<(), PlatformClipboardError> {
        #[cfg(not(target_os = "android"))]
        arboard::Clipboard::new()?.set_text(text)?;
        #[cfg(target_os = "android")]
        android::write_text(&text).map_err(PlatformClipboardError::Platform)?;
        Ok(())
    }
}
