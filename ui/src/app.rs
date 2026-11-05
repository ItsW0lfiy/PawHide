//! PawHide native Slint frontend.
//! Capture affinity, injection, helper timeouts and automatic rules remain in pawhide-core/CLI.
#![cfg(windows)]
use crate::{
    capture_native::{CaptureSession, TARGET_FPS},
    icons,
    operations,
};
use pawhide_core::{
    config::{self, Config, Preferences},
    monitor,
    platform::{self, Window, EXCLUDED},
};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, Sender}},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW},
};

slint::include_modules!();

#[derive(Clone)]
struct Display {
    handle: usize,
    device: String,
    label: String,
}

fn physical_monitors() -> Result<Vec<Display>, String> {
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, arg: LPARAM) -> i32 {
        let out = unsafe { &mut *(arg as *mut Vec<Display>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 {
            return 1;
        }
        let end = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        let device = String::from_utf16_lossy(&info.szDevice[..end]);
        let number = device.rsplit("DISPLAY").next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(999);
        let rect = info.monitorInfo.rcMonitor;
        let primary = info.monitorInfo.dwFlags & 1 != 0;
        out.push(Display {
            handle: monitor as usize,
            device,
            label: format!("Screen {number} · {}×{}{}", rect.right - rect.left, rect.bottom - rect.top,
                if primary { " · primary" } else { "" }),
        });
        1
    }
    let mut displays = Vec::new();
    if unsafe { EnumDisplayMonitors(
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        Some(collect),
        &mut displays as *mut _ as LPARAM,
    ) } == 0 {
        return Err("Cannot enumerate displays".into());
    }
    displays.sort_by(|a, b| a.device.cmp(&b.device));
    if displays.is_empty() { return Err("No physical display detected".into()); }
    Ok(displays)
}

struct Capture {
    displays: Vec<Display>,
    selected_device: Option<String>,
    session: Option<CaptureSession>,
    last_scan: Instant,
    last_second: Instant,
    start_at: Instant,
    previous: (u64, u64, u64),
    last_processed: f32,
    last_source: f32,
    displayed: u64,
    last_displayed: u64,
    error: String,
}
impl Capture {
    fn new() -> Self {
        Self {
            displays: Vec::new(), selected_device: None, session: None,
            last_scan: Instant::now() - Duration::from_secs(10),
            last_second: Instant::now(), start_at: Instant::now(),
            previous: (0, 0, 0), last_processed: 0.0, last_source: 0.0,
            displayed: 0, last_displayed: 0, error: String::new(),
        }
    }
    fn selected(&self) -> Option<&Display> {
        self.selected_device.as_ref().and_then(|id| self.displays.iter().find(|d| &d.device == id))
    }
    fn stop(&mut self) {
        self.session = None;
        self.previous = (0, 0, 0);
        self.displayed = 0;
        self.last_displayed = 0;
        self.last_processed = 0.0;
        self.last_source = 0.0;
    }
    fn start(&mut self, preferences: &Preferences) {
        self.stop();
        if let Some(handle) = self.selected().map(|d| d.handle) {
            self.session = Some(CaptureSession::start(
                handle, preferences.capture_cursor, preferences.capture_native_resolution,
            ));
            self.error.clear();
            self.start_at = Instant::now();
            self.last_second = Instant::now();
        } else {
            self.error = "Select a connected display first.".into();
        }
    }
    fn scan(&mut self, ui: &PawHideWindow) {
        if self.last_scan.elapsed() < Duration::from_secs(3) { return; }
        self.last_scan = Instant::now();
        match physical_monitors() {
            Ok(found) => {
                let chosen = self.selected_device.clone()
                    .or_else(|| found.first().map(|d| d.device.clone()));
                let index = chosen.as_ref().and_then(|d| found.iter().position(|x| &x.device == d));
                let previously_present = self.selected().is_some();
                self.displays = found;
                self.selected_device = chosen;
                ui.set_displays(ModelRc::new(VecModel::from(
                    self.displays.iter().map(|d| SharedString::from(d.label.as_str())).collect::<Vec<_>>()
                )));
                if let Some(i) = index {
                    ui.set_display_index(i as i32);
                } else {
                    ui.set_display_index(-1);
                    if previously_present && self.session.is_some() {
                        self.stop();
                        self.error = "Selected display disconnected. Reconnect it or select another display.".into();
                    }
                }
            }
            Err(reason) => {
                self.displays.clear();
                self.stop();
                self.error = reason;
                ui.set_displays(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
                ui.set_display_index(-1);
            }
        }
    }
    fn tick(&mut self, ui: &PawHideWindow) {
        self.scan(ui);
        let show_preview = ui.get_page() == 2 || ui.get_thumbnail();
        if let Some(session) = &self.session {
            if let Some(error) = session.error() {
                self.error = error;
                self.stop();
            } else if self.start_at.elapsed() >= Duration::from_secs(10)
                && session.counts().1 == 0
            {
                self.error = "Capture produced no usable frames within 10 seconds. Check the display and retry.".into();
                self.stop();
            } else {
                if let Some(frame) = session.take_latest() {
                    if show_preview && frame.width > 0 && frame.height > 0 {
                        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                            &frame.rgba, frame.width, frame.height,
                        );
                        ui.set_preview(Image::from_rgba8(buffer));
                        self.displayed += 1;
                    }
                    session.recycle(frame);
                }
                if self.last_second.elapsed() >= Duration::from_secs(1) {
                    let counts = session.counts();
                    let seconds = self.last_second.elapsed().as_secs_f32().max(0.01);
                    self.last_source = counts.0.saturating_sub(self.previous.0) as f32 / seconds;
                    self.last_processed = counts.1.saturating_sub(self.previous.1) as f32 / seconds;
                    let preview = self.displayed.saturating_sub(self.last_displayed) as f32 / seconds;
                    ui.set_capture_stats(format!(
                        "Source {:.0} · Processed {:.0} · Preview {:.0} FPS · Target {TARGET_FPS}",
                        self.last_source, self.last_processed, preview
                    ).into());
                    self.previous = counts;
                    self.last_displayed = self.displayed;
                    self.last_second = Instant::now();
                }
            }
        }
        ui.set_capture_running(self.session.is_some());
        ui.set_capture_error(self.error.as_str().into());
    }
}

struct Snapshot {
    windows: Vec<Window>,
    config: Config,
    monitor_running: bool,
    log: String,
}

#[derive(Clone)]
struct RowAction {
    windows: Vec<Window>,
    key: String,
    expanded: bool,
}
struct Completion {
    message: String,
    detail: String,
    failures: Vec<(usize, u32)>,
    error: bool,
}
struct Runtime {
    windows: Vec<Window>,
    config: Config,
    rows: Vec<RowAction>,
    expanded: HashSet<String>,
    icons: HashMap<String, Image>,
    icon_pending: HashSet<String>,
    icon_failures: HashMap<String, Instant>,
    failed_windows: HashSet<(usize, u32)>,
    retry_after: HashMap<(usize, u32), Instant>,
    icon_tx: Sender<(String, Option<icons::IconPixels>)>,
    icon_rx: Receiver<(String, Option<icons::IconPixels>)>,
    snapshot_tx: Sender<Result<Snapshot, String>>,
    snapshot_rx: Receiver<Result<Snapshot, String>>,
    work_tx: Sender<Completion>,
    work_rx: Receiver<Completion>,
    monitor_tx: Sender<Result<String, String>>,
    monitor_rx: Receiver<Result<String, String>>,
    capture: Capture,
    activity: VecDeque<String>,
    cancel: Arc<AtomicBool>,
    busy: bool,
    discovering: bool,
    monitor_running: bool,
    monitor_starting: bool,
    monitor_error: bool,
    last_discovery: Instant,
    last_monitor_attempt: Instant,
    last_log: String,
    dirty: bool,
}
impl Runtime {
    fn new() -> Self {
        let (icon_tx, icon_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (work_tx, work_rx) = mpsc::channel();
        let (monitor_tx, monitor_rx) = mpsc::channel();
        Self {
            windows: Vec::new(), config: Config::default(), rows: Vec::new(),
            expanded: HashSet::new(), icons: HashMap::new(), icon_pending: HashSet::new(),
            icon_failures: HashMap::new(), failed_windows: HashSet::new(),
            retry_after: HashMap::new(), icon_tx, icon_rx, snapshot_tx, snapshot_rx,
            work_tx, work_rx, monitor_tx, monitor_rx,
            capture: Capture::new(), activity: VecDeque::new(),
            cancel: Arc::new(AtomicBool::new(false)), busy: false,
            discovering: false, monitor_running: false, monitor_starting: false,
            monitor_error: false, last_discovery: Instant::now() - Duration::from_secs(10),
            last_monitor_attempt: Instant::now() - Duration::from_secs(60),
            last_log: String::new(), dirty: true,
        }
    }
    fn record(&mut self, message: impl Into<String>, detail: impl AsRef<str>, error: bool, ui: &PawHideWindow) {
        let message = message.into();
        let text = if detail.as_ref().trim().is_empty() { message.clone() } else {
            format!("{message}\n{}", detail.as_ref().trim())
        };
        if self.activity.len() >= 200 { self.activity.pop_front(); }
        self.activity.push_back(text);
        ui.set_status_message(message.into());
        ui.set_has_error(error);
        ui.set_activity(ModelRc::new(VecModel::from(
            self.activity.iter().rev().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>()
        )));
    }
    fn discover(&mut self) {
        if self.discovering || self.last_discovery.elapsed() < Duration::from_secs(2) { return; }
        self.discovering = true;
        self.last_discovery = Instant::now();
        let sender = self.snapshot_tx.clone();
        thread::spawn(move || {
            let result: Result<Snapshot, String> = (|| {
                let cfg = config::load().map_err(|e| format!("Configuration: {e:#}"))?;
                let windows = platform::windows().map_err(|e| format!("Window discovery: {e:#}"))?;
                let log = config::config_path().ok()
                    .and_then(|p| std::fs::read_to_string(p.with_file_name("auto.log")).ok())
                    .unwrap_or_default();
                Ok(Snapshot {
                    windows, config: cfg, monitor_running: monitor::running(),
                    log: log.chars().rev().take(16_384).collect::<String>().chars().rev().collect(),
                })
            })();
            let _ = sender.send(result);
        });
    }
    fn ensure_monitor(&mut self) {
        if self.monitor_running || self.monitor_starting ||
            (self.config.applications.is_empty() && self.config.folders.is_empty()) ||
            self.last_monitor_attempt.elapsed() < Duration::from_secs(30)
        { return; }
        self.monitor_starting = true;
        self.last_monitor_attempt = Instant::now();
        let sender = self.monitor_tx.clone();
        thread::spawn(move || {
            let _ = sender.send(operations::command(
                &["auto".into(), "start".into()], &AtomicBool::new(false),
            ));
        });
    }
    fn monitor_state(&self) -> &'static str {
        if self.monitor_running { "Running" }
        else if self.monitor_starting { "Starting" }
        else if self.config.applications.is_empty() && self.config.folders.is_empty() { "Not configured" }
        else if self.monitor_error { "Error" }
        else { "Not running" }
    }
    fn spawn_work(
        &mut self, ui: &PawHideWindow, summary: &str,
        work: impl FnOnce(Arc<AtomicBool>) -> Completion + Send + 'static,
    ) {
        if self.busy { return; }
        self.busy = true;
        ui.set_busy(true);
        ui.set_status_message(summary.into());
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let sender = self.work_tx.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(cancel)))
                .unwrap_or(Completion {
                    message: "Operation worker stopped unexpectedly".into(),
                    detail: "Outcome unverified. Inspect the actual capture and Activity.".into(),
                    failures: Vec::new(), error: true,
                });
            let _ = sender.send(result);
        });
    }
    fn command(&mut self, ui: &PawHideWindow, args: Vec<String>) {
        self.spawn_work(ui, "Applying operation…", move |cancel| {
            match operations::command(&args, &cancel) {
                Ok(detail) => Completion { message: "Operation completed".into(), detail, failures: Vec::new(), error: false },
                Err(detail) => Completion { message: "Operation failed".into(), detail, failures: Vec::new(), error: true },
            }
        });
    }
    fn change_preferences(&mut self, ui: &PawHideWindow) {
        let prefs = Preferences {
            show_system_windows: ui.get_show_system(),
            capture_cursor: ui.get_include_cursor(),
            capture_native_resolution: ui.get_native_resolution(),
            capture_thumbnail: ui.get_thumbnail(),
        };
        if self.config.preferences == prefs { return; }
        let previous = self.config.preferences.clone();
        self.config.preferences = prefs;
        if let Some(session) = &self.capture.session {
            session.set_cursor(self.config.preferences.capture_cursor);
        }
        if previous.capture_native_resolution != self.config.preferences.capture_native_resolution
            && self.capture.session.is_some() {
            self.capture.start(&self.config.preferences);
        }
        self.dirty = true;
        let value = self.config.preferences.clone();
        let result: Result<(), String> = (|| {
            let mut cfg = config::load().map_err(|e| format!("{e:#}"))?;
            cfg.preferences = value;
            config::save(&cfg).map_err(|e| format!("{e:#}"))
        })();
        match result {
            Ok(()) => self.record("Preferences saved", "", false, ui),
            Err(detail) => self.record("Preferences could not be saved", detail, true, ui),
        }
    }
    fn row_status(&self, windows: &[Window]) -> &'static str {
        let excluded = windows.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
        if excluded == windows.len() { "Excluded" }
        else if excluded > 0 { "Partially excluded" }
        else if windows.iter().all(|w| !platform::can_protect_window(w)) { "Unsupported" }
        else if windows.iter().any(|w| self.failed_windows.contains(&(w.hwnd, w.pid))) { "Failed" }
        else if windows.iter().any(|w| w.affinity == Some(1)) { "Blacked / WDA_MONITOR" }
        else if windows.iter().any(|w| w.affinity != Some(0)) { "Unknown" }
        else { "Visible" }
    }
    fn queue_icon(&mut self, name: &str, window: &Window) {
        let key = name.to_ascii_lowercase();
        if self.icons.contains_key(&key) || self.icon_pending.len() >= 4
            || self.icon_pending.contains(&key)
            || self.icon_failures.get(&key).is_some_and(|when| when.elapsed() < Duration::from_secs(30))
        { return; }
        self.icon_pending.insert(key.clone());
        let sender = self.icon_tx.clone();
        let (hwnd, pid) = (window.hwnd, window.pid);
        thread::spawn(move || {
            let result = icons::from_window(hwnd, pid);
            let _ = sender.send((key, result));
        });
    }
    fn refresh_rows(&mut self, ui: &PawHideWindow) {
        let query = ui.get_search().to_ascii_lowercase();
        let filter = ui.get_filter();
        let mut groups: BTreeMap<String, Vec<Window>> = BTreeMap::new();
        let current = self.windows.clone();
        for window in current {
            if !self.config.preferences.show_system_windows
                && (window.tool_window || platform::is_background_ui_window(&window))
            { continue; }
            groups.entry(window.exe.to_ascii_lowercase()).or_default().push(window);
        }
        let mut entries = Vec::new();
        self.rows.clear();
        for (name, windows) in groups {
            if !query.is_empty() && !name.contains(&query)
                && !windows.iter().any(|w| w.title.to_ascii_lowercase().contains(&query))
            { continue; }
            let status = self.row_status(&windows);
            if match filter {
                1 => !windows.iter().any(|w| w.affinity == Some(EXCLUDED)),
                2 => !windows.iter().any(|w| w.affinity == Some(0)),
                3 => matches!(status, "Excluded" | "Visible"),
                _ => false,
            } { continue; }
            let explorer = name == "explorer.exe";
            let expanded = self.expanded.contains(&name);
            let details = windows.len() > 1 || explorer;
            self.queue_icon(&name, &windows[0]);
            let image = self.icons.get(&name).cloned().unwrap_or_default();
            let all_excluded = windows.iter().all(|w| w.affinity == Some(EXCLUDED));
            let supported = !explorer && windows.iter().any(platform::can_protect_window);
            let cooling = windows.iter().any(|w|
                self.retry_after.get(&(w.hwnd, w.pid)).is_some_and(|when| when.elapsed() < Duration::from_secs(30)));
            entries.push(WindowEntry {
                name: SharedString::from(name.as_str()), count: windows.len().to_string().into(),
                state: status.into(),
                action: (if details { if expanded { "Collapse" } else { "Windows" } }
                    else if all_excluded { "Show" } else { "Hide" }).into(),
                icon: image.clone(),
                enabled: supported && !cooling,
                expandable: details,
                child: false,
            });
            self.rows.push(RowAction { key: name.clone(), windows: windows.clone(), expanded: details });
            if expanded && details {
                for window in windows {
                    let status = self.row_status(std::slice::from_ref(&window));
                    let supported = platform::can_protect_window(&window)
                        && !self.retry_after.get(&(window.hwnd, window.pid))
                            .is_some_and(|when| when.elapsed() < Duration::from_secs(30));
                    let excluded = window.affinity == Some(EXCLUDED);
                    entries.push(WindowEntry {
                        name: (if window.title.trim().is_empty() { "(untitled window)" } else { window.title.as_str() }).into(),
                        count: format!("0x{:X}", window.hwnd).into(), state: status.into(),
                        action: (if excluded { "Show" } else { "Hide" }).into(),
                        icon: image.clone(), enabled: supported, expandable: false, child: true,
                    });
                    self.rows.push(RowAction { key: name.clone(), windows: vec![window], expanded: false });
                }
            }
        }
        ui.set_window_rows(ModelRc::new(VecModel::from(entries)));
        ui.set_applications(ModelRc::new(VecModel::from(
            self.config.applications.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>()
        )));
        ui.set_folders(ModelRc::new(VecModel::from(
            self.config.folders.iter().map(|p| SharedString::from(p.display().to_string())).collect::<Vec<_>>()
        )));
        ui.set_excluded_count(format!("{} excluded", self.windows.iter().filter(|w| w.affinity == Some(EXCLUDED)).count()).into());
        ui.set_auto_status(self.monitor_state().into());
        self.dirty = false;
    }
    fn action_row(&mut self, ui: &PawHideWindow, index: usize) {
        let Some(record) = self.rows.get(index).cloned() else { return; };
        if record.expanded {
            if !self.expanded.insert(record.key.clone()) { self.expanded.remove(&record.key); }
            self.dirty = true;
            return;
        }
        if self.busy || record.windows.is_empty() { return; }
        let windows = record.windows;
        let protect = !windows.iter().all(|w| w.affinity == Some(EXCLUDED));
        if windows.iter().any(|w| !platform::can_protect_window(w)) { return; }
        for w in &windows { self.failed_windows.remove(&(w.hwnd, w.pid)); }
        self.spawn_work(ui, if protect { "Applying capture exclusion…" } else { "Restoring capture…" }, move |cancel| {
            let total = windows.len();
            let mut successes = 0;
            let mut failures = Vec::new();
            let mut messages = Vec::new();
            for window in windows {
                if cancel.load(Ordering::Relaxed) {
                    messages.push("Cancelled; any operation already in progress may still complete.".to_owned());
                    break;
                }
                match operations::apply(&window, protect, &cancel) {
                    Ok(()) => {
                        successes += 1;
                        messages.push(format!("{} · HWND 0x{:X}: affinity readback verified", window.exe, window.hwnd));
                    }
                    Err(err) => {
                        failures.push((window.hwnd, window.pid));
                        messages.push(format!("{} · HWND 0x{:X}: {err}", window.exe, window.hwnd));
                    }
                }
            }
            Completion {
                message: format!("{successes}/{total} windows {} · verify real capture output",
                    if protect { "excluded" } else { "restored" }),
                detail: messages.join("\n"),
                failures,
                error: successes != total,
            }
        });
    }
    fn tick(&mut self, ui: &PawHideWindow) {
        while let Ok(result) = self.snapshot_rx.try_recv() {
            self.discovering = false;
            match result {
                Ok(snapshot) => {
                    self.windows = snapshot.windows;
                    self.config = snapshot.config;
                    self.monitor_running = snapshot.monitor_running;
                    if self.monitor_running { self.monitor_error = false; }
                    self.failed_windows.retain(|(hwnd, pid)|
                        self.windows.iter().any(|w| w.hwnd == *hwnd && w.pid == *pid));
                    if snapshot.log != self.last_log && !snapshot.log.is_empty() {
                        let line = snapshot.log.lines().last().unwrap_or_default();
                        if !line.is_empty() { self.record("AutoHide log updated", line, line.contains("ERROR"), ui); }
                        self.last_log = snapshot.log;
                    }
                    ui.set_show_system(self.config.preferences.show_system_windows);
                    ui.set_include_cursor(self.config.preferences.capture_cursor);
                    ui.set_native_resolution(self.config.preferences.capture_native_resolution);
                    ui.set_thumbnail(self.config.preferences.capture_thumbnail);
                    self.dirty = true;
                }
                Err(err) => self.record("Window discovery failed", err, true, ui),
            }
        }
        while let Ok(result) = self.work_rx.try_recv() {
            self.busy = false;
            ui.set_busy(false);
            for key in result.failures {
                self.failed_windows.insert(key);
                self.retry_after.insert(key, Instant::now());
            }
            self.record(result.message, result.detail, result.error, ui);
            self.last_discovery = Instant::now() - Duration::from_secs(10);
            self.dirty = true;
        }
        while let Ok(result) = self.monitor_rx.try_recv() {
            self.monitor_starting = false;
            self.monitor_running = monitor::running();
            self.monitor_error = result.is_err() || !self.monitor_running;
            match result {
                Ok(msg) => self.record("AutoHide started", msg, false, ui),
                Err(err) => self.record("AutoHide startup failed; retry in 30 seconds", err, true, ui),
            }
            self.dirty = true;
        }
        while let Ok((key, pixels)) = self.icon_rx.try_recv() {
            self.icon_pending.remove(&key);
            if let Some(p) = pixels {
                if p.width > 0 && p.height > 0 {
                    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&p.rgba, p.width as u32, p.height as u32);
                    self.icons.insert(key, Image::from_rgba8(buffer));
                }
            } else {
                self.icon_failures.insert(key, Instant::now());
            }
            self.dirty = true;
        }
        self.discover();
        self.ensure_monitor();
        self.capture.tick(ui);
        if self.dirty { self.refresh_rows(ui); }
        ui.set_auto_status(self.monitor_state().into());
    }
}

fn with_runtime<F: FnOnce(&mut Runtime, &PawHideWindow)>(
    state: &Rc<RefCell<Runtime>>, ui: &slint::Weak<PawHideWindow>, work: F,
) {
    if let Some(ui) = ui.upgrade() {
        work(&mut state.borrow_mut(), &ui);
    }
}
pub fn run() -> Result<(), slint::PlatformError> {
    let ui = PawHideWindow::new()?;
    let state = Rc::new(RefCell::new(Runtime::new()));
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_window_action(move |index| with_runtime(&state, &weak, |s, u| s.action_row(u, index as usize)));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_window_expand(move |index| with_runtime(&state, &weak, |s, u| s.action_row(u, index as usize)));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_search_changed(move || with_runtime(&state, &weak, |s, u| {
            s.dirty = true;
            s.refresh_rows(u);
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_add_application(move |name| with_runtime(&state, &weak, |s, u| {
            if !name.trim().is_empty() { s.command(u, vec!["auto".into(), "add".into(), name.to_string()]); }
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_remove_application(move |name| with_runtime(&state, &weak, |s, u| {
            s.command(u, vec!["auto".into(), "remove".into(), name.to_string()]);
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_add_folder(move |folder| with_runtime(&state, &weak, |s, u| {
            if !folder.trim().is_empty() { s.command(u, vec!["folders".into(), "add".into(), folder.to_string()]); }
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_remove_folder(move |folder| with_runtime(&state, &weak, |s, u| {
            s.command(u, vec!["folders".into(), "remove".into(), folder.to_string()]);
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_capture_toggle(move || with_runtime(&state, &weak, |s, u| {
            if s.capture.session.is_some() { s.capture.stop(); }
            else { s.capture.start(&s.config.preferences); }
            s.capture.tick(u);
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_display_selected(move |index| with_runtime(&state, &weak, |s, u| {
            let next = s.capture.displays.get(index.max(0) as usize).map(|d| d.device.clone());
            if let Some(next) = next {
                s.capture.selected_device = Some(next);
                if s.capture.session.is_some() { s.capture.start(&s.config.preferences); }
                s.capture.tick(u);
            }
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_preferences_changed(move || with_runtime(&state, &weak, |s, u| s.change_preferences(u)));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_startup_action(move |cmd| with_runtime(&state, &weak, |s, u| {
            s.command(u, vec!["startup".into(), cmd.to_string()]);
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_taskbar_action(move |cmd| with_runtime(&state, &weak, |s, u| {
            s.command(u, vec!["capture".into(), "taskbar".into(), cmd.to_string()]);
        }));
    }
    {
        let weak = ui.as_weak(); let state = state.clone();
        ui.on_cancel_operation(move || with_runtime(&state, &weak, |s, u| {
            s.cancel.store(true, Ordering::Relaxed);
            u.set_status_message("Cancellation requested; remote operations may still finish.".into());
        }));
    }

    state.borrow_mut().tick(&ui);
    let weak = ui.as_weak();
    let timer_state = state.clone();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(ui) = weak.upgrade() {
            timer_state.borrow_mut().tick(&ui);
        }
    });
    let result = ui.run();
    state.borrow().cancel.store(true, Ordering::Relaxed);
    drop(timer);
    result
}
