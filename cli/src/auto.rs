use crate::AutoCommand;
use pawhide_core::{config, platform};
use anyhow::{Context, Result, ensure};
use std::{collections::HashMap, fs::OpenOptions, os::windows::process::CommandExt, process::{Command, Stdio}, sync::{Mutex, OnceLock}, thread, time::{Duration, Instant}};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0},
    System::Threading::{CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, CREATE_NO_WINDOW, EVENT_MODIFY_STATE},
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS},
    },
};

const EVENT_NAME: &str = "Local\\PawHide.Auto.Stop";
static EVENTS: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();

fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }

fn running_handle() -> Option<HANDLE> {
    let name = wide(EVENT_NAME);
    let h = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if h.is_null() { None } else { Some(h) }
}

fn is_running() -> bool {
    if let Some(h) = running_handle() { unsafe { CloseHandle(h) }; true } else { false }
}

fn list_rules() -> Result<()> {
    let cfg = config::load()?;
    if cfg.applications.is_empty() {
        println!("No AutoHide applications configured. Try: pawhide auto add PicView.exe");
    } else {
        println!("AutoHide applications:");
        for name in cfg.applications { println!("  {name}"); }
    }
    Ok(())
}

fn status() -> Result<()> {
    println!("AutoHide: {}", if is_running() { "Running" } else { "Stopped" });
    let cfg = config::load()?;
    println!("Configured applications: {}", cfg.applications.len());
    for name in cfg.applications {
        let found = platform::resolve(&name)?;
        let verified = found.iter().filter(|w| w.affinity == Some(platform::EXCLUDED)).count();
        println!("  {:<25} {verified}/{} excluded", name, found.len());
    }
    Ok(())
}

fn start() -> Result<()> {
    ensure!(!is_running(), "AutoHide is already running");
    ensure!(!config::load()?.applications.is_empty(), "add an application first (pawhide auto add PicView.exe)");
    let log_path = config::config_path()?.with_file_name("auto.log");
    std::fs::create_dir_all(log_path.parent().expect("log has parent"))?;
    let log = OpenOptions::new().create(true).append(true).open(&log_path)?;
    let executable = std::env::current_exe()?;
    let child = Command::new(executable)
        .args(["auto", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("could not start AutoHide monitor")?;
    for _ in 0..15 {
        if is_running() { println!("AutoHide started (PID {}). Log: {}", child.id(), log_path.display()); return Ok(()); }
        thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("monitor did not report running. Inspect {}", log_path.display())
}

fn stop() -> Result<()> {
    let h = running_handle().context("AutoHide is not running")?;
    let success = unsafe { SetEvent(h) };
    unsafe { CloseHandle(h) };
    ensure!(success != 0, "could not signal AutoHide monitor");
    for _ in 0..20 {
        if !is_running() { println!("AutoHide stopped. Existing exclusions remain until explicitly shown or windows close."); return Ok(()); }
        thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("stop was requested, but AutoHide has not exited yet")
}

pub fn command(command: AutoCommand) -> Result<()> {
    match command {
        AutoCommand::Add {application} => {
            let name = config::normalize_exe(&application)?;
            let mut cfg = config::load()?;
            if cfg.applications.contains(&name) { println!("Already configured: {name}"); }
            else {
                cfg.applications.push(name.clone());
                cfg.applications.sort();
                config::save(&cfg)?;
                println!("Added {name} to AutoHide. {}", if is_running() { "The monitor will pick it up shortly." } else { "Run 'pawhide auto start' to activate." });
            }
            Ok(())
        }
        AutoCommand::Remove {application} => {
            let name = config::normalize_exe(&application)?;
            let mut cfg = config::load()?;
            let before = cfg.applications.len();
            cfg.applications.retain(|n| n != &name);
            if before == cfg.applications.len() { println!("Not configured: {name}"); }
            else { config::save(&cfg)?; println!("Removed {name} from AutoHide. Already-hidden windows remain excluded until you run 'pawhide show {name}'."); }
            Ok(())
        }
        AutoCommand::List => list_rules(),
        AutoCommand::Status => status(),
        AutoCommand::Start => start(),
        AutoCommand::Stop => stop(),
        AutoCommand::Run => run_monitor(),
        AutoCommand::Sync => {
            let cfg = config::load()?;
            ensure!(!cfg.applications.is_empty(), "no AutoHide rules configured");
            let mut retries = HashMap::new();
            scan(&cfg, &mut retries, true)?;
            println!("AutoHide rules applied to currently open windows");
            Ok(())
        },
    }
}

unsafe extern "system" fn win_event_callback(
    _hook: *mut core::ffi::c_void,
    event: u32,
    hwnd: *mut core::ffi::c_void,
    object_id: i32,
    child_id: i32,
    _thread: u32,
    _time: u32,
) {
    if (event == EVENT_OBJECT_CREATE || event == EVENT_OBJECT_SHOW) && !hwnd.is_null() && object_id == 0 && child_id == 0 {
        if let Some(queue) = EVENTS.get() {
            if let Ok(mut values) = queue.lock() {
                if values.len() < 4096 { values.push(hwnd as usize); }
            }
        }
    }
}

/// The long-running AutoHide monitor must never perform DLL injection in its
/// own thread. A failing app can otherwise freeze monitoring and stop signals.
fn protect_bounded(w: &platform::Window) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut child = Command::new(exe)
        .args(["hide", &format!("0x{:X}", w.hwnd)])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().context("could not launch protection helper")?;
    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => {
                let output = child.wait_with_output()?;
                if output.status.success() { return Ok(()); }
                let error = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("{}", error.trim());
            }
            None if start.elapsed() >= Duration::from_secs(8) => {
                let _ = child.kill(); let _ = child.wait();
                anyhow::bail!("protection timed out after 8s; remote state unverified");
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn scan(cfg: &config::Config, retries: &mut HashMap<(usize, u32), Instant>, log_failures: bool) -> Result<()> {
    let now = Instant::now();
    let all = platform::windows()?;
    retries.retain(|(hwnd, pid), _| all.iter().any(|w| w.hwnd == *hwnd && w.pid == *pid));
    for w in all {
        if !cfg.applications.iter().any(|name| w.exe.eq_ignore_ascii_case(name)) { continue; }
        if w.affinity == Some(platform::EXCLUDED) { retries.remove(&(w.hwnd, w.pid)); continue; }
        if !platform::can_protect_window(&w) { continue; }
        // Do not continually re-inject into uncooperative processes.
        if retries.get(&(w.hwnd, w.pid)).is_some_and(|at| now.duration_since(*at) < Duration::from_secs(120)) { continue; }
        retries.insert((w.hwnd, w.pid), now);
        match protect_bounded(&w) {
            Ok(()) => { retries.remove(&(w.hwnd, w.pid)); },
            Err(e) => { if log_failures { eprintln!("ERROR {} 0x{:X}: {e:#}", w.exe, w.hwnd); } },
        }
    }
    Ok(())
}

fn run_monitor() -> Result<()> {
    let name = wide(EVENT_NAME);
    let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    ensure!(!stop.is_null(), "could not create AutoHide monitor event");
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(stop) };
        anyhow::bail!("another AutoHide monitor is running");
    }
    EVENTS.get_or_init(|| Mutex::new(Vec::new()));
    let hook = unsafe { SetWinEventHook(EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, std::ptr::null_mut(), Some(win_event_callback), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) };
    if hook.is_null() { unsafe { CloseHandle(stop) }; anyhow::bail!("SetWinEventHook failed") }
    let mut cfg = config::load()?;
    let mut retries = HashMap::new();
    let mut last_scan = Instant::now() - Duration::from_secs(10);
    let mut last_reload = Instant::now();
    loop {
        if unsafe { WaitForSingleObject(stop, 0) } == WAIT_OBJECT_0 { break; }
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe { TranslateMessage(&msg); DispatchMessageW(&msg); }
        }
        let pending = EVENTS.get().and_then(|events| events.lock().ok()).is_some_and(|mut queue| {
            if queue.is_empty() { false } else { queue.clear(); true }
        });
        if last_reload.elapsed() >= Duration::from_secs(1) {
            match config::load() { Ok(new) => { if new.applications != cfg.applications { last_scan = Instant::now() - Duration::from_secs(10); } cfg = new; }, Err(err) => eprintln!("WARN configuration reload: {err:#}") }
            last_reload = Instant::now();
        }
        if pending || last_scan.elapsed() >= Duration::from_secs(2) {
            if let Err(err) = scan(&cfg, &mut retries, true) { eprintln!("WARN window scan: {err:#}"); }
            last_scan = Instant::now();
        }
        thread::sleep(Duration::from_millis(40));
    }
    unsafe { UnhookWinEvent(hook); CloseHandle(stop); }
    Ok(())
}
