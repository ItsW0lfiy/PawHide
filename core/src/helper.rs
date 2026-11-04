//! Bounded local CLI transport. Drain both pipes while the child is running.
#![cfg(windows)]
use anyhow::{Context, Result, bail};
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn run(args: &[String], timeout: Duration, cancelled: impl Fn() -> bool) -> Result<String> {
    let exe = std::env::current_exe()?.with_file_name("pawhide.exe");
    run_at(&exe, args, timeout, cancelled)
}

fn run_at(
    exe: &std::path::Path,
    args: &[String],
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> Result<String> {
    let mut child = Command::new(exe)
        .args(args)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot start {}", exe.display()))?;
    fn drain(pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            // Keep diagnostics bounded while continuing to drain excess output.
            let mut pipe = pipe;
            let mut buffer = [0; 4096];
            while let Ok(n) = pipe.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                let keep = n.min(65536_usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
            }
            String::from_utf8_lossy(&bytes).trim().to_owned()
        })
    }
    let stdout = drain(child.stdout.take().context("missing helper stdout")?);
    let stderr = drain(child.stderr.take().context("missing helper stderr")?);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if cancelled() || started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "operation cancelled or timed out after {:.1}s; outcome is unverified",
                    started.elapsed().as_secs_f32()
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if !status.success() {
        bail!(
            "helper {status}: {}",
            if err.is_empty() { &out } else { &err }
        );
    }
    Ok(out)
}

pub fn apply(
    window: &crate::platform::Window,
    protect: bool,
    automatic: bool,
    cancelled: impl Fn() -> bool,
) -> Result<()> {
    apply_shell(window, protect, automatic, false, cancelled)
}

pub fn apply_shell(
    window: &crate::platform::Window,
    protect: bool,
    automatic: bool,
    allow_shell: bool,
    cancelled: impl Fn() -> bool,
) -> Result<()> {
    let mut args = vec![
        "apply-window".into(),
        format!("0x{:X}", window.hwnd),
        window.pid.to_string(),
        window.exe.clone(),
        protect.to_string(),
        automatic.to_string(),
    ];
    if allow_shell {
        args.push("--allow-shell".into());
    }
    run(&args, Duration::from_secs(8), cancelled)
        .context("affinity NOT verified; a remote operation already started may still complete")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "only invoked by the bounded transport regression test"]
    fn child_stall() {
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn timeout_and_cancellation_reap_the_helper() {
        let exe = std::env::current_exe().unwrap();
        let args = ["--ignored", "--exact", "helper::tests::child_stall"].map(String::from);
        let started = Instant::now();
        assert!(
            run_at(&exe, &args, Duration::from_millis(150), || false)
                .unwrap_err()
                .to_string()
                .contains("unverified")
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        let started = Instant::now();
        assert!(run_at(&exe, &args, Duration::from_secs(30), || true).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
