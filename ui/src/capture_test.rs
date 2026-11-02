//! Minimal native test view for a single physical display.
//! Capture occurs on a worker thread; only the latest frame reaches the UI.
#![cfg(windows)]

use crate::capture_native::{CaptureSession, TARGET_FPS};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{BOOL, LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW},
};

struct Display {
    handle: usize,
    label: String,
    device: String,
}

fn monitors() -> Result<Vec<Display>, String> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR, _: HDC, _: *mut RECT, arg: LPARAM,
    ) -> BOOL {
        let out = unsafe { &mut *(arg as *mut Vec<Display>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 { return 1; }
        let end = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        let device = String::from_utf16_lossy(&info.szDevice[..end]);
        let number = device.rsplit("DISPLAY").next()
            .and_then(|x| x.parse::<u32>().ok()).unwrap_or(999);
        let box_ = info.monitorInfo.rcMonitor;
        let width = box_.right - box_.left;
        let height = box_.bottom - box_.top;
        let primary = info.monitorInfo.dwFlags & 1 != 0;
        out.push(Display {
            handle: monitor as usize,
            label: format!("Screen {number} · {width}×{height}{}", if primary { " · primary" } else { "" }),
            device,
        });
        1
    }
    let mut out = Vec::new();
    if unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect),
            &mut out as *mut _ as LPARAM)
    } == 0 { return Err("Cannot enumerate physical monitors".to_owned()); }
    out.sort_by(|a, b| a.device.cmp(&b.device));
    if out.is_empty() { return Err("No physical monitor found".to_owned()); }
    Ok(out)
}

pub struct CaptureTest {
    monitors: Vec<Display>,
    selected: usize,
    cursor: bool,
    session: Option<CaptureSession>,
    texture: Option<TextureHandle>,
    error: Option<String>,
    last_count: u64,
    last_check: Instant,
    fps: f32,
}

impl CaptureTest {
    pub fn new() -> Self {
        let result = monitors();
        let error = result.as_ref().err().cloned();
        Self {
            monitors: result.unwrap_or_default(), selected: 0, cursor: false,
            session: None, texture: None, error,
            last_count: 0, last_check: Instant::now(), fps: 0.0,
        }
    }
    pub fn stop(&mut self) {
        self.session = None;
        self.texture = None;
        self.fps = 0.0;
        self.last_count = 0;
    }
    fn start(&mut self) {
        self.stop();
        self.error = None;
        if let Some(monitor) = self.monitors.get(self.selected) {
            self.session = Some(CaptureSession::start(monitor.handle, self.cursor));
            self.last_check = Instant::now();
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut restart = false;
        ui.horizontal(|ui| {
            let label = self.monitors.get(self.selected)
                .map(|monitor| monitor.label.as_str()).unwrap_or("No screens");
            egui::ComboBox::from_id_salt("pawhide-capture-monitor")
                .selected_text(label).width(235.0).show_ui(ui, |ui| {
                    for (index, monitor) in self.monitors.iter().enumerate() {
                        if ui.selectable_value(&mut self.selected, index, &monitor.label).changed() {
                            restart = true;
                        }
                    }
                });
            let running = self.session.is_some();
            if ui.add_enabled(!self.monitors.is_empty(),
                egui::Button::new(if running { "Pause" } else { "Start / Resume" })
            ).clicked() {
                if running { self.stop(); } else { self.start(); }
            }
            if ui.button("Refresh screens").clicked() {
                self.stop();
                match monitors() {
                    Ok(found) => { self.monitors = found; self.selected = 0; self.error = None; }
                    Err(error) => { self.monitors.clear(); self.error = Some(error); }
                }
            }
            if ui.checkbox(&mut self.cursor, "Cursor").on_hover_text(
                "Off by default to avoid pointer doubling/flicker. When enabled Windows captures the cursor natively."
            ).changed() { restart = true; }
            ui.label(RichText::new(format!("{TARGET_FPS} FPS target")).color(Color32::GRAY).small());
        });
        if restart && self.session.is_some() { self.start(); }

        if let Some(capture) = &self.session {
            if let Some(error) = capture.error() {
                self.error = Some(error);
                self.stop();
            } else {
                if let Some(frame) = capture.take_latest() {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width, frame.height], &frame.rgba);
                    if let Some(texture) = &mut self.texture {
                        texture.set(image, egui::TextureOptions::LINEAR);
                    } else {
                        self.texture = Some(ctx.load_texture("pawhide-native-test", image,
                            egui::TextureOptions::LINEAR));
                    }
                }
                if self.last_check.elapsed() >= Duration::from_secs(1) {
                    let count = capture.count();
                    let elapsed = self.last_check.elapsed().as_secs_f32();
                    self.fps = count.saturating_sub(self.last_count) as f32 / elapsed;
                    self.last_count = count;
                    self.last_check = Instant::now();
                }
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Actual {:.1} FPS", self.fps)).color(Color32::GRAY).size(11.0));
            if self.monitors.len() < 2 {
                ui.label(RichText::new("Single monitor: the preview may appear inside itself.")
                    .color(Color32::LIGHT_YELLOW).size(11.0));
            } else {
                ui.label(RichText::new("Place PawHide on the OTHER monitor to avoid recursion.")
                    .color(Color32::GRAY).size(11.0));
            }
        });
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(Color32::LIGHT_RED));
        }
        let room = ui.available_size();
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let scale = (room.x / size.x).min(room.y / size.y).max(0.01);
            ui.centered_and_justified(|ui| {
                ui.add(egui::Image::new((texture.id(), size * scale)));
            });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space((room.y * 0.35).max(24.0));
                ui.label(RichText::new("Native screen capture preview").size(18.0).color(Color32::LIGHT_GRAY));
                ui.label(RichText::new("Choose a monitor and press Start. No FFmpeg, recording, or separate app required.")
                    .color(Color32::GRAY));
            });
        }
    }
}
