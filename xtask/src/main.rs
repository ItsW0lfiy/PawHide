//! Project-local build driver invoked with `cargo all`.
//! Runs validation, builds the Rust workspace, and produces BOTH distributions.
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn run_command(command: &mut Command, label: &str) -> Result<(), String> {
    println!("\n==> {label}");
    let status = command
        .status()
        .map_err(|error| format!("{label}: could not launch process: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed (exit status: {status})"))
    }
}

fn cargo(root: &Path, args: &[&str], label: &str) -> Result<(), String> {
    let executable: OsString = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    run_command(Command::new(executable).current_dir(root).args(args), label)
}

fn powershell(root: &Path, script: &Path, label: &str) -> Result<(), String> {
    // Prefer PowerShell 7, but Windows PowerShell 5.1 is fully supported.
    for executable in ["pwsh.exe", "powershell.exe"] {
        let mut command = Command::new(executable);
        command
            .current_dir(root)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script);
        match command.status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => return Err(format!("{label} failed (exit status: {status})")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{label}: {error}")),
        }
    }
    Err("PowerShell was not found; install PowerShell 7 or use Windows PowerShell".into())
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    if arguments.next().as_deref() != Some("all") || arguments.next().is_some() {
        return Err("Usage: cargo all".into());
    }
    if !cfg!(windows) {
        return Err("PawHide distribution packaging requires Windows x64".into());
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("cannot find PawHide workspace root")?
        .to_path_buf();

    cargo(&root, &["fmt", "--all", "--check"], "Check Rust formatting")?;
    cargo(
        &root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        "Lint all targets",
    )?;
    cargo(
        &root,
        &["build", "--release", "--locked"],
        "Build release GUI, CLI and agent",
    )?;
    cargo(
        &root,
        &["test", "--workspace", "--locked"],
        "Run Rust workspace tests",
    )?;

    println!("\n==> Validate NSIS bootstrap with local fixtures");
    powershell(
        &root,
        &root.join("installer/test-bootstrap.ps1"),
        "NSIS bootstrap fixture tests",
    )?;

    println!("\n==> Package both distributions");
    powershell(
        &root,
        &root.join("tools/Package-PawHide.ps1"),
        "Portable ZIP and PawHide-setup.exe",
    )?;

    println!("\nPawHide build and packaging complete. See dist/.");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\nPawHide build failed: {message}");
            ExitCode::FAILURE
        }
    }
}
