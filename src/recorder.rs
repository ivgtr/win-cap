use crate::encoder::Encoder;
use std::{
    fs::OpenOptions,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use win_cap::Region;
use windows::{
    Win32::{
        Graphics::{
            Direct3D11::{
                D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Multithread, ID3D11Texture2D,
            },
            Dxgi::IDXGISurface,
        },
        System::WinRT::Direct3D11::CreateDirect3D11SurfaceFromDXGISurface,
    },
    core::Interface,
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};

pub struct Recording {
    pub stop: Arc<AtomicBool>,
    pub frames: Arc<AtomicU64>,
    pub dropped: Arc<AtomicU64>,
    pub worker: JoinHandle<Result<PathBuf, String>>,
}

struct Flags {
    region: Region,
    output: PathBuf,
    fps: u32,
    frames: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

struct Capture {
    flags: Flags,
    encoder: Option<Encoder>,
    first_timestamp: Option<i64>,
    first_at: Option<Instant>,
    last_timestamp: i64,
}

impl GraphicsCaptureApiHandler for Capture {
    type Flags = Flags;
    type Error = String;

    fn new(ctx: Context<Flags>) -> Result<Self, String> {
        // The encoder consumes textures on other MTA threads. Serialize access
        // to the D3D11 immediate context through the device's native protection.
        unsafe {
            let _ = ctx
                .device_context
                .cast::<ID3D11Multithread>()
                .map_err(|e| e.to_string())?
                .SetMultithreadProtected(true);
        }
        let f = ctx.flags;
        let encoder = Encoder::new(f.output.clone(), f.region.width, f.region.height, f.fps);
        Ok(Self {
            flags: f,
            encoder: Some(encoder),
            first_timestamp: None,
            first_at: None,
            last_timestamp: -1,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _: InternalCaptureControl,
    ) -> Result<(), String> {
        let r = self.flags.region;
        if r.x + r.width > frame.width() || r.y + r.height > frame.height() {
            return Err("録画中にモニターの解像度が変わりました。録画を停止しました。".into());
        }
        let timestamp = frame.timestamp().map_err(|e| e.to_string())?.Duration;
        let first = *self.first_timestamp.get_or_insert_with(|| {
            self.first_at = Some(Instant::now());
            timestamp
        });
        let ticks = timestamp - first;
        if self.last_timestamp >= 0
            && ticks - self.last_timestamp < 10_000_000 / i64::from(self.flags.fps)
        {
            return Ok(());
        }
        // Pace attempted frames as well as accepted frames. A full queue must
        // not make us allocate/crop at the monitor's unrestricted refresh rate.
        self.last_timestamp = ticks;
        let surface = (|| -> windows::core::Result<_> {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: r.width,
                Height: r.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: frame.desc().Format,
                SampleDesc: frame.desc().SampleDesc,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                ..Default::default()
            };
            let mut texture: Option<ID3D11Texture2D> = None;
            unsafe {
                frame
                    .device()
                    .CreateTexture2D(&desc, None, Some(&mut texture))?
            };
            let texture = texture.ok_or_else(windows::core::Error::empty)?;
            let bounds = D3D11_BOX {
                left: r.x,
                top: r.y,
                right: r.x + r.width,
                bottom: r.y + r.height,
                front: 0,
                back: 1,
            };
            unsafe {
                frame.device_context().CopySubresourceRegion(
                    &texture,
                    0,
                    0,
                    0,
                    0,
                    frame.as_raw_texture(),
                    0,
                    Some(&bounds),
                )
            };
            unsafe { frame.device_context().Flush() };
            // GPU crop only: no staging texture, Map, or CPU pixel buffer.
            unsafe { CreateDirect3D11SurfaceFromDXGISurface(&texture.cast::<IDXGISurface>()?)? }
                .cast()
        })()
        .map_err(|e| e.to_string())?;
        if self
            .encoder
            .as_mut()
            .expect("capture encoder")
            .send(surface, ticks)?
        {
            self.flags.frames.fetch_add(1, Ordering::Relaxed);
        } else {
            self.flags.dropped.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), String> {
        Err("録画対象のモニターが切断されました。".into())
    }
}

impl Capture {
    fn finish(&mut self, stopped_at: Instant) -> Result<(), String> {
        let ticks = self.first_at.map_or(0, |t| {
            (stopped_at.saturating_duration_since(t).as_nanos() / 100) as i64
        });
        let result = self.encoder.take().expect("capture encoder").finish(ticks);
        if self.flags.frames.load(Ordering::Relaxed) == 0 {
            result?;
            return Err("フレームを取得できませんでした。短すぎる録画やキャプチャー非対応の画面を確認してください。".into());
        }
        result
    }
}

pub fn start(
    region: Region,
    monitor_handle: usize,
    fps: u32,
    cursor: bool,
) -> Result<Recording, String> {
    if region.width < win_cap::MIN_REGION_SIZE
        || region.height < win_cap::MIN_REGION_SIZE
        || !region.width.is_multiple_of(2)
        || !region.height.is_multiple_of(2)
    {
        return Err("録画範囲は48 × 48px以上で、幅と高さが偶数である必要があります。".into());
    }
    if !matches!(fps, 30 | 60) {
        return Err("対応するフレームレートは30または60fpsです。".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("録画ファイルの時刻を取得できません: {e}"))?
        .as_nanos();
    let temporary =
        std::env::temp_dir().join(format!("win-cap-{}-{stamp}.mp4", std::process::id()));
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|e| format!("一時録画ファイルを作成できません: {e}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicU64::new(0));
    let worker_stop = stop.clone();
    let flags = Flags {
        region,
        output: temporary.clone(),
        fps,
        frames: frames.clone(),
        dropped: dropped.clone(),
    };
    let worker = thread::spawn(move || {
        let result = (|| {
            let monitor = Monitor::from_raw_hmonitor(monitor_handle as *mut std::ffi::c_void);
            let settings = Settings::new(
                monitor,
                if cursor {
                    CursorCaptureSettings::WithCursor
                } else {
                    CursorCaptureSettings::WithoutCursor
                },
                DrawBorderSettings::Default,
                SecondaryWindowSettings::Default,
                // Custom WGC intervals require newer Windows; application throttling
                // above is used on all supported systems, including Windows 10.
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                flags,
            );
            let control = Capture::start_free_threaded(settings).map_err(|e| e.to_string())?;
            let callback = control.callback();
            while !worker_stop.load(Ordering::Relaxed) && !control.is_finished() {
                if callback
                    .lock()
                    .encoder
                    .as_ref()
                    .is_some_and(Encoder::is_finished)
                {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let stopped_at = Instant::now();
            let capture_result = control.stop().map_err(|e| e.to_string());
            let encode_result = callback.lock().finish(stopped_at);
            capture_result?;
            encode_result?;
            Ok(temporary.clone())
        })();
        result.map_err(|e: String| format!("{e}\n一時ファイル: {}", temporary.display()))
    });
    Ok(Recording {
        stop,
        frames,
        dropped,
        worker,
    })
}
