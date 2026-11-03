//! Reusable, Windows-focused PawHide functionality shared by CLI and GUI.
//! Capture exclusion is best-effort, NOT a security boundary against all capture methods.
pub mod config;
#[cfg(windows)]
pub mod explorer;
pub mod folders;
#[cfg(windows)]
pub mod helper;
#[cfg(windows)]
pub mod monitor;
pub mod ownership;
#[cfg(windows)]
pub mod platform;

use anyhow::{Result, ensure};

/// Only a literal hexadecimal `0x...` is interpreted as a window handle.
pub fn parse_hwnd(input: &str) -> Result<Option<usize>> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        let value = usize::from_str_radix(hex, 16)?;
        ensure!(value != 0, "window handle cannot be zero");
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_hex_handles() {
        assert_eq!(parse_hwnd("0x10").unwrap(), Some(16));
        assert!(parse_hwnd("0x0").is_err());
        assert_eq!(parse_hwnd("PicView.exe").unwrap(), None);
    }
}
