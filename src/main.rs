//! Claude plan usage drawn directly on the Windows 11 taskbar.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod model;

#[cfg(windows)]
mod api;
#[cfg(windows)]
mod render;
#[cfg(windows)]
mod state;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("claude-quota-taskbar only runs on Windows");
}

#[cfg(windows)]
mod app {
    use crate::api::{self, wide, FetchError};
    use crate::{render, state};
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering::Relaxed};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::HiDpi::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WM_UPDATED: u32 = WM_APP + 1;
    const TIMER_REPAINT: usize = 1;
    const TIMER_TOPMOST: usize = 2;
    const CMD_REFRESH: usize = 1;
    const CMD_QUIT: usize = 2;
    /// Poll gently: the endpoint is undocumented and rate-limits aggressively.
    const POLL_OK: u64 = 180;
    const POLL_RATE_LIMITED: u64 = 300;
    const POLL_RETRY: u64 = 60;

    static REFRESH: AtomicBool = AtomicBool::new(false);
    static DRAGGING: AtomicBool = AtomicBool::new(false);
    static DRAG_DX: AtomicI32 = AtomicI32::new(0);
    static TASKBAR_LEFT: AtomicI32 = AtomicI32::new(0);
    static SCALE_BITS: AtomicU32 = AtomicU32::new(0);
    static HEIGHT: AtomicI32 = AtomicI32::new(48);
    static HWND_RAW: AtomicUsize = AtomicUsize::new(0);

    fn scale() -> f32 {
        f32::from_bits(SCALE_BITS.load(Relaxed))
    }

    fn now_secs() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }

    fn offset_file() -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("claude-quota-taskbar").join("offset"))
    }

    fn load_offset() -> Option<i32> {
        std::fs::read_to_string(offset_file()?).ok()?.trim().parse().ok()
    }

    fn save_offset(v: i32) {
        if let Some(p) = offset_file() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, v.to_string());
        }
    }

    pub fn run() {
        unsafe {
            let mutex = CreateMutexW(null(), 0, wide("Local\\claude-quota-taskbar").as_ptr());
            if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
                return;
            }
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            SCALE_BITS.store((GetDpiForSystem() as f32 / 96.0).to_bits(), Relaxed);

            // Sit inside the primary taskbar.
            let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
            let mut tb = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if tray.is_null() || GetWindowRect(tray, &mut tb) == 0 {
                let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
                tb = RECT { left: 0, top: sh - (48.0 * scale()) as i32, right: sw, bottom: sh };
            }
            let h = tb.bottom - tb.top;
            HEIGHT.store(h, Relaxed);
            TASKBAR_LEFT.store(tb.left, Relaxed);
            let x = tb.left + load_offset().unwrap_or((190.0 * scale()) as i32);

            let hinst = GetModuleHandleW(null());
            let class = wide("ClaudeQuotaTaskbar");
            let mut wc: WNDCLASSEXW = zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinst;
            wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
            wc.lpszClassName = class.as_ptr();
            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class.as_ptr(),
                wide("Claude usage").as_ptr(),
                WS_POPUP,
                x,
                tb.top,
                render::width(scale()),
                h,
                null_mut(),
                null_mut(),
                hinst,
                null(),
            );
            if hwnd.is_null() {
                return;
            }
            HWND_RAW.store(hwnd as usize, Relaxed);
            SetLayeredWindowAttributes(hwnd, render::KEY, 0, LWA_COLORKEY);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetTimer(hwnd, TIMER_REPAINT, 30_000, None);
            SetTimer(hwnd, TIMER_TOPMOST, 5_000, None);

            std::thread::spawn(fetch_loop);

            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn fetch_loop() {
        let hwnd = HWND_RAW.load(Relaxed);
        loop {
            let started = Instant::now();
            let wait = {
                let r = api::fetch_usage();
                let mut s = state::STATE.lock().unwrap();
                match r {
                    Ok(u) => {
                        s.usage = Some(u);
                        s.error = None;
                        POLL_OK
                    }
                    Err(e) => {
                        s.error = Some(e.message());
                        if matches!(e, FetchError::RateLimited) { POLL_RATE_LIMITED } else { POLL_RETRY }
                    }
                }
            };
            unsafe { PostMessageW(hwnd as HWND, WM_UPDATED, 0, 0) };
            // Sleep, waking early for a manual refresh (at most one per 15 s).
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let age = started.elapsed();
                if age >= Duration::from_secs(wait) || (REFRESH.swap(false, Relaxed) && age >= Duration::from_secs(15)) {
                    break;
                }
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                render::paint(hdc, render::width(scale()), HEIGHT.load(Relaxed), scale(), &state::snapshot(), now_secs());
                EndPaint(hwnd, &ps);
                0
            }
            WM_ERASEBKGND => 1,
            WM_UPDATED => {
                InvalidateRect(hwnd, null(), 0);
                0
            }
            WM_TIMER => {
                if wp == TIMER_REPAINT {
                    InvalidateRect(hwnd, null(), 0);
                } else {
                    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
                0
            }
            WM_LBUTTONDOWN => {
                let (mut pt, mut r) = (zeroed::<POINT>(), zeroed::<RECT>());
                GetCursorPos(&mut pt);
                GetWindowRect(hwnd, &mut r);
                DRAG_DX.store(pt.x - r.left, Relaxed);
                DRAGGING.store(true, Relaxed);
                SetCapture(hwnd);
                0
            }
            WM_MOUSEMOVE => {
                if DRAGGING.load(Relaxed) {
                    let (mut pt, mut r) = (zeroed::<POINT>(), zeroed::<RECT>());
                    GetCursorPos(&mut pt);
                    GetWindowRect(hwnd, &mut r);
                    SetWindowPos(hwnd, null_mut(), pt.x - DRAG_DX.load(Relaxed), r.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
                0
            }
            WM_LBUTTONUP => {
                if DRAGGING.swap(false, Relaxed) {
                    ReleaseCapture();
                    let mut r: RECT = zeroed();
                    GetWindowRect(hwnd, &mut r);
                    save_offset(r.left - TASKBAR_LEFT.load(Relaxed));
                }
                0
            }
            WM_RBUTTONUP => {
                let menu = CreatePopupMenu();
                AppendMenuW(menu, MF_STRING, CMD_REFRESH, wide("Refresh now").as_ptr());
                AppendMenuW(menu, MF_STRING, CMD_QUIT, wide("Quit").as_ptr());
                let mut pt: POINT = zeroed();
                GetCursorPos(&mut pt);
                SetForegroundWindow(hwnd);
                TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, null());
                DestroyMenu(menu);
                0
            }
            WM_COMMAND => {
                match wp & 0xffff {
                    CMD_REFRESH => REFRESH.store(true, Relaxed),
                    CMD_QUIT => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
