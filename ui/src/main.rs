//! PawHide Slint desktop GUI. The Rust protection and capture engines are unchanged.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod capture_native;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod operations;
#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod app;

#[cfg(not(windows))]
fn main() {
    eprintln!("PawHideUI is supported on Windows.");
}

#[cfg(windows)]
fn main() -> Result<(), slint::PlatformError> {
    let Some(_instance) = single_instance::acquire() else {
        return Ok(());
    };
    app::run()
}
