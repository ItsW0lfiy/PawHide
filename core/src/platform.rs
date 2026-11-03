use crate::parse_hwnd;
use anyhow::{Context, Result, bail, ensure};
use dll_syringe::{
    Syringe,
    process::{OwnedProcess, Process},
};
use std::{ffi::c_void, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GA_ROOT, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetPropW, GetShellWindow,
        GetWindowDisplayAffinity, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindow, IsWindowVisible, WS_EX_TOOLWINDOW,
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

fn handle(hwnd: usize) -> *mut c_void {
    hwnd as *mut c_void
}

fn process_name(pid: u32) -> String {
    unsafe {
        let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return format!("pid:{pid}");
        }
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        let success = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if success == 0 {
            return format!("pid:{pid}");
        }
        let image = String::from_utf16_lossy(&buf[..len as usize]);
        image
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&image)
            .to_string()
    }
}

fn affinity(hwnd: usize) -> Option<u32> {
    let mut value = 0u32;
    if unsafe { GetWindowDisplayAffinity(handle(hwnd), &mut value) } == 0 {
        None
    } else {
        Some(value)
    }
}

unsafe extern "system" fn enumerate(hwnd: *mut c_void, data: isize) -> i32 {
    let out = unsafe { &mut *(data as *mut Vec<Window>) };
    if unsafe { IsWindowVisible(hwnd) } == 0 || unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
        return 1;
    }
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 || len > 8192 {
        return 1;
    }
    let mut buf = vec![0u16; len as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if copied <= 0 {
        return 1;
    }
    let title = String::from_utf16_lossy(&buf[..copied as usize]).replace(['\r', '\n', '\t'], " ");
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return 1;
    }
    let tool_window =
        ((unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32) & WS_EX_TOOLWINDOW) != 0;
    out.push(Window {
        hwnd: hwnd as usize,
        pid,
        exe: process_name(pid),
        title,
        affinity: affinity(hwnd as usize),
        tool_window,
    });
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
    ensure!(
        !name.contains(['\\', '/', ':', '*', '?']),
        "pass an .exe name (PicView.exe) or 0x window handle"
    );
    let name = if name.ends_with(".exe") {
        name
    } else {
        format!("{name}.exe")
    };
    Ok(all
        .into_iter()
        .filter(|w| w.exe.eq_ignore_ascii_case(&name))
        .collect())
}

pub fn state(a: Option<u32>) -> &'static str {
    match a {
        Some(EXCLUDED) => "Excluded",
        Some(1) => "Blacked",
        Some(0) => "Visible",
        _ => "Unknown",
    }
}

pub fn shell_pid() -> u32 {
    let h = unsafe { GetShellWindow() };
    let mut pid = 0u32;
    if !h.is_null() {
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
    }
    pid
}

/// Explorer may host the desktop and file windows in the same process.
/// Only real folder-window classes are eligible. Never target taskbar/desktop.
pub fn is_explorer_folder(w: &Window) -> bool {
    if !w.exe.eq_ignore_ascii_case("explorer.exe") {
        return false;
    }
    let mut name = [0u16; 128];
    let n = unsafe { GetClassNameW(handle(w.hwnd), name.as_mut_ptr(), name.len() as i32) };
    if n == 0 {
        return false;
    }
    matches!(
        String::from_utf16_lossy(&name[..n as usize]).as_str(),
        "CabinetWClass" | "ExploreWClass"
    )
}

pub fn can_protect_window(w: &Window) -> bool {
    // The GUI delegates injection to pawhide.exe, so its own window IS eligible.
    // apply_window still rejects injecting into the calling process itself.
    if unsupported_reason(&w.exe).is_some() {
        return false;
    }
    (!w.exe.eq_ignore_ascii_case("explorer.exe") && w.pid != shell_pid()) || is_explorer_folder(w)
}

pub fn unsupported_reason(exe: &str) -> Option<&'static str> {
    let exe = exe.to_ascii_lowercase();
    if exe.starts_with("roblox") {
        return Some("Roblox injection is unsupported; anti-tamper is never bypassed");
    }
    if matches!(
        exe.as_str(),
        "discord.exe" | "discordptb.exe" | "discordcanary.exe" | "discorddevelopment.exe"
    ) {
        return Some("PawHide never injects into Discord; protect the source application instead");
    }
    if exe.starts_with("pid:") {
        return Some("process identity could not be verified; access may be restricted");
    }
    None
}

fn agent_path() -> Result<PathBuf> {
    let path = std::env::current_exe()?.with_file_name("pawhide_agent.dll");
    ensure!(
        path.is_file(),
        "pawhide_agent.dll is missing beside pawhide.exe; build the complete workspace"
    );
    Ok(path)
}

/// Returns true if the OS-reported affinity was successfully read back.
/// A successful injector call without readback is never called verified.
pub fn apply_window(w: &Window, protect: bool, allow_shell: bool) -> Result<bool> {
    apply_owned(w, protect, allow_shell, false)
}

pub fn automatic_original(hwnd: usize) -> Option<u32> {
    let name: Vec<u16> = crate::ownership::AUTO_PROPERTY
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let value = unsafe { GetPropW(handle(hwnd), name.as_ptr()) } as usize;
    value.checked_sub(1).map(|v| v as u32)
}

pub fn apply_owned(w: &Window, protect: bool, allow_shell: bool, automatic: bool) -> Result<bool> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| apply_owned_inner(w, protect, allow_shell, automatic)))
        .unwrap_or_else(|panic| {
            let message = panic.downcast_ref::<String>().map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied()).unwrap_or("unknown injector failure");
            if message.contains("injected_module_handle.is_null()") {
                bail!("target loader returned no agent module and no usable Windows error; DLL injection failed, exclusion NOT verified (dll-syringe null-module assertion)")
            }
            bail!("injector failed internally; affinity NOT verified: {message}")
        })
}

fn apply_owned_inner(
    w: &Window,
    protect: bool,
    allow_shell: bool,
    automatic: bool,
) -> Result<bool> {
    ensure!(
        unsafe { IsWindow(handle(w.hwnd)) } != 0,
        "window disappeared; try again"
    );
    let mut live_pid = 0u32;
    unsafe { GetWindowThreadProcessId(handle(w.hwnd), &mut live_pid) };
    ensure!(
        live_pid == w.pid,
        "window owner changed; refusing to target stale HWND"
    );
    let expected = if protect {
        EXCLUDED
    } else if automatic {
        automatic_original(w.hwnd).unwrap_or(affinity(w.hwnd).unwrap_or(0))
    } else {
        0
    };
    ensure!(
        w.pid != unsafe { GetCurrentProcessId() },
        "cannot inject into PawHide itself"
    );
    // Roblox is protected by anti-tamper. Do not attempt to bypass or inject.
    if let Some(reason) = unsupported_reason(&w.exe) {
        bail!("{reason}");
    }
    ensure!(
        process_name(w.pid).eq_ignore_ascii_case(&w.exe),
        "process identity changed or is inaccessible"
    );
    if (w.pid == shell_pid() || w.exe.eq_ignore_ascii_case("explorer.exe"))
        && !allow_shell
        && !is_explorer_folder(w)
    {
        bail!(
            "Windows desktop/taskbar protection is blocked; only individual Explorer folder windows are supported"
        );
    }
    let process = OwnedProcess::from_pid(w.pid).with_context(|| {
        format!(
            "cannot access {} (PID {}): privileges, architecture or security restrictions",
            w.exe, w.pid
        )
    })?;
    ensure!(process.is_alive(), "target process exited");
    ensure!(
        process.is_x64()?,
        "unsupported target architecture: this agent requires x64"
    );
    let syringe = Syringe::for_process(process);
    let agent = syringe
        .find_or_inject(agent_path()?)
        .with_context(|| format!("could not load agent into {}; not protected", w.exe))?;
    let proc = unsafe {
        syringe.get_raw_procedure::<extern "system" fn(usize, u32, u32) -> u32>(
            agent,
            "PawhideApplyOwned",
        )
    }
    .context("failed to resolve agent procedure")?
    .context("agent export missing; restart the target application after upgrading PawHide")?;
    ensure!(
        unsafe { IsWindow(handle(w.hwnd)) } != 0,
        "window disappeared during injection"
    );
    unsafe { GetWindowThreadProcessId(handle(w.hwnd), &mut live_pid) };
    ensure!(
        live_pid == w.pid && syringe.process().is_alive(),
        "window owner exited or changed during injection"
    );
    let value = proc
        .call(
            w.hwnd,
            if protect { EXCLUDED } else { 0 },
            u32::from(automatic),
        )
        .context("agent procedure failed")?;
    ensure!(
        value == 0,
        "SetWindowDisplayAffinity returned 0x{value:08X}"
    );
    unsafe { GetWindowThreadProcessId(handle(w.hwnd), &mut live_pid) };
    ensure!(
        live_pid == w.pid && syringe.process().is_alive(),
        "window owner changed before affinity verification"
    );
    // Manual ownership may legitimately prevent an automatic restore.
    let expected = if automatic
        && !protect
        && automatic_original(w.hwnd).is_none()
        && affinity(w.hwnd) == Some(EXCLUDED)
    {
        EXCLUDED
    } else {
        expected
    };
    match affinity(w.hwnd) {
        Some(a) if a == expected => Ok(true),
        Some(a) => bail!("affinity mismatch (requested 0x{expected:X}, received 0x{a:X})"),
        None => Ok(false),
    }
}

/// Hide non-interactive Windows infrastructure from the GUI's *default* list.
/// The CLI and an explicit "Show system windows" option still expose them.
pub fn is_background_ui_window(w: &Window) -> bool {
    const INTERNAL: &[&str] = &[
        "textinputhost.exe",
        "applicationframehost.exe",
        "shellexperiencehost.exe",
        "searchhost.exe",
        "startmenuexperiencehost.exe",
        "lockapp.exe",
        "superhuman.webui.exe",
    ];
    INTERNAL.iter().any(|n| w.exe.eq_ignore_ascii_case(n))
}

/// Capture-only experiment: identify *only* Windows taskbar top-level windows.
/// Unlike normal window discovery these windows often have no title.
/// No window styles, position, visibility or taskbar auto-hide preferences change.
pub fn is_taskbar_class_name(class_name: &str) -> bool {
    matches!(class_name, "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

pub fn taskbar_windows() -> Result<Vec<Window>> {
    unsafe extern "system" fn collect(hwnd: *mut c_void, data: isize) -> i32 {
        let windows = unsafe { &mut *(data as *mut Vec<Window>) };
        if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
            return 1;
        }
        let mut class = [0_u16; 128];
        let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        if len <= 0 {
            return 1;
        }
        let name = String::from_utf16_lossy(&class[..len as usize]);
        if !is_taskbar_class_name(&name) {
            return 1;
        }
        let mut pid = 0_u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == 0 {
            return 1;
        }
        let exe = process_name(pid);
        // Never inject into an unrelated window impersonating a taskbar class.
        if !exe.eq_ignore_ascii_case("explorer.exe") {
            return 1;
        }
        windows.push(Window {
            hwnd: hwnd as usize,
            pid,
            exe,
            title: if name == "Shell_TrayWnd" {
                "Primary Windows taskbar"
            } else {
                "Secondary Windows taskbar"
            }
            .into(),
            affinity: affinity(hwnd as usize),
            tool_window: false,
        });
        1
    }
    let mut result = Vec::new();
    ensure!(
        unsafe { EnumWindows(Some(collect), &mut result as *mut _ as isize) } != 0,
        "could not enumerate taskbar windows"
    );
    Ok(result)
}

#[cfg(test)]
mod taskbar_tests {
    use super::{is_taskbar_class_name, unsupported_reason};
    #[test]
    fn capture_clients_and_antitamper_targets_are_never_injected() {
        for exe in [
            "Discord.exe",
            "DiscordCanary.exe",
            "DiscordPTB.exe",
            "RobloxPlayerBeta.exe",
            "pid:42",
        ] {
            assert!(unsupported_reason(exe).is_some(), "{exe}");
        }
        assert!(unsupported_reason("floorp.exe").is_none());
        assert!(unsupported_reason("explorer.exe").is_none());
    }
    #[test]
    fn exact_taskbar_classes_only() {
        assert!(is_taskbar_class_name("Shell_TrayWnd"));
        assert!(is_taskbar_class_name("Shell_SecondaryTrayWnd"));
        assert!(!is_taskbar_class_name("CabinetWClass"));
        assert!(!is_taskbar_class_name("Progman"));
        assert!(!is_taskbar_class_name("Shell_TrayWnd_extra"));
    }
}
