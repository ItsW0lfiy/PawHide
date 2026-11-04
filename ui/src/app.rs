use crate::{
    capture_test::CaptureTest,
    icons::{self, IconPixels},
    operations,
};
use eframe::egui::{self, Color32, RichText};
use pawhide_core::{
    config::{self, Config},
    monitor,
    platform::{self, EXCLUDED, Window},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

const BACKGROUND: Color32 = Color32::from_rgb(20, 18, 26);
const SIDEBAR: Color32 = Color32::from_rgb(26, 23, 33);
const PANEL: Color32 = Color32::from_rgb(32, 28, 41);
const HOVER: Color32 = Color32::from_rgb(43, 36, 55);
const OUTLINE: Color32 = Color32::from_rgb(60, 50, 73);
const PURPLE: Color32 = Color32::from_rgb(188, 150, 249);
const PURPLE_DIM: Color32 = Color32::from_rgb(78, 54, 112);
const TEXT: Color32 = Color32::from_rgb(239, 234, 246);
const MUTED: Color32 = Color32::from_rgb(162, 153, 179);
const SUCCESS: Color32 = Color32::from_rgb(147, 216, 173);
const WARNING: Color32 = Color32::from_rgb(233, 187, 123);
const ERROR: Color32 = Color32::from_rgb(240, 137, 150);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Windows,
    Rules,
    Capture,
    Activity,
    Settings,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Excluded,
    Visible,
    Attention,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Name,
    Count,
    Status,
}

struct Completion {
    summary: String,
    details: String,
    error: bool,
    failed_windows: Vec<(usize, u32)>,
}
impl Completion {
    fn result(result: Result<String, String>) -> Self {
        match result {
            Ok(summary) => Self {
                summary,
                details: String::new(),
                error: false,
                failed_windows: Vec::new(),
            },
            Err(details) => Self {
                summary: "Operation failed. See Activity for details.".into(),
                details,
                error: true,
                failed_windows: Vec::new(),
            },
        }
    }
}
struct Activity {
    at: Duration,
    summary: String,
    details: String,
    error: bool,
}
struct Snapshot {
    windows: Vec<Window>,
    config: Config,
    monitor: bool,
    log: String,
}

pub struct App {
    windows: Vec<Window>,
    config: Config,
    page: Page,
    filter: Filter,
    sort: Sort,
    descending: bool,
    search: String,
    new_rule: String,
    new_folder: String,
    expanded: HashSet<String>,
    note: String,
    note_error: bool,
    activity: VecDeque<Activity>,
    started: Instant,
    busy: bool,
    cancel: Arc<AtomicBool>,
    completion_tx: Sender<Completion>,
    completion_rx: Receiver<Completion>,
    last_refresh: Instant,
    discovering: bool,
    snapshot_tx: Sender<Result<Snapshot, String>>,
    snapshot_rx: Receiver<Result<Snapshot, String>>,
    monitoring: bool,
    monitor_starting: bool,
    monitor_error: bool,
    last_monitor_attempt: Instant,
    monitor_tx: Sender<Result<String, String>>,
    monitor_rx: Receiver<Result<String, String>>,
    failed_windows: HashSet<(usize, u32)>,
    retry_after: HashMap<(usize, u32), Instant>,
    icon_sender: Sender<(String, Option<IconPixels>)>,
    icon_receiver: Receiver<(String, Option<IconPixels>)>,
    icon_pending: HashSet<String>,
    app_icons: HashMap<String, Option<egui::TextureHandle>>,
    icon_failed_at: HashMap<String, Instant>,
    logo: egui::TextureHandle,
    capture: CaptureTest,
    last_auto_log: String,
}

fn logo_image() -> egui::ColorImage {
    let png = image::load_from_memory(include_bytes!("../assets/pawhide.png"))
        .expect("bundled PawHide logo is valid")
        .to_rgba8();
    egui::ColorImage::from_rgba_unmultiplied(
        [png.width() as usize, png.height() as usize],
        png.as_raw(),
    )
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        ctx.set_style(style);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = BACKGROUND;
        visuals.faint_bg_color = SIDEBAR;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.inactive.bg_fill = PANEL;
        visuals.widgets.hovered.bg_fill = HOVER;
        visuals.widgets.active.bg_fill = PURPLE_DIM;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, OUTLINE);
        visuals.selection.bg_fill = PURPLE_DIM;
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, PURPLE);
        ctx.set_visuals(visuals);
        let (completion_tx, completion_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (monitor_tx, monitor_rx) = mpsc::channel();
        let (icon_sender, icon_receiver) = mpsc::channel();
        Self {
            windows: Vec::new(),
            config: Config::default(),
            page: Page::Windows,
            filter: Filter::All,
            sort: Sort::Name,
            descending: false,
            search: String::new(),
            new_rule: String::new(),
            new_folder: String::new(),
            expanded: HashSet::new(),
            note: String::new(),
            note_error: false,
            activity: VecDeque::new(),
            started: Instant::now(),
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            completion_tx,
            completion_rx,
            last_refresh: Instant::now() - Duration::from_secs(10),
            discovering: false,
            snapshot_tx,
            snapshot_rx,
            monitoring: false,
            monitor_starting: false,
            monitor_error: false,
            last_monitor_attempt: Instant::now() - Duration::from_secs(60),
            monitor_tx,
            monitor_rx,
            failed_windows: HashSet::new(),
            retry_after: HashMap::new(),
            icon_sender,
            icon_receiver,
            icon_pending: HashSet::new(),
            app_icons: HashMap::new(),
            icon_failed_at: HashMap::new(),
            logo: ctx.load_texture("pawhide-brand", logo_image(), egui::TextureOptions::LINEAR),
            capture: CaptureTest::new(),
            last_auto_log: String::new(),
        }
    }

    fn record(&mut self, summary: String, details: String, error: bool) {
        self.note.clone_from(&summary);
        self.note_error = error;
        if self.activity.len() == 200 {
            self.activity.pop_front();
        }
        self.activity.push_back(Activity {
            at: self.started.elapsed(),
            summary,
            details,
            error,
        });
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.discovering || self.last_refresh.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.discovering = true;
        self.last_refresh = Instant::now();
        let sender = self.snapshot_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| -> Result<Snapshot, String> {
                let config = config::load().map_err(|e| format!("Configuration: {e:#}"))?;
                let windows =
                    platform::windows().map_err(|e| format!("Window discovery: {e:#}"))?;
                // Read only the end of the local monitor log; never collect window titles.
                use std::io::{Read, Seek, SeekFrom};
                let log = config::config_path()
                    .ok()
                    .and_then(|p| std::fs::File::open(p.with_file_name("auto.log")).ok())
                    .and_then(|mut file| {
                        let len = file.metadata().ok()?.len();
                        file.seek(SeekFrom::Start(len.saturating_sub(16384))).ok()?;
                        let mut bytes = Vec::new();
                        file.take(16384).read_to_end(&mut bytes).ok()?;
                        Some(String::from_utf8_lossy(&bytes).into_owned())
                    })
                    .unwrap_or_default();
                Ok(Snapshot {
                    windows,
                    config,
                    monitor: monitor::running(),
                    log,
                })
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn ensure_monitor(&mut self, ctx: &egui::Context) {
        if self.monitoring
            || self.monitor_starting
            || self.config.applications.is_empty() && self.config.folders.is_empty()
            || self.last_monitor_attempt.elapsed() < Duration::from_secs(30)
        {
            return;
        }
        self.monitor_starting = true;
        self.last_monitor_attempt = Instant::now();
        let sender = self.monitor_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result =
                operations::command(&["auto".into(), "start".into()], &AtomicBool::new(false));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn job(
        &mut self,
        ctx: &egui::Context,
        summary: &str,
        work: impl FnOnce(Arc<AtomicBool>) -> Completion + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.note = summary.into();
        self.note_error = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let sender = self.completion_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(cancel)))
                .unwrap_or_else(|_| {
                    Completion::result(Err(
                        "The worker stopped unexpectedly; affinity is unverified.".into(),
                    ))
                });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn apply(&mut self, windows: Vec<Window>, protect: bool, ctx: &egui::Context) {
        if windows.is_empty() {
            return;
        }
        for w in &windows {
            self.failed_windows.remove(&(w.hwnd, w.pid));
        }
        self.job(ctx, if protect { "Applying exclusions…" } else { "Restoring capture…" }, move |cancel| {
            let total = windows.len(); let mut passed = 0; let mut failures = Vec::new(); let mut details = Vec::new();
            for window in windows {
                if cancel.load(Ordering::Relaxed) { details.push("Cancelled; remaining windows were not changed. A pending remote operation may still complete.".into()); break; }
                match operations::apply(&window, protect, &cancel) {
                    Ok(()) => { passed += 1; details.push(format!("{} · HWND 0x{:X}: affinity {} verified", window.exe, window.hwnd, if protect { "0x11" } else { "0x0" })); },
                    Err(error) => { failures.push((window.hwnd, window.pid)); details.push(format!("{} · HWND 0x{:X}: {error}", window.exe, window.hwnd)); },
                }
            }
            Completion { summary: format!("{passed}/{total} windows {} · affinity readback", if protect { "excluded" } else { "restored" }),
                details: details.join("\n"), error: passed != total, failed_windows: failures }
        });
    }

    fn command(&mut self, ctx: &egui::Context, args: &[&str]) {
        let args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        self.job(ctx, "Applying setting…", move |cancel| {
            Completion::result(operations::command(&args, &cancel))
        });
    }

    fn edit_config(
        &mut self,
        ctx: &egui::Context,
        edit: impl FnOnce(&mut Config) -> Result<String, String> + Send + 'static,
    ) {
        self.job(ctx, "Saving rules…", move |_| {
            Completion::result((|| {
                let mut cfg = config::load().map_err(|e| format!("{e:#}"))?;
                let note = edit(&mut cfg)?;
                config::save(&cfg).map_err(|e| format!("{e:#}"))?;
                Ok(note)
            })())
        });
        self.last_monitor_attempt = Instant::now() - Duration::from_secs(60);
    }

    fn ensure_icon(&mut self, name: &str, window: &Window, ctx: &egui::Context) {
        let key = name.to_ascii_lowercase();
        if self.app_icons.get(&key).is_some_and(|v| v.is_some())
            || self.icon_pending.len() >= 4
            || self
                .icon_failed_at
                .get(&key)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(30))
            || !self.icon_pending.insert(key.clone())
        {
            return;
        }
        let (hwnd, pid) = (window.hwnd, window.pid);
        let sender = self.icon_sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send((key, icons::from_window(hwnd, pid)));
            ctx.request_repaint();
        });
    }

    fn status(&self, windows: &[Window]) -> (&'static str, Color32) {
        let excluded = windows
            .iter()
            .filter(|w| w.affinity == Some(EXCLUDED))
            .count();
        if excluded == windows.len() {
            return ("Excluded", SUCCESS);
        }
        if excluded > 0 {
            return ("Partially excluded", WARNING);
        }
        if windows.iter().all(|w| !platform::can_protect_window(w)) {
            return ("Unsupported", MUTED);
        }
        if windows
            .iter()
            .any(|w| self.failed_windows.contains(&(w.hwnd, w.pid)))
        {
            return ("Failed", ERROR);
        }
        if windows.iter().any(|w| w.affinity == Some(1)) {
            return ("Blacked / WDA_MONITOR", WARNING);
        }
        if windows.iter().any(|w| w.affinity != Some(0)) {
            return ("Unknown", WARNING);
        }
        ("Visible", MUTED)
    }

    fn titlebar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("title")
            .exact_height(43.0)
            .frame(
                egui::Frame::NONE
                    .fill(SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((self.logo.id(), egui::vec2(29.0, 29.0))));
                    ui.label(RichText::new("PawHide").strong().size(16.0));
                    ui.label(RichText::new("CAPTURE PRIVACY").size(9.0).color(MUTED));
                    let drag = ui.add_sized(
                        [(ui.available_width() - 78.0).max(0.0), 28.0],
                        egui::Label::new(" ").sense(egui::Sense::drag()),
                    );
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if ui
                        .add_sized([30.0, 27.0], egui::Button::new("−").frame(false))
                        .on_hover_text("Minimize")
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    if ui
                        .add_sized([30.0, 27.0], egui::Button::new("×").frame(false))
                        .on_hover_text("Close")
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }

    fn navigation(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(137.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(SIDEBAR)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                ui.add_space(14.0);
                for (page, label) in [
                    (Page::Windows, "Windows"),
                    (Page::Rules, "Rules"),
                    (Page::Capture, "Capture"),
                    (Page::Activity, "Activity"),
                    (Page::Settings, "Settings"),
                ] {
                    let selected = self.page == page;
                    let response = ui.add_sized(
                        [ui.available_width(), 34.0],
                        egui::Button::new(
                            RichText::new(format!("     {label}"))
                                .size(12.0)
                                .color(if selected { TEXT } else { MUTED }),
                        )
                        .fill(if selected { PURPLE_DIM } else { SIDEBAR })
                        .stroke(egui::Stroke::NONE),
                    );
                    paint_nav_icon(
                        ui.painter(),
                        response.rect.left_center() + egui::vec2(16.0, 0.0),
                        page,
                        if selected { TEXT } else { MUTED },
                    );
                    if response.clicked() {
                        self.page = page;
                    }
                    ui.add_space(3.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(format!("v{} · development", env!("CARGO_PKG_VERSION")))
                            .size(10.0)
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new("Visible to you.\nPrivate in capture.")
                            .size(10.5)
                            .color(PURPLE),
                    );
                });
            });
    }

    fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.label(RichText::new(title).size(23.0).strong());
        ui.label(RichText::new(subtitle).size(11.0).color(MUTED));
        ui.add_space(8.0);
    }

    fn table_row(&mut self, ui: &mut egui::Ui, name: &str, windows: &[Window], individual: bool) {
        let (status, color) = self.status(windows);
        let all_excluded = windows.iter().all(|w| w.affinity == Some(EXCLUDED));
        let supported = windows.iter().all(platform::can_protect_window);
        let explorer = !individual && name.eq_ignore_ascii_case("explorer.exe");
        let cooling_down = windows.iter().any(|w| {
            self.retry_after
                .get(&(w.hwnd, w.pid))
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        });
        if !individual {
            self.ensure_icon(name, &windows[0], ui.ctx());
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), if individual { 32.0 } else { 41.0 }),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(
            rect,
            4.0,
            if response.hovered() {
                HOVER
            } else if individual {
                BACKGROUND
            } else {
                PANEL
            },
        );
        let widths = table_widths(rect.width());
        let mut x = rect.left();
        for (column, width) in widths.into_iter().enumerate() {
            let cell = egui::Rect::from_min_size(
                egui::pos2(x + 5.0, rect.top()),
                egui::vec2(width - 10.0, rect.height()),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(cell).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(cell));
                match column {
                    0 if !individual => {
                        if let Some(Some(icon)) = self.app_icons.get(&name.to_ascii_lowercase()) {
                            ui.add(egui::Image::new((icon.id(), egui::vec2(24.0, 24.0))));
                        } else { ui.label(RichText::new("▣").size(20.0).color(MUTED)); }
                    }
                    1 => {
                        if !individual {
                            let expanded = self.expanded.contains(name);
                            let disclosure = ui.add_sized([17.0, 25.0], egui::Button::new(if expanded { "⌄" } else { "›" }).frame(false))
                                .on_hover_text("Expand individual windows");
                            disclosure.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), format!("{} windows for {name}", if expanded { "Collapse" } else { "Expand" })));
                            if disclosure.clicked() {
                                if expanded { self.expanded.remove(name); } else { self.expanded.insert(name.into()); }
                            }
                        } else { ui.add_space(20.0); }
                        let title = if individual { &windows[0].title } else { name };
                        let response = ui.add(egui::Label::new(RichText::new(title).size(11.5).color(if individual { MUTED } else { TEXT })).truncate())
                            .on_hover_text(if individual { format!("{title}\nPID {} · HWND 0x{:X}", windows[0].pid, windows[0].hwnd) } else { title.into() });
                        if individual && all_excluded {
                            response.context_menu(|ui| {
                                if ui.button("Keep excluded manually").on_hover_text("Folder cleanup will preserve this manual choice.").clicked() {
                                    self.apply(windows.to_vec(), true, ui.ctx()); ui.close();
                                }
                            });
                        }
                    }
                    2 if !individual => { ui.label(RichText::new(windows.len().to_string()).size(11.0).color(MUTED)); }
                    3 => { ui.add(egui::Label::new(RichText::new(status).size(10.5).color(color)).truncate())
                        .on_hover_text(format!("{status}\nWindows affinity readback only. Verify the actual capture output.\n{}", if individual { format!("Affinity: {:?}", windows[0].affinity) } else { "Expand for individual window states.".into() })); }
                    4 => {
                        if explorer {
                            if ui.add_sized([78.0, 25.0], egui::Button::new("Select…")).on_hover_text("Choose an individual Explorer folder window; shell-wide exclusion is blocked.").clicked() { self.expanded.insert(name.into()); }
                        } else if ui.add_enabled(!self.busy && supported && !cooling_down, egui::Button::new(RichText::new(if all_excluded { "Show" } else { "Hide" }).size(11.0)
                            .color(if all_excluded { MUTED } else { PURPLE })).min_size(egui::vec2(78.0, 25.0)))
                            .on_hover_text(if cooling_down { "Retry available 30 seconds after failure; see Activity." } else if supported { "Change capture affinity; your physical desktop stays visible." } else { "DLL injection is unsupported for this application." }).clicked() {
                            self.apply(windows.to_vec(), !all_excluded, ui.ctx());
                        }
                    }
                    _ => {},
                }
            });
            x += width;
        }
    }

    fn windows_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Windows",
            "Choose what compatible captures can see. Your desktop stays visible and usable.",
        );
        ui.add_sized(
            [ui.available_width(), 28.0],
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search applications or window titles"),
        );
        ui.horizontal(|ui| {
            for (filter, label) in [
                (Filter::All, "All"),
                (Filter::Excluded, "Excluded"),
                (Filter::Visible, "Visible"),
                (Filter::Attention, "Needs attention"),
            ] {
                ui.selectable_value(&mut self.filter, filter, label);
            }
            // Automatic two-second refresh must not look like an endless
            // loading state once we already have a usable window list.
            if self.discovering && self.windows.is_empty() {
                ui.spinner();
            }
        });
        let mut groups: BTreeMap<String, Vec<Window>> = BTreeMap::new();
        for window in &self.windows {
            if !self.config.preferences.show_system_windows
                && (window.tool_window || platform::is_background_ui_window(window))
            {
                continue;
            }
            groups
                .entry(window.exe.to_ascii_lowercase())
                .or_default()
                .push(window.clone());
        }
        let query = self.search.trim().to_lowercase();
        let mut groups: Vec<_> = groups
            .into_iter()
            .filter(|(name, windows)| {
                let status = self.status(windows).0;
                (query.is_empty()
                    || name.contains(&query)
                    || windows
                        .iter()
                        .any(|w| w.title.to_lowercase().contains(&query)))
                    && match self.filter {
                        Filter::All => true,
                        Filter::Excluded => windows.iter().any(|w| w.affinity == Some(EXCLUDED)),
                        Filter::Visible => windows.iter().any(|w| w.affinity == Some(0)),
                        Filter::Attention => !matches!(status, "Excluded" | "Visible"),
                    }
            })
            .collect();
        groups.sort_by(|a, b| {
            let order = match self.sort {
                Sort::Name => a.0.cmp(&b.0),
                Sort::Count => a.1.len().cmp(&b.1.len()),
                Sort::Status => self.status(&a.1).0.cmp(self.status(&b.1).0),
            }
            .then(a.0.cmp(&b.0));
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 27.0), egui::Sense::hover());
        let mut x = rect.left();
        for (index, cell_width) in table_widths(width).into_iter().enumerate() {
            let cell = egui::Rect::from_min_size(
                egui::pos2(x + 5.0, rect.top()),
                egui::vec2(cell_width - 10.0, 27.0),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(cell)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    let (label, sort) = match index {
                        1 => ("Application", Some(Sort::Name)),
                        2 => ("Windows", Some(Sort::Count)),
                        3 => ("Capture status", Some(Sort::Status)),
                        4 => ("Action", None),
                        _ => ("", None),
                    };
                    if let Some(sort) = sort {
                        if ui
                            .button(
                                RichText::new(format!(
                                    "{label}{}",
                                    if sort == self.sort {
                                        if self.descending { " v" } else { " ^" }
                                    } else {
                                        ""
                                    }
                                ))
                                .size(10.0)
                                .color(MUTED),
                            )
                            .clicked()
                        {
                            if self.sort == sort {
                                self.descending = !self.descending;
                            } else {
                                self.sort = sort;
                                self.descending = false;
                            }
                        }
                    } else {
                        ui.label(RichText::new(label).size(10.0).color(MUTED));
                    }
                },
            );
            x += cell_width;
        }
        egui::ScrollArea::vertical()
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: false,
                ..Default::default()
            })
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if groups.is_empty() {
                    ui.label(RichText::new("No matching windows").color(MUTED));
                }
                for (key, mut windows) in groups {
                    windows.sort_by(|a, b| a.title.cmp(&b.title));
                    let name = windows[0].exe.clone();
                    // Use the same case-insensitive identity for disclosure and grouping.
                    let expanded = self.expanded.contains(&name);
                    ui.push_id(key, |ui| {
                        self.table_row(ui, &name, &windows, false);
                        if expanded {
                            for window in &windows {
                                ui.push_id(window.hwnd, |ui| {
                                    self.table_row(ui, &name, std::slice::from_ref(window), true)
                                });
                            }
                        }
                    });
                }
            });
    }

    fn rules_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Rules",
            "Saved rules start monitoring automatically. Newly opened windows can appear before a rule applies.",
        );
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Monitoring: {}", self.monitor_state()))
                    .color(if self.monitoring { SUCCESS } else { WARNING }),
            );
            if self.monitor_starting {
                ui.spinner();
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new("Applications").strong());
            ui.horizontal(|ui| {
                ui.add_sized([(ui.available_width() - 84.0).max(100.0), 27.0], egui::TextEdit::singleline(&mut self.new_rule).hint_text(RichText::new("Example: application.exe").color(MUTED).italics()));
                if ui.add_enabled(!self.busy && !self.new_rule.trim().is_empty(), egui::Button::new("Add rule")).clicked() {
                    let name = self.new_rule.trim().to_owned();
                    self.command(ui.ctx(), &["auto", "add", &name]);
                    self.new_rule.clear();
                }
            });
            if self.config.applications.is_empty() { ui.label(RichText::new("No application rules").color(MUTED)); }
            for name in self.config.applications.clone() {
                ui.horizontal(|ui| {
                    ui.add_sized([(ui.available_width() - 72.0).max(100.0), 26.0], egui::Label::new(&name).truncate().halign(egui::Align::LEFT)).on_hover_text(&name);
                    if ui.add_enabled(!self.busy, egui::Button::new("Remove")).clicked() {
                        self.command(ui.ctx(), &["auto", "remove", &name]);
                    }
                });
            }
            ui.add_space(12.0); ui.separator(); ui.add_space(6.0);
            ui.label(RichText::new("Protected folders · Explorer").strong());
            ui.horizontal(|ui| {
                ui.add_sized([(ui.available_width() - 84.0).max(100.0), 27.0], egui::TextEdit::singleline(&mut self.new_folder).hint_text(RichText::new(r"Example: C:\Path\To\Folder").color(MUTED).italics()));
                if ui.add_enabled(!self.busy && !self.new_folder.trim().is_empty(), egui::Button::new("Add rule")).clicked() {
                    let path = self.new_folder.trim().to_owned();
                    self.command(ui.ctx(), &["folders", "add", &path]); self.new_folder.clear();
                }
            });
            if self.config.folders.is_empty() { ui.label(RichText::new("No protected folders").color(MUTED)); }
            for path in self.config.folders.clone() {
                ui.horizontal(|ui| {
                    ui.add_sized([(ui.available_width() - 72.0).max(100.0), 26.0], egui::Label::new(path.display().to_string()).truncate().halign(egui::Align::LEFT)).on_hover_text(path.display().to_string());
                    if ui.add_enabled(!self.busy, egui::Button::new("Remove")).clicked() {
                        self.command(ui.ctx(), &["folders", "remove", &path.to_string_lossy()]);
                    }
                });
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Folder rules use Explorer's actual locations. Tabs share one exclusion; uncertain locations keep automatic exclusion in place. Manual exclusions are preserved.").size(11.0).color(MUTED));
            ui.label(RichText::new("Other document apps require explicit file-open integration; folder rules do not inspect their titles.").size(11.0).color(MUTED));
        });
    }

    fn activity_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Activity",
            "Recent operations on this device. Affinity verification does not verify a Discord stream.",
        );
        if ui.small_button("Clear session history").clicked() {
            self.activity.clear();
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.activity.is_empty() {
                    ui.label(RichText::new("No operations yet").color(MUTED));
                }
                for (index, event) in self.activity.iter().enumerate().rev() {
                    let seconds = event.at.as_secs();
                    egui::CollapsingHeader::new(
                        RichText::new(format!(
                            "{:02}:{:02}  {}",
                            seconds / 60,
                            seconds % 60,
                            event.summary
                        ))
                        .size(11.5)
                        .color(if event.error { ERROR } else { TEXT }),
                    )
                    .id_salt(index)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(if event.details.is_empty() {
                                    "No additional diagnostics."
                                } else {
                                    &event.details
                                })
                                .monospace()
                                .size(10.5)
                                .color(MUTED),
                            )
                            .wrap(),
                        );
                    });
                }
            });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Settings",
            "Local preferences and optional advanced controls.",
        );
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut preferences = self.config.preferences.clone();
            let mut changed = false;
            ui.label(RichText::new("General").strong());
            changed |= ui.checkbox(&mut preferences.show_system_windows, "Include system and helper windows").changed();
            ui.add_space(8.0); ui.separator();
            ui.label(RichText::new("Startup").strong());
            ui.label(RichText::new("Rules monitor automatically while PawHide is open. Optionally start monitoring at Windows sign-in.").size(11.0).color(MUTED));
            ui.horizontal(|ui| {
                for (label, command) in [("Enable at sign-in", "enable"), ("Disable at sign-in", "disable"), ("Check startup", "status")] {
                    if ui.add_enabled(!self.busy, egui::Button::new(label)).clicked() { self.command(ui.ctx(), &["startup", command]); }
                }
            });
            ui.add_space(8.0); ui.separator(); ui.label(RichText::new("Capture").strong());
            changed |= ui.checkbox(&mut preferences.capture_cursor, "Include cursor").changed();
            changed |= ui.checkbox(&mut preferences.capture_native_resolution, "Native resolution preview (higher GPU and memory use)").changed();
            changed |= ui.checkbox(&mut preferences.capture_thumbnail, "Floating thumbnail while browsing other pages").changed();
            if changed && !self.busy {
                self.config.preferences = preferences.clone();
                self.edit_config(ui.ctx(), move |cfg| { cfg.preferences = preferences; Ok("Preferences saved.".into()) });
            }
            ui.add_space(8.0); ui.separator();
            egui::CollapsingHeader::new("Advanced · taskbar capture experiment").show(ui, |ui| {
                ui.label(RichText::new("Opt-in Explorer injection. Check the actual stream after any change. Local taskbar visibility stays unchanged.").size(11.0).color(WARNING));
                ui.horizontal(|ui| {
                    for (label, action) in [("Exclude taskbar", "hide"), ("Restore capture", "show"), ("Check affinity", "status")] {
                        if ui.add_enabled(!self.busy, egui::Button::new(label)).clicked() { self.command(ui.ctx(), &["capture", "taskbar", action]); }
                    }
                });
            });
            ui.add_space(8.0); ui.separator(); ui.label(RichText::new("About PawHide").strong());
            ui.label(RichText::new(format!("Version {} · Windows x64 · development build", env!("CARGO_PKG_VERSION"))).color(MUTED).size(11.0));
            ui.hyperlink_to("Project repository", "https://github.com/ItsW0lfiy/PawHide");
            ui.label(RichText::new("Exclusion applies to compatible capture methods, including some screenshot tools. Unsupported backends, task switcher previews, and newly opened windows may expose content. Always check a real test stream.").size(11.0).color(WARNING));
            if let Ok(path) = config::config_path() { ui.label(RichText::new(path.display().to_string()).size(10.0).color(MUTED)); }
        });
    }

    fn monitor_state(&self) -> &'static str {
        if self.monitoring {
            "Running"
        } else if self.monitor_starting {
            "Starting"
        } else if self.config.applications.is_empty() && self.config.folders.is_empty() {
            "Not configured"
        } else if self.monitor_error {
            "Error"
        } else {
            // No start attempt is currently in flight; this is not "Starting".
            // ensure_monitor() will attempt recovery on its bounded retry cadence.
            "Not running"
        }
    }
}

fn paint_nav_icon(painter: &egui::Painter, center: egui::Pos2, page: Page, color: Color32) {
    let stroke = egui::Stroke::new(1.2_f32, color);
    let rect = egui::Rect::from_center_size(center, egui::vec2(12.0, 12.0));
    match page {
        Page::Windows => {
            painter.rect_stroke(rect, 1.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    rect.left_top() + egui::vec2(0.0, 3.0),
                    rect.right_top() + egui::vec2(0.0, 3.0),
                ],
                stroke,
            );
        }
        Page::Rules => {
            for y in [-4.0, 0.0, 4.0] {
                painter.circle_filled(center + egui::vec2(-5.0, y), 1.0, color);
                painter.line_segment(
                    [center + egui::vec2(-1.0, y), center + egui::vec2(6.0, y)],
                    stroke,
                );
            }
        }
        Page::Capture => {
            painter.rect_stroke(rect, 1.0, stroke, egui::StrokeKind::Inside);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + egui::vec2(-2.0, -3.0),
                    center + egui::vec2(3.0, 0.0),
                    center + egui::vec2(-2.0, 3.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        Page::Activity => {
            painter.line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
            painter.add(egui::Shape::line(
                vec![
                    center + egui::vec2(-6.0, 2.0),
                    center + egui::vec2(-2.0, -1.0),
                    center + egui::vec2(1.0, 2.0),
                    center + egui::vec2(5.0, -5.0),
                ],
                stroke,
            ));
        }
        Page::Settings => {
            for y in [-4.0, 0.0, 4.0] {
                painter.line_segment(
                    [center + egui::vec2(-6.0, y), center + egui::vec2(6.0, y)],
                    stroke,
                );
                painter.circle_filled(
                    center + egui::vec2(if y == 0.0 { 3.0 } else { -2.0 }, y),
                    2.0,
                    color,
                );
            }
        }
    }
}

// A shared column definition keeps headers, application rows and expanded HWNDs aligned.
fn table_widths(width: f32) -> [f32; 5] {
    [
        36.0,
        (width - 36.0 - 70.0 - 151.0 - 95.0).max(80.0),
        70.0,
        151.0,
        95.0,
    ]
}

fn resize_geometry(
    size: egui::Vec2,
    direction: egui::ResizeDirection,
    delta: egui::Vec2,
) -> (egui::Vec2, egui::Vec2) {
    use egui::ResizeDirection::*;
    let west = matches!(direction, West | NorthWest | SouthWest);
    let east = matches!(direction, East | NorthEast | SouthEast);
    let north = matches!(direction, North | NorthWest | NorthEast);
    let south = matches!(direction, South | SouthWest | SouthEast);
    let width = if west {
        size.x - delta.x
    } else if east {
        size.x + delta.x
    } else {
        size.x
    };
    let height = if north {
        size.y - delta.y
    } else if south {
        size.y + delta.y
    } else {
        size.y
    };
    let next = egui::vec2(width.max(800.0), height.max(520.0));
    (
        next,
        egui::vec2(
            if west { size.x - next.x } else { 0.0 },
            if north { size.y - next.y } else { 0.0 },
        ),
    )
}

fn resize_handles(ctx: &egui::Context) {
    use egui::{CursorIcon, ResizeDirection};
    let rect = ctx.viewport_rect();
    let direction = |pos: egui::Pos2| {
        if !rect.contains(pos) {
            return None;
        }
        let left = pos.x < rect.left() + 12.0;
        let right = pos.x > rect.right() - 12.0;
        let top = pos.y < rect.top() + 12.0;
        let bottom = pos.y > rect.bottom() - 12.0;
        match (left, right, top, bottom) {
            (true, _, true, _) => Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwSe)),
            (_, true, true, _) => Some((ResizeDirection::NorthEast, CursorIcon::ResizeNeSw)),
            (true, _, _, true) => Some((ResizeDirection::SouthWest, CursorIcon::ResizeNeSw)),
            (_, true, _, true) => Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwSe)),
            (true, _, _, _) => Some((ResizeDirection::West, CursorIcon::ResizeHorizontal)),
            (_, true, _, _) => Some((ResizeDirection::East, CursorIcon::ResizeHorizontal)),
            (_, _, true, _) => Some((ResizeDirection::North, CursorIcon::ResizeVertical)),
            (_, _, _, true) => Some((ResizeDirection::South, CursorIcon::ResizeVertical)),
            _ => None,
        }
    };
    if let Some((_, cursor)) = ctx.pointer_hover_pos().and_then(direction) {
        ctx.set_cursor_icon(cursor);
    }
    let pressed = ctx.input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } => direction(*pos).map(|(direction, cursor)| (*pos, direction, cursor)),
            _ => None,
        })
    });
    if let Some((origin, direction, _)) = pressed {
        let delta = ctx.input(|i| i.pointer.latest_pos().unwrap_or(origin)) - origin;
        let (size, offset) = resize_geometry(rect.size(), direction, delta);
        // Windows begins native sizing at the current cursor. Preserve movement
        // coalesced with mouse-down before handing subsequent movement to it.
        if delta != egui::Vec2::ZERO {
            if offset != egui::Vec2::ZERO
                && let Some(outer) = ctx.input(|i| i.viewport().outer_rect)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer.min + offset));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        if ctx.input(|i| i.pointer.primary_down()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(result) = self.completion_rx.try_recv() {
            self.busy = false;
            for key in &result.failed_windows {
                self.retry_after.insert(*key, Instant::now());
            }
            self.failed_windows.extend(result.failed_windows);
            self.record(result.summary, result.details, result.error);
            self.last_refresh = Instant::now() - Duration::from_secs(10);
        }
        while let Ok(result) = self.snapshot_rx.try_recv() {
            self.discovering = false;
            match result {
                Ok(snapshot) => {
                    self.windows = snapshot.windows;
                    self.config = snapshot.config;
                    self.monitoring = snapshot.monitor;
                    if self.monitoring {
                        self.monitor_error = false;
                    }
                    self.failed_windows
                        .retain(|key| self.windows.iter().any(|w| (w.hwnd, w.pid) == *key));
                    if snapshot.log != self.last_auto_log && !snapshot.log.is_empty() {
                        let previous = self.last_auto_log.lines().last().unwrap_or("");
                        let lines = snapshot.log.lines().collect::<Vec<_>>();
                        let start = lines
                            .iter()
                            .rposition(|line| *line == previous)
                            .map_or(lines.len().saturating_sub(10), |i| i + 1);
                        for line in &lines[start..] {
                            self.record(
                                "AutoHide diagnostic".into(),
                                (*line).into(),
                                line.contains("ERROR") || line.contains("WARN"),
                            );
                        }
                    }
                    self.last_auto_log = snapshot.log;
                }
                Err(error) => {
                    if self.note != "Discovery failed. See Activity." {
                        self.record("Discovery failed. See Activity.".into(), error, true);
                    }
                }
            }
        }
        while let Ok(result) = self.monitor_rx.try_recv() {
            self.monitor_starting = false;
            self.monitoring = monitor::running();
            self.monitor_error = result.is_err() || !self.monitoring;
            match result {
                Ok(details) => self.record("AutoHide monitor started.".into(), details, false),
                Err(details) => self.record(
                    "AutoHide could not start. Retrying in 30 seconds.".into(),
                    details,
                    true,
                ),
            }
        }
        while let Ok((key, pixels)) = self.icon_receiver.try_recv() {
            self.icon_pending.remove(&key);
            if pixels.is_none() {
                self.icon_failed_at.insert(key.clone(), Instant::now());
            }
            let texture = pixels.map(|p| {
                ctx.load_texture(
                    format!("icon-{key}"),
                    egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba),
                    egui::TextureOptions::LINEAR,
                )
            });
            self.app_icons.insert(key, texture);
        }
        self.refresh(ctx);
        self.ensure_monitor(ctx);
        self.capture.configure(
            self.config.preferences.capture_cursor,
            self.config.preferences.capture_native_resolution,
        );
        self.capture.tick(
            ctx,
            self.page == Page::Capture || self.config.preferences.capture_thumbnail,
        );
        for (summary, error) in self.capture.take_events() {
            self.record(summary, String::new(), error);
        }
        self.titlebar(ctx);
        egui::TopBottomPanel::bottom("status").exact_height(30.0)
            .frame(egui::Frame::NONE.fill(SIDEBAR).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ctx, |ui| { ui.horizontal(|ui| {
                let count = self.windows.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
                ui.label(RichText::new(format!("{count} excluded")).size(10.5).color(SUCCESS));
                ui.separator(); ui.label(RichText::new(format!("AutoHide: {}", self.monitor_state())).size(10.5).color(MUTED));
                if self.busy {
                    ui.spinner();
                    if ui.small_button("Cancel").on_hover_text("Stop remaining work. Remote operations already started may still complete.").clicked() { self.cancel.store(true, Ordering::Relaxed); }
                }
                let capture_width = if self.capture.is_active() { 215.0 } else { 0.0 };
                let note_width = (ui.available_width() - capture_width).max(10.0);
                if ui.add_sized([note_width, 20.0], egui::Label::new(RichText::new(if self.note_error { "! Operation needs attention" } else if self.busy { &self.note } else { "" }).size(10.0).color(WARNING)).truncate().sense(egui::Sense::click()))
                    .on_hover_text(&self.note).clicked() { self.page = Page::Activity; }
                if self.capture.is_active() && ui.small_button(self.capture.indicator()).clicked() { self.page = Page::Capture; }
            }); });
        self.navigation(ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BACKGROUND).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                let reserved = if self.page != Page::Capture && self.config.preferences.capture_thumbnail && self.capture.is_active() { 170.0 } else { 0.0 };
                let room = egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(ui.available_width(), (ui.available_height() - reserved).max(180.0)));
                ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| match self.page {
                Page::Windows => self.windows_page(ui), Page::Rules => self.rules_page(ui),
                Page::Capture => { Self::heading(ui, "Capture", "Live native monitor preview. Inspect the stream you actually intend to share, too."); if let Some((cursor, native)) = self.capture.ui(ui, ctx) {
                    let mut preferences = self.config.preferences.clone(); preferences.capture_cursor = cursor; preferences.capture_native_resolution = native;
                    self.config.preferences = preferences.clone();
                    self.edit_config(ctx, move |cfg| { cfg.preferences = preferences; Ok("Capture preferences saved.".into()) });
                } },
                Page::Activity => self.activity_page(ui), Page::Settings => self.settings_page(ui),
                });
            });
        if self.page != Page::Capture && self.config.preferences.capture_thumbnail {
            self.capture.thumbnail(ctx);
        }
        resize_handles(ctx);
        ctx.request_repaint_after(Duration::from_millis(
            if self.busy || self.monitor_starting {
                100
            } else {
                500
            },
        ));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn run() -> eframe::Result<()> {
    let png = image::load_from_memory(include_bytes!("../assets/pawhide.png"))
        .expect("bundled PNG is valid")
        .to_rgba8();
    let icon = egui::IconData {
        width: png.width(),
        height: png.height(),
        rgba: png.into_raw(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PawHide")
            .with_inner_size([900.0, 580.0])
            .with_min_inner_size([800.0, 520.0])
            .with_maximize_button(false)
            .with_resizable(true)
            .with_decorations(false)
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native(
        "PawHide",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesced_resize_clamps_size_and_keeps_opposite_edges_anchored() {
        use egui::ResizeDirection::*;
        let size = egui::vec2(900.0, 580.0);
        assert_eq!(
            resize_geometry(size, SouthEast, egui::vec2(-100.0, -60.0)),
            (egui::vec2(800.0, 520.0), egui::Vec2::ZERO)
        );
        assert_eq!(
            resize_geometry(size, NorthWest, egui::vec2(300.0, 300.0)),
            (egui::vec2(800.0, 520.0), egui::vec2(100.0, 60.0))
        );
        assert_eq!(
            resize_geometry(size, West, egui::vec2(-100.0, 0.0)),
            (egui::vec2(1000.0, 580.0), egui::vec2(-100.0, 0.0))
        );
    }
}
