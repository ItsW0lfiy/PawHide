//! Optional native PawHide manager. CLI and DLL agent remain independent.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("PawHideUI requires Windows.");
}

#[cfg(windows)]
mod capture_native;
#[cfg(windows)]
mod capture_test;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod operations;
#[cfg(windows)]
mod single_instance;

#[cfg(windows)]
mod app;

#[cfg(windows)]
fn main() -> eframe::Result<()> {
    let Some(_instance) = single_instance::acquire() else {
        return Ok(());
    };
    app::run()
}
