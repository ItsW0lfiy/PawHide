//! Optional native PawHide manager. CLI and DLL agent remain independent.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() { eprintln!("PawHideUI requires Windows."); }

#[cfg(windows)]
mod capture_native;
#[cfg(windows)]
mod capture_test;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
mod operations;

#[cfg(windows)]
mod windows_ui {
    use eframe::egui::{self, Color32, RichText};
    use pawhide_core::{config, folders, platform::{self, Window, EXCLUDED}};
    use std::{
        collections::{BTreeMap, HashMap, HashSet},
        os::windows::process::CommandExt,
        sync::{mpsc::{self, Receiver, Sender}, Arc},
        thread,
        time::{Duration, Instant},
        path::PathBuf,
    };
    use crate::icons::{self, IconPixels};
    use crate::capture_test::CaptureTest;

    const BACKGROUND: Color32 = Color32::from_rgb(17, 15, 22);
    const SIDEBAR: Color32 = Color32::from_rgb(23, 20, 29);
    const PANEL: Color32 = Color32::from_rgb(31, 27, 39);
    const HOVER: Color32 = Color32::from_rgb(43, 36, 55);
    const OUTLINE: Color32 = Color32::from_rgb(62, 51, 75);
    const PURPLE: Color32 = Color32::from_rgb(183, 141, 248);
    const PURPLE_DIM: Color32 = Color32::from_rgb(92, 67, 129);
    const TEXT: Color32 = Color32::from_rgb(240, 235, 246);
    const MUTED: Color32 = Color32::from_rgb(169, 158, 184);
    const SUCCESS: Color32 = Color32::from_rgb(150, 220, 172);
    const WARNING: Color32 = Color32::from_rgb(229, 184, 120);
    const ERROR: Color32 = Color32::from_rgb(239, 132, 146);
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Page { Windows, Auto, Folders, Capture, Settings }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Filter { All, Hidden }

    pub struct App {
        windows: Vec<Window>,
        rules: Vec<String>,
        folders: Vec<PathBuf>,
        page: Page,
        filter: Filter,
        show_system_windows: bool,
        search: String,
        new_rule: String,
        new_folder: String,
        expanded: HashSet<String>,
        note: String,
        note_error: bool,
        busy: bool,
        last_refresh: Instant,
        completion_tx: Sender<(String, bool)>,
        completion_rx: Receiver<(String, bool)>,
        icon_sender: Sender<(String, Option<IconPixels>)>,
        icon_receiver: Receiver<(String, Option<IconPixels>)>,
        icon_pending: HashSet<String>,
        app_icons: HashMap<String, Option<egui::TextureHandle>>,
        icon_failed_at: HashMap<String, Instant>,
        logo: egui::TextureHandle,
        capture_test: CaptureTest,
    }

    fn logo_image() -> egui::ColorImage {
        let bytes = include_bytes!("../assets/pawhide.png");
        let png = image::load_from_memory(bytes).expect("bundled PawHide logo must be a PNG").to_rgba8();
        let (width, height) = png.dimensions();
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], png.as_raw())
    }

    impl App {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(6.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 5.0);
            cc.egui_ctx.set_style(style);
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BACKGROUND;
            visuals.window_fill = PANEL;
            visuals.extreme_bg_color = BACKGROUND;
            visuals.faint_bg_color = SIDEBAR;
            visuals.widgets.noninteractive.bg_fill = PANEL;
            visuals.widgets.inactive.bg_fill = PANEL;
            visuals.widgets.hovered.bg_fill = HOVER;
            visuals.widgets.active.bg_fill = PURPLE_DIM;
            visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, OUTLINE);
            visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, PURPLE_DIM);
            visuals.selection.bg_fill = PURPLE_DIM;
            visuals.selection.stroke = egui::Stroke::new(1.0_f32, PURPLE);
            cc.egui_ctx.set_visuals(visuals);
            let (completion_tx, completion_rx) = mpsc::channel();
            let (icon_sender, icon_receiver) = mpsc::channel();
            let logo = cc.egui_ctx.load_texture("pawhide-brand", logo_image(), egui::TextureOptions::LINEAR);
            let mut app = Self {
                windows: Vec::new(), rules: Vec::new(), folders: Vec::new(), page: Page::Windows,
                filter: Filter::All, show_system_windows: false, search: String::new(), new_rule: String::new(), new_folder: String::new(),
                expanded: HashSet::new(), note: String::new(), note_error: false,
                busy: false, last_refresh: Instant::now() - Duration::from_secs(10),
                completion_tx, completion_rx, icon_sender, icon_receiver,
                icon_pending: HashSet::new(), app_icons: HashMap::new(), icon_failed_at: HashMap::new(), logo,
                capture_test: CaptureTest::new(),
            };
            app.refresh();
            app
        }

        fn refresh(&mut self) {
            match platform::windows() {
                Ok(found) => self.windows = found.into_iter().filter(|w| !w.tool_window && (self.show_system_windows || !platform::is_background_ui_window(w))).collect(),
                Err(error) => { self.note = format!("Window discovery failed: {error:#}"); self.note_error = true; }
            }
            match config::load() {
                Ok(cfg) => { self.rules = cfg.applications; self.folders = cfg.folders; },
                Err(error) => { self.note = format!("Configuration error: {error:#}"); self.note_error = true; }
            }
            self.last_refresh = Instant::now();
        }

        fn ensure_app_icon(&mut self, app: &str, hwnd: usize, pid: u32, ctx: &egui::Context) {
            let key = app.to_ascii_lowercase();
            if self.app_icons.get(&key).is_some_and(|icon| icon.is_some()) { return; }
            if self.icon_failed_at.get(&key).is_some_and(|at| at.elapsed() < Duration::from_secs(30)) { return; }
            if !self.icon_pending.insert(key.clone()) { return; }
            let sender = self.icon_sender.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let result = icons::from_window(hwnd, pid);
                let _ = sender.send((key, result));
                ctx.request_repaint();
            });
        }

        fn app_icon(&self, ui: &mut egui::Ui, app: &str) {
            let name = app.to_ascii_lowercase();
            if let Some(Some(icon)) = self.app_icons.get(&name) {
                ui.add(egui::Image::new((icon.id(), egui::vec2(34.0, 34.0))).corner_radius(5));
            } else {
                egui::Frame::NONE.fill(HOVER).corner_radius(5).show(ui, |ui| {
                    ui.add_sized([34.0, 34.0], egui::Label::new(RichText::new("▣").size(19.0).color(MUTED)));
                });
            }
        }

        /// Single logical operation; per-window failures are reported, never silently ignored.
        fn apply(&mut self, windows: Vec<Window>, hide: bool, ctx: egui::Context) {
            if self.busy || windows.is_empty() { return; }
            self.busy = true;
            self.note_error = false;
            self.note = if hide { "Protecting windows…".into() } else { "Restoring windows…".into() };
            let sender = self.completion_tx.clone();
            thread::spawn(move || {
                let count = windows.len();
                let mut passed = 0;
                let mut failures = Vec::new();
                for w in windows {
                    match crate::operations::apply_bounded(&w, hide, Duration::from_secs(8)) {
                        Ok(()) => passed += 1,
                        Err(err) => failures.push(format!("{} ({}): {err}", w.exe, w.title)),
                    }
                }
                let action = if hide { "protected" } else { "restored" };
                let message = if failures.is_empty() {
                    format!("{passed}/{count} windows {action} and verified")
                } else {
                    format!("{passed}/{count} {action}; {}", failures.join(" | "))
                };
                let _ = sender.send((message, !failures.is_empty()));
                ctx.request_repaint();
            });
        }

        fn auto_command(&mut self, command: &'static str, ctx: egui::Context) {
            if self.busy { return; }
            self.busy = true;
            self.note_error = false;
            self.note = format!("AutoHide: {command}…");
            let sender = self.completion_tx.clone();
            thread::spawn(move || {
                let result = std::env::current_exe()
                    .map(|exe| exe.with_file_name("pawhide.exe"))
                    .map_err(|e| e.to_string())
                    .and_then(|exe| std::process::Command::new(exe)
                        .args(["auto", command]).creation_flags(CREATE_NO_WINDOW)
                        .output().map_err(|e| e.to_string()));
                let (message, failed) = match result {
                    Ok(output) if output.status.success() => (String::from_utf8_lossy(&output.stdout).trim().to_owned(), false),
                    Ok(output) => (String::from_utf8_lossy(&output.stderr).trim().to_owned(), true),
                    Err(err) => (format!("AutoHide unavailable: {err}. Put pawhide.exe beside PawHideUI.exe"), true),
                };
                let _ = sender.send((message, failed));
                ctx.request_repaint();
            });
        }

        fn add_rule(&mut self) {
            let result: Result<(), String> = (|| {
                let application = config::normalize_exe(&self.new_rule).map_err(|e| e.to_string())?;
                let mut cfg = config::load().map_err(|e| e.to_string())?;
                if !cfg.applications.contains(&application) {
                    cfg.applications.push(application);
                    cfg.applications.sort();
                    config::save(&cfg).map_err(|e| e.to_string())?;
                }
                self.new_rule.clear();
                Ok(())
            })();
            match result {
                Ok(()) => { self.note = "AutoHide application saved".into(); self.note_error = false; },
                Err(err) => { self.note = err; self.note_error = true; },
            }
            self.refresh();
        }

        fn remove_rule(&mut self, name: &str) {
            match config::load() {
                Ok(mut cfg) => {
                    cfg.applications.retain(|item| item != name);
                    match config::save(&cfg) {
                        Ok(()) => { self.note = format!("Removed {name}"); self.note_error = false; },
                        Err(err) => { self.note = err.to_string(); self.note_error = true; },
                    }
                },
                Err(err) => { self.note = err.to_string(); self.note_error = true; },
            }
            self.refresh();
        }

        fn add_folder(&mut self, ctx: egui::Context) {
            let result: Result<String, String> = (|| {
                let folder = folders::validated_folder(std::path::Path::new(self.new_folder.trim()))
                    .map_err(|e| e.to_string())?;
                let mut cfg = config::load().map_err(|e| e.to_string())?;
                if !cfg.folders.contains(&folder) { cfg.folders.push(folder.clone()); }
                config::save(&cfg).map_err(|e| e.to_string())?;
                self.new_folder.clear();
                Ok(format!("Folder registered: {}", folder.display()))
            })();
            match result {
                Ok(note) => { self.note = note; self.note_error = false; self.refresh(); self.auto_command("start", ctx); return; }
                Err(error) => { self.note = error; self.note_error = true; }
            }
            self.refresh();
        }
        fn remove_folder(&mut self, folder: &PathBuf) {
            match config::load() {
                Ok(mut cfg) => {
                    cfg.folders.retain(|f| f != folder);
                    match config::save(&cfg) {
                        Ok(()) => { self.note = "Folder rule removed".into(); self.note_error = false; }
                        Err(e) => { self.note = e.to_string(); self.note_error = true; }
                    }
                }
                Err(e) => { self.note = e.to_string(); self.note_error = true; }
            }
            self.refresh();
        }
        fn folder_page(&mut self, ui: &mut egui::Ui) {
            Self::heading(ui, "Folders", "Document-path protection rules");
            ui.horizontal(|ui| {
                let available = (ui.available_width() - 70.0).max(120.0);
                ui.add_sized([available, 29.0], egui::TextEdit::singleline(&mut self.new_folder)
                    .hint_text(r"D:\Private\Work"));
                if ui.add_sized([60.0, 29.0], egui::Button::new("Add")).clicked() {
                    self.add_folder(ui.ctx().clone());
                }
            });
            ui.add_space(10.0);
            if self.folders.is_empty() { ui.label(RichText::new("No protected folders yet.").color(MUTED)); }
            for folder in self.folders.clone() {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 75.0).max(100.0);
                    ui.add_sized([width, 29.0], egui::Label::new(
                        RichText::new(folder.display().to_string()).size(11.0).color(TEXT)).truncate())
                        .on_hover_text(folder.display().to_string());
                    if ui.button("Remove").clicked() { self.remove_folder(&folder); }
                });
                ui.separator();
            }
            ui.add_space(9.0);
            ui.label(RichText::new("Explorer: open folder windows are automatically monitored by the background service. Wolfmark: needs an exact file-open notification from Wolfmark; title guessing is not safe.")
                .color(WARNING).size(11.0));
        }

        fn titlebar(&mut self, ctx: &egui::Context) {
            egui::TopBottomPanel::top("window-title")
                .exact_height(40.0)
                .frame(egui::Frame::NONE.fill(SIDEBAR).inner_margin(egui::Margin::symmetric(12, 5)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new((self.logo.id(), egui::vec2(28.0, 28.0))).corner_radius(6));
                        ui.label(RichText::new("PawHide").strong().size(15.0).color(TEXT));
                        let available = (ui.available_width() - 78.0).max(0.0);
                        let drag = ui.add_sized([available, 29.0], egui::Label::new(" ").sense(egui::Sense::drag()));
                        if drag.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }
                        if ui.add_sized([30.0, 27.0], egui::Button::new(RichText::new("−").size(16.0).color(MUTED))
                            .fill(SIDEBAR).stroke(egui::Stroke::NONE)).on_hover_text("Minimize").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        if ui.add_sized([30.0, 27.0], egui::Button::new(RichText::new("×").size(19.0).color(TEXT))
                            .fill(SIDEBAR).stroke(egui::Stroke::NONE)).on_hover_text("Close").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }

        fn navigation(&mut self, ctx: &egui::Context) {
            egui::SidePanel::left("pawhide-nav").exact_width(126.0).resizable(false)
                .frame(egui::Frame::NONE.fill(SIDEBAR).inner_margin(egui::Margin::same(10)))
                .show(ctx, |ui| {
                    ui.add_space(12.0);
                    for (page, label) in [
                        (Page::Windows, "Windows"), (Page::Auto, "AutoHide"), (Page::Folders, "Folders"),
                        (Page::Capture, "Capture Test"), (Page::Settings, "Settings"),
                    ] {
                        let selected = self.page == page;
                        let color = if selected { TEXT } else { MUTED };
                        let fill = if selected { PURPLE_DIM } else { SIDEBAR };
                        if ui.add_sized([ui.available_width(), 31.0], egui::Button::new(
                            RichText::new(label).size(12.0).color(color)).fill(fill).stroke(egui::Stroke::NONE)
                        ).clicked() {
                            if self.page == Page::Capture && page != Page::Capture { self.capture_test.stop(); }
                            self.page = page;
                        }
                        ui.add_space(4.0);
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                        ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(10.0).color(MUTED));
                        ui.label(RichText::new("LOCAL • WINDOWS").size(9.5).color(PURPLE));
                    });
                });
        }

        fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(17.0).strong().color(TEXT));
                ui.label(RichText::new(description).size(11.0).color(MUTED));
            });
            ui.add_space(4.0);
        }

        /// The group button is intentionally OUTSIDE the disclosure. No extra click to protect.
        fn application_row(&mut self, ui: &mut egui::Ui, name: &str, windows: &[Window]) {
            let protected = windows.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
            let all_protected = protected == windows.len();
            let shell = windows.iter().any(|w| w.pid == platform::shell_pid());
            let supported = windows.iter().any(platform::can_protect_window);
            let expanded = self.expanded.contains(name);
            let count = windows.len();
            self.ensure_app_icon(name, windows[0].hwnd, windows[0].pid, ui.ctx());
            egui::Frame::NONE.fill(PANEL).stroke(egui::Stroke::new(1.0_f32, OUTLINE)).corner_radius(6)
                .inner_margin(egui::Margin::symmetric(9, 7)).show(ui, |ui| {
                    let width = ui.available_width();
                    ui.horizontal(|ui| {
                        self.app_icon(ui, name);
                        let name_width = (width - 167.0).max(100.0);
                        ui.allocate_ui_with_layout(egui::vec2(name_width, 37.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.add(egui::Label::new(RichText::new(name).size(12.3).strong().color(TEXT)).truncate())
                                .on_hover_text(name);
                            let caption = if shell { "Explorer · expand to select a folder window".to_owned() }
                                else if !supported { "Protected application · unsupported".to_owned() }
                                else { format!("{protected}/{count} hidden · {count} window{}", if count == 1 { "" } else { "s" }) };
                            ui.label(RichText::new(caption).size(10.3).color(if protected > 0 { SUCCESS } else { MUTED }));
                        });
                        let action = if all_protected { "Show" } else { "Hide" };
                        let result = ui.add_enabled(!self.busy && !shell && supported, egui::Button::new(button_text(action, all_protected))
                            .fill(if all_protected { HOVER } else { SIDEBAR })
                            .stroke(egui::Stroke::new(1.0_f32, if all_protected { OUTLINE } else { PURPLE_DIM }))
                            .min_size(egui::vec2(60.0, 27.0)));
                        if result.on_hover_text(if shell { "Select an individual Explorer folder window below (desktop/taskbar remain blocked)" }
                            else if !supported { "PawHide cannot inject into this protected application" }
                            else if all_protected { "Restore capture of every window in this group" }
                            else { "Exclude all windows in this group from compatible capture" }).clicked() {
                            let selected = windows.iter().filter(|w| if all_protected { w.affinity != Some(0) } else { w.affinity != Some(EXCLUDED) })
                                .cloned().collect();
                            self.apply(selected, !all_protected, ui.ctx().clone());
                        }
                        let arrow = if expanded { "⌄" } else { "›" };
                        if ui.add_sized([23.0, 27.0], egui::Button::new(RichText::new(arrow).color(PURPLE))
                            .fill(PANEL).stroke(egui::Stroke::NONE)).on_hover_text("Individual windows").clicked() {
                            if expanded { self.expanded.remove(name); } else { self.expanded.insert(name.to_owned()); }
                        }
                    });
                    if expanded {
                        ui.add_space(6.0);
                        for window in windows {
                            let hidden = window.affinity == Some(EXCLUDED);
                            let allowed = platform::can_protect_window(window);
                            let (state, color) = match window.affinity {
                                Some(EXCLUDED) => ("Hidden", SUCCESS),
                                Some(0) => ("Visible", MUTED),
                                Some(1) => ("Blacked", WARNING),
                                _ => ("Unknown", WARNING),
                            };
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                let title_width = (ui.available_width() - 115.0).max(75.0);
                                ui.add_sized([title_width, 22.0], egui::Label::new(
                                    RichText::new(&window.title).size(11.0).color(TEXT)).truncate())
                                    .on_hover_text(format!("{}\nPID {} · HWND 0x{:X}", window.title, window.pid, window.hwnd));
                                ui.label(RichText::new(state).size(10.0).color(color));
                                let action = if hidden { "Show" } else { "Hide" };
                                if ui.add_enabled(!self.busy && allowed, egui::Button::new(
                                    RichText::new(action).size(10.0).color(PURPLE))
                                    .fill(HOVER).stroke(egui::Stroke::NONE).min_size(egui::vec2(43.0, 21.0)))
                                    .clicked() {
                                    self.apply(vec![window.clone()], !hidden, ui.ctx().clone());
                                }
                            });
                        }
                    }
                });
        }

        fn windows_page(&mut self, ui: &mut egui::Ui) {
            let hidden = self.windows.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
            Self::heading(ui, "Windows", &format!("{hidden}/{} hidden", self.windows.len()));
            ui.horizontal(|ui| {
                let search_width = (ui.available_width() - 75.0).max(100.0);
                ui.add_sized([search_width, 27.0], egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search programs or window titles"));
                if ui.add_sized([65.0, 27.0], egui::Button::new("Refresh")).clicked() { self.refresh(); }
            });
            ui.horizontal(|ui| {
                if ui.selectable_label(self.filter == Filter::All, "All programs").clicked() { self.filter = Filter::All; }
                if ui.selectable_label(self.filter == Filter::Hidden, format!("Hidden ({hidden})")).clicked() { self.filter = Filter::Hidden; }
                ui.checkbox(&mut self.show_system_windows, "System windows")
                    .on_hover_text("Include internal input hosts and app helper processes");
            });
            ui.add_space(3.0);
            let query = self.search.trim().to_ascii_lowercase();
            let mut groups: BTreeMap<String, Vec<Window>> = BTreeMap::new();
            // Keep complete groups. A title search or "Hidden" filter must not
            // turn "Hide all" into "Hide only a silently filtered subset".
            let visible_apps: HashSet<String> = self.windows.iter().filter(|w| {
                (self.filter == Filter::All || w.affinity == Some(EXCLUDED))
                    && (query.is_empty() || w.exe.to_ascii_lowercase().contains(&query)
                        || w.title.to_ascii_lowercase().contains(&query))
            }).map(|w| w.exe.clone()).collect();
            for w in &self.windows {
                if visible_apps.contains(&w.exe) {
                    groups.entry(w.exe.clone()).or_default().push(w.clone());
                }
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if groups.is_empty() { ui.add_space(12.0); ui.label(RichText::new("No matching applications").color(MUTED)); }
                for (name, mut group) in groups {
                    group.sort_by(|a, b| a.title.cmp(&b.title));
                    self.application_row(ui, &name, &group);
                    ui.add_space(3.0);
                }
            });
        }

        fn auto_page(&mut self, ui: &mut egui::Ui) {
            Self::heading(ui, "AutoHide", "Monitor application windows");
            egui::Frame::NONE.fill(PANEL).corner_radius(6).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.label(RichText::new("Background monitor").strong().color(TEXT));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (label, command) in [("Start", "start"), ("Stop", "stop"), ("Status", "status")] {
                        if ui.add_enabled(!self.busy, egui::Button::new(RichText::new(label).color(TEXT)))
                            .clicked() { self.auto_command(command, ui.ctx().clone()); }
                    }
                });
            });
            ui.add_space(11.0);
            ui.label(RichText::new(format!("Saved applications ({})", self.rules.len())).strong().color(TEXT));
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 65.0).max(90.0);
                ui.add_sized([width, 27.0], egui::TextEdit::singleline(&mut self.new_rule).hint_text("PicView.exe"));
                if ui.add_sized([55.0, 27.0], egui::Button::new("Add")).clicked() { self.add_rule(); }
            });
            ui.add_space(5.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.rules.is_empty() { ui.label(RichText::new("No AutoHide applications configured").color(MUTED)); }
                for name in self.rules.clone() {
                    egui::Frame::NONE.fill(PANEL).corner_radius(5).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - 75.0).max(75.0);
                            ui.add_sized([width, 24.0], egui::Label::new(RichText::new(&name).color(TEXT)).truncate());
                            if ui.button("Remove").clicked() { self.remove_rule(&name); }
                        });
                    });
                    ui.add_space(3.0);
                }
            });
        }

        fn taskbar_command(&mut self, action: &'static str, ctx: egui::Context) {
            if self.busy { return; }
            self.busy = true;
            self.note = format!("Checking taskbar capture: {action}…");
            self.note_error = false;
            let sender = self.completion_tx.clone();
            thread::spawn(move || {
                let result = crate::operations::taskbar_bounded(action, Duration::from_secs(8));
                let (message, failed) = match result {
                    Ok(text) => (text, false),
                    Err(error) => (error, true),
                };
                let _ = sender.send((message, failed));
                ctx.request_repaint();
            });
        }

        fn settings_page(&mut self, ui: &mut egui::Ui) {
            Self::heading(ui, "Settings", "Local configuration");
            ui.label(RichText::new("Taskbar in screen capture · experimental").strong().color(TEXT));
            match platform::taskbar_windows() {
                Ok(found) if !found.is_empty() => {
                    let excluded = found.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
                    let unknown = found.iter().filter(|w| w.affinity.is_none()).count();
                    ui.label(RichText::new(format!(
                        "{excluded}/{} taskbars marked excluded · {unknown} unverified", found.len()
                    )).size(10.5).color(if excluded == found.len() { SUCCESS } else { MUTED }));
                }
                Ok(_) => { ui.label(RichText::new("No Explorer taskbar window detected").size(10.5).color(WARNING)); }
                Err(err) => { ui.label(RichText::new(format!("Taskbar discovery: {err}")).size(10.5).color(WARNING)); }
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy, egui::Button::new("Exclude from capture"))
                    .on_hover_text("May fail on some Windows versions. Never changes taskbar visibility on your monitor.")
                    .clicked() { self.taskbar_command("hide", ui.ctx().clone()); }
                if ui.add_enabled(!self.busy, egui::Button::new("Restore capture"))
                    .clicked() { self.taskbar_command("show", ui.ctx().clone()); }
                if ui.add_enabled(!self.busy, egui::Button::new("Status"))
                    .clicked() { self.taskbar_command("status", ui.ctx().clone()); }
            });
            ui.label(RichText::new("Targets Explorer taskbar windows only. Does not hide the taskbar locally. Injection into Explorer is experimental; verify the actual Discord stream.").size(10.5).color(WARNING));
            ui.add_space(9.0);
            ui.label(RichText::new("Alt+Tab and Task View are generated by Windows; the existing exclusion API cannot guarantee viewer-only removal of their window titles or previews. No local switcher changes are made.").size(10.5).color(MUTED));
            ui.add_space(11.0);
            egui::Frame::NONE.fill(PANEL).corner_radius(6).inner_margin(egui::Margin::same(11)).show(ui, |ui| {
                ui.label(RichText::new("Configuration").strong().color(TEXT));
                if let Ok(path) = config::config_path() {
                    ui.add(egui::Label::new(RichText::new(path.display().to_string()).size(11.0).color(MUTED)).wrap());
                }
                ui.add_space(10.0);
                ui.label(RichText::new("Capture exclusion also affects compatible screenshot software (including ShareX). It's not Discord-specific.")
                    .size(11.0).color(MUTED));
                ui.add_space(8.0);
                ui.label(RichText::new("Always confirm privacy in a real test stream. Windows affinity readback alone isn't a guarantee.")
                    .size(11.0).color(WARNING));
            });
        }
    }

    fn button_text(action: &str, protected: bool) -> RichText {
        RichText::new(action).size(11.0).strong().color(if protected { TEXT } else { PURPLE })
    }

    impl eframe::App for App {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            while let Ok((message, error)) = self.completion_rx.try_recv() {
                self.note = message; self.note_error = error; self.busy = false; self.refresh();
            }
            while let Ok((key, image)) = self.icon_receiver.try_recv() {
                self.icon_pending.remove(&key);
                if image.is_none() { self.icon_failed_at.insert(key.clone(), Instant::now()); }
                else { self.icon_failed_at.remove(&key); }
                let handle = image.map(|pixels| ctx.load_texture(
                    format!("app-icon-{key}"),
                    egui::ColorImage::from_rgba_unmultiplied([pixels.width, pixels.height], &pixels.rgba),
                    egui::TextureOptions::LINEAR,
                ));
                self.app_icons.insert(key, handle);
            }
            if !self.busy && self.last_refresh.elapsed() >= Duration::from_secs(2) { self.refresh(); }
            self.titlebar(ctx);
            self.navigation(ctx);
            egui::TopBottomPanel::bottom("status")
                .exact_height(27.0)
                .frame(egui::Frame::NONE.fill(SIDEBAR).inner_margin(egui::Margin::symmetric(10, 5)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let count = self.windows.iter().filter(|w| w.affinity == Some(EXCLUDED)).count();
                        ui.label(RichText::new(format!("{count} protected / {} windows", self.windows.len())).size(10.5).color(SUCCESS));
                        if self.busy { ui.spinner(); }
                        if !self.note.is_empty() {
                            ui.separator();
                            ui.add(egui::Label::new(RichText::new(&self.note).size(10.5)
                                .color(if self.note_error { ERROR } else { MUTED })).truncate());
                        }
                    });
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(BACKGROUND).inner_margin(egui::Margin::same(12)))
                .show(ctx, |ui| match self.page {
                    Page::Windows => self.windows_page(ui),
                    Page::Auto => self.auto_page(ui),
                    Page::Folders => self.folder_page(ui),
                    Page::Capture => self.capture_test.ui(ui, ctx),
                    Page::Settings => self.settings_page(ui),
                });
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }

    pub fn run() -> eframe::Result<()> {
        let png = image::load_from_memory(include_bytes!("../assets/pawhide.png"))
            .expect("bundled icon PNG is valid")
            .resize_exact(256, 256, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let icon = egui::IconData { rgba: png.into_raw(), width: 256, height: 256 };
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("PawHide")
                .with_inner_size([1080.0, 700.0])
                .with_min_inner_size([880.0, 580.0])
                .with_max_inner_size([1600.0, 1000.0])
                .with_maximize_button(false)
                .with_resizable(true)
                .with_decorations(false)
                .with_icon(Arc::new(icon)),
            ..Default::default()
        };
        eframe::run_native("PawHide", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
    }
}

#[cfg(windows)]
fn main() -> eframe::Result<()> {
    let Some(_instance) = single_instance::acquire() else { return Ok(()); };
    windows_ui::run()
}
