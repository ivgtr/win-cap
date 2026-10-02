use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::DirectX::Direct3D11::IDirect3DSurface,
    Media::{
        Core::{
            MediaStreamSample, MediaStreamSource, MediaStreamSourceSampleRequestedEventArgs,
            MediaStreamSourceStartingEventArgs, VideoStreamDescriptor,
        },
        MediaProperties::{
            MediaEncodingProfile, MediaEncodingSubtypes, VideoEncodingProperties,
            VideoEncodingQuality,
        },
        Transcoding::MediaTranscoder,
    },
    Storage::{FileAccessMode, StorageFile},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{AgileReference, HSTRING},
};

struct Sample {
    surface: AgileReference<IDirect3DSurface>,
    ticks: i64,
}

pub struct Encoder {
    sender: Option<SyncSender<Sample>>,
    last_surface: Option<AgileReference<IDirect3DSurface>>,
    last_ticks: i64,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl Encoder {
    pub fn new(path: PathBuf, width: u32, height: u32, fps: u32) -> Self {
        // At most two queued GPU surfaces. A slow encoder drops new frames rather
        // than accumulating a recording's worth of textures in RAM/VRAM.
        let (sender, receiver) = mpsc::sync_channel::<Sample>(2);
        let worker = thread::spawn(move || {
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
            let result = encode(path, width, height, fps, receiver).map_err(|e| e.to_string());
            unsafe { RoUninitialize() };
            result
        });
        Self {
            sender: Some(sender),
            last_surface: None,
            last_ticks: -1,
            worker: Some(worker),
        }
    }

    pub fn send(&mut self, surface: IDirect3DSurface, ticks: i64) -> Result<bool, String> {
        let surface = AgileReference::new(&surface).map_err(|e| e.to_string())?;
        let sample = Sample {
            surface: surface.clone(),
            ticks,
        };
        match self.sender.as_ref().expect("live encoder").try_send(sample) {
            Ok(()) => {
                self.last_surface = Some(surface);
                self.last_ticks = ticks;
                Ok(true)
            }
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err("動画エンコーダーが停止しました。".into()),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_some_and(JoinHandle::is_finished)
    }

    pub fn finish(mut self, ticks: i64) -> Result<(), String> {
        // Extend an unchanged desktop to the stop time. WGC is event driven;
        // without this sample a static recording could have near-zero duration.
        if let Some(surface) = self.last_surface.take() {
            let _ = self.sender.as_ref().expect("live encoder").send(Sample {
                surface,
                ticks: ticks.max(self.last_ticks + 1),
            });
        }
        self.join()
    }

    fn join(&mut self) -> Result<(), String> {
        // Disconnect means end-of-stream, including when there were no frames.
        self.sender.take();
        self.worker
            .take()
            .expect("encoder worker")
            .join()
            .map_err(|_| "動画エンコーダーのスレッドが異常終了しました。".to_string())?
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.join();
        }
    }
}

fn encode(
    path: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    receiver: mpsc::Receiver<Sample>,
) -> windows::core::Result<()> {
    let properties = VideoEncodingProperties::CreateUncompressed(
        &MediaEncodingSubtypes::Bgra8()?,
        width,
        height,
    )?;
    properties.FrameRate()?.SetNumerator(fps)?;
    properties.FrameRate()?.SetDenominator(1)?;
    let descriptor = VideoStreamDescriptor::Create(&properties)?;
    let source = MediaStreamSource::CreateFromDescriptor(&descriptor)?;
    source.SetBufferTime(TimeSpan { Duration: 0 })?;
    let pending = Arc::new(Mutex::new(None::<Sample>));
    let receiver = Arc::new(Mutex::new(receiver));
    let start_receiver = receiver.clone();
    let start_pending = pending.clone();
    let starting = source.Starting(&TypedEventHandler::<
        MediaStreamSource,
        MediaStreamSourceStartingEventArgs,
    >::new(move |_, args| {
        let args = args.as_ref().ok_or_else(windows::core::Error::empty)?;
        let first = start_receiver.lock().expect("sample receiver").recv().ok();
        let position = first.as_ref().map_or(0, |s| s.ticks);
        *start_pending.lock().expect("pending sample") = first;
        args.Request()?
            .SetActualStartPosition(TimeSpan { Duration: position })
    }))?;
    let requested = source.SampleRequested(&TypedEventHandler::<
        MediaStreamSource,
        MediaStreamSourceSampleRequestedEventArgs,
    >::new(move |_, args| {
        let args = args.as_ref().ok_or_else(windows::core::Error::empty)?;
        let sample = pending
            .lock()
            .expect("pending sample")
            .take()
            .or_else(|| receiver.lock().expect("sample receiver").recv().ok());
        match sample {
            Some(s) => args
                .Request()?
                .SetSample(&MediaStreamSample::CreateFromDirect3D11Surface(
                    &s.surface.resolve()?,
                    TimeSpan { Duration: s.ticks },
                )?),
            None => args.Request()?.SetSample(None),
        }
    }))?;
    let result = (|| {
        let profile = MediaEncodingProfile::CreateMp4(VideoEncodingQuality::HD1080p)?;
        profile.SetAudio(None)?;
        let video = profile.Video()?;
        video.SetSubtype(&HSTRING::from("H264"))?;
        video.SetWidth(width)?;
        video.SetHeight(height)?;
        // ~0.12 bits/pixel/frame, bounded to sensible screen recording rates.
        let bitrate = (u64::from(width) * u64::from(height) * u64::from(fps) * 12 / 100)
            .clamp(1_000_000, 30_000_000) as u32;
        video.SetBitrate(bitrate)?;
        video.FrameRate()?.SetNumerator(fps)?;
        video.FrameRate()?.SetDenominator(1)?;
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.join()?;
        let stream = file.OpenAsync(FileAccessMode::ReadWrite)?.join()?;
        let transcoder = MediaTranscoder::new()?;
        // Hardware encoders have vendor-specific minimum dimensions. Small
        // clips use the Windows software path from the start, retaining the
        // selected dimensions rather than padding or retrying another format.
        transcoder.SetHardwareAccelerationEnabled(hardware_acceleration(width, height))?;
        let prepared = transcoder
            .PrepareMediaStreamSourceTranscodeAsync(&source, &stream, &profile)?
            .join()?;
        if !prepared.CanTranscode()? {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004005u32 as i32),
                format!("H.264/MP4を準備できません: {:?}", prepared.FailureReason()?),
            ));
        }
        prepared.TranscodeAsync()?.join()?;
        stream.FlushAsync()?.join()?;
        stream.Close()
    })();
    let _ = source.RemoveStarting(starting);
    let _ = source.RemoveSampleRequested(requested);
    result
}

pub fn hardware_acceleration(width: u32, height: u32) -> bool {
    width >= 256 && height >= 256
}
