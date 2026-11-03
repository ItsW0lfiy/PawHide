//! Bounded disposable CLI operations; injection never runs in the GUI process.
#![cfg(windows)]
use pawhide_core::{helper, platform::Window};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub fn apply(window: &Window, protect: bool, cancel: &AtomicBool) -> Result<(), String> {
    helper::apply(window, protect, false, || cancel.load(Ordering::Relaxed))
        .map_err(|e| format!("{e:#}"))
}

pub fn command(args: &[String], cancel: &AtomicBool) -> Result<String, String> {
    helper::run(args, Duration::from_secs(8), || {
        cancel.load(Ordering::Relaxed)
    })
    .map_err(|e| format!("{e:#}"))
}
