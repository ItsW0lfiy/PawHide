//! Minimal native test view for a single physical display.
//! Capture occurs on a worker thread; only the latest frame reaches the UI.
#![cfg(windows)]

use crate::capture_native::{CaptureSession, TARGET_FPS};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW},
};

struct Display {
    handle: usize,
    label: String,
    device: String,
}

fn monitors() -> Result<Vec<Display>, String> {
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, arg: LPARAM) -> i32 {
        let out = unsafe { &mut *(arg as *mut Vec<Display>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 {
            return 1;
        }
        let end = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        let device = String::from_utf16_lossy(&info.szDevice[..end]);
        let number = device
            .rsplit("DISPLAY")
            .next()
            .and_then(|x| x.parse::<u32>().ok())
            .unwrap_or(999);
        let box_ = info.monitorInfo.rcMonitor;
        let width = box_.right - box_.left;
        let height = box_.bottom - box_.top;
        let primary = info.monitorInfo.dwFlags & 1 != 0;
        out.push(Display {
            handle: monitor as usize,
            label: format!(
                "Screen {number} · {width}×{height}{}",
                if primary { " · primary" } else { "" }
            ),
            device,
        });
        1
    }
    let mut out: Vec<Display> = Vec::new();
    if unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut out as *mut _ as LPARAM,
        )
    } == 0
    {
        return Err("Cannot enumerate physical monitors".to_owned());
    }
    out.sort_by(|a, b| a.device.cmp(&b.device));
    if out.is_empty() {
        return Err("No physical monitor found".to_owned());
    }
    Ok(out)
}

pub struct CaptureTest {
    monitors: Vec<Display>,
    selected: usize,
    selected_device: Option<String>,
    cursor: bool,
    native: bool,
    requested: bool,
    session: Option<CaptureSession>,
    texture: Option<TextureHandle>,
    error: Option<String>,
    last_counts: (u64, u64, u64),
    last_check: Instant,
    capture_started_at: Instant,
    last_monitor_scan: Instant,
    captured_fps: f32,
    processed_fps: f32,
    render_fps: f32,
    processing_ms: f32,
    uploads: u64,
    presented: u64,
    last_presented: u64,
    current_sequence: u64,
    drawn_sequence: u64,
    events: Vec<(String, bool)>,
    context: Option<egui::Context>,
}
impl CaptureTest {
    pub fn new() -> Self {
        Self {
            monitors: Vec::new(),
            selected: 0,
            selected_device: None,
            cursor: false,
            native: false,
            requested: false,
            session: None,
            texture: None,
            error: None,
            last_counts: (0, 0, 0),
            last_check: Instant::now(),
            capture_started_at: Instant::now(),
            last_monitor_scan: Instant::now() - Duration::from_secs(5),
            captured_fps: 0.0,
            processed_fps: 0.0,
            render_fps: 0.0,
            processing_ms: 0.0,
            uploads: 0,
            presented: 0,
            last_presented: 0,
            current_sequence: 0,
            drawn_sequence: 0,
            events: Vec::new(),
            context: None,
        }
    }
    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }
    pub fn indicator(&self) -> String {
        let screen = self
            .monitors
            .get(self.selected)
            .map(|m| m.label.split(" · ").next().unwrap_or("Screen"))
            .unwrap_or("Screen disconnected");
        format!("Capture active · {screen} · {:.0} FPS", self.processed_fps)
    }
    pub fn take_events(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.events)
    }
    pub fn configure(&mut self, cursor: bool, native: bool) {
        self.cursor = cursor;
        if let Some(session) = &self.session {
            session.set_cursor(cursor);
        }
        if self.native != native {
            self.native = native;
            if self.session.is_some() {
                self.start();
            }
        }
    }
    fn clear_session(&mut self) {
        self.session = None;
        self.texture = None;
        self.captured_fps = 0.0;
        self.processed_fps = 0.0;
        self.render_fps = 0.0;
        self.last_counts = (0, 0, 0);
        self.uploads = 0;
        self.presented = 0;
        self.last_presented = 0;
    }
    fn start(&mut self) {
        self.clear_session();
        self.requested = true;
        self.error = None;
        if let Some(monitor) = self.monitors.get(self.selected) {
            self.selected_device = Some(monitor.device.clone());
            self.session = Some(CaptureSession::start(
                monitor.handle,
                self.cursor,
                self.native,
                self.context.clone(),
            ));
            self.last_check = Instant::now();
            self.capture_started_at = self.last_check;
            self.events
                .push((format!("Capture starting · {}", monitor.label), false));
        }
    }
    fn refresh_monitors(&mut self) {
        if self.last_monitor_scan.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.last_monitor_scan = Instant::now();
        match monitors() {
            Ok(found) => {
                if self.selected_device.is_none() {
                    self.selected_device = found.first().map(|m| m.device.clone());
                }
                let next = self
                    .selected_device
                    .as_ref()
                    .and_then(|d| found.iter().position(|m| &m.device == d));
                let old_handle = self.monitors.get(self.selected).map(|m| m.handle);
                let new_handle = next.map(|i| found[i].handle);
                let changed = old_handle != new_handle;
                self.monitors = found;
                if let Some(index) = next {
                    self.selected = index;
                    if self.requested && (changed || self.session.is_none()) {
                        self.start();
                    }
                } else {
                    if self.session.is_some() {
                        self.events.push((
                            "Capture source disconnected; waiting for that display.".into(),
                            true,
                        ));
                    }
                    self.clear_session();
                    self.selected = usize::MAX;
                    self.error = Some(
                        "Selected display disconnected. Reconnect it or choose another display."
                            .into(),
                    );
                }
            }
            Err(error) => {
                if self.session.is_some() {
                    self.events.push((error.clone(), true));
                }
                self.clear_session();
                self.monitors.clear();
                self.selected = usize::MAX;
                self.error = Some(error);
            }
        }
    }
    pub fn tick(&mut self, ctx: &egui::Context, preview_visible: bool) {
        self.context = Some(ctx.clone());
        self.refresh_monitors();
        if let Some(capture) = &self.session {
            // A WGC session can be created successfully but never deliver
            // its first frame. Avoid a permanent "Waiting for a new frame".
            if self.capture_started_at.elapsed() >= Duration::from_secs(10)
                && capture.counts().0 == 0
            {
                let reason = "No frames received from Windows Graphics Capture after 10 seconds. Try pausing and starting the preview, or choose another monitor.".to_owned();
                self.events.push((reason.clone(), true));
                self.error = Some(reason);
                self.requested = false;
                self.clear_session();
                return;
            }
            if let Some(error) = capture.error() {
                self.events.push((format!("Capture failed: {error}"), true));
                self.requested = error.starts_with("Capture source closed");
                self.error = Some(error);
                self.clear_session();
                return;
            }
            if let Some(frame) = capture.take_latest() {
                if preview_visible {
                    let image = frame.image;
                    if let Some(texture) = &mut self.texture {
                        texture.set(image, egui::TextureOptions::LINEAR);
                    } else {
                        self.texture = Some(ctx.load_texture(
                            "native-preview",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    self.uploads += 1;
                    self.current_sequence += 1;
                } else {
                    capture.recycle(frame);
                }
            }
            if self.last_check.elapsed() >= Duration::from_secs(1) {
                let (captured, processed, time) = capture.counts();
                let elapsed = self.last_check.elapsed().as_secs_f32();
                let delta = processed.saturating_sub(self.last_counts.1);
                self.captured_fps = captured.saturating_sub(self.last_counts.0) as f32 / elapsed;
                self.processed_fps = delta as f32 / elapsed;
                self.render_fps =
                    self.presented.saturating_sub(self.last_presented) as f32 / elapsed;
                self.processing_ms = if delta == 0 {
                    0.0
                } else {
                    time.saturating_sub(self.last_counts.2) as f32 / delta as f32 / 1000.0
                };
                self.last_counts = (captured, processed, time);
                self.last_presented = self.presented;
                self.last_check = Instant::now();
            }
            ctx.request_repaint_after(Duration::from_millis(1000 / TARGET_FPS * 2));
        }
    }
    fn draw(&mut self, ui: &mut egui::Ui, room: egui::Vec2) {
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let scale = (room.x / size.x).min(room.y / size.y).max(0.01);
            ui.centered_and_justified(|ui| {
                ui.add(egui::Image::new((texture.id(), size * scale)));
            });
            if self.drawn_sequence != self.current_sequence {
                self.presented += 1;
                self.drawn_sequence = self.current_sequence;
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(if self.is_active() {
                        "Waiting for a new frame…"
                    } else {
                        "Choose a display and start a preview."
                    })
                    .color(Color32::GRAY),
                );
            });
        }
    }
    pub fn thumbnail(&mut self, ctx: &egui::Context) {
        if !self.is_active() {
            return;
        }
        egui::Window::new("Live capture")
            .id(egui::Id::new("capture-thumbnail"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -40.0])
            .fixed_size([240.0, 135.0])
            .resizable(false)
            .collapsible(true)
            .show(ctx, |ui| {
                self.draw(ui, egui::vec2(ui.available_width(), 125.0));
            });
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) -> Option<(bool, bool)> {
        let mut changed = false;
        let mut restart = false;
        ui.horizontal(|ui| {
            let label = self
                .monitors
                .get(self.selected)
                .map(|m| m.label.as_str())
                .unwrap_or("Display disconnected");
            egui::ComboBox::from_id_salt("capture-source")
                .selected_text(label)
                .width(235.0)
                .show_ui(ui, |ui| {
                    for (index, monitor) in self.monitors.iter().enumerate() {
                        if ui
                            .selectable_value(&mut self.selected, index, &monitor.label)
                            .changed()
                        {
                            self.selected_device = Some(monitor.device.clone());
                            restart = self.requested;
                        }
                    }
                });
            if ui
                .add_enabled(
                    self.selected < self.monitors.len() || self.requested,
                    egui::Button::new(if self.requested {
                        "Pause"
                    } else {
                        "Start / Resume"
                    }),
                )
                .clicked()
            {
                if self.requested {
                    self.requested = false;
                    self.clear_session();
                    self.events.push(("Capture paused.".into(), false));
                } else {
                    self.start();
                }
            }
            changed |= ui.checkbox(&mut self.cursor, "Cursor").changed();
        });
        if restart {
            self.start();
        }
        if changed && let Some(session) = &self.session {
            session.set_cursor(self.cursor);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Source {:.0} · Processed {:.0} · Preview {:.0} FPS", self.captured_fps, self.processed_fps, self.render_fps)).size(11.0).color(Color32::GRAY))
                .on_hover_text("Source: distinct acquired WGC frames. Processed: completed readbacks. Preview: distinct images submitted in a visible preview. Static displays may update less frequently.");
            ui.label(RichText::new(format!("{:.1} ms CPU processing", self.processing_ms)).size(10.0).color(Color32::GRAY));
        });
        ui.label(RichText::new("Place PawHide on another display or exclude PawHide's window to avoid preview recursion.").size(10.5).color(Color32::GRAY));
        if let Some(error) = &self.error {
            egui::CollapsingHeader::new(
                RichText::new("Capture unavailable · details").color(Color32::LIGHT_RED),
            )
            .show(ui, |ui| {
                ui.label(error);
            });
        }
        let room = ui.available_size();
        self.draw(ui, room);
        changed.then_some((self.cursor, self.native))
    }
}
