#![allow(unsafe_op_in_unsafe_fn)]

use crate::recorder::{self, Recording};
use std::{cell::RefCell, mem::size_of, path::PathBuf, sync::atomic::Ordering, time::Instant};
use win_cap::{Rect, Region};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Controls::Dialogs::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

const SELECT: u16 = 100;
const RECORD: u16 = 101;
const FPS: u16 = 102;
const CURSOR: u16 = 103;
const MINIMIZE: u16 = 104;
const HOTKEY_RECORD: i32 = 1;
const HOTKEY_STOP: i32 = 2;

struct App {
    window: HWND,
    select: HWND,
    record: HWND,
    fps: HWND,
    cursor: HWND,
    minimize: HWND,
    status: HWND,
    region_label: HWND,
    region: Option<(Region, usize)>,
    recording: Option<Recording>,
    pending: Option<PathBuf>,
    saving: Option<Saving>,
    started: Instant,
    stopping: bool,
    closing: bool,
}

struct Saving {
    destination: PathBuf,
    worker: std::thread::JoinHandle<Result<Option<String>, String>>,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn show_error(window: Option<HWND>, message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            window,
            PCWSTR(message.as_ptr()),
            w!("win-cap"),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn run() -> Result<(), String> {
    unsafe {
        // Set awareness before creating any windows: all selection coordinates
        // and capture dimensions refer to physical pixels, even at mixed DPI.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .map_err(|e| e.to_string())?;
        let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("WinCapMain"),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread().to_string());
        }
        let overlay_class = WNDCLASSW {
            lpfnWndProc: Some(selection_proc),
            hInstance: instance,
            lpszClassName: w!("WinCapSelection"),
            hCursor: LoadCursorW(None, IDC_CROSS).map_err(|e| e.to_string())?,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        if RegisterClassW(&overlay_class) == 0 {
            return Err(windows::core::Error::from_thread().to_string());
        }
        let scale = GetDpiForSystem() as f32 / 96.0;
        let font = CreateFontW(
            -(16.0 * scale) as i32,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Yu Gothic UI"),
        );
        if font.0.is_null() {
            return Err("UIフォントを作成できません。".into());
        }
        let app = Box::new(RefCell::new(App {
            window: HWND::default(),
            select: HWND::default(),
            record: HWND::default(),
            fps: HWND::default(),
            cursor: HWND::default(),
            minimize: HWND::default(),
            status: HWND::default(),
            region_label: HWND::default(),
            region: None,
            recording: None,
            pending: None,
            saving: None,
            started: Instant::now(),
            stopping: false,
            closing: false,
        }));
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (460.0 * scale) as i32,
            bottom: (284.0 * scale) as i32,
        };
        AdjustWindowRectEx(&mut rect, style, false, WINDOW_EX_STYLE::default())
            .map_err(|e| e.to_string())?;
        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("WinCapMain"),
            w!("win-cap — 範囲録画"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance),
            Some((&*app as *const RefCell<App>).cast()),
        )
        .map_err(|e| e.to_string())?;
        // Windows 10 2004+ can omit the controller even if the user restores it
        // over the selected region during recording.
        if let Err(error) = SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) {
            let _ = DestroyWindow(window);
            let _ = DeleteObject(font.into());
            return Err(format!("操作画面を録画から除外できません: {error}"));
        }
        let mut state = app.borrow_mut();
        state.window = window;
        set_text(
            window,
            &format!("win-cap v{} — 範囲録画", env!("CARGO_PKG_VERSION")),
        );
        let setup = (|| -> windows::core::Result<()> {
            state.region_label = child(
                window,
                "STATIC",
                "範囲未選択",
                20,
                20,
                420,
                28,
                0,
                0,
                scale,
                font,
            )?;
            state.select = child(
                window,
                "BUTTON",
                "範囲を選択",
                20,
                60,
                200,
                38,
                SELECT,
                WS_TABSTOP.0,
                scale,
                font,
            )?;
            state.record = child(
                window,
                "BUTTON",
                "録画開始",
                240,
                60,
                200,
                38,
                RECORD,
                WS_TABSTOP.0,
                scale,
                font,
            )?;
            child(
                window,
                "STATIC",
                "フレームレート",
                20,
                119,
                124,
                24,
                0,
                0,
                scale,
                font,
            )?;
            state.fps = child(
                window,
                "COMBOBOX",
                "",
                150,
                114,
                90,
                140,
                FPS,
                CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0 | WS_VSCROLL.0,
                scale,
                font,
            )?;
            for text in ["30 fps", "60 fps"] {
                let text = wide(text);
                SendMessageW(
                    state.fps,
                    CB_ADDSTRING,
                    Some(WPARAM(0)),
                    Some(LPARAM(text.as_ptr() as isize)),
                );
            }
            SendMessageW(state.fps, CB_SETCURSEL, Some(WPARAM(0)), Some(LPARAM(0)));
            state.cursor = child(
                window,
                "BUTTON",
                "カーソルを含める",
                265,
                115,
                175,
                28,
                CURSOR,
                BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
                scale,
                font,
            )?;
            SendMessageW(state.cursor, BM_SETCHECK, Some(WPARAM(1)), Some(LPARAM(0)));
            state.minimize = child(
                window,
                "BUTTON",
                "録画中は最小化する",
                20,
                153,
                420,
                28,
                MINIMIZE,
                BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0,
                scale,
                font,
            )?;
            SendMessageW(
                state.minimize,
                BM_SETCHECK,
                Some(WPARAM(1)),
                Some(LPARAM(0)),
            );
            state.status = child(
                window,
                "STATIC",
                "範囲を選び、録画を開始してください。",
                20,
                200,
                420,
                26,
                0,
                0,
                scale,
                font,
            )?;
            child(
                window,
                "STATIC",
                "開始/停止: Ctrl + Shift + F9　停止: Ctrl + Shift + F10",
                20,
                243,
                420,
                25,
                0,
                0,
                scale,
                font,
            )?;
            RegisterHotKey(
                Some(window),
                HOTKEY_RECORD,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                VK_F9.0 as u32,
            )?;
            RegisterHotKey(
                Some(window),
                HOTKEY_STOP,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                VK_F10.0 as u32,
            )?;
            Ok(())
        })();
        if let Err(e) = setup {
            let _ = DestroyWindow(window);
            let _ = DeleteObject(font.into());
            return Err(format!("画面またはショートカットを初期化できません: {e}"));
        }
        state.controls();
        drop(state);
        let _ = ShowWindow(window, SW_SHOW);
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == -1 {
                return Err(windows::core::Error::from_thread().to_string());
            }
            if result == 0 {
                break;
            }
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = DeleteObject(font.into());
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn child(
    parent: HWND,
    class: &str,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: u16,
    extra: u32,
    scale: f32,
    font: HFONT,
) -> windows::core::Result<HWND> {
    let class = wide(class);
    let text = wide(text);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PCWSTR(class.as_ptr()),
        PCWSTR(text.as_ptr()),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | extra),
        (x as f32 * scale) as i32,
        (y as f32 * scale) as i32,
        (width as f32 * scale) as i32,
        (height as f32 * scale) as i32,
        Some(parent),
        Some(HMENU(id as usize as *mut _)),
        None,
        None,
    )?;
    SendMessageW(
        window,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
    Ok(window)
}

unsafe fn set_text(window: HWND, text: &str) {
    let text = wide(text);
    let _ = SetWindowTextW(window, PCWSTR(text.as_ptr()));
}

impl App {
    unsafe fn controls(&self) {
        let busy = self.recording.is_some() || self.saving.is_some();
        let pending = self.pending.is_some();
        let _ = EnableWindow(self.select, !busy && !pending);
        let _ = EnableWindow(self.fps, !busy && !pending);
        let _ = EnableWindow(self.cursor, !busy && !pending);
        let _ = EnableWindow(self.minimize, !busy && !pending);
        let _ = EnableWindow(
            self.record,
            !self.stopping && (busy || pending || self.region.is_some()),
        );
        set_text(
            self.record,
            if busy {
                "録画停止"
            } else if pending {
                "録画を保存"
            } else {
                "録画開始"
            },
        );
    }

    unsafe fn select_region(&mut self) -> Result<(), String> {
        if self.recording.is_some() || self.pending.is_some() {
            return Ok(());
        }
        let _ = ShowWindow(self.window, SW_HIDE);
        let result = select_region(self.window);
        let _ = ShowWindow(self.window, SW_SHOW);
        let _ = SetForegroundWindow(self.window);
        if let Some((region, monitor)) = result? {
            self.region = Some((region, monitor));
            set_text(
                self.region_label,
                &format!(
                    "{} × {} px　位置: {}, {}",
                    region.width, region.height, region.screen.left, region.screen.top
                ),
            );
            set_text(
                self.status,
                if crate::encoder::hardware_acceleration(region.width, region.height) {
                    "MP4・音声なし（GPU圧縮を優先）"
                } else {
                    "MP4・音声なし（小範囲はCPU圧縮）"
                },
            );
        }
        self.controls();
        Ok(())
    }

    unsafe fn toggle(&mut self) -> Result<(), String> {
        if self.saving.is_some() {
            return Ok(());
        }
        if self.recording.is_some() {
            self.stop();
            return Ok(());
        }
        if self.pending.is_some() {
            return self.save_pending();
        }
        let Some((region, monitor)) = self.region else {
            return self.select_region();
        };
        let fps = match SendMessageW(self.fps, CB_GETCURSEL, None, None).0 {
            0 => 30,
            1 => 60,
            _ => return Err("フレームレートを選択してください。".into()),
        };
        let cursor = SendMessageW(self.cursor, BM_GETCHECK, None, None).0 == 1;
        let recording = recorder::start(region, monitor, fps, cursor)?;
        self.recording = Some(recording);
        self.started = Instant::now();
        self.stopping = false;
        // Timer runs both while recording and while MP4 is being finalized.
        if SetTimer(Some(self.window), 1, 250, None) == 0 {
            self.stop();
            let recording = self.recording.take().expect("recording");
            let _ = recording.worker.join();
            self.stopping = false;
            return Err("録画の状態更新タイマーを開始できません。".into());
        }
        self.controls();
        set_text(self.status, "録画を準備しています…");
        if SendMessageW(self.minimize, BM_GETCHECK, None, None).0 == 1 {
            let _ = ShowWindow(self.window, SW_MINIMIZE);
        }
        Ok(())
    }

    unsafe fn stop(&mut self) {
        if let Some(recording) = &self.recording {
            recording.stop.store(true, Ordering::Relaxed);
            self.stopping = true;
            set_text(self.status, "録画を停止しています…");
            self.controls();
        }
    }

    unsafe fn tick(&mut self) {
        if self.saving.is_some() {
            self.tick_save();
            return;
        }
        let Some(recording) = &self.recording else {
            return;
        };
        if recording.worker.is_finished() {
            let recording = self.recording.take().expect("finished recording");
            let dropped = recording.dropped.load(Ordering::Relaxed);
            let result = recording
                .worker
                .join()
                .unwrap_or_else(|_| Err("録画処理が異常終了しました。".into()));
            let _ = KillTimer(Some(self.window), 1);
            self.stopping = false;
            self.controls();
            let _ = ShowWindow(self.window, SW_RESTORE);
            let _ = SetForegroundWindow(self.window);
            match result {
                Ok(path) => {
                    self.pending = Some(path);
                    self.controls();
                    set_text(
                        self.status,
                        if dropped > 0 {
                            "録画完了（一部フレームを省略）"
                        } else {
                            "録画完了・保存先を選択してください。"
                        },
                    );
                    if let Err(e) = self.save_pending() {
                        show_error(Some(self.window), &e);
                    }
                }
                Err(e) => {
                    set_text(self.status, "録画に失敗しました。");
                    show_error(Some(self.window), &e);
                }
            }
            if self.closing && self.saving.is_none() {
                self.close_pending();
            }
        } else if !self.stopping {
            let frames = recording.frames.load(Ordering::Relaxed);
            if frames == 0 {
                set_text(self.status, "最初のフレームを待っています…");
            } else {
                let elapsed = self.started.elapsed().as_secs();
                set_text(
                    self.status,
                    &format!(
                        "録画中 {:02}:{:02}　{} フレーム",
                        elapsed / 60,
                        elapsed % 60,
                        frames
                    ),
                );
            }
        }
    }

    unsafe fn save_pending(&mut self) -> Result<(), String> {
        let Some(source) = self.pending.clone() else {
            return Ok(());
        };
        let Some(destination) = save_path(self.window)? else {
            set_text(
                self.status,
                "録画は残っています。「録画を保存」で再試行できます。",
            );
            self.controls();
            return Ok(());
        };
        set_text(self.status, "MP4を保存しています…");
        let worker_destination = destination.clone();
        self.saving = Some(Saving {
            destination,
            worker: std::thread::spawn(move || {
                win_cap::save::recording(&source, &worker_destination)
            }),
        });
        self.stopping = true;
        if SetTimer(Some(self.window), 1, 100, None) == 0 {
            // A timer failure must not abandon an in-flight copy or discard its
            // source. Join only this exceptional path and report its result.
            let saving = self.saving.take().expect("save task");
            self.complete_save(saving);
        }
        self.controls();
        Ok(())
    }

    unsafe fn tick_save(&mut self) {
        if !self.saving.as_ref().is_some_and(|s| s.worker.is_finished()) {
            return;
        }
        let saving = self.saving.take().expect("finished save");
        let _ = KillTimer(Some(self.window), 1);
        self.complete_save(saving);
        if self.closing {
            self.close_pending();
        }
    }

    unsafe fn complete_save(&mut self, saving: Saving) {
        self.stopping = false;
        let warning = match saving
            .worker
            .join()
            .unwrap_or_else(|_| Err("保存処理が異常終了しました。".into()))
        {
            Ok(warning) => warning,
            Err(error) => {
                self.controls();
                set_text(
                    self.status,
                    "保存できませんでした。「録画を保存」で再試行できます。",
                );
                let path = self
                    .pending
                    .as_ref()
                    .map_or_else(String::new, |p| p.display().to_string());
                show_error(
                    Some(self.window),
                    &format!("{error}\n録画は残っています: {path}"),
                );
                return;
            }
        };
        self.pending = None;
        self.controls();
        set_text(self.status, "保存完了");
        let text = wide(&format!(
            "保存しました。\n{}{}",
            saving.destination.display(),
            warning.map_or_else(String::new, |w| format!("\n{w}"))
        ));
        MessageBoxW(
            Some(self.window),
            PCWSTR(text.as_ptr()),
            w!("win-cap"),
            MB_OK | MB_ICONINFORMATION,
        );
    }

    unsafe fn close_pending(&self) {
        if let Some(path) = &self.pending {
            let text = wide(&format!(
                "未保存の録画を残して終了します。\n{}",
                path.display()
            ));
            MessageBoxW(
                Some(self.window),
                PCWSTR(text.as_ptr()),
                w!("win-cap"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        let _ = DestroyWindow(self.window);
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const RefCell<App>;
    // Win32 and modal dialogs can synchronously re-enter this procedure.
    // RefCell suppresses nested actions while one UI action owns the state.
    match message {
        WM_COMMAND if !pointer.is_null() => {
            let Ok(mut app) = (&*pointer).try_borrow_mut() else {
                return LRESULT(0);
            };
            let result = match (wparam.0 & 0xffff) as u16 {
                SELECT => app.select_region(),
                RECORD => app.toggle(),
                _ => Ok(()),
            };
            if let Err(e) = result {
                show_error(Some(window), &e);
            }
            LRESULT(0)
        }
        WM_HOTKEY if !pointer.is_null() => {
            let Ok(mut app) = (&*pointer).try_borrow_mut() else {
                return LRESULT(0);
            };
            if wparam.0 as i32 == HOTKEY_STOP {
                app.stop();
            } else if let Err(e) = app.toggle() {
                show_error(Some(window), &e);
            }
            LRESULT(0)
        }
        WM_TIMER if !pointer.is_null() => {
            if let Ok(mut app) = (&*pointer).try_borrow_mut() {
                app.tick();
            }
            LRESULT(0)
        }
        WM_CLOSE if !pointer.is_null() => {
            let Ok(mut app) = (&*pointer).try_borrow_mut() else {
                return LRESULT(0);
            };
            if app.recording.is_some() || app.saving.is_some() {
                app.closing = true;
                app.stop();
            } else {
                app.close_pending();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = UnregisterHotKey(Some(window), HOTKEY_RECORD);
            let _ = UnregisterHotKey(Some(window), HOTKEY_STOP);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

struct Selection {
    origin: POINT,
    start: Option<POINT>,
    end: POINT,
    monitor: usize,
    bounds: Rect,
    finished: bool,
    result: Option<Result<(Region, usize), String>>,
}

unsafe fn select_region(owner: HWND) -> Result<Option<(Region, usize)>, String> {
    let origin = POINT {
        x: GetSystemMetrics(SM_XVIRTUALSCREEN),
        y: GetSystemMetrics(SM_YVIRTUALSCREEN),
    };
    let mut selection = Box::new(Selection {
        origin,
        start: None,
        end: origin,
        monitor: 0,
        bounds: Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        finished: false,
        result: None,
    });
    let window = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
        w!("WinCapSelection"),
        w!("ドラッグで録画範囲を選択 / Escでキャンセル"),
        WS_POPUP,
        origin.x,
        origin.y,
        GetSystemMetrics(SM_CXVIRTUALSCREEN),
        GetSystemMetrics(SM_CYVIRTUALSCREEN),
        Some(owner),
        None,
        None,
        Some((&mut *selection as *mut Selection).cast()),
    )
    .map_err(|e| e.to_string())?;
    if let Err(e) = SetLayeredWindowAttributes(
        window,
        windows::Win32::Foundation::COLORREF(0),
        100,
        LWA_ALPHA,
    ) {
        let _ = DestroyWindow(window);
        return Err(e.to_string());
    }
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetForegroundWindow(window);
    let mut message = MSG::default();
    loop {
        if selection.finished {
            break;
        }
        let result = GetMessageW(&mut message, None, 0, 0).0;
        if result <= 0 {
            let _ = DestroyWindow(window);
            if result == 0 {
                PostQuitMessage(message.wParam.0 as i32);
                return Ok(None);
            }
            return Err(windows::core::Error::from_thread().to_string());
        }
        // Do not dispatch owner hotkeys while the modal selection is borrowing App.
        if message.hwnd == owner && message.message == WM_HOTKEY {
            continue;
        }
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    let _ = DestroyWindow(window);
    selection.result.transpose()
}

unsafe extern "system" fn selection_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Selection;
    if pointer.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    match message {
        WM_LBUTTONDOWN => {
            let s = &mut *pointer;
            let mut point = POINT::default();
            if let Err(e) = GetCursorPos(&mut point) {
                s.result = Some(Err(e.to_string()));
                s.finished = true;
                return LRESULT(0);
            }
            let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                s.result = Some(Err(windows::core::Error::from_thread().to_string()));
                s.finished = true;
                return LRESULT(0);
            }
            s.start = Some(point);
            s.end = point;
            s.monitor = monitor.0 as usize;
            s.bounds = Rect {
                left: info.rcMonitor.left,
                top: info.rcMonitor.top,
                right: info.rcMonitor.right,
                bottom: info.rcMonitor.bottom,
            };
            SetCapture(window);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let s = &mut *pointer;
            if s.start.is_some() {
                let mut point = POINT::default();
                if GetCursorPos(&mut point).is_ok() {
                    point.x = point.x.clamp(s.bounds.left, s.bounds.right);
                    point.y = point.y.clamp(s.bounds.top, s.bounds.bottom);
                    s.end = point;
                    let _ = InvalidateRect(Some(window), None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let s = &mut *pointer;
            if let Some(start) = s.start {
                let mut end = s.end;
                if let Err(e) = GetCursorPos(&mut end) {
                    s.result = Some(Err(e.to_string()));
                } else {
                    s.result = Some(
                        Region::select((start.x, start.y), (end.x, end.y), s.bounds)
                            .map(|r| (r, s.monitor)),
                    );
                }
                s.finished = true;
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            (*pointer).finished = true;
            LRESULT(0)
        }
        WM_CLOSE => {
            (*pointer).finished = true;
            LRESULT(0)
        }
        WM_PAINT => {
            let s = &*pointer;
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(window, &mut paint);
            let mut client = RECT::default();
            let _ = GetClientRect(window, &mut client);
            FillRect(dc, &client, HBRUSH(GetStockObject(BLACK_BRUSH).0));
            SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00ffffff));
            SetBkMode(dc, TRANSPARENT);
            let mut caption: Vec<u16> = "ドラッグで範囲選択（1つのモニター内） / Escでキャンセル"
                .encode_utf16()
                .collect();
            let mut label = RECT {
                left: 30,
                top: 30,
                right: client.right - 30,
                bottom: 90,
            };
            DrawTextW(dc, &mut caption, &mut label, DT_LEFT | DT_TOP);
            if let Some(start) = s.start {
                let rect = RECT {
                    left: start.x.min(s.end.x) - s.origin.x,
                    top: start.y.min(s.end.y) - s.origin.y,
                    right: start.x.max(s.end.x) - s.origin.x,
                    bottom: start.y.max(s.end.y) - s.origin.y,
                };
                let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x0000ff80));
                FrameRect(dc, &rect, brush);
                let _ = DeleteObject(brush.into());
            }
            let _ = EndPaint(window, &paint);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn save_path(owner: HWND) -> Result<Option<PathBuf>, String> {
    let mut filename = vec![0u16; 32768];
    let default = wide("capture.mp4");
    filename[..default.len()].copy_from_slice(&default);
    let filter: Vec<u16> = "MP4 video (*.mp4)\0*.mp4\0\0".encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: windows::core::PWSTR(filename.as_mut_ptr()),
        nMaxFile: filename.len() as u32,
        lpstrDefExt: w!("mp4"),
        lpstrTitle: w!("録画を保存（新しいファイル名）"),
        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !GetSaveFileNameW(&mut dialog).as_bool() {
        let error = CommDlgExtendedError();
        return if error == COMMON_DLG_ERRORS(0) {
            Ok(None)
        } else {
            Err(format!("保存先を選択できません: {}", error.0))
        };
    }
    use std::os::windows::ffi::OsStringExt;
    let length = filename
        .iter()
        .position(|c| *c == 0)
        .expect("terminated file path");
    let path = PathBuf::from(std::ffi::OsString::from_wide(&filename[..length]));
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
    {
        return Err("保存形式はMP4です。.mp4のファイル名を指定してください。".into());
    }
    Ok(Some(path))
}
