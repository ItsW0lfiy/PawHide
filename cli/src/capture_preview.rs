//! One optional Windows Graphics Capture preview, launched directly by pawhide.exe.
#![cfg(windows)]
use crate::{capture_native::{CaptureSession, PreviewFrame, TARGET_FPS}, presentation as display};
use anyhow::{Context, Result, bail, ensure};
use pawhide_core::config;
use std::{
    ffi::c_void, mem::size_of, ptr,
    sync::{Mutex, OnceLock}, thread, time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BLACK_BRUSH, DIB_RGB_COLORS, EnumDisplayMonitors, GetMonitorInfoW,
        GetStockObject, HDC, HMONITOR, MONITORINFOEXW, SRCCOPY, StretchDIBits, BeginPaint,
        EndPaint, FillRect, InvalidateRect, PAINTSTRUCT,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GetClientRect, LoadCursorW, PeekMessageW,
        PostQuitMessage, RegisterClassW, SetWindowDisplayAffinity, SetWindowPos,
        SetWindowTextW, TranslateMessage, WNDCLASSW, CW_USEDEFAULT, IDC_ARROW, MSG,
        PM_REMOVE, SWP_NOMOVE, SWP_NOSIZE, WM_DESTROY, WM_KEYDOWN, WM_PAINT,
        WM_QUIT, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    },
};
const EXCLUDED: u32 = 0x11;
static FRAME: OnceLock<Mutex<Option<PreviewFrame>>> = OnceLock::new();

#[derive(Clone)]
struct Monitor {
    handle: usize, name: String, width: i32, height: i32, primary: bool,
}
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn scan_monitors() -> Result<Vec<Monitor>> {
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, param: LPARAM) -> i32 {
        // SAFETY: EnumDisplayMonitors invokes this callback synchronously.
        let found = unsafe { &mut *(param as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 { return 1; }
        let end = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        let r = info.monitorInfo.rcMonitor;
        found.push(Monitor {
            handle: monitor as usize,
            name: String::from_utf16_lossy(&info.szDevice[..end]),
            width: r.right - r.left, height: r.bottom - r.top,
            primary: info.monitorInfo.dwFlags & 1 != 0,
        });
        1
    }
    let mut found: Vec<Monitor> = Vec::new();
    ensure!(unsafe {
        EnumDisplayMonitors(ptr::null_mut(), ptr::null_mut(), Some(collect), &mut found as *mut _ as LPARAM)
    } != 0, "Windows could not enumerate displays");
    found.sort_by(|a, b| b.primary.cmp(&a.primary).then(a.name.cmp(&b.name)));
    ensure!(!found.is_empty(), "No displays detected");
    Ok(found)
}
pub fn list_monitors() -> Result<()> {
    display::header("CAPTURE MONITORS", "Windows Graphics Capture sources");
    for (i, m) in scan_monitors()?.iter().enumerate() {
        println!("  {:>2}  {:<16} {:>5} x {:<5} {}", i + 1, m.name, m.width, m.height,
            if m.primary { display::good("PRIMARY") } else { String::new() });
    }
    display::note("Run: pawhide capture preview --monitor NUMBER");
    Ok(())
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut paint) };
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            unsafe { FillRect(dc, &rect, GetStockObject(BLACK_BRUSH) as _) };
            if let Some(slot) = FRAME.get()
                && let Ok(frame) = slot.lock()
                && let Some(f) = frame.as_ref()
                && f.width > 0 && f.height > 0
            {
                let cw = (rect.right - rect.left).max(1);
                let ch = (rect.bottom - rect.top).max(1);
                let fw = f.width as i32;
                let fh = f.height as i32;
                let (dw, dh) = if (cw as i64 * fh as i64) <= (ch as i64 * fw as i64) {
                    (cw, (cw as i64 * fh as i64 / fw as i64) as i32)
                } else {
                    ((ch as i64 * fw as i64 / fh as i64) as i32, ch)
                };
                let mut bitmap = BITMAPINFO::default();
                bitmap.bmiHeader.biSize = size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
                bitmap.bmiHeader.biWidth = fw;
                bitmap.bmiHeader.biHeight = -fh;
                bitmap.bmiHeader.biPlanes = 1;
                bitmap.bmiHeader.biBitCount = 32;
                bitmap.bmiHeader.biCompression = BI_RGB;
                unsafe {
                    StretchDIBits(dc, (cw - dw) / 2, (ch - dh) / 2, dw, dh,
                        0, 0, fw, fh, f.rgba.as_ptr() as *const c_void,
                        &bitmap, DIB_RGB_COLORS, SRCCOPY);
                }
            }
            unsafe { EndPaint(hwnd, &paint) };
            0
        }
        WM_DESTROY => { unsafe { PostQuitMessage(0) }; 0 }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn create_preview(thumbnail: bool) -> Result<HWND> {
    let class = wide("PawHideCapturePreview");
    let title = wide("PawHide - Live Capture Preview");
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    ensure!(!instance.is_null(), "Could not locate PawHide module");
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc), hInstance: instance,
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        lpszClassName: class.as_ptr(), ..Default::default()
    };
    ensure!(unsafe { RegisterClassW(&window_class) } != 0, "Could not register preview window");
    let hwnd = unsafe {
        CreateWindowExW(if thumbnail { WS_EX_TOPMOST } else { 0 },
            class.as_ptr(), title.as_ptr(), WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT,
            if thumbnail { 470 } else { 1050 }, if thumbnail { 320 } else { 660 },
            ptr::null_mut(), ptr::null_mut(), instance, ptr::null())
    };
    ensure!(!hwnd.is_null(), "Could not create preview window");
    // Avoid recursion: hide only our own preview from compatible capture.
    if unsafe { SetWindowDisplayAffinity(hwnd, EXCLUDED) } == 0 {
        eprintln!("{}", display::warn("Warning: Self-exclusion failed; the preview might capture itself."));
    }
    Ok(hwnd)
}
fn caption(m: &Monitor, source: f64, processed: f64, shown: f64) -> String {
    format!("PawHide Preview | {} {}x{} | source {:.0} - processed {:.0} - preview {:.0} FPS (target {})",
        m.name, m.width, m.height, source, processed, shown, TARGET_FPS)
}

pub fn preview(
    requested: Option<usize>, cursor_override: Option<bool>,
    native_override: Option<bool>, thumbnail_override: Option<bool>,
) -> Result<()> {
    let cfg = config::load()?;
    let mut cursor = cursor_override.unwrap_or(cfg.preferences.capture_cursor);
    let mut native = native_override.unwrap_or(cfg.preferences.capture_native_resolution);
    let mut topmost = thumbnail_override.unwrap_or(cfg.preferences.capture_thumbnail);
    let mut monitors = scan_monitors()?;
    let mut selected = requested.unwrap_or(1);
    ensure!(selected > 0 && selected <= monitors.len(),
        "Monitor {} is unavailable; run pawhide capture monitors", selected);
    let mut session = Some(CaptureSession::start(monitors[selected - 1].handle, cursor, native));
    FRAME.get_or_init(|| Mutex::new(None));
    let hwnd = create_preview(topmost)?;
    display::header("LIVE CAPTURE PREVIEW", "Windows Graphics Capture, not a Discord capture test");
    println!("  Source   {} ({} x {})", monitors[selected - 1].name,
        monitors[selected - 1].width, monitors[selected - 1].height);
    println!("  Options  cursor={}  native={}  topmost={}", cursor, native, topmost);
    display::note("Preview keys: M next monitor | C cursor | N native/scaled | T topmost | P pause/resume | R restart | Esc close");
    display::note("Change protection in another terminal, then inspect the captured window here.");
    let mut last = Instant::now();
    let mut last_counts = (0_u64, 0_u64);
    let mut shown = 0_u64;
    let mut last_frame = Instant::now();
    let mut received_frame = false;
    loop {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if msg.message == WM_QUIT { return Ok(()); }
            if msg.message == WM_KEYDOWN && msg.hwnd == hwnd {
                match msg.wParam as u32 {
                    0x1B => { unsafe { DestroyWindow(hwnd); } },
                    0x43 => {
                        cursor = !cursor;
                        if let Some(active) = &session { active.set_cursor(cursor); }
                        println!("Cursor capture: {}", cursor);
                    }
                    0x4E => {
                        native = !native;
                        if session.is_some() {
                            session = Some(CaptureSession::start(monitors[selected - 1].handle, cursor, native));
                            received_frame = false;
                            last_frame = Instant::now();
                        }
                        println!("Native resolution: {}", native);
                    }
                    0x4D => {
                        monitors = scan_monitors()?;
                        selected = selected % monitors.len() + 1;
                        if session.is_some() {
                            session = Some(CaptureSession::start(monitors[selected - 1].handle, cursor, native));
                            received_frame = false;
                            last_frame = Instant::now();
                        }
                        println!("Monitor: {} ({})", monitors[selected - 1].name, selected);
                    }
                    0x50 => {
                        if session.is_some() {
                            session = None;
                            println!("Preview paused; source capture stopped.");
                        } else {
                            session = Some(CaptureSession::start(monitors[selected - 1].handle, cursor, native));
                            last_frame = Instant::now();
                            received_frame = false;
                            println!("Preview resumed.");
                        }
                    }
                    0x52 => {
                        session = Some(CaptureSession::start(monitors[selected - 1].handle, cursor, native));
                        last_frame = Instant::now();
                        received_frame = false;
                        println!("Preview restarted");
                    }
                    0x54 => {
                        topmost = !topmost;
                        unsafe { SetWindowPos(hwnd, if topmost { -1_isize as HWND } else { -2_isize as HWND },
                            0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE) };
                        println!("Always on top: {}", topmost);
                    }
                    _ => {}
                }
            }
            unsafe { TranslateMessage(&msg); DispatchMessageW(&msg) };
        }
        if let Some(mut frame) = session.as_ref().and_then(CaptureSession::take_latest) {
            // Native Windows DIBs use BGRA, capture frames are RGBA.
            for p in frame.rgba.as_chunks_mut::<4>().0 { p.swap(0, 2); }
            let mut guard = FRAME.get().context("No frame buffer")?.lock()
                .map_err(|_| anyhow::anyhow!("Frame buffer unavailable"))?;
            if let Some(mut old) = guard.replace(frame) {
                for p in old.rgba.as_chunks_mut::<4>().0 { p.swap(0, 2); }
                if let Some(active) = &session { active.recycle(old); }
            }
            drop(guard);
            unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
            shown += 1;
            last_frame = Instant::now();
            received_frame = true;
        }
        if let Some(error) = session.as_ref().and_then(CaptureSession::error) {
            bail!("Capture worker failed: {}", error);
        }
        if last.elapsed() >= Duration::from_secs(1) {
            let (acquired, processed, _) = session.as_ref().map(CaptureSession::counts).unwrap_or((0, 0, 0));
            let elapsed = last.elapsed().as_secs_f64().max(0.001);
            let title = if session.is_some() {
                caption(&monitors[selected - 1],
                    acquired.saturating_sub(last_counts.0) as f64 / elapsed,
                    processed.saturating_sub(last_counts.1) as f64 / elapsed,
                    shown as f64 / elapsed)
            } else {
                format!("PawHide Preview - PAUSED - {}", monitors[selected - 1].name)
            };
            unsafe { SetWindowTextW(hwnd, wide(&title).as_ptr()) };
            last_counts = (acquired, processed);
            shown = 0;
            last = Instant::now();
        }
        // A static desktop may legitimately stop producing changed frames.
        // Time out only before the first frame of each new capture session.
        if session.is_some() && !received_frame && last_frame.elapsed() > Duration::from_secs(15) {
            bail!("No capture frames received for 15 seconds. Check display selection and capture permissions.");
        }
        thread::sleep(Duration::from_millis(12));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captions_report_capture_source() {
        let m = Monitor { handle: 0, name: "DISPLAY1".into(), width: 1920, height: 1080, primary: true };
        assert!(caption(&m, 60.0, 59.0, 58.0).contains("59"));
    }
}
