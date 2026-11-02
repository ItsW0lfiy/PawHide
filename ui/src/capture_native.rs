//! Native Windows.Graphics.Capture preview. Capture-only: does not modify local
//! window visibility, process injection, taskbar settings or Discord.
#![cfg(windows)]

use std::{
    ffi::c_void, ptr,
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    core::{factory, Interface},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{DirectXPixelFormat, Direct3D11::IDirect3DDevice},
    },
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CPU_ACCESS_READ,
                D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext,
                ID3D11Resource, ID3D11Texture2D,
            },
            Dxgi::IDXGIDevice, Gdi::HMONITOR,
        },
        System::WinRT::{
            RoInitialize, RO_INIT_MULTITHREADED,
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
};

pub const TARGET_FPS: u64 = 60;
const MAX_WIDTH: u32 = 1280;
const MAX_HEIGHT: u32 = 720;

pub struct PreviewFrame {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub struct CaptureSession {
    latest: Arc<Mutex<Option<PreviewFrame>>>,
    frames: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl CaptureSession {
    pub fn start(monitor: usize, include_cursor: bool) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let frames = Arc::new(AtomicU64::new(0));
        let error = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (new_frame, count, failed, stopping) =
            (Arc::clone(&latest), Arc::clone(&frames), Arc::clone(&error), Arc::clone(&stop));
        let worker = thread::spawn(move || {
            if let Err(reason) = capture_loop(monitor, include_cursor, &stopping, &new_frame, &count) {
                if let Ok(mut value) = failed.lock() { *value = Some(reason); }
            }
        });
        Self { latest, frames, error, stop, worker: Some(worker) }
    }
    pub fn take_latest(&self) -> Option<PreviewFrame> { self.latest.lock().ok()?.take() }
    pub fn count(&self) -> u64 { self.frames.load(Ordering::Relaxed) }
    pub fn error(&self) -> Option<String> { self.error.lock().ok()?.clone() }
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
impl Drop for CaptureSession { fn drop(&mut self) { self.stop(); } }

fn capture_loop(
    monitor: usize, include_cursor: bool, stop: &AtomicBool,
    latest: &Mutex<Option<PreviewFrame>>, count: &AtomicU64,
) -> Result<(), String> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|e| format!("WinRT initialization: {e}"))?;
    let item: GraphicsCaptureItem = unsafe {
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| format!("Capture interop: {e}"))?;
        interop.CreateForMonitor(HMONITOR(monitor as *mut c_void))
            .map_err(|e| format!("Capture monitor: {e}"))?
    };
    let size = item.Size().map_err(|e| e.to_string())?;
    if size.Width <= 0 || size.Height <= 0 { return Err("Monitor has invalid dimensions".into()); }

    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE(ptr::null_mut()),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION,
            Some(&mut device), None, Some(&mut context))
            .map_err(|e| format!("D3D11: {e}"))?;
    }
    let device: ID3D11Device = device.ok_or("Missing D3D11 device")?;
    let context: ID3D11DeviceContext = context.ok_or("Missing D3D11 context")?;
    let dxgi: IDXGIDevice = device.cast().map_err(|e| e.to_string())?;
    let winrt: IDirect3DDevice = unsafe {
        CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
            .map_err(|e| format!("D3D bridge: {e}"))?
            .cast().map_err(|e| format!("D3D bridge cast: {e}"))?
    };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)
        .map_err(|e| format!("Capture frame pool: {e}"))?;
    let session = pool.CreateCaptureSession(&item)
        .map_err(|e| format!("Capture session: {e}"))?;
    // Windows performs cursor composition itself. Off by default avoids
    // duplicate pointer drawing and flickering in a nested preview.
    session.SetIsCursorCaptureEnabled(include_cursor)
        .map_err(|e| format!("Cursor capture setting: {e}"))?;
    let _ = session.SetIsBorderRequired(false);
    session.StartCapture().map_err(|e| format!("Start capture: {e}"))?;

    let mut staging: Option<ID3D11Texture2D> = None;
    let mut staging_size = (0, 0);
    let mut last = Instant::now() - Duration::from_millis(20);
    while !stop.load(Ordering::Acquire) {
        if let Ok(frame) = pool.TryGetNextFrame() {
            if last.elapsed() >= Duration::from_micros(1_000_000 / TARGET_FPS) {
                last = Instant::now();
                if let Ok(surface) = frame.Surface() {
                    if let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() {
                        if let Ok(texture) = unsafe { access.GetInterface::<ID3D11Texture2D>() } {
                            if let Ok(image) = unsafe {
                                readback(&device, &context, &texture, &mut staging, &mut staging_size)
                            } {
                                if let Ok(mut slot) = latest.lock() { *slot = Some(image); }
                                count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            let _ = frame.Close();
        }
        thread::sleep(Duration::from_millis(3));
    }
    let _ = session.Close();
    let _ = pool.Close();
    Ok(())
}

unsafe fn readback(
    device: &ID3D11Device, ctx: &ID3D11DeviceContext, texture: &ID3D11Texture2D,
    staging: &mut Option<ID3D11Texture2D>, dimensions: &mut (u32, u32),
) -> windows::core::Result<PreviewFrame> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        if desc.Width == 0 || desc.Height == 0 || desc.Width > 16384 || desc.Height > 16384 {
            return Err(windows::core::Error::from_win32());
        }
        if *dimensions != (desc.Width, desc.Height) {
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut new_texture = None;
            device.CreateTexture2D(&desc, None, Some(&mut new_texture))?;
            *staging = new_texture;
            *dimensions = (desc.Width, desc.Height);
        }
        let target = staging.as_ref().expect("Staging texture exists");
        let src: ID3D11Resource = texture.cast()?;
        let dst: ID3D11Resource = target.cast()?;
        ctx.CopyResource(&dst, &src);
        let mut data = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&dst, 0, D3D11_MAP_READ, 0, Some(&mut data))?;
        let (sw, sh) = (desc.Width as usize, desc.Height as usize);
        let scale = (MAX_WIDTH as f64 / sw as f64).min(MAX_HEIGHT as f64 / sh as f64).min(1.0);
        let (w, h) = ((sw as f64 * scale).round().max(1.0) as usize,
                      (sh as f64 * scale).round().max(1.0) as usize);
        let mut rgba = vec![0_u8; w * h * 4];
        let pitch = data.RowPitch as usize;
        let pixels = data.pData as *const u8;
        if !pixels.is_null() && pitch >= sw * 4 {
            for y in 0..h {
                for x in 0..w {
                    let source = pixels.add((y * sh / h) * pitch + (x * sw / w) * 4);
                    let dest = (y * w + x) * 4;
                    rgba[dest] = *source.add(2);
                    rgba[dest + 1] = *source.add(1);
                    rgba[dest + 2] = *source;
                    rgba[dest + 3] = 255;
                }
            }
        }
        ctx.Unmap(&dst, 0);
        Ok(PreviewFrame { rgba, width: w, height: h })
    }
}
