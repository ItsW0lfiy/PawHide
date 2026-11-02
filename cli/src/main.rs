#[cfg(windows)] mod auto;
#[cfg(windows)] mod commands;
#[cfg(windows)] mod startup;
mod presentation;

use anyhow::Result;
#[cfg(not(windows))] use anyhow::bail;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name="pawhide", version, about="Control window visibility in compatible screen captures")]
struct Cli {
    #[command(subcommand)] command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List applications or windows; optionally filter to one executable.
    List {
        target: Option<String>,
        #[arg(long)] windows: bool,
        #[arg(long)] all: bool,
        #[arg(long)] json: bool,
    },
    /// Exclude one application or a selected window from compatible captures.
    Hide {
        target: Option<String>,
        #[arg(long)] title: Option<String>,
        /// Pick a specific window from an interactive numbered list.
        #[arg(long)] pick: bool,
        #[arg(long)] allow_shell: bool,
        #[arg(long, short)] verbose: bool,
    },
    /// Restore normal capture visibility.
    Show {
        target: Option<String>,
        #[arg(long)] title: Option<String>,
        #[arg(long)] pick: bool,
        #[arg(long)] allow_shell: bool,
        #[arg(long, short)] verbose: bool,
    },
    /// Show exclusion state (omit target for summary).
    Status {
        target: Option<String>,
        #[arg(long, short)] verbose: bool,
        #[arg(long)] json: bool,
    },
    /// Inspect windows, owners, handles, titles and affinity readback.
    Inspect { target: String, #[arg(long)] json: bool },
    /// Maintain background protection rules (alias: always `auto`).
    Auto { #[command(subcommand)] command: Option<AutoCommand> },
    /// Manage per-user Windows login startup.
    Startup { #[command(subcommand)] command: StartupCommand },
    /// Inspect or edit PawHide configuration.
    Config { #[command(subcommand)] command: ConfigCommand },
    /// Read monitor errors and activity.
    Logs { #[command(subcommand)] command: Option<LogsCommand> },
    /// Print version without additional technical details.
    Version,
}

#[derive(Subcommand, Debug)]
enum AutoCommand {
    Add { application: String },
    Remove { application: String },
    List, Status, Start, Stop,
    /// Apply configured rules to currently open windows immediately.
    Sync,
    #[command(hide=true)] Run,
}

#[derive(Subcommand, Debug)]
enum StartupCommand { Enable, Disable, Status }
#[derive(Subcommand, Debug)]
enum ConfigCommand { Path, Show, Validate, Edit }
#[derive(Subcommand, Debug)]
enum LogsCommand { Path, Show { #[arg(long,default_value_t=30)] lines: usize }, Follow }

fn main() {
    if let Err(err) = run() {
        eprintln!("{} {}", presentation::bad("Error:"), err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    #[cfg(windows)] {
        use Command::*;
        return match cli.command {
            List {target,windows,all,json} => commands::list(target.as_deref(),windows,all,json),
            Hide {target,title,pick,allow_shell,verbose} => commands::change(target.as_deref(),title.as_deref(),pick,true,allow_shell,verbose),
            Show {target,title,pick,allow_shell,verbose} => commands::change(target.as_deref(),title.as_deref(),pick,false,allow_shell,verbose),
            Status {target,verbose,json} => commands::status(target.as_deref(),verbose,json),
            Inspect {target,json} => commands::inspect(&target,json),
            Auto {command} => auto::command(command.unwrap_or(AutoCommand::Status)),
            Startup {command} => startup::command(command),
            Config {command} => commands::config(command),
            Logs {command} => commands::logs(command.unwrap_or(LogsCommand::Show { lines:30 })),
            Version => { println!("PawHide {}", env!("CARGO_PKG_VERSION")); Ok(()) },
        };
    }
    #[cfg(not(windows))] {
        let _ = cli;
        bail!("PawHide requires Windows 10 (2004+) or Windows 11");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn cli_definitions_valid() { Cli::command().debug_assert(); }
    #[test]
    fn auto_alias_is_just_auto() {
        assert!(Cli::try_parse_from(["pawhide","auto","status"]).is_ok());
        assert!(Cli::try_parse_from(["pawhide","autohide","status"]).is_err());
    }
    #[test]
    fn manual_title_and_pick_syntax() {
        assert!(Cli::try_parse_from(["pawhide","hide","Wolfmark.exe","--title","Concept.md"]).is_ok());
        assert!(Cli::try_parse_from(["pawhide","show","--pick"]).is_ok());
    }
}
