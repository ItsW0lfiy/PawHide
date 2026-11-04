//! Query Explorer's real folder path through ShellWindows COM, not its title.
//! This is specific to Windows file-manager windows, not arbitrary documents.
#![cfg(windows)]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use windows::{
    Win32::{
        System::{
            Com::{
                CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Variant::VARIANT,
        },
        UI::Shell::{IShellWindows, IWebBrowser2, ShellWindows},
    },
    core::Interface,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GA_ROOT, GetAncestor, GetClassNameW,
};

/// Returns a map from top-level HWND to actual filesystem location. Modern
/// Explorer tabs can share HWNDs; the whole window is excluded when *any*
/// registered tab has a protected location.
#[derive(Default, Serialize, Deserialize)]
pub struct FolderLocations {
    pub paths: Vec<PathBuf>,
    pub uncertain: bool,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub windows: HashMap<usize, FolderLocations>,
    pub incomplete: bool,
}

pub fn open_folder_windows() -> Result<Snapshot> {
    let mut found = Snapshot::default();
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        hr.ok()
            .context("could not initialize Explorer COM apartment")?;
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        let _guard = ComGuard;
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)
            .context("cannot enumerate Explorer folder windows")?;
        for idx in 0..shell.Count()?.max(0) {
            // Construct an owned VT_I4 index without directly mutating
            // ManuallyDrop union fields (rejected by current Rust nightly).
            let index = VARIANT::from(idx);
            let Ok(item) = shell.Item(&index) else {
                found.incomplete = true;
                continue;
            };
            let Ok(browser) = item.cast::<IWebBrowser2>() else {
                found.incomplete = true;
                continue;
            };
            let Ok(hwnd) = browser.HWND() else {
                found.incomplete = true;
                continue;
            };
            if hwnd.0 == 0 {
                continue;
            }
            let root = GetAncestor(hwnd.0 as _, GA_ROOT) as usize;
            if root == 0 {
                found.incomplete = true;
                continue;
            }
            let entry = found.windows.entry(root).or_default();
            let path = browser
                .LocationURL()
                .ok()
                .and_then(|url| url::Url::parse(&url.to_string()).ok())
                .and_then(|url| url.to_file_path().ok());
            if let Some(path) = path {
                entry.paths.push(path);
            } else {
                entry.uncertain = true;
            }
        }
    }
    // ShellWindows can omit inactive Windows 11 tabs. Never restore an HWND
    // when more native tab hosts exist than the returned locations account for.
    unsafe extern "system" fn count_tabs(hwnd: *mut core::ffi::c_void, data: isize) -> i32 {
        let mut class = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        if len > 0 && String::from_utf16_lossy(&class[..len as usize]) == "ShellTabWindowClass" {
            unsafe {
                *(data as *mut usize) += 1;
            }
        }
        1
    }
    for (&hwnd, locations) in &mut found.windows {
        let mut tabs = 0usize;
        unsafe {
            EnumChildWindows(hwnd as _, Some(count_tabs), &mut tabs as *mut _ as isize);
        }
        locations.uncertain |= tabs > locations.paths.len();
    }
    Ok(found)
}
