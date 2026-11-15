//! User-scoped startup via HKCU Run. No administrator privileges, no Task Scheduler service.
use crate::{StartupCommand, presentation as display};
use anyhow::{Context, Result, ensure};
use std::process::Command;
const REGKEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "PawHide Auto";
fn invoke(args: &[&str]) -> Result<std::process::Output> {
    Command::new("reg.exe")
        .args(args)
        .output()
        .context("unable to execute Windows reg.exe")
}
fn startup_command() -> Result<String> {
    let binary = pawhide_core::folders::display_path(&std::env::current_exe()?.canonicalize()?);
    // Preserve the absolute executable path as an argv-style quoted string.
    Ok(format!("\"{}\" auto start", binary.display()))
}
fn query() -> Result<Option<String>> {
    let output = invoke(&["query", REGKEY, "/v", VALUE])?;
    if !output.status.success() {
        return Ok(None);
    }
    let decoded = String::from_utf8_lossy(&output.stdout);
    let value = decoded
        .lines()
        .find_map(|line| line.split_once("REG_SZ").map(|(_, v)| v.trim().to_owned()));
    Ok(value)
}
pub fn command(cmd: StartupCommand) -> Result<()> {
    display::header(
        "WINDOWS STARTUP",
        "Current-user AutoHide sign-in registration",
    );
    match cmd {
        StartupCommand::Enable => {
            let target = startup_command()?;
            let output = invoke(&[
                "add", REGKEY, "/v", VALUE, "/t", "REG_SZ", "/d", &target, "/f",
            ])?;
            ensure!(
                output.status.success(),
                "Windows refused to register startup: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            display::success("AutoHide will start at Windows sign-in.");
            display::field("Command", target);
        }
        StartupCommand::Disable => {
            if query()?.is_none() {
                display::note("Startup was already disabled.");
                return Ok(());
            }
            let output = invoke(&["delete", REGKEY, "/v", VALUE, "/f"])?;
            ensure!(
                output.status.success(),
                "unable to remove PawHide startup entry"
            );
            display::success("PawHide Windows startup disabled.");
        }
        StartupCommand::Status => match query()? {
            Some(command) => {
                display::field("State", display::good("Enabled"));
                display::field("Command", command);
            }
            None => display::field("State", display::warn("Disabled")),
        },
    }
    Ok(())
}
