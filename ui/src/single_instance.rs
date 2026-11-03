//! One PawHide window per interactive Windows session.
#![cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow},
};

pub struct SingleInstance(HANDLE);
impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn acquire() -> Option<SingleInstance> {
    let name: Vec<u16> = "Local\\PawHide.UI.Singleton\0".encode_utf16().collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return None;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(handle);
        }
        let title: Vec<u16> = "PawHide\0".encode_utf16().collect();
        let existing = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !existing.is_null() {
            unsafe {
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
            }
        }
        None
    } else {
        Some(SingleInstance(handle))
    }
}
