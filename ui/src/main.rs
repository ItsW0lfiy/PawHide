//! Optional native PawHide desktop manager (no WebView or Electron).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() { eprintln!("PawHideUI is available on Windows only."); }

#[cfg(windows)]
mod windows_ui {
    use eframe::egui::{self, Color32, RichText};
    use pawhide_core::{config, platform::{self, Window, EXCLUDED}};
    use std::{collections::BTreeMap, sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};
    use std::os::windows::process::CommandExt;

    const BG: Color32 = Color32::from_rgb(17,20,26);
    const PANEL: Color32 = Color32::from_rgb(24,28,36);
    const ACCENT: Color32 = Color32::from_rgb(111,189,191);
    const GREEN: Color32 = Color32::from_rgb(111,211,156);
    const MUTED: Color32 = Color32::from_rgb(154,166,183);
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Page { Windows, AutoHide, Settings }

    pub struct App {
        windows: Vec<Window>,
        rules: Vec<String>,
        page: Page,
        search: String,
        new_rule: String,
        note: String,
        last_refresh: Instant,
        busy: bool,
        completion_tx: Sender<String>,
        completion_rx: Receiver<String>,
    }

    impl App {
        fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(11.0, 12.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            cc.egui_ctx.set_style(style);
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BG;
            visuals.window_fill = PANEL;
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(38,43,53);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(52,64,75);
            visuals.selection.bg_fill = Color32::from_rgb(45,91,96);
            cc.egui_ctx.set_visuals(visuals);
            let (completion_tx, completion_rx) = mpsc::channel();
            let mut app = Self {
                windows: Vec::new(), rules: Vec::new(), page: Page::Windows,
                search: String::new(), new_rule: String::new(), note: String::new(),
                last_refresh: Instant::now() - Duration::from_secs(10),
                busy: false, completion_tx, completion_rx,
            };
            app.refresh();
            app
        }
        fn refresh(&mut self) {
            match platform::windows() {
                Ok(windows) => self.windows = windows.into_iter().filter(|w| !w.tool_window).collect(),
                Err(err) => self.note = format!("Window discovery failed: {err:#}"),
            }
            match config::load() {
                Ok(cfg) => self.rules = cfg.applications,
                Err(err) => self.note = format!("Config error: {err:#}"),
            }
            self.last_refresh = Instant::now();
        }
        fn protection(&mut self, window: &Window, hide: bool, ctx: egui::Context) {
            if self.busy { return }
            self.busy = true;
            self.note = format!("Working on {}…", window.exe);
            let w = window.clone();
            let sender = self.completion_tx.clone();
            // Native injection is synchronous and can be slow: keep it off the UI thread.
            thread::spawn(move || {
                let message = match platform::apply_window(&w, hide, false) {
                    Ok(true) => format!("{} — {}", w.exe, if hide {"Excluded"} else {"Visible"}),
                    Ok(false) => format!("{} — Windows could not verify capture state", w.exe),
                    Err(err) => format!("{} — {err:#}", w.exe),
                };
                let _ = sender.send(message);
                ctx.request_repaint();
            });
        }
        fn add_rule(&mut self) {
            let result: anyhow_like::Result = (|| {
                let name=config::normalize_exe(&self.new_rule).map_err(|e|e.to_string())?;
                let mut cfg=config::load().map_err(|e|e.to_string())?;
                if !cfg.applications.contains(&name) {
                    cfg.applications.push(name.clone());cfg.applications.sort();
                    config::save(&cfg).map_err(|e|e.to_string())?;
                }
                self.new_rule.clear();
                Ok(())
            })();
            match result {Ok(())=>self.note="AutoHide rule saved".into(),Err(err)=>self.note=err}
            self.refresh();
        }
        fn remove_rule(&mut self, name:&str) {
            match config::load() {
                Ok(mut cfg) => {
                    cfg.applications.retain(|x|x!=name);
                    match config::save(&cfg){Ok(())=>self.note=format!("Removed {name} from AutoHide"),Err(err)=>self.note=err.to_string()}
                },
                Err(err)=>self.note=err.to_string(),
            }
            self.refresh();
        }
        fn auto_command(&mut self, argument:&str) {
            // AutoHide worker lives in the CLI; GUI uses the stable `auto` contract.
            // Manual window operations never shell out and use pawhide-core directly.
            let result = std::env::current_exe()
                .map(|path|path.with_file_name("pawhide.exe"))
                .map_err(|e|e.to_string())
                .and_then(|path|std::process::Command::new(path)
                    .args(["auto",argument])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output().map_err(|e|e.to_string()));
            self.note = match result {
                Ok(output) if output.status.success()=>String::from_utf8_lossy(&output.stdout).trim().into(),
                Ok(output)=>String::from_utf8_lossy(&output.stderr).trim().into(),
                Err(err)=>format!("Cannot control AutoHide: {err}. Put pawhide.exe beside PawHideUI.exe"),
            };
            self.refresh();
        }
        fn heading(ui:&mut egui::Ui,title:&str,detail:&str) {
            ui.label(RichText::new(title).size(24.0).strong().color(Color32::WHITE));
            ui.label(RichText::new(detail).size(13.0).color(MUTED));
            ui.add_space(12.0);
        }
        fn windows_page(&mut self,ui:&mut egui::Ui) {
            Self::heading(ui,"Windows","Manage individual windows without hunting for hexadecimal handles.");
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add_sized([320.0,30.0],egui::TextEdit::singleline(&mut self.search).hint_text("Application or window title"));
                if ui.button("Refresh").clicked() {self.refresh()}
            });
            ui.separator();
            let mut grouped:BTreeMap<String,Vec<Window>> = BTreeMap::new();
            for w in &self.windows {
                if !self.search.is_empty() && !w.exe.to_lowercase().contains(&self.search.to_lowercase())
                    && !w.title.to_lowercase().contains(&self.search.to_lowercase()) {continue}
                grouped.entry(w.exe.clone()).or_default().push(w.clone());
            }
            egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui| {
                if grouped.is_empty(){ui.label("No matching windows");}
                for (app,items) in grouped {
                    let hidden=items.iter().filter(|w|w.affinity==Some(EXCLUDED)).count();
                    egui::CollapsingHeader::new(format!("{app}    ·    {hidden}/{} protected",items.len()))
                        .id_salt(&app).default_open(hidden>0 || items.len()>1)
                        .show(ui,|ui|{
                            for w in items {
                                ui.horizontal(|ui|{
                                    ui.vertical(|ui|{
                                        ui.label(RichText::new(&w.title).color(Color32::WHITE));
                                        ui.label(RichText::new(format!("Window 0x{:X} · PID {}",w.hwnd,w.pid)).small().color(MUTED));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                                        if w.affinity == Some(EXCLUDED) {
                                            if ui.add_enabled(!self.busy, egui::Button::new("Show")).clicked(){self.protection(&w,false,ui.ctx().clone())}
                                            ui.colored_label(GREEN,"Protected");
                                        } else {
                                            if ui.add_enabled(!self.busy, egui::Button::new("Hide")).clicked(){self.protection(&w,true,ui.ctx().clone())}
                                            ui.colored_label(MUTED,if w.affinity==Some(0){"Visible"}else{"Unknown"});
                                        }
                                    });
                                });
                                ui.separator();
                            }
                        });
                }
            });
        }
        fn auto_page(&mut self,ui:&mut egui::Ui) {
            Self::heading(ui,"AutoHide","Save executable rules and manage the background monitor.");
            ui.horizontal(|ui| {
                if ui.button("Start monitoring").clicked(){self.auto_command("start")}
                if ui.button("Stop monitoring").clicked(){self.auto_command("stop")}
                if ui.button("Check status").clicked(){self.auto_command("status")}
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_sized([300.0,30.0],egui::TextEdit::singleline(&mut self.new_rule).hint_text("Example: PicView.exe"));
                if ui.button("Add application").clicked(){self.add_rule()}
            });
            ui.add_space(6.0);
            for name in self.rules.clone() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&name).color(Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        if ui.button("Remove").clicked(){self.remove_rule(&name)}
                    });
                });
                ui.separator();
            }
            ui.label(RichText::new("AutoHide is asynchronous: activate and verify before beginning a sensitive screen share.").small().color(MUTED));
        }
        fn settings_page(&mut self,ui:&mut egui::Ui) {
            Self::heading(ui,"Settings","PawHide is local-only. Your rules are stored on this PC.");
            if let Ok(path)=config::config_path(){
                ui.label(RichText::new("Configuration file").strong());
                ui.monospace(path.display().to_string());
            }
            ui.add_space(12.0);
            ui.label("Normal Windows capture exclusion applies to all compatible capture programs, including ShareX. It cannot selectively exclude Discord.");
            ui.add_space(12.0);
            ui.label(RichText::new("Capture affinity readback is not proof against every capture method.").color(MUTED));
        }
    }

    mod anyhow_like { pub type Result = std::result::Result<(),String>; }

    impl eframe::App for App {
        fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
            while let Ok(message) = self.completion_rx.try_recv() {
                self.note = message; self.busy = false; self.refresh();
            }
            if !self.busy && self.last_refresh.elapsed()>=Duration::from_secs(2){self.refresh()}
            egui::SidePanel::left("pawhide_navigation").resizable(false).min_width(180.0).show(ctx,|ui|{
                ui.add_space(18.0);
                ui.label(RichText::new("🐾 PawHide").size(22.0).strong().color(ACCENT));
                ui.add_space(20.0);
                ui.selectable_value(&mut self.page,Page::Windows,"Windows");
                ui.selectable_value(&mut self.page,Page::AutoHide,"AutoHide");
                ui.selectable_value(&mut self.page,Page::Settings,"Settings");
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT),|ui|{
                    ui.label(RichText::new(format!("v{} · Windows",env!("CARGO_PKG_VERSION"))).small().color(MUTED));
                });
            });
            egui::TopBottomPanel::bottom("pawhide_notifications").show(ctx,|ui|{
                ui.horizontal(|ui|{
                    let protected=self.windows.iter().filter(|w|w.affinity==Some(EXCLUDED)).count();
                    ui.label(RichText::new(format!("{protected} protected / {} windows",self.windows.len())).color(GREEN));
                    ui.separator();
                    ui.label(RichText::new(&self.note).color(MUTED));
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG).inner_margin(24.0)).show(ctx,|ui|{
                match self.page {
                    Page::Windows=>self.windows_page(ui),
                    Page::AutoHide=>self.auto_page(ui),
                    Page::Settings=>self.settings_page(ui),
                }
            });
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }

    pub fn run()->eframe::Result<()> {
        let opts=eframe::NativeOptions {
            viewport:egui::ViewportBuilder::default().with_title("PawHide")
                .with_inner_size([960.0,660.0]).with_min_inner_size([730.0,480.0]),
            ..Default::default()
        };
        eframe::run_native("PawHide",opts,Box::new(|cc|Ok(Box::new(App::new(cc)))))
    }
}

#[cfg(windows)]
fn main()->eframe::Result<()> {windows_ui::run()}
