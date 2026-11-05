//! Native capture with event-driven acquisition, GPU scaling and reusable readback.
#![cfg(windows)]
use std::{
    collections::VecDeque,
    ffi::c_void,
    mem::ManuallyDrop,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::*,
            Dxgi::{Common::DXGI_RATIONAL, DXGI_ERROR_WAS_STILL_DRAWING, IDXGIDevice},
            Gdi::HMONITOR,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
        },
    },
    core::{Interface, factory},
};

pub const TARGET_FPS: u64 = 60;
pub struct PreviewFrame {
    /// Straight-alpha RGBA pixels, ready for Slint's SharedPixelBuffer.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
#[derive(Default)]
struct Counters {
    acquired: AtomicU64,
    processed: AtomicU64,
    processing_us: AtomicU64,
}

pub struct CaptureSession {
    latest: Arc<Mutex<Option<PreviewFrame>>>,
    recycle: Arc<Mutex<Vec<Vec<u8>>>>,
    counters: Arc<Counters>,
    error: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    cursor: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl CaptureSession {
    pub fn start(
        monitor: usize,
        include_cursor: bool,
        native_resolution: bool,
    ) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let recycle = Arc::new(Mutex::new(Vec::new()));
        let counters = Arc::new(Counters::default());
        let error = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let cursor = Arc::new(AtomicBool::new(include_cursor));
        let (slot, buffers, stats, failed, stopping, pointer) = (
            Arc::clone(&latest),
            Arc::clone(&recycle),
            Arc::clone(&counters),
            Arc::clone(&error),
            Arc::clone(&stop),
            Arc::clone(&cursor),
        );
        let worker = thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                capture_loop(
                    monitor,
                    native_resolution,
                    &pointer,
                    &stopping,
                    FrameOutput {
                        latest: &slot,
                        buffers: &buffers,
                        counters: &stats,
                    },
                )
            }));
            let reason = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(_) => Some("Capture worker stopped unexpectedly.".into()),
            };
            if let Some(reason) = reason
                && let Ok(mut value) = failed.lock()
            {
                *value = Some(reason);
            }
        });
        Self {
            latest,
            recycle,
            counters,
            error,
            stop,
            cursor,
            worker: Some(worker),
        }
    }
    pub fn take_latest(&self) -> Option<PreviewFrame> {
        self.latest.lock().ok()?.take()
    }
    pub fn recycle(&self, frame: PreviewFrame) {
        if let Ok(mut pool) = self.recycle.lock()
            && pool.len() < 3
        {
            pool.push(frame.rgba);
        }
    }
    pub fn counts(&self) -> (u64, u64, u64) {
        (
            self.counters.acquired.load(Ordering::Relaxed),
            self.counters.processed.load(Ordering::Relaxed),
            self.counters.processing_us.load(Ordering::Relaxed),
        )
    }
    pub fn set_cursor(&self, cursor: bool) {
        self.cursor.store(cursor, Ordering::Relaxed);
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }
}
impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // A driver stall must not freeze the UI during pause, switching or exit.
        if let Some(worker) = self.worker.take()
            && worker.is_finished()
        {
            let _ = worker.join();
        }
    }
}

struct CaptureResources {
    pool: Direct3D11CaptureFramePool,
    session: windows::Graphics::Capture::GraphicsCaptureSession,
    token: i64,
    item: GraphicsCaptureItem,
    closed_token: i64,
}
impl Drop for CaptureResources {
    fn drop(&mut self) {
        let _ = self.item.RemoveClosed(self.closed_token);
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}

struct FrameOutput<'a> {
    latest: &'a Mutex<Option<PreviewFrame>>,
    buffers: &'a Mutex<Vec<Vec<u8>>>,
    counters: &'a Counters,
}

fn capture_loop(
    monitor: usize,
    native_resolution: bool,
    cursor: &AtomicBool,
    stop: &AtomicBool,
    output: FrameOutput<'_>,
) -> Result<(), String> {
    let FrameOutput {
        latest,
        buffers,
        counters,
    } = output;
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|e| format!("WinRT initialization: {e}"))?;
    let _apartment = Apartment;
    let item: GraphicsCaptureItem = unsafe {
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .and_then(|i| i.CreateForMonitor(HMONITOR(monitor as *mut c_void)))
            .map_err(|e| format!("Capture monitor: {e}"))?
    };
    let mut size = item.Size().map_err(|e| e.to_string())?;
    if size.Width <= 0 || size.Height <= 0 {
        return Err("Monitor dimensions are invalid.".into());
    }
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(ptr::null_mut()),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .map_err(|e| format!("D3D11 device: {e}"))?;
    let device = device.ok_or("Missing D3D11 device")?;
    let context = context.ok_or("Missing D3D11 context")?;
    let dxgi: IDXGIDevice = device.cast().map_err(|e| e.to_string())?;
    let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .and_then(|d| d.cast())
        .map_err(|e| format!("D3D bridge: {e}"))?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        3,
        size,
    )
    .map_err(|e| format!("Frame pool: {e}"))?;
    let session = pool
        .CreateCaptureSession(&item)
        .map_err(|e| e.to_string())?;
    let mut applied_cursor = cursor.load(Ordering::Relaxed);
    session
        .SetIsCursorCaptureEnabled(applied_cursor)
        .map_err(|e| format!("Cursor setting: {e}"))?;
    let _ = session.SetIsBorderRequired(false);
    let (signal, arrived) = mpsc::sync_channel(1);
    let token = pool
        .FrameArrived(&TypedEventHandler::new(move |_, _| {
            let _ = signal.try_send(());
            Ok(())
        }))
        .map_err(|e| e.to_string())?;
    let closed = Arc::new(AtomicBool::new(false));
    let closed_flag = Arc::clone(&closed);
    let closed_token = item
        .Closed(&TypedEventHandler::new(move |_, _| {
            closed_flag.store(true, Ordering::Release);
            Ok(())
        }))
        .map_err(|e| e.to_string())?;
    let resources = CaptureResources {
        pool,
        session,
        token,
        item,
        closed_token,
    };
    resources
        .session
        .StartCapture()
        .map_err(|e| format!("Start capture: {e}"))?;
    let mut pipeline: Option<Readback> = None;
    let mut last_timestamp = None;
    let mut pacer = FramePacer::default();
    while !stop.load(Ordering::Acquire) {
        let _ = arrived.recv_timeout(Duration::from_millis(20));
        if closed.load(Ordering::Acquire) {
            return Err("Capture source closed; waiting for the selected display.".into());
        }
        let wanted_cursor = cursor.load(Ordering::Relaxed);
        if wanted_cursor != applied_cursor {
            resources
                .session
                .SetIsCursorCaptureEnabled(wanted_cursor)
                .map_err(|e| format!("Cursor setting: {e}"))?;
            applied_cursor = wanted_cursor;
        }
        let mut newest = None;
        loop {
            let Some(frame) = frame_or_idle(resources.pool.TryGetNextFrame())
                .map_err(|error| format!("Frame acquisition: {error}"))?
            else {
                break;
            };
            let timestamp = frame
                .SystemRelativeTime()
                .map_err(|e| e.to_string())?
                .Duration;
            if last_timestamp == Some(timestamp) {
                let _ = frame.Close();
                continue;
            }
            last_timestamp = Some(timestamp);
            counters.acquired.fetch_add(1, Ordering::Relaxed);
            if let Some(old) = newest.replace(frame) {
                let _ = old.Close();
            }
        }
        let started = Instant::now();
        if let Some(frame) = newest {
            if !pacer.admit(
                frame
                    .SystemRelativeTime()
                    .map_err(|e| e.to_string())?
                    .Duration,
            ) {
                let _ = frame.Close();
                continue;
            }
            let content = frame.ContentSize().map_err(|e| e.to_string())?;
            if content.Width <= 0 || content.Height <= 0 {
                let _ = frame.Close();
                continue;
            }
            if content != size {
                frame.Close().map_err(|e| e.to_string())?;
                size = content;
                resources
                    .pool
                    .Recreate(&winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 3, size)
                    .map_err(|e| format!("Monitor resize: {e}"))?;
                pipeline = None;
                continue;
            }
            let surface = frame
                .Surface()
                .map_err(|e| format!("Capture surface: {e}"))?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| e.to_string())?;
            let texture: ID3D11Texture2D =
                unsafe { access.GetInterface() }.map_err(|e| e.to_string())?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&mut desc);
            }
            if pipeline
                .as_ref()
                .is_none_or(|p| p.source != (desc.Width, desc.Height))
            {
                pipeline = Some(unsafe { Readback::new(&device, &context, desc, native_resolution) }
                    .map_err(|e| format!("Preview pipeline: {e}. Try native resolution in Settings if GPU scaling is unavailable."))?);
            }
            unsafe {
                pipeline
                    .as_mut()
                    .ok_or("Missing preview pipeline")?
                    .submit(&device, &context, &texture)
            }
            .map_err(|e| format!("GPU frame copy: {e}"))?;
            frame.Close().map_err(|e| e.to_string())?;
        }
        if let Some(pipeline) = &mut pipeline
            && let Some(image) = unsafe { pipeline.take_ready(&context, buffers) }
                .map_err(|e| format!("Frame readback: {e}"))?
        {
            if let Ok(mut slot) = latest.lock()
                && let Some(old) = slot.replace(image)
                && let Ok(mut pool) = buffers.lock()
                && pool.len() < 3
            {
                pool.push(old.rgba);
            }
            counters.processed.fetch_add(1, Ordering::Relaxed);

            counters
                .processing_us
                .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        }
    }
    Ok(())
}

fn frame_or_idle(
    result: windows::core::Result<windows::Graphics::Capture::Direct3D11CaptureFrame>,
) -> windows::core::Result<Option<windows::Graphics::Capture::Direct3D11CaptureFrame>> {
    match result {
        Ok(frame) => Ok(Some(frame)),
        // windows-core represents an S_OK/null WinRT interface as Error::empty.
        Err(error) if error.code() == windows::core::HRESULT(0) => Ok(None),
        Err(error) => Err(error),
    }
}

#[derive(Default)]
struct FramePacer {
    next: Option<i64>,
}
impl FramePacer {
    fn admit(&mut self, timestamp: i64) -> bool {
        let interval = 10_000_000 / TARGET_FPS as i64;
        let next = self.next.get_or_insert(timestamp);
        // Absorb sub-millisecond jitter without resetting the pacing phase.
        if timestamp + 10_000 < *next {
            return false;
        }
        let periods = ((timestamp + 10_000 - *next) / interval) + 1;
        *next += periods * interval;
        true
    }
}

struct Scaler {
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    output: ID3D11Texture2D,
    view: ID3D11VideoProcessorOutputView,
}
impl Scaler {
    unsafe fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        desc: D3D11_TEXTURE2D_DESC,
        width: u32,
        height: u32,
    ) -> windows::core::Result<Self> {
        unsafe {
            let video: ID3D11VideoDevice = device.cast()?;
            let video_context: ID3D11VideoContext = context.cast()?;
            let rate = DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            };
            let enumerator =
                video.CreateVideoProcessorEnumerator(&D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                    InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                    InputFrameRate: rate,
                    InputWidth: desc.Width,
                    InputHeight: desc.Height,
                    OutputFrameRate: rate,
                    OutputWidth: width,
                    OutputHeight: height,
                    Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
                })?;
            let processor = video.CreateVideoProcessor(&enumerator, 0)?;
            let mut output = None;
            device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    ..desc
                },
                None,
                Some(&mut output),
            )?;
            let output = output.ok_or_else(|| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004003u32 as i32),
                    "D3D11 returned an empty resource",
                )
            })?;
            let mut view = None;
            video.CreateVideoProcessorOutputView(
                &output,
                &enumerator,
                &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                },
                Some(&mut view),
            )?;
            let view = view.ok_or_else(|| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004003u32 as i32),
                    "D3D11 returned an empty resource",
                )
            })?;
            let source = RECT {
                left: 0,
                top: 0,
                right: desc.Width as i32,
                bottom: desc.Height as i32,
            };
            let dest = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            video_context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&source));
            video_context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&dest));
            video_context.VideoProcessorSetOutputTargetRect(&processor, true, Some(&dest));
            video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            Ok(Self {
                device: video,
                context: video_context,
                enumerator,
                processor,
                output,
                view,
            })
        }
    }
    unsafe fn scale(&self, input: &ID3D11Texture2D) -> windows::core::Result<()> {
        unsafe {
            let mut view = None;
            self.device.CreateVideoProcessorInputView(
                input,
                &self.enumerator,
                &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                },
                Some(&mut view),
            )?;
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: ManuallyDrop::new(view),
                ..Default::default()
            };
            let result = self.context.VideoProcessorBlt(
                &self.processor,
                &self.view,
                0,
                std::slice::from_ref(&stream),
            );
            ManuallyDrop::drop(&mut stream.pInputSurface);
            result
        }
    }
}
struct Readback {
    source: (u32, u32),
    width: u32,
    height: u32,
    scaler: Option<Scaler>,
    slots: Vec<ID3D11Texture2D>,
    pending: VecDeque<usize>,
    next: usize,
}
impl Readback {
    unsafe fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        desc: D3D11_TEXTURE2D_DESC,
        native: bool,
    ) -> windows::core::Result<Self> {
        unsafe {
            if desc.Width == 0 || desc.Height == 0 || desc.Width > 16384 || desc.Height > 16384 {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80070057u32 as i32),
                    "Invalid texture dimensions",
                ));
            }
            let scale = if native {
                1.0
            } else {
                (1920.0 / desc.Width as f64)
                    .min(1080.0 / desc.Height as f64)
                    .min(1.0)
            };
            let (width, height) = (
                (desc.Width as f64 * scale).round().max(1.0) as u32,
                (desc.Height as f64 * scale).round().max(1.0) as u32,
            );
            let scaler = if (width, height) == (desc.Width, desc.Height) {
                None
            } else {
                Some(Scaler::new(device, context, desc, width, height)?)
            };
            let mut slots = Vec::new();
            for _ in 0..3 {
                let mut texture = None;
                device.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: width,
                        Height: height,
                        MipLevels: 1,
                        ArraySize: 1,
                        Usage: D3D11_USAGE_STAGING,
                        BindFlags: 0,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        MiscFlags: 0,
                        ..desc
                    },
                    None,
                    Some(&mut texture),
                )?;
                slots.push(texture.ok_or_else(|| {
                    windows::core::Error::new(
                        windows::core::HRESULT(0x80004003u32 as i32),
                        "D3D11 returned an empty resource",
                    )
                })?);
            }
            Ok(Self {
                source: (desc.Width, desc.Height),
                width,
                height,
                scaler,
                slots,
                pending: VecDeque::new(),
                next: 0,
            })
        }
    }
    unsafe fn submit(
        &mut self,
        _device: &ID3D11Device,
        ctx: &ID3D11DeviceContext,
        texture: &ID3D11Texture2D,
    ) -> windows::core::Result<()> {
        unsafe {
            // If the GPU is still busy, drop an old preview rather than block capture.
            self.pending.retain(|&slot| slot != self.next);
            let source = if let Some(scaler) = &self.scaler {
                scaler.scale(texture)?;
                &scaler.output
            } else {
                texture
            };
            ctx.CopyResource(&self.slots[self.next], source);
            ctx.Flush();
            self.pending.push_back(self.next);
            self.next = (self.next + 1) % self.slots.len();
            Ok(())
        }
    }
    unsafe fn take_ready(
        &mut self,
        ctx: &ID3D11DeviceContext,
        buffers: &Mutex<Vec<Vec<u8>>>,
    ) -> windows::core::Result<Option<PreviewFrame>> {
        unsafe {
            let Some(&index) = self.pending.front() else {
                return Ok(None);
            };
            let target = &self.slots[index];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            match ctx.Map(
                target,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => return Ok(None),
                result => result?,
            }
            self.pending.pop_front();
            let width = self.width as usize;
            let height = self.height as usize;
            if mapped.pData.is_null() || (mapped.RowPitch as usize) < width * 4 {
                ctx.Unmap(target, 0);
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80070057u32 as i32),
                    "Invalid mapped frame layout",
                ));
            }
            let mut pixels = buffers
                .lock()
                .ok()
                .and_then(|mut pool| pool.pop())
                .unwrap_or_default();
            pixels.resize(width * height * 4, 0);
            for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
                let src = std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    width * 4,
                );
                for (input, output) in src.chunks_exact(4).zip(row.chunks_exact_mut(4)) {
                    output.copy_from_slice(&[input[2], input[1], input[0], 255]);
                }
            }
            ctx.Unmap(target, 0);
            Ok(Some(PreviewFrame { rgba: pixels, width: width as u32, height: height as u32 }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

    #[test]
    fn empty_winrt_frame_is_idle_but_real_acquisition_errors_propagate() {
        let empty = unsafe {
            <windows::Graphics::Capture::Direct3D11CaptureFrame as windows::core::Type<_>>::from_abi(
                ptr::null_mut(),
            )
        };
        assert!(frame_or_idle(empty).unwrap().is_none());
        let device_lost =
            windows::core::Error::from(windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED);
        assert!(frame_or_idle(Err(device_lost)).is_err());
    }

    #[test]
    fn timestamp_pacing_keeps_sixty_hz_and_bounds_high_refresh_work() {
        let mut pacer = FramePacer::default();
        for frame in 0..600 {
            assert!(pacer.admit(frame * 10_000_000 / 60 + if frame % 2 == 0 { 0 } else { -3000 }));
        }
        let mut pacer = FramePacer::default();
        let processed = (0..144)
            .filter(|frame| pacer.admit(frame * 10_000_000 / 144))
            .count();
        assert!(
            (59..=61).contains(&processed),
            "{processed} processed from 144 distinct frames"
        );
    }

    #[test]
    #[ignore = "requires a native D3D11 GPU"]
    fn gpu_scaling_and_readback_preserve_color() {
        unsafe {
            let (mut device, mut context) = (None, None);
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE(ptr::null_mut()),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .unwrap();
            let (device, context) = (device.unwrap(), context.unwrap());
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 2560,
                Height: 1440,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                ..Default::default()
            };
            let data: Vec<u8> = [31, 83, 171, 255].repeat((desc.Width * desc.Height) as usize);
            let init = D3D11_SUBRESOURCE_DATA {
                pSysMem: data.as_ptr().cast(),
                SysMemPitch: desc.Width * 4,
                SysMemSlicePitch: 0,
            };
            let mut texture = None;
            device
                .CreateTexture2D(&desc, Some(&init), Some(&mut texture))
                .unwrap();
            let texture = texture.unwrap();
            let mut readback = Readback::new(&device, &context, desc, false).unwrap();
            assert_eq!((readback.width, readback.height), (1920, 1080));
            readback.submit(&device, &context, &texture).unwrap();
            let started = Instant::now();
            let frame = loop {
                if let Some(frame) = readback
                    .take_ready(&context, &Mutex::new(Vec::new()))
                    .unwrap()
                {
                    break frame;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(3),
                    "GPU readback timed out"
                );
                thread::sleep(Duration::from_millis(1));
            };
            let elapsed = started.elapsed();
            for pixel in frame.rgba.chunks_exact(4) {
                for (actual, expected) in pixel.iter().zip([171u8, 83, 31, 255]) {
                    assert!(
                        actual.abs_diff(expected) <= 2,
                        "RGB scaling changed color: {pixel:?}"
                    );
                }
            }
            println!(
                "2560x1440 -> 1920x1080 GPU scaling/readback: {:.2} ms",
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }
}
