use crate::{
    CaptureCommand, ConfigCommand, FolderCommand, LogsCommand, PreferenceKey, TaskbarCommand,
    WindowFilter,
    presentation as display,
};
use anyhow::{Context, Result, bail, ensure};
use pawhide_core::{
    config, folders,
    platform::{self, EXCLUDED, Window},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, IsTerminal, Write},
    process::Command,
    thread,
    time::Duration,
};

fn entry(w: &Window) -> Value {
    json!({ "application":w.exe, "pid":w.pid, "hwnd":format!("0x{:X}",w.hwnd),
        "title":w.title, "affinity":w.affinity,
        "capture_state":platform::state(w.affinity) })
}
fn matches_name(w: &Window, name: &str) -> bool {
    w.exe.eq_ignore_ascii_case(name.trim())
        || w.exe.eq_ignore_ascii_case(&format!("{}.exe", name.trim()))
}
fn target_windows(target: Option<&str>, title: Option<&str>, pick: bool) -> Result<Vec<Window>> {
    ensure!(
        pick || target.is_some(),
        "specify an application (PicView.exe), 0xHANDLE or --pick"
    );
    ensure!(
        title.is_none() || target.is_some(),
        "--title requires an application or window target"
    );
    let mut windows = match target {
        Some(x) => platform::resolve(x)?,
        None => platform::windows()?
            .into_iter()
            .filter(|w| !w.tool_window)
            .collect(),
    };
    if let Some(substring) = title {
        ensure!(!substring.is_empty(), "--title may not be empty");
        windows.retain(|w| w.title.to_lowercase().contains(&substring.to_lowercase()));
        ensure!(
            windows.len() <= 1 || pick,
            "{} windows match that title; refine --title or use --pick",
            windows.len()
        );
    }
    ensure!(!windows.is_empty(), "no matching visible windows found");
    if pick {
        windows.sort_by(|a, b| a.exe.cmp(&b.exe).then(a.title.cmp(&b.title)));
        display::header("SELECT WINDOW", "Choose a single window to change");
        for (i, w) in windows.iter().enumerate() {
            println!(
                "  {:>2}. {:<25} {:<9} {}",
                i + 1,
                display::brief(&w.exe, 25),
                platform::state(w.affinity),
                display::brief(&w.title, 75)
            );
        }
        print!("Window number (or q to cancel): ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("q") {
            bail!("selection cancelled")
        }
        let number: usize = input
            .trim()
            .parse()
            .context("enter a valid window number")?;
        ensure!(
            (1..=windows.len()).contains(&number),
            "selection out of range"
        );
        Ok(vec![windows.swap_remove(number - 1)])
    } else {
        Ok(windows)
    }
}

fn matches_filter(window: &Window, filter: WindowFilter) -> bool {
    match filter {
        WindowFilter::All => true,
        WindowFilter::Excluded => window.affinity == Some(EXCLUDED),
        WindowFilter::Visible => window.affinity == Some(0),
        WindowFilter::Attention => {
            !matches!(window.affinity, Some(0) | Some(EXCLUDED))
                || platform::unsupported_reason(&window.exe).is_some()
        }
    }
}

pub fn list(
    target: Option<&str>,
    show_windows: bool,
    all: bool,
    search: Option<&str>,
    filter: WindowFilter,
    json_output: bool,
) -> Result<()> {
    // Saved GUI-era system-window preference is now respected by CLI listings.
    let include_tools = all || config::load()?.preferences.show_system_windows;
    let mut windows = platform::windows()?;
    let query = search.unwrap_or("").trim().to_lowercase();
    windows.retain(|w| {
        (include_tools || !w.tool_window)
            && target.is_none_or(|name| matches_name(w, name))
            && (query.is_empty()
                || w.exe.to_lowercase().contains(&query)
                || w.title.to_lowercase().contains(&query))
            && matches_filter(w, filter)
    });
    windows.sort_by(|a, b| a.exe.to_lowercase().cmp(&b.exe.to_lowercase()).then(a.title.cmp(&b.title)));
    if json_output {
        println!("{}", serde_json::to_string_pretty(&windows.iter().map(entry).collect::<Vec<_>>())?);
        return Ok(());
    }
    display::header("WINDOWS", "Capture affinity from Windows. Verify your actual stream separately.");
    if show_windows || target.is_some() {
        let rows = windows.iter().map(|w| {
            let capture = match w.affinity {
                Some(EXCLUDED) => display::Cell::good("Excluded"),
                Some(0) => display::Cell::plain("Visible"),
                _ => display::Cell::warn("Unverified"),
            };
            vec![display::Cell::plain(&w.exe), capture, display::Cell::dim(&w.title)]
        }).collect::<Vec<_>>();
        display::table(&["Application", "Capture", "Window title"], &rows);
        display::field("Windows found", windows.len());
        display::next("pawhide inspect APPLICATION  |  pawhide hide APPLICATION");
    } else {
        let mut groups: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for w in &windows {
            let e = groups.entry(w.exe.clone()).or_default();
            e.0 += 1;
            if w.affinity == Some(EXCLUDED) { e.1 += 1; }
            if w.affinity.is_none() { e.2 += 1; }
        }
        let rows = groups.iter().map(|(name, (count, protected, unknown))| {
            let state = format!("{protected}/{count}{}", if *unknown > 0 { " (unknown)" } else { "" });
            vec![display::Cell::plain(name), display::Cell::plain(count.to_string()),
                 if *protected == *count { display::Cell::good(state) }
                 else if *unknown > 0 { display::Cell::warn(state) }
                 else { display::Cell::plain(state) }]
        }).collect::<Vec<_>>();
        display::table(&["Application", "Windows", "Excluded"], &rows);
        display::field("Applications", groups.len());
        display::field("Windows", windows.len());
        display::next("pawhide list --windows  |  pawhide list --search TEXT");
    }
    Ok(())
}

/// Verify the local PawHide installation without injecting into any process.
/// A successful diagnostic does not imply that Discord's capture is secure.
pub fn doctor(json_output: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let dll = exe.with_file_name("pawhide_agent.dll");
    let agent_present = dll.is_file();
    let config_path = config::config_path()?;
    let configuration = config::load();
    let config_valid = configuration.is_ok();
    let rule_count = configuration
        .as_ref()
        .map(|cfg| cfg.applications.len() + cfg.folders.len())
        .unwrap_or(0);
    let monitor_running = pawhide_core::monitor::running();
    let discovered = platform::windows();
    let window_count = discovered.as_ref().map(Vec::len).unwrap_or(0);
    let windows_available = discovered.is_ok();
    let log_path = config_path.with_file_name("auto.log");
    let problems: Vec<String> = [
        (!agent_present).then(|| format!("Agent DLL missing: {}", dll.display())),
        configuration
            .as_ref()
            .err()
            .map(|error| format!("Configuration cannot be loaded: {error}")),
        discovered
            .as_ref()
            .err()
            .map(|error| format!("Cannot enumerate windows: {error}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let notes: Vec<String> = [
        (!monitor_running && rule_count > 0).then(|| {
            "Saved rules exist, but AutoHide is stopped. Run pawhide auto start.".to_string()
        }),
        (monitor_running && rule_count == 0).then(|| {
            "AutoHide is running without saved rules; inspect pawhide auto status.".to_string()
        }),
    ]
    .into_iter()
    .flatten()
    .collect();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": problems.is_empty(),
                "executable": exe.display().to_string(),
                "agent": dll.display().to_string(),
                "agent_present": agent_present,
                "config": config_path.display().to_string(),
                "config_valid": config_valid,
                "saved_rules": rule_count,
                "monitor_running": monitor_running,
                "windows_available": windows_available,
                "detected_windows": window_count,
                "log": log_path.display().to_string(),
                "problems": problems,
                "notes": notes,
                "capture_verified": false
            }))?
        );
    } else {
        display::header("DIAGNOSTICS", "Read-only checks; no protected application is modified");
        display::field("Agent DLL", format!("{}  {}", if agent_present { display::good("OK") } else { display::bad("MISSING") }, dll.display()));
        display::field("Configuration", format!("{}  {}", if config_valid { display::good("OK") } else { display::bad("ERROR") }, config_path.display()));
        display::field("Saved rules", rule_count);
        display::field("AutoHide", if monitor_running { display::good("Running") } else { display::warn("Stopped") });
        display::field("Window discovery", format!("{} ({} found)", if windows_available { display::good("OK") } else { display::bad("ERROR") }, window_count));
        display::field("Log", log_path.display());
        for note in &notes { display::warning(note); }
        for problem in &problems { display::error(problem); }
        display::note("All affinity readbacks are unverified until checked in a capture preview or real stream.");
    }
    ensure!(
        problems.is_empty(),
        "PawHide doctor found {} installation problem(s)",
        problems.len()
    );
    Ok(())
}

/// A live terminal status view. Does not affect capture state or saved rules.
pub fn watch(interval: u64) -> Result<()> {
    let terminal = io::stdout().is_terminal();
    loop {
        if terminal {
            print!("\x1B[2J\x1B[H");
        } else {
            println!("\n--- PawHide snapshot ---");
        }
        display::header("LIVE STATUS", &format!("AutoHide: {} | Refresh: {interval}s | Ctrl+C to stop",
            if pawhide_core::monitor::running() { "Running" } else { "Stopped" }
        ));
        list(None, false, false, None, WindowFilter::All, false)?;
        io::stdout().flush()?;
        thread::sleep(Duration::from_secs(interval));
    }
}

pub fn change(
    target: Option<&str>, title: Option<&str>, pick: bool, protect: bool,
    allow_shell: bool, verbose: bool,
) -> Result<()> {
    let chosen = target_windows(target, title, pick)?;
    if chosen.len() > 1 && chosen.iter().any(|w| {
        w.exe.eq_ignore_ascii_case("explorer.exe") || w.pid == platform::shell_pid()
    }) {
        bail!("Explorer group changes are blocked; use --title, --pick, or a specific 0x handle")
    }
    let total = chosen.len();
    let mut success = 0;
    display::header(if protect { "EXCLUDE WINDOWS" } else { "RESTORE CAPTURE" },
        "Windows affinity will be checked after each operation");
    for w in &chosen {
        match pawhide_core::helper::apply_shell(w, protect, false, allow_shell, || false) {
            Ok(()) => {
                success += 1;
                display::success(&format!("{}: {}", w.exe, if protect { "Excluded" } else { "Visible" }));
                if verbose { display::field("Window", format!("0x{:X} | PID {} | {}", w.hwnd, w.pid, w.title)); }
            }
            Err(err) => display::error(&format!("{}: {err:#}", w.exe)),
        }
    }
    display::field("Verified", format!("{success}/{total}"));
    display::note("An affinity readback does not guarantee Discord capture behavior.");
    ensure!(success == total, "{} window(s) failed or could not be verified", total - success);
    Ok(())
}

pub fn status(target: Option<&str>, verbose: bool, json_output: bool) -> Result<()> {
    let found = if let Some(target) = target {
        let selected = platform::resolve(target)?;
        ensure!(!selected.is_empty(), "no visible windows match '{target}'");
        selected
    } else {
        platform::windows()?.into_iter().filter(|w| !w.tool_window).collect()
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(
            &json!({"excluded":found.iter().filter(|w|w.affinity==Some(EXCLUDED)).count(),
                    "visible":found.iter().filter(|w|w.affinity==Some(0)).count(),
                    "unknown":found.iter().filter(|w|!matches!(w.affinity,Some(0)|Some(EXCLUDED))).count(),
                    "windows":found.iter().map(entry).collect::<Vec<_>>() })
        )?);
        return Ok(());
    }
    display::header("PROTECTION STATUS", "Windows affinity readback, not confirmed Discord capture output");
    if target.is_some() {
        let rows = found.iter().map(|w| {
            let state = if w.affinity == Some(EXCLUDED) { display::Cell::good("Excluded") }
                else if w.affinity == Some(0) { display::Cell::plain("Visible") }
                else { display::Cell::warn("Unverified") };
            let detail = if verbose {
                format!("PID {} | 0x{:X} | affinity {:?}", w.pid, w.hwnd, w.affinity)
            } else { w.title.clone() };
            vec![display::Cell::plain(&w.exe), state, display::Cell::dim(detail)]
        }).collect::<Vec<_>>();
        display::table(&["Application", "Capture", if verbose { "Process / handle" } else { "Window title" }], &rows);
    }
    let excluded = found.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
    let visible = found.iter().filter(|w| w.affinity == Some(0)).count();
    display::field("Excluded", display::good(&excluded.to_string()));
    display::field("Visible", visible);
    display::field("Unknown", found.len() - excluded - visible);
    display::note("An excluded affinity is not proof of an actual receiving stream.");
    Ok(())
}



pub fn inspect(target: &str, json_output: bool) -> Result<()> {
    let windows = platform::resolve(target)?;
    ensure!(!windows.is_empty(), "no visible windows match '{target}'");
    if json_output {
        println!("{}", serde_json::to_string_pretty(&windows.iter().map(entry).collect::<Vec<_>>())?);
        return Ok(());
    }
    display::header("WINDOW INSPECTOR", "Window ownership and capture affinity details");
    for w in windows {
        display::section(&w.exe);
        display::field("Window title", &w.title);
        display::field("Handle", format!("0x{:X}", w.hwnd));
        display::field("Process ID", w.pid);
        display::field("Capture", platform::state(w.affinity));
        display::field("Tool window", w.tool_window);
    }
    Ok(())
}

pub fn folders(command: FolderCommand) -> Result<()> {
    match command {
        FolderCommand::Add { path } => {
            let canonical = folders::validated_folder(&path)?;
            let mut cfg = config::load()?;
            if !cfg.folders.contains(&canonical) {
                cfg.folders.push(canonical.clone());
                config::save(&cfg)?;
            }
            println!("Protected folder registered: {}", canonical.display());
            println!(
                "Explorer folder windows will be monitored. For Wolfmark documents, exact file-open notifications remain required."
            );
            crate::auto::start()?;
        }
        FolderCommand::Remove { path } => {
            // Allow removing missing/renamed folders and old \?\ saved paths.
            let canonical = folders::display_path(&path);
            let mut cfg = config::load()?;
            cfg.folders.retain(|folder| {
                folder != &canonical
                    && folders::validated_folder(folder).ok().as_ref() != Some(&canonical)
            });
            config::save(&cfg)?;
            println!("Folder rule removed: {}", canonical.display());
        }
        FolderCommand::List => {
            let cfg = config::load()?;
            if cfg.folders.is_empty() {
                println!("No protected folders registered.");
            } else {
                for folder in &cfg.folders {
                    println!("{}", folder.display());
                }
            }
        }
        FolderCommand::Match { path } => {
            let cfg = config::load()?;
            match folders::match_document(&cfg.folders, &path)? {
                Some(folder) => println!("Protected by {}", folder.display()),
                None => println!("Not protected by a folder rule"),
            }
        }
        FolderCommand::Notify { hwnd, path } => {
            let handle = pawhide_core::parse_hwnd(&hwnd)?.context("use 0x... HWND")?;
            let cfg = config::load()?;
            if let Some(folder) = folders::match_document(&cfg.folders, &path)? {
                let window = platform::resolve(&hwnd)?
                    .into_iter()
                    .find(|w| w.hwnd == handle)
                    .context("window handle is not a visible top-level window")?;
                ensure!(
                    platform::can_protect_window(&window),
                    "this application window cannot be protected"
                );
                if window.affinity != Some(EXCLUDED) {
                    crate::auto::protect_bounded(&window)?;
                }
                println!(
                    "Protected {} because document is inside {}",
                    window.exe,
                    folder.display()
                );
            } else {
                println!("No matching protected folder; left capture state unchanged");
            }
        }
    }
    Ok(())
}

pub fn config(command: ConfigCommand) -> Result<()> {
    let path = config::config_path()?;
    match command {
        ConfigCommand::Path => println!("{}", path.display()),
        ConfigCommand::Show => println!("{}", serde_json::to_string_pretty(&config::load()?)?),
        ConfigCommand::Preferences => {
            let p = config::load()?.preferences;
            display::header("PREFERENCES", "Saved options shared by the CLI and WGC preview");
            display::field("show-system-windows", p.show_system_windows);
            display::field("capture-cursor", p.capture_cursor);
            display::field("native-resolution", p.capture_native_resolution);
            display::field("thumbnail", p.capture_thumbnail);
            display::next("pawhide config set capture-cursor true");
        }
        ConfigCommand::Set { key, value } => {
            let mut cfg = config::load()?;
            let name = match key {
                PreferenceKey::ShowSystemWindows => { cfg.preferences.show_system_windows = value; "show-system-windows" }
                PreferenceKey::CaptureCursor => { cfg.preferences.capture_cursor = value; "capture-cursor" }
                PreferenceKey::NativeResolution => { cfg.preferences.capture_native_resolution = value; "native-resolution" }
                PreferenceKey::Thumbnail => { cfg.preferences.capture_thumbnail = value; "thumbnail" }
            };
            config::save(&cfg)?;
            display::success(&format!("Saved {name} = {value}"));
            display::note("Application and folder rules were preserved.");
        }
        ConfigCommand::Validate => {
            config::load()?;
            display::success(&format!("Configuration valid: {}", path.display()));
        }
        ConfigCommand::Edit => {
            if !path.exists() { config::save(&config::Config::default())?; }
            Command::new("notepad.exe").arg(&path).spawn().context("cannot open Notepad")?;
            display::success(&format!("Editing {}", path.display()));
        }
    }
    Ok(())
}

pub fn logs(command: LogsCommand) -> Result<()> {
    let path = config::config_path()?.with_file_name("auto.log");
    fn print_line(line: &str) {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("WARN") { println!("  {}", display::warn(line)); }
        else if upper.starts_with("ERROR") { println!("  {}", display::bad(line)); }
        else if upper.starts_with("INFO") { println!("  {}", display::dim(line)); }
        else { println!("  {line}"); }
    }
    match command {
        LogsCommand::Path => println!("{}", path.display()),
        LogsCommand::Show { lines } => {
            display::header("ACTIVITY LOG", "Recent AutoHide operations and diagnostics");
            if !path.exists() {
                display::note(&format!("No log yet: {}", path.display()));
                return Ok(());
            }
            let content = fs::read_to_string(&path)?;
            for line in content.lines().rev().take(lines.min(5000)).collect::<Vec<_>>().into_iter().rev() {
                print_line(line);
            }
            display::next("pawhide logs follow  |  pawhide doctor");
        }
        LogsCommand::Follow => {
            display::header("FOLLOW ACTIVITY", "Live monitor log; Ctrl+C to stop");
            display::field("Source", path.display());
            let mut offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            loop {
                if let Ok(content) = fs::read(&path) {
                    if (content.len() as u64) < offset { offset = 0; }
                    if (content.len() as u64) > offset {
                        let appended = String::from_utf8_lossy(&content[offset as usize..]);
                        for line in appended.lines() { print_line(line); }
                        io::stdout().flush()?;
                        offset = content.len() as u64;
                    }
                }
                thread::sleep(Duration::from_millis(400));
            }
        }
    }
}

/// Only explicitly identified Explorer taskbars are eligible. This affects
/// compatible capture streams, NOT local window visibility or the taskbar mode.
pub fn capture(command: CaptureCommand) -> Result<()> {
    match command {
        CaptureCommand::Monitors => crate::capture_preview::list_monitors(),
        CaptureCommand::Preview { monitor, cursor, native_resolution, thumbnail } => {
            crate::capture_preview::preview(monitor, cursor, native_resolution, thumbnail)
        }
        CaptureCommand::Taskbar { command } => {
            let taskbars = platform::taskbar_windows()?;
            ensure!(!taskbars.is_empty(), "no Explorer taskbar windows detected");
            match command {
                TaskbarCommand::Status => {
                    for bar in &taskbars {
                        println!(
                            "{} Â· {} Â· HWND 0x{:X}",
                            bar.title,
                            platform::state(bar.affinity),
                            bar.hwnd
                        );
                    }
                    println!("Capture affinity does not verify Discord's actual captured frames.");
                    Ok(())
                }
                TaskbarCommand::Hide | TaskbarCommand::Show => {
                    let hide = matches!(command, TaskbarCommand::Hide);
                    let target = if hide { EXCLUDED } else { 0 };
                    let mut passed = 0usize;
                    for bar in &taskbars {
                        // Explicit shell exception applies only to discovered
                        // Shell_TrayWnd / Shell_SecondaryTrayWnd windows.
                        if bar.affinity == Some(target) {
                            passed += 1;
                            continue;
                        }
                        match pawhide_core::helper::apply_shell(bar, hide, false, true, || false)
                            .map(|()| true)
                        {
                            Ok(true) => {
                                passed += 1;
                                println!(
                                    "{}: {}",
                                    bar.title,
                                    if hide {
                                        "excluded from compatible capture"
                                    } else {
                                        "capture restored"
                                    }
                                );
                            }
                            Ok(false) => eprintln!(
                                "{}: affinity readback unavailable; state NOT verified",
                                bar.title
                            ),
                            Err(error) => eprintln!("{}: {error:#}", bar.title),
                        }
                    }
                    ensure!(
                        passed == taskbars.len(),
                        "{passed}/{} taskbars verified; check capture and restore if needed",
                        taskbars.len()
                    );
                    println!("Local taskbar visibility and Windows settings were not changed.");
                    Ok(())
                }
            }
        }
    }
}
