//! Minimal, synchronous payload. No DllMain work, hooks, polling or background threads.
//! Loaded only into processes explicitly selected by the local CLI.

#[cfg(windows)]
mod windows_agent {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetWindowThreadProcessId, IsWindow, SetWindowDisplayAffinity,
    };

    pub const INVALID_WINDOW: u32 = 0xFFFF_FF01;
    pub const WRONG_PROCESS: u32 = 0xFFFF_FF02;
    pub const NOT_TOP_LEVEL: u32 = 0xFFFF_FF03;
    pub const UNKNOWN_FAILURE: u32 = 0xFFFF_FF04;

    /// 0 on success; Win32 error code or sentinel on failure.
    /// hwnd is the raw HWND as an integer; affinity is WDA_NONE/WDA_MONITOR/WDA_EXCLUDEFROMCAPTURE.
    #[unsafe(no_mangle)]
    pub extern "system" fn PawhideApply(hwnd: usize, affinity: u32) -> u32 {
        // Never let a panic unwind across the Windows RPC boundary.
        std::panic::catch_unwind(|| {
            let handle = hwnd as *mut core::ffi::c_void;
            if unsafe { IsWindow(handle) } == 0 {
                return INVALID_WINDOW;
            }
            let mut owner_pid = 0u32;
            unsafe { GetWindowThreadProcessId(handle, &mut owner_pid) };
            if owner_pid == 0 || owner_pid != unsafe { GetCurrentProcessId() } {
                return WRONG_PROCESS;
            }
            if unsafe { GetAncestor(handle, GA_ROOT) } != handle {
                return NOT_TOP_LEVEL;
            }
            if !matches!(affinity, 0 | 1 | 0x11) {
                return UNKNOWN_FAILURE;
            }
            if unsafe { SetWindowDisplayAffinity(handle, affinity) } == 0 {
                let code = unsafe { GetLastError() };
                return if code == 0 { UNKNOWN_FAILURE } else { code };
            }
            0
        })
        .unwrap_or(UNKNOWN_FAILURE)
    }
}
