//! Minimal, synchronous payload. No DllMain work, hooks, polling or background threads.
//! Loaded only into processes explicitly selected by the local CLI.

#[cfg(windows)]
mod windows_agent {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetPropW, GetWindowDisplayAffinity, GetWindowThreadProcessId,
        IsWindow, RemovePropW, SetPropW, SetWindowDisplayAffinity,
    };

    pub const INVALID_WINDOW: u32 = 0xFFFF_FF01;
    pub const WRONG_PROCESS: u32 = 0xFFFF_FF02;
    pub const NOT_TOP_LEVEL: u32 = 0xFFFF_FF03;
    pub const UNKNOWN_FAILURE: u32 = 0xFFFF_FF04;

    /// 0 on success; Win32 error code or sentinel on failure.
    /// hwnd is the raw HWND as an integer; affinity is WDA_NONE/WDA_MONITOR/WDA_EXCLUDEFROMCAPTURE.
    #[unsafe(no_mangle)]
    pub extern "system" fn PawhideApply(hwnd: usize, affinity: u32) -> u32 {
        PawhideApplyOwned(hwnd, affinity, 0)
    }

    static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// mode 0: explicit manual choice; mode 1: automatic rule reconciliation.
    /// Properties are destroyed by Windows with the HWND, including on crashes.
    #[unsafe(no_mangle)]
    pub extern "system" fn PawhideApplyOwned(hwnd: usize, affinity: u32, mode: u32) -> u32 {
        // Never let a panic unwind across the Windows RPC boundary.
        std::panic::catch_unwind(|| {
            let Ok(_lock) = APPLY_LOCK.lock() else {
                return UNKNOWN_FAILURE;
            };
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
            if !matches!(affinity, 0 | 1 | 0x11) || mode > 1 {
                return UNKNOWN_FAILURE;
            }
            let auto: Vec<u16> = "PawHide.AutoAffinity.v1\0".encode_utf16().collect();
            let manual: Vec<u16> = "PawHide.ManualAffinity.v1\0".encode_utf16().collect();
            let original = unsafe { GetPropW(handle, auto.as_ptr()) } as usize;
            let manually_owned = !unsafe { GetPropW(handle, manual.as_ptr()) }.is_null();
            let mut current = 0;
            if unsafe { GetWindowDisplayAffinity(handle, &mut current) } == 0 {
                return UNKNOWN_FAILURE;
            }
            let mut desired = affinity;
            if mode == 1 {
                if affinity == 0 {
                    if original == 0 {
                        return 0;
                    }
                    // A later manual Hide takes priority over automatic cleanup.
                    if manually_owned {
                        unsafe {
                            RemovePropW(handle, auto.as_ptr());
                        }
                        return 0;
                    }
                    desired = (original - 1) as u32;
                } else if current == 0x11 {
                    return 0; // Never claim a pre-existing exclusion.
                } else if original == 0
                    && unsafe { SetPropW(handle, auto.as_ptr(), (current as usize + 1) as _) } == 0
                {
                    return UNKNOWN_FAILURE;
                }
            } else if affinity == 0x11
                && unsafe { SetPropW(handle, manual.as_ptr(), 1usize as _) } == 0
            {
                return UNKNOWN_FAILURE;
            }

            if unsafe { SetWindowDisplayAffinity(handle, desired) } == 0 {
                let code = unsafe { GetLastError() };
                return if code == 0 { UNKNOWN_FAILURE } else { code };
            }
            if affinity == 0 {
                unsafe {
                    RemovePropW(handle, auto.as_ptr());
                }
            }
            if mode == 0 && affinity == 0 {
                unsafe {
                    RemovePropW(handle, manual.as_ptr());
                }
            }
            0
        })
        .unwrap_or(UNKNOWN_FAILURE)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetWindowDisplayAffinity,
        };

        #[test]
        #[ignore = "requires an interactive Windows compositor"]
        fn native_ownership_roundtrip() {
            unsafe {
                let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0,
                    0,
                    0,
                    120,
                    80,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                assert!(!hwnd.is_null());
                struct WindowGuard(*mut core::ffi::c_void);
                impl Drop for WindowGuard {
                    fn drop(&mut self) {
                        unsafe {
                            DestroyWindow(self.0);
                        }
                    }
                }
                let _guard = WindowGuard(hwnd);
                let read = || {
                    let mut value = 0;
                    assert_ne!(GetWindowDisplayAffinity(hwnd, &mut value), 0);
                    value
                };
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0x11, 1), 0);
                assert_eq!(read(), 0x11);
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0, 1), 0);
                assert_eq!(read(), 0);
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0x11, 1), 0);
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0x11, 0), 0);
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0, 1), 0);
                assert_eq!(read(), 0x11); // Manual pin survives automatic cleanup.
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0, 0), 0);
                assert_eq!(read(), 0);
                assert_eq!(SetWindowDisplayAffinity(hwnd, 1), 1);
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0x11, 1), 0);
                assert_eq!(PawhideApplyOwned(hwnd as usize, 0, 1), 0);
                assert_eq!(read(), 1); // Restore WDA_MONITOR, not arbitrary visibility.
            }
        }
    }
}
