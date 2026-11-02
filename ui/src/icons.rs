//! Extract the *actual* icon of each running application, not a generic paw icon.
//! All Shell/GDI work happens on a worker thread (SHGetFileInfo can block).
#![cfg(windows)]

use std::{ffi::c_void, mem::{size_of, zeroed}, ptr};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
        BITMAPINFO, BI_RGB, DIB_RGB_COLORS,
    },
    System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION},
    UI::{Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON},
         WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL}},
};

pub struct IconPixels {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

fn executable_path(pid: u32) -> Option<Vec<u16>> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return None; }
        let mut buffer = vec![0_u16; 32768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(handle);
        if ok == 0 || length == 0 { return None; }
        buffer.truncate(length as usize);
        buffer.push(0);
        Some(buffer)
    }
}

/// None is a normal outcome for protected or inaccessible system processes.
pub fn from_pid(pid: u32) -> Option<IconPixels> {
    let path = executable_path(pid)?;
    unsafe {
        let mut info: SHFILEINFOW = zeroed();
        if SHGetFileInfoW(path.as_ptr(), 0, &mut info, size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON) == 0 || info.hIcon.is_null() {
            return None;
        }
        let icon = info.hIcon;
        const SIZE: usize = 48;
        let dc = CreateCompatibleDC(ptr::null_mut());
        if dc.is_null() { DestroyIcon(icon); return None; }
        let mut header: BITMAPINFO = zeroed();
        header.bmiHeader.biSize = size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
        header.bmiHeader.biWidth = SIZE as i32;
        header.bmiHeader.biHeight = -(SIZE as i32); // top-down BGRA pixel order
        header.bmiHeader.biPlanes = 1;
        header.bmiHeader.biBitCount = 32;
        header.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &header, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() { DeleteObject(bitmap as _); }
            DeleteDC(dc); DestroyIcon(icon); return None;
        }
        // Transparent areas must start zeroed; GDI may otherwise blend with garbage.
        std::ptr::write_bytes(bits as *mut u8, 0, SIZE * SIZE * 4);
        let previous = SelectObject(dc, bitmap as _);
        let drawn = DrawIconEx(dc, 0, 0, icon, SIZE as i32, SIZE as i32, 0, ptr::null_mut(), DI_NORMAL);
        let mut rgba = vec![0_u8; SIZE * SIZE * 4];
        if drawn != 0 {
            let src = std::slice::from_raw_parts(bits as *const u8, rgba.len());
            for (from, to) in src.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
                let alpha = from[3];
                // GDI writes premultiplied BGRA. Convert to straight RGBA for egui.
                let channel = |value: u8| if alpha == 0 { 0 } else { ((value as u32 * 255) / alpha as u32).min(255) as u8 };
                to.copy_from_slice(&[channel(from[2]), channel(from[1]), channel(from[0]), alpha]);
            }
        }
        if !previous.is_null() { SelectObject(dc, previous); }
        DeleteObject(bitmap as _);
        DeleteDC(dc);
        DestroyIcon(icon);
        if drawn == 0 || rgba.chunks_exact(4).all(|p| p[3] == 0) { None }
        else { Some(IconPixels { rgba, width: SIZE, height: SIZE }) }
    }
}
