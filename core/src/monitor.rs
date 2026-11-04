#![cfg(windows)]
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{EVENT_MODIFY_STATE, OpenEventW},
};

pub const STOP_EVENT: &str = "Local\\PawHide.Auto.Stop";

pub fn running() -> bool {
    let name: Vec<u16> = STOP_EVENT.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    unsafe {
        CloseHandle(handle);
    }
    true
}
