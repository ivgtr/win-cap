#![cfg(windows)]

#[path = "../src/encoder.rs"]
mod encoder;

use std::{
    fs::OpenOptions,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use windows::{
    Graphics::DirectX::Direct3D11::IDirect3DSurface,
    Storage::StorageFile,
    Win32::{
        Graphics::{
            Direct3D11::{
                D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, ID3D11Multithread, ID3D11Texture2D,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                IDXGISurface,
            },
        },
        System::WinRT::{
            Direct3D11::CreateDirect3D11SurfaceFromDXGISurface, RO_INIT_MULTITHREADED,
            RoInitialize, RoUninitialize,
        },
    },
    core::{HSTRING, Interface},
};

// Needs an interactive Windows machine with a D3D11 GPU. No screen content is
// captured: only a solid synthetic texture is encoded into a temporary MP4.
#[test]
#[ignore = "requires a Windows GPU; run with --ignored --nocapture"]
fn mp4_encoding_keeps_small_and_normal_dimensions_and_static_duration() {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.unwrap();
    let (device, context) = windows_capture::d3d11::create_d3d_device().unwrap();
    unsafe {
        let _ = context
            .cast::<ID3D11Multithread>()
            .unwrap()
            .SetMultithreadProtected(true);
    }
    let mut failures = Vec::new();
    for (width, height) in [(48, 48), (64, 64), (104, 84), (256, 256), (640, 480)] {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("win-cap-test-{width}-{height}-{stamp}.mp4"));
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            ..Default::default()
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .unwrap();
        }
        let texture = texture.unwrap();
        let mut view = None;
        unsafe {
            device
                .CreateRenderTargetView(&texture, None, Some(&mut view))
                .unwrap();
            context.ClearRenderTargetView(&view.unwrap(), &[0.1, 0.3, 0.8, 1.0]);
            context.Flush();
        }
        let surface: IDirect3DSurface = unsafe {
            CreateDirect3D11SurfaceFromDXGISurface(&texture.cast::<IDXGISurface>().unwrap())
                .unwrap()
                .cast()
                .unwrap()
        };
        let mut encoder = encoder::Encoder::new(path.clone(), width, height, 30);
        assert!(encoder.send(surface, 0).unwrap());
        std::thread::sleep(Duration::from_millis(100));
        eprintln!(
            "{width}x{height}: encoder finished before stop: {}",
            encoder.is_finished()
        );
        let result = encoder.finish(10_000_000);
        if let Err(error) = result {
            failures.push(format!("{width}x{height}: {error}"));
        } else {
            let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
                .unwrap()
                .join()
                .unwrap();
            let properties = file
                .Properties()
                .unwrap()
                .GetVideoPropertiesAsync()
                .unwrap()
                .join()
                .unwrap();
            let output_width = properties.Width().unwrap();
            let output_height = properties.Height().unwrap();
            let ticks = properties.Duration().unwrap().Duration;
            eprintln!(
                "{width}x{height}: output {output_width}x{output_height}, duration {ticks} ticks"
            );
            if output_width != width || output_height != height || ticks < 9_000_000 {
                failures.push(format!(
                    "{width}x{height}: invalid output geometry/duration"
                ));
            }
        }
        std::fs::remove_file(&path).unwrap();
    }
    unsafe {
        RoUninitialize();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
