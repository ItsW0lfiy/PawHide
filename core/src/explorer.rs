//! Query Explorer's real folder path through ShellWindows COM, not its title.
//! This is specific to Windows file-manager windows, not arbitrary documents.
#![cfg(windows)]
use std::{collections::HashMap, path::PathBuf};
use anyhow::{Result, Context};
use windows::{
    core::Interface,
    Win32::{
        System::{Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED}, Variant::VARIANT},
        UI::Shell::{IShellWindows, IWebBrowser2, ShellWindows},
    },
};

/// Returns a map from top-level HWND to actual filesystem location. Modern
/// Explorer tabs can share HWNDs; the whole window is excluded when *any*
/// registered tab has a protected location.
pub fn open_folder_windows() -> Result<HashMap<usize, Vec<PathBuf>>> {
    let mut found = HashMap::new();
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        hr.ok().context("could not initialize Explorer COM apartment")?;
        struct ComGuard;
        impl Drop for ComGuard { fn drop(&mut self) { unsafe { CoUninitialize() }; } }
        let _guard = ComGuard;
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)
            .context("cannot enumerate Explorer folder windows")?;
        for idx in 0..shell.Count()?.max(0) {
            // Construct an owned VT_I4 index without directly mutating
            // ManuallyDrop union fields (rejected by current Rust nightly).
            let index = VARIANT::from(idx);
            let Ok(item) = shell.Item(&index) else { continue; };
            let Ok(browser) = item.cast::<IWebBrowser2>() else { continue; };
            let Ok(hwnd) = browser.HWND() else { continue; };
            if hwnd.0 == 0 { continue; }
            let Ok(url) = browser.LocationURL() else { continue; };
            let Ok(uri) = url::Url::parse(&url.to_string()) else { continue; };
            if uri.scheme() != "file" { continue; }
            if let Ok(path) = uri.to_file_path() { found.entry(hwnd.0 as usize).or_insert_with(Vec::new).push(path); }
        }
    }
    Ok(found)
}
