#[cfg(windows)]
mod auto;
#[cfg(windows)]
mod capture_native;
#[cfg(windows)]
mod capture_preview;
#[cfg(windows)]
mod commands;
mod presentation;
#[cfg(windows)]
mod startup;

use anyhow::Result;
#[cfg(not(windows))]
use anyhow::bail;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "pawhide",
    version,
    arg_required_else_help = true,
    about = "Protect selected Windows application windows from compatible screen captures",
    long_about = "PawHide excludes selected application windows from compatible screen captures without hiding them on the physical desktop.\n\nA successful affinity readback is NOT proof of what Discord or every capture backend actually shows. Verify the stream before sharing sensitive content.",
    after_help = "EXAMPLES:\n  pawhide list --search picview\n  pawhide list --windows --filter excluded\n  pawhide hide PicView.exe --verbose\n  pawhide show PicView.exe\n  pawhide auto add PicView.exe\n  pawhide auto restart\n  pawhide doctor\n\nTIP: Run 'pawhide <COMMAND> --help' for options, examples, and safety notes."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(hide = true)]
    ApplyWindow {
        hwnd: String,
        pid: u32,
        exe: String,
        #[arg(action = clap::ArgAction::Set)]
        protect: bool,
        #[arg(action = clap::ArgAction::Set)]
        automatic: bool,
        #[arg(long)]
        allow_shell: bool,
    },
    #[command(hide = true)]
    ExplorerSnapshot,
    /// List detected applications or individual windows.
    #[command(
        after_help = "EXAMPLES:\n  pawhide list\n  pawhide list PicView.exe --windows\n  pawhide list --search terminal --filter visible\n  pawhide list --filter attention --json\n\nA capture state is a Windows affinity readback, not a verified Discord stream."
    )]
    List {
        /// Optional executable name (e.g. PicView.exe or PicView).
        #[arg(value_name = "APPLICATION")]
        target: Option<String>,
        /// Display individual window titles instead of grouping by executable.
        #[arg(
            long,
            help = "Show each top-level window separately, including its title and capture state"
        )]
        windows: bool,
        /// Include tool/background windows normally omitted from listings.
        #[arg(
            long,
            help = "Include detected tool/background windows (does not bypass safety restrictions)"
        )]
        all: bool,
        /// Match text in executable names or window titles (case-insensitive).
        #[arg(
            long,
            value_name = "TEXT",
            help = "Filter by application name or window title, case-insensitively"
        )]
        search: Option<String>,
        /// Show only matching capture states.
        #[arg(long, value_enum, default_value_t = WindowFilter::All, value_name = "STATE", help = "Filter: all, excluded, visible, or attention")]
        filter: WindowFilter,
        /// Machine-readable JSON array of matching windows.
        #[arg(
            long,
            help = "Write machine-readable JSON instead of a formatted table"
        )]
        json: bool,
    },
    /// Exclude a supported application/window from compatible captures.
    #[command(
        after_help = "EXAMPLES:\n  pawhide hide PicView.exe\n  pawhide hide PicView.exe --title Photo\n  pawhide hide --pick\n  pawhide hide 0x12345 --verbose\n\nA successful command verifies Windows affinity only. Always inspect your actual stream."
    )]
    Hide {
        /// Executable name or 0x-prefixed window handle; omit when using --pick.
        #[arg(value_name = "APPLICATION|0xHWND")]
        target: Option<String>,
        /// Match a window by a substring of its title; use --pick for ambiguous matches.
        #[arg(
            long,
            value_name = "TEXT",
            help = "Select only windows with this title substring (requires a target)"
        )]
        title: Option<String>,
        /// Select one window interactively by number.
        #[arg(long, help = "Open a numbered terminal picker to choose one window")]
        pick: bool,
        /// Advanced: allow explicitly selected Explorer/shell windows.
        #[arg(
            long,
            help = "Advanced: permit supported shell windows after strict HWND checks"
        )]
        allow_shell: bool,
        /// Print each affected window handle and title.
        #[arg(
            long,
            short,
            help = "Display window IDs and titles alongside the result"
        )]
        verbose: bool,
    },
    /// Restore normal capture visibility for a window/application.
    #[command(
        after_help = "EXAMPLES:\n  pawhide show PicView.exe\n  pawhide show --pick\n  pawhide show 0x12345 --verbose\n\nThis changes compatible capture visibility; it does not minimize or unhide desktop windows."
    )]
    Show {
        /// Executable name or 0x-prefixed window handle; omit when using --pick.
        #[arg(value_name = "APPLICATION|0xHWND")]
        target: Option<String>,
        /// Select windows matching this title substring.
        #[arg(
            long,
            value_name = "TEXT",
            help = "Select only windows with this title substring (requires a target)"
        )]
        title: Option<String>,
        /// Open a numbered terminal picker to choose one window.
        #[arg(long, help = "Choose a single window interactively by number")]
        pick: bool,
        /// Advanced: allow explicitly selected Explorer/shell windows.
        #[arg(
            long,
            help = "Advanced: permit supported shell windows after strict HWND checks"
        )]
        allow_shell: bool,
        /// Print each affected window handle and title.
        #[arg(
            long,
            short,
            help = "Display window IDs and titles alongside the result"
        )]
        verbose: bool,
    },
    /// Inspect affinity readback (omit the target for an overall summary).
    #[command(
        after_help = "EXAMPLES:\n  pawhide status\n  pawhide status PicView.exe -v\n  pawhide status --json\n\nAn 'excluded' status does not guarantee that every capture backend honors it."
    )]
    Status {
        /// Optional executable name or 0x-prefixed window handle.
        #[arg(value_name = "APPLICATION|0xHWND")]
        target: Option<String>,
        /// Include HWND, PID, and raw affinity data.
        #[arg(
            long,
            short,
            help = "Include window handles, process IDs and raw affinity values"
        )]
        verbose: bool,
        /// Machine-readable snapshot.
        #[arg(long, help = "Output JSON for scripts or integrations")]
        json: bool,
    },
    /// Inspect window ownership, title, HWND, PID and affinity.
    Inspect {
        /// Executable name or 0x-prefixed window handle.
        #[arg(value_name = "APPLICATION|0xHWND")]
        target: String,
        /// Write window information as JSON.
        #[arg(long, help = "Output machine-readable JSON")]
        json: bool,
    },
    /// Manage automatic protection rules and the background monitor.
    #[command(
        after_help = "EXAMPLES:\n  pawhide auto add PicView.exe\n  pawhide auto list\n  pawhide auto status\n  pawhide auto restart\n  pawhide auto sync\n\nStopping monitoring does not undo already applied exclusions."
    )]
    Auto {
        #[command(subcommand)]
        command: Option<AutoCommand>,
    },
    /// Enable, disable or inspect AutoHide at Windows sign-in.
    Startup {
        #[command(subcommand)]
        command: StartupCommand,
    },
    /// Show, validate or edit the local JSON configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Read or follow the AutoHide diagnostic log.
    Logs {
        #[command(subcommand)]
        command: Option<LogsCommand>,
    },
    /// Manage protected folders and document-open notifications.
    Folders {
        #[command(subcommand)]
        command: FolderCommand,
    },
    /// Experimental capture-affinity controls (e.g. taskbar).
    Capture {
        #[command(subcommand)]
        command: CaptureCommand,
    },
    /// Check installation, agent DLL, rules, monitor, and window discovery.
    #[command(
        after_help = "Checks the local installation without injecting into any application.\nEXAMPLES:\n  pawhide doctor\n  pawhide doctor --json"
    )]
    Doctor {
        /// Produce a machine-readable diagnostics report.
        #[arg(long, help = "Print a structured JSON diagnostics report")]
        json: bool,
    },
    /// Repeatedly show window protection state in the terminal (Ctrl+C to stop).
    Watch {
        /// Refresh interval in seconds, minimum 1.
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..), value_name = "SECONDS", help = "Seconds between refreshes (default: 2, minimum: 1)")]
        interval: u64,
    },
    /// Print version without additional technical details.
    Version,
}

#[derive(Subcommand, Debug)]
enum AutoCommand {
    /// Save an executable rule and start monitoring if needed.
    Add {
        /// Executable (PicView.exe; ".exe" is optional).
        #[arg(value_name = "APPLICATION")]
        application: String,
    },
    /// Remove a saved rule; manual exclusions remain.
    Remove {
        /// Executable name to remove from AutoHide.
        #[arg(value_name = "APPLICATION")]
        application: String,
    },
    /// List saved AutoHide executable rules.
    List,
    /// Show monitor state, rule counts, and matching application states.
    Status,
    /// Start the monitor (requires at least one application or folder rule).
    Start,
    /// Request a graceful monitor stop; existing exclusions remain.
    Stop,
    /// Gracefully stop then start monitoring; never force-kills the process.
    Restart,
    /// Apply configured rules to currently open windows immediately.
    Sync,
    #[command(hide = true)]
    Run,
}

#[derive(Subcommand, Debug)]
enum FolderCommand {
    /// Save a protected directory (starts AutoHide if necessary).
    Add {
        /// Existing directory to protect.
        #[arg(value_name = "DIRECTORY")]
        path: std::path::PathBuf,
    },
    /// Remove a saved folder rule without changing manual exclusions.
    Remove {
        /// Saved directory path to remove.
        #[arg(value_name = "DIRECTORY")]
        path: std::path::PathBuf,
    },
    /// List configured protected directories.
    List,
    /// Check whether a document path falls under a protected directory.
    Match {
        /// Path to a document (need not be open).
        #[arg(value_name = "FILE")]
        path: std::path::PathBuf,
    },
    /// For cooperating apps: notify an HWND and exact file opened in it.
    Notify {
        /// Verified 0x-prefixed window handle.
        #[arg(value_name = "0xHWND")]
        hwnd: String,
        /// Exact file path opened in the target application window.
        #[arg(value_name = "FILE")]
        path: std::path::PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum CaptureCommand {
    /// Show available physical Windows capture displays.
    Monitors,
    /// Launch the small, native live capture preview window.
    #[command(
        after_help = "Keys in preview: M next monitor, C cursor, N native resolution, T topmost, R restart, Esc close.\nA WGC preview does not prove Discord capture safety."
    )]
    Preview {
        /// 1-based display number from capture monitors (default: primary display).
        #[arg(long, value_name = "NUMBER")]
        monitor: Option<usize>,
        /// Capture the pointer; overrides the saved preference for this session.
        #[arg(long, action = clap::ArgAction::Set, value_name = "true|false")]
        cursor: Option<bool>,
        /// Capture at native monitor resolution; overrides saved preference.
        #[arg(long, action = clap::ArgAction::Set, value_name = "true|false")]
        native_resolution: Option<bool>,
        /// Make the preview compact and always on top; overrides saved preference.
        #[arg(long, action = clap::ArgAction::Set, value_name = "true|false")]
        thumbnail: Option<bool>,
    },
    /// Experimentally exclude/restore capture of Windows taskbar windows.
    Taskbar {
        #[command(subcommand)]
        command: TaskbarCommand,
    },
}

#[derive(Subcommand, Debug)]
enum TaskbarCommand {
    /// Read taskbar window affinity without changing it.
    Status,
    /// Attempt to exclude taskbar surfaces from compatible captures.
    Hide,
    /// Restore normal taskbar capture visibility.
    Show,
}

#[derive(Subcommand, Debug)]
enum StartupCommand {
    /// Register the background monitor for current-user Windows sign-in.
    Enable,
    /// Remove PawHide's current-user startup entry.
    Disable,
    /// Show the registered startup command, if any.
    Status,
}
#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Show the current CLI and preview preferences.
    Preferences,
    /// Set one preference without disturbing application or folder rules.
    Set {
        /// Saved preference to change.
        #[arg(value_enum)]
        key: PreferenceKey,
        /// Enabled/disabled.
        #[arg(action = clap::ArgAction::Set, value_name = "true|false")]
        value: bool,
    },
    /// Print the JSON configuration path.
    Path,
    /// Print all configured rules and preferences in JSON.
    Show,
    /// Check whether saved configuration can be parsed.
    Validate,
    /// Open the configuration in Notepad.
    Edit,
}
#[derive(Subcommand, Debug)]
enum LogsCommand {
    /// Print the AutoHide log file path.
    Path,
    /// Print recent monitor log lines.
    Show {
        /// Number of newest lines (default: 30, maximum: 5000).
        #[arg(
            long,
            default_value_t = 30,
            value_name = "COUNT",
            help = "Show the last COUNT lines (default 30, capped at 5000)"
        )]
        lines: usize,
    },
    /// Stream new AutoHide log lines until interrupted (Ctrl+C).
    Follow,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum PreferenceKey {
    ShowSystemWindows,
    CaptureCursor,
    NativeResolution,
    Thumbnail,
}
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum WindowFilter {
    All,
    Excluded,
    Visible,
    Attention,
}

fn main() {
    if let Err(err) = run() {
        presentation::error(&format!("{err:#}"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    #[cfg(windows)]
    {
        use Command::*;
        match cli.command {
            ApplyWindow {
                hwnd,
                pid,
                exe,
                protect,
                automatic,
                allow_shell,
            } => {
                std::panic::set_hook(Box::new(|_| {}));
                let handle = pawhide_core::parse_hwnd(&hwnd)?
                    .ok_or_else(|| anyhow::anyhow!("use a 0x window handle"))?;
                let window = pawhide_core::platform::operation_window(handle, allow_shell)?
                    .ok_or_else(|| {
                        anyhow::anyhow!("window disappeared or is not an eligible top-level window")
                    })?;
                anyhow::ensure!(
                    window.pid == pid && window.exe.eq_ignore_ascii_case(&exe),
                    "window owner changed; stale target rejected"
                );
                anyhow::ensure!(
                    pawhide_core::platform::apply_owned(&window, protect, allow_shell, automatic)?,
                    "affinity readback unavailable"
                );
                Ok(())
            }
            ExplorerSnapshot => {
                println!(
                    "{}",
                    serde_json::to_string(&pawhide_core::explorer::open_folder_windows()?)?
                );
                Ok(())
            }
            List {
                target,
                windows,
                all,
                search,
                filter,
                json,
            } => commands::list(
                target.as_deref(),
                windows,
                all,
                search.as_deref(),
                filter,
                json,
            ),
            Hide {
                target,
                title,
                pick,
                allow_shell,
                verbose,
            } => commands::change(
                target.as_deref(),
                title.as_deref(),
                pick,
                true,
                allow_shell,
                verbose,
            ),
            Show {
                target,
                title,
                pick,
                allow_shell,
                verbose,
            } => commands::change(
                target.as_deref(),
                title.as_deref(),
                pick,
                false,
                allow_shell,
                verbose,
            ),
            Status {
                target,
                verbose,
                json,
            } => commands::status(target.as_deref(), verbose, json),
            Inspect { target, json } => commands::inspect(&target, json),
            Auto { command } => auto::command(command.unwrap_or(AutoCommand::Status)),
            Startup { command } => startup::command(command),
            Config { command } => commands::config(command),
            Logs { command } => commands::logs(command.unwrap_or(LogsCommand::Show { lines: 30 })),
            Folders { command } => commands::folders(command),
            Capture { command } => commands::capture(command),
            Doctor { json } => commands::doctor(json),
            Watch { interval } => commands::watch(interval),
            Version => {
                println!("PawHide {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = cli;
        bail!("PawHide requires Windows 10 (2004+) or Windows 11");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn cli_definitions_valid() {
        Cli::command().debug_assert();
    }
    #[test]
    fn auto_alias_is_just_auto() {
        assert!(Cli::try_parse_from(["pawhide", "auto", "status"]).is_ok());
        assert!(Cli::try_parse_from(["pawhide", "autohide", "status"]).is_err());
    }
    #[test]
    fn folder_subcommands() {
        assert!(Cli::try_parse_from(["pawhide", "folders", "add", "C:\\Private"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pawhide",
                "folders",
                "notify",
                "0xABCD",
                "C:\\Private\\doc.md"
            ])
            .is_ok()
        );
    }
    #[test]
    fn taskbar_capture_commands() {
        for action in ["hide", "show", "status"] {
            assert!(Cli::try_parse_from(["pawhide", "capture", "taskbar", action]).is_ok());
        }
        assert!(Cli::try_parse_from(["pawhide", "capture", "taskbar", "disable-locally"]).is_err());
    }
    #[test]
    fn new_diagnostics_and_list_filters_parse() {
        assert!(Cli::try_parse_from(["pawhide", "doctor", "--json"]).is_ok());
        assert!(Cli::try_parse_from(["pawhide", "auto", "restart"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "pawhide", "list", "--search", "PicView", "--filter", "excluded", "--json"
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["pawhide", "watch", "--interval", "0"]).is_err());
        assert!(Cli::try_parse_from(["pawhide", "capture", "monitors"]).is_ok());
        assert!(Cli::try_parse_from(["pawhide", "capture", "preview", "--monitor", "2"]).is_ok());
        assert!(Cli::try_parse_from(["pawhide", "config", "preferences"]).is_ok());
        assert!(
            Cli::try_parse_from(["pawhide", "config", "set", "capture-cursor", "true"]).is_ok()
        );
    }
    #[test]
    fn manual_title_and_pick_syntax() {
        assert!(
            Cli::try_parse_from(["pawhide", "hide", "Wolfmark.exe", "--title", "Concept.md"])
                .is_ok()
        );
        assert!(Cli::try_parse_from(["pawhide", "show", "--pick"]).is_ok());
    }
}
