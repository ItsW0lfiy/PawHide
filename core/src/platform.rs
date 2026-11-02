use crate::parse_hwnd;
use anyhow::{Context, Result, bail, ensure};
use dll_syringe::{Syringe, process::OwnedProcess};
use std::{ffi::c_void, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION},
    UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetShellWindow, GetWindowDisplayAffinity,
        GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindow, IsWindowVisible, GA_ROOT, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    },
};

pub const EXCLUDED: u32 = 0x11;

#[derive(Clone, Debug)]
pub struct Window {
    pub hwnd: usize,
    pub pid: u32,
    pub exe: String,
    pub title: String,
    pub affinity: Option<u32>,
    pub tool_window: bool,
}

fn handle(hwnd: usize) -> *mut c_void { hwnd as *mut c_void }

fn process_name(pid: u32) -> String {
    unsafe {
        let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() { return format!("pid:{pid}"); }
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        let success = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if success == 0 { return format!("pid:{pid}"); }
        let image = String::from_utf16_lossy(&buf[..len as usize]);
        image.rsplit(['\\', '/']).next().unwrap_or(&image).to_string()
    }
}

fn affinity(hwnd: usize) -> Option<u32> {
    let mut value = 0u32;
    if unsafe { GetWindowDisplayAffinity(handle(hwnd), &mut value) } == 0 { None }
    else { Some(value) }
}

unsafe extern "system" fn enumerate(hwnd: *mut c_void, data: isize) -> i32 {
    let out = unsafe { &mut *(data as *mut Vec<Window>) };
    if unsafe { IsWindowVisible(hwnd) } == 0 || unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd { return 1; }
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 || len > 8192 { return 1; }
    let mut buf = vec![0u16; len as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if copied <= 0 { return 1; }
    let title = String::from_utf16_lossy(&buf[..copied as usize]).replace(['\r', '\n', '\t'], " ");
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 { return 1; }
    let tool_window = ((unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32) & WS_EX_TOOLWINDOW) != 0;
    out.push(Window { hwnd: hwnd as usize, pid, exe: process_name(pid), title, affinity: affinity(hwnd as usize), tool_window });
    1
}

pub fn windows() -> Result<Vec<Window>> {
    let mut result = Vec::<Window>::new();
    let ok = unsafe { EnumWindows(Some(enumerate), &mut result as *mut _ as isize) };
    ensure!(ok != 0, "Windows could not enumerate desktop windows");
    Ok(result)
}

pub fn resolve(target: &str) -> Result<Vec<Window>> {
    let all = windows()?;
    if let Some(hwnd) = parse_hwnd(target)? {
        let found = all.into_iter().find(|w| w.hwnd == hwnd);
        return Ok(found.into_iter().collect());
    }
    // For manual targeting, Explorer is permitted only with explicit --allow-shell.
    let name = target.trim().to_ascii_lowercase();
    ensure!(!name.contains(['\\', '/', ':', '*', '?']), "pass an .exe name (PicView.exe) or 0x window handle");
    let name = if name.ends_with(".exe") { name } else { format!("{name}.exe") };
    Ok(all.into_iter().filter(|w| w.exe.eq_ignore_ascii_case(&name)).collect())
}

pub fn state(a: Option<u32>) -> &'static str {
    match a { Some(EXCLUDED) => "Excluded", Some(1) => "Blacked", Some(0) => "Visible", _ => "Unknown" }
}

pub fn shell_pid() -> u32 {
    let h = unsafe { GetShellWindow() };
    let mut pid = 0u32;
    if !h.is_null() { unsafe { GetWindowThreadProcessId(h, &mut pid) }; }
    pid
}

fn agent_path() -> Result<PathBuf> {
    let path = std::env::current_exe()?.with_file_name("pawhide_agent.dll");
    ensure!(path.is_file(), "pawhide_agent.dll is missing beside pawhide.exe; build the complete workspace");
    Ok(path)
}

/// Returns true if the OS-reported affinity was successfully read back.
/// A successful injector call without readback is never called verified.
pub fn apply_window(w: &Window, protect: bool, allow_shell: bool) -> Result<bool> {
    ensure!(unsafe { IsWindow(handle(w.hwnd)) } != 0, "window disappeared; try again");
    let mut live_pid = 0u32;
    unsafe { GetWindowThreadProcessId(handle(w.hwnd), &mut live_pid) };
    ensure!(live_pid == w.pid, "window owner changed; refusing to target stale HWND");
    let expected = if protect { EXCLUDED } else { 0 };
    if affinity(w.hwnd) == Some(expected) { return Ok(true); }
    ensure!(w.pid != unsafe { GetCurrentProcessId() }, "cannot inject into PawHide itself");
    if w.pid == shell_pid() && !allow_shell {
        bail!("Windows shell protection is blocked (Explorer shares a process with desktop/taskbar). Select a specific handle and --allow-shell for experimental testing");
    }
    let process = OwnedProcess::from_pid(w.pid)
        .with_context(|| format!("cannot access {} (PID {}): privileges, architecture or security restrictions", w.exe, w.pid))?;
    let syringe = Syringe::for_process(process);
    let agent = syringe.find_or_inject(agent_path()?)
        .with_context(|| format!("could not load agent into {}; not protected", w.exe))?;
    let proc = unsafe {
        syringe.get_raw_procedure::<extern "system" fn(usize, u32) -> u32>(agent, "PawhideApply")
    }.context("failed to resolve agent procedure")?.context("agent export PawhideApply not found")?;
    let value = proc.call(w.hwnd, expected).context("agent procedure failed")?;
    ensure!(value == 0, "SetWindowDisplayAffinity returned 0x{value:08X}");
    match affinity(w.hwnd) {
        Some(a) if a == expected => Ok(true),
        Some(a) => bail!("affinity mismatch (requested 0x{expected:X}, received 0x{a:X})"),
        None => Ok(false),
    }
}

