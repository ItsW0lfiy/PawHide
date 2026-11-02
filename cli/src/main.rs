mod config;
#[cfg(windows)] mod platform;
#[cfg(windows)] mod auto;

use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "pawhide", version, about = "Hide selected windows from compatible screen captures")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List applications; use --windows for individual windows.
    List {
        #[arg(long)] windows: bool,
        #[arg(long)] all: bool,
    },
    /// Exclude an application (PicView.exe) or window (0x123456) from capture.
    Hide {
        target: String,
        #[arg(long)] allow_shell: bool,
    },
    /// Restore normal capture for an application or window.
    Show {
        target: String,
        #[arg(long)] allow_shell: bool,
    },
    /// Inspect current protection; omit target for a compact overview.
    Status { target: Option<String> },
    /// Persistent application protection: add, remove, list, start, stop, status.
    Auto {
        #[command(subcommand)] command: Option<AutoCommand>,
    },
}

#[derive(Subcommand, Debug)]
enum AutoCommand {
    Add { application: String },
    Remove { application: String },
    List,
    Status,
    Start,
    Stop,
    #[command(hide = true)] Run,
}

pub fn parse_hwnd(input: &str) -> Result<Option<usize>> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        let value = usize::from_str_radix(hex, 16)?;
        ensure!(value != 0, "window handle cannot be zero");
        Ok(Some(value))
    } else { Ok(None) }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = <Cli as Parser>::parse();
    #[cfg(windows)] {
        use Command::*;
        return match cli.command {
            List {windows, all} => platform::list(windows, all),
            Hide {target, allow_shell} => platform::change(&target, true, allow_shell),
            Show {target, allow_shell} => platform::change(&target, false, allow_shell),
            Status {target} => platform::show_status(target.as_deref()),
            Auto {command} => auto::command(command.unwrap_or(AutoCommand::Status)),
        };
    }
    #[cfg(not(windows))] {
        let _ = cli;
        anyhow::bail!("PawHide requires Windows 10 2004+ or Windows 11");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_hex_is_a_handle() {
        assert_eq!(parse_hwnd("0x10").unwrap(), Some(16));
        assert_eq!(parse_hwnd("PicView.exe").unwrap(), None);
        assert!(parse_hwnd("0x0").is_err());
    }
}
