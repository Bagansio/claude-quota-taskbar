//! Claude plan usage drawn directly on the Windows 11 taskbar.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

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
    eprintln!("claude-quota-taskbar only runs on Windows; on Linux use claude-quota-waybar");
}

#[cfg(windows)]
mod app {
    use crate::api::{self, wide};
    use crate::{render, state};
    use claude_quota_core::fetch::next_poll_secs;
    use claude_quota_core::{now_secs, WakeDetector, MIN_MANUAL_REFRESH_SECS};
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering::Relaxed};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::HiDpi::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WM_UPDATED: u32 = WM_APP + 1;
    const TIMER_REPAINT: usize = 1;
    const TIMER_TOPMOST: usize = 2;
    const CMD_REFRESH: usize = 1;
    const CMD_QUIT: usize = 2;

    static REFRESH: AtomicBool = AtomicBool::new(false);
    /// Set on resume from sleep or session unlock: refetch now and retry quickly while offline.
    static WOKE: AtomicBool = AtomicBool::new(false);
    // wParam values (defined locally; they are plain numbers in the Win32 headers).
    const WTS_SESSION_UNLOCK: usize = 0x8;
    const PBT_APMRESUMESUSPEND: usize = 0x7;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
    static DRAGGING: AtomicBool = AtomicBool::new(false);
    static DRAG_DX: AtomicI32 = AtomicI32::new(0);
    static TASKBAR_LEFT: AtomicI32 = AtomicI32::new(0);
    static SCALE_BITS: AtomicU32 = AtomicU32::new(0);
    static HEIGHT: AtomicI32 = AtomicI32::new(48);
    /// True when embedded as a child of the taskbar window (follows its z-order and visibility).
    static IS_CHILD: AtomicBool = AtomicBool::new(false);
    static HWND_RAW: AtomicUsize = AtomicUsize::new(0);
    /// Broadcast by Explorer after it (re)starts; the taskbar window is new and we must re-place ourselves.
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

    fn scale() -> f32 {
        f32::from_bits(SCALE_BITS.load(Relaxed))
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

    /// Rectangle of the primary taskbar (falls back to the bottom 48 px of the screen).
    unsafe fn taskbar_rect() -> RECT {
        let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
        let mut tb = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if tray.is_null() || GetWindowRect(tray, &mut tb) == 0 {
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            tb = RECT { left: 0, top: sh - (48.0 * scale()) as i32, right: sw, bottom: sh };
        }
        tb
    }

    /// Is the taskbar anywhere above us in the z-order? (Other windows may sit in between.)
    unsafe fn taskbar_above(hwnd: HWND, tray: HWND) -> bool {
        let mut w = GetWindow(hwnd, GW_HWNDPREV);
        while !w.is_null() {
            if w == tray {
                return true;
            }
            w = GetWindow(w, GW_HWNDPREV);
        }
        false
    }

    /// Re-anchor to the (possibly new) taskbar, keeping the user's horizontal offset.
    unsafe fn reposition(hwnd: HWND) {
        let tb = taskbar_rect();
        let h = tb.bottom - tb.top;
        HEIGHT.store(h, Relaxed);
        TASKBAR_LEFT.store(tb.left, Relaxed);
        let offset = load_offset().unwrap_or((190.0 * scale()) as i32);
        let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
        if IS_CHILD.load(Relaxed) && !tray.is_null() {
            // Explorer may have created a new taskbar window: re-attach.
            SetParent(hwnd, tray);
            SetWindowPos(hwnd, HWND_TOP, offset, 0, render::width(scale()), h, SWP_NOACTIVATE);
        } else {
            SetWindowPos(hwnd, HWND_TOPMOST, tb.left + offset, tb.top, render::width(scale()), h, SWP_NOACTIVATE);
        }
        InvalidateRect(hwnd, null(), 0);
    }

    pub fn run() {
        unsafe {
            let mutex = CreateMutexW(null(), 0, wide("Local\\claude-quota-taskbar").as_ptr());
            if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
                return;
            }
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            SCALE_BITS.store((GetDpiForSystem() as f32 / 96.0).to_bits(), Relaxed);

            let tb = taskbar_rect();
            let h = tb.bottom - tb.top;
            HEIGHT.store(h, Relaxed);
            TASKBAR_LEFT.store(tb.left, Relaxed);
            let offset = load_offset().unwrap_or((190.0 * scale()) as i32);
            // Preferably a child of the taskbar itself, so Start, flyouts and full-screen apps cover it
            // like any other taskbar content. Falls back to a topmost popup if there is no taskbar window.
            let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
            let child = !tray.is_null();
            IS_CHILD.store(child, Relaxed);
            let (style, ex, x, y, parent) = if child {
                (WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS, WS_EX_LAYERED | WS_EX_NOACTIVATE, offset, 0, tray)
            } else {
                (WS_POPUP, WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, tb.left + offset, tb.top, null_mut())
            };

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
                ex,
                class.as_ptr(),
                wide("Claude usage").as_ptr(),
                style,
                x,
                y,
                render::width(scale()),
                h,
                parent,
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
            SetTimer(hwnd, TIMER_TOPMOST, 250, None);
            TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Relaxed);

            // Power and session notifications only reach top-level windows, and the widget is a
            // child of the taskbar: listen on a separate hidden one.
            let pclass = wide("ClaudeQuotaTaskbarPower");
            let mut pwc: WNDCLASSEXW = zeroed();
            pwc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            pwc.lpfnWndProc = Some(power_wndproc);
            pwc.hInstance = hinst;
            pwc.lpszClassName = pclass.as_ptr();
            RegisterClassExW(&pwc);
            let power = CreateWindowExW(WS_EX_TOOLWINDOW, pclass.as_ptr(), null(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hinst, null());
            if !power.is_null() {
                WTSRegisterSessionNotification(power, NOTIFY_FOR_THIS_SESSION);
            }

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
        // Backstop for resume notifications that never arrive (wall clock vs monotonic clock).
        let mut wake = WakeDetector::new();
        let mut woke_at: Option<i64> = None;
        loop {
            let started = Instant::now();
            let wait = {
                let r = api::fetch_usage();
                let wait = next_poll_secs(&r, woke_at.map(|w| now_secs() - w));
                let mut s = state::STATE.lock().unwrap();
                match r {
                    Ok(u) => {
                        s.usage = Some(u);
                        s.error = None;
                    }
                    Err(e) => s.error = Some(e.message()),
                }
                wait
            };
            unsafe { PostMessageW(hwnd as HWND, WM_UPDATED, 0, 0) };
            // Sleep, waking early for a manual refresh or a wake (at most one per 15 s; a request
            // that comes sooner is kept and served once 15 s have passed, not dropped).
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if wake.check() | WOKE.swap(false, Relaxed) {
                    woke_at = Some(now_secs());
                    REFRESH.store(true, Relaxed);
                }
                let age = started.elapsed();
                if age >= Duration::from_secs(wait) || (REFRESH.load(Relaxed) && age >= Duration::from_secs(MIN_MANUAL_REFRESH_SECS)) {
                    REFRESH.store(false, Relaxed);
                    break;
                }
            }
        }
    }

    unsafe extern "system" fn power_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let woke = match msg {
            WM_POWERBROADCAST => wp == PBT_APMRESUMEAUTOMATIC || wp == PBT_APMRESUMESUSPEND,
            WM_WTSSESSION_CHANGE => wp == WTS_SESSION_UNLOCK,
            _ => false,
        };
        if woke {
            WOKE.store(true, Relaxed);
            // Explorer may have rebuilt or resized the taskbar meanwhile.
            let widget = HWND_RAW.load(Relaxed) as HWND;
            if !widget.is_null() {
                reposition(widget);
            }
            return if msg == WM_POWERBROADCAST { 1 } else { 0 };
        }
        DefWindowProcW(hwnd, msg, wp, lp)
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
                    if IS_CHILD.load(Relaxed) {
                        // The taskbar's XAML layer is a sibling that can rise above us: stay on top of it.
                        if !GetWindow(hwnd, GW_HWNDPREV).is_null() {
                            SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                        }
                        return 0;
                    }
                    // Popup fallback: only fight the taskbar for the top spot; leave other topmost
                    // windows, e.g. screenshot overlays, alone.
                    let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), null());
                    if !tray.is_null() && taskbar_above(hwnd, tray) {
                        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
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
                    let mut pt = zeroed::<POINT>();
                    GetCursorPos(&mut pt);
                    let sx = pt.x - DRAG_DX.load(Relaxed);
                    if IS_CHILD.load(Relaxed) {
                        SetWindowPos(hwnd, null_mut(), sx - TASKBAR_LEFT.load(Relaxed), 0, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    } else {
                        let mut r = zeroed::<RECT>();
                        GetWindowRect(hwnd, &mut r);
                        SetWindowPos(hwnd, null_mut(), sx, r.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
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
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                reposition(hwnd);
                0
            }
            m if m == TASKBAR_CREATED.load(Relaxed) && m != 0 => {
                reposition(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
