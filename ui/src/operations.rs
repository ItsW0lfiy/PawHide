//! Run potentially hanging Windows DLL-injection calls in a disposable CLI
//! helper process. A timeout recovers the GUI; it does not claim the target is protected.
#![cfg(windows)]
use pawhide_core::platform::Window;
use std::{os::windows::process::CommandExt, process::{Command, Stdio}, thread, time::{Duration, Instant}};
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn apply_bounded(window: &Window, hide: bool, timeout: Duration) -> Result<(), String> {
    let helper = std::env::current_exe()
        .map_err(|e| format!("cannot locate PawHide: {e}"))?
        .with_file_name("pawhide.exe");
    let mut child = Command::new(&helper)
        .arg(if hide { "hide" } else { "show" })
        .arg(format!("0x{:X}", window.hwnd))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn().map_err(|e| format!("cannot start {}: {e}", helper.display()))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                if output.status.success() { return Ok(()); }
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let message = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
                return Err(if message.is_empty() { "helper failed without an error message".into() } else { message.into() });
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s; state NOT verified (the remote operation may still be pending)", timeout.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(err) => { let _ = child.kill(); let _ = child.wait(); return Err(err.to_string()); }
        }
    }
}
