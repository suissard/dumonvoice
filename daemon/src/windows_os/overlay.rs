//! Micro-indicateur flottant (HUD) : fenêtre Win32 `WS_EX_NOACTIVATE | WS_EX_TOPMOST |
//! WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED` — ne vole jamais le focus,
//! absente d'Alt-Tab, traversée par la souris. Reçoit aussi `WM_POWERBROADCAST`
//! (sortie de veille) pour réinitialiser l'audio et les hooks.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint,
    FillRect, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, SelectObject, SetBkMode, SetTextColor,
    SetWindowRgn, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DT_END_ELLIPSIS, HGDIOBJ, LOGFONTW, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, KillTimer, PostMessageW,
    RegisterClassExW, SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, TranslateMessage,
    HMENU, HWND_TOPMOST, LWA_ALPHA, MSG, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WM_APP, WM_DISPLAYCHANGE, WM_PAINT,
    WM_POWERBROADCAST, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceIcon {
    PcMic,
    MobileMic,
    MobileKeyboard,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayState {
    Hidden,
    Listening(SourceIcon),
    Processing(String),
    Success(String),
    Error(String),
}

const WM_OVERLAY_UPDATE: u32 = WM_APP + 10;
const TIMER_ANIM: usize = 1;
const PBT_APMRESUMESUSPEND: usize = 0x7;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

struct Model {
    state: OverlayState,
    since: Instant,
    levels: VecDeque<f32>,
    alpha: u8,
}

struct Shared {
    model: Mutex<Model>,
    level: Box<dyn Fn() -> f32 + Send + Sync>,
    position: Box<dyn Fn() -> String + Send + Sync>,
    enabled: Box<dyn Fn() -> bool + Send + Sync>,
    on_resume: Box<dyn Fn() + Send + Sync>,
    hwnd: AtomicIsize,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

#[derive(Clone)]
pub struct OverlayHandle;

impl OverlayHandle {
    pub fn update(&self, state: OverlayState) {
        let Some(s) = SHARED.get() else { return };
        {
            let mut m = s.model.lock();
            if m.state == state {
                return;
            }
            m.state = state;
            m.since = Instant::now();
            m.levels.clear();
        }
        let h = s.hwnd.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(super::system_state::hwnd_from(h), WM_OVERLAY_UPDATE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn scale() -> f32 {
    (unsafe { GetDpiForSystem() } as f32 / 96.0).max(1.0)
}

fn size() -> (i32, i32) {
    let s = scale();
    ((260.0 * s) as i32, (44.0 * s) as i32)
}

fn place(hwnd: HWND, pos: &str) {
    unsafe {
        let mon = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let (w, h) = size();
        let margin = (24.0 * scale()) as i32;
        let x = if pos.ends_with("left") {
            wa.left + margin
        } else if pos.ends_with("right") {
            wa.right - w - margin
        } else {
            wa.left + (wa.right - wa.left - w) / 2
        };
        let y = if pos.starts_with("top") { wa.top + margin } else { wa.bottom - h - margin };
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, h, h);
        SetWindowRgn(hwnd, rgn, true);
    }
}

fn label(state: &OverlayState) -> (String, COLORREF) {
    match state {
        OverlayState::Hidden => (String::new(), rgb(120, 120, 120)),
        OverlayState::Listening(src) => (
            match src {
                SourceIcon::PcMic => "🎙  Écoute (micro PC)…".to_string(),
                SourceIcon::MobileMic => "📱  Écoute (micro mobile)…".to_string(),
                SourceIcon::MobileKeyboard => "⌨  Texte mobile reçu".to_string(),
            },
            rgb(239, 68, 68),
        ),
        OverlayState::Processing(t) => (format!("⏳  {t}"), rgb(245, 158, 11)),
        OverlayState::Success(t) => (format!("✓  {t}"), rgb(34, 197, 94)),
        OverlayState::Error(t) => (format!("⚠  {t}"), rgb(239, 68, 68)),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

unsafe fn paint(hwnd: HWND, s: &Shared) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let (w, h) = size();
    let bg = CreateSolidBrush(rgb(24, 27, 34));
    let full = RECT { left: 0, top: 0, right: w, bottom: h };
    FillRect(hdc, &full, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));

    let m = s.model.lock();
    let (text, accent) = label(&m.state);
    let sc = scale();
    // Pastille d'état.
    let dot = (10.0 * sc) as i32;
    let pad = (16.0 * sc) as i32;
    let ab = CreateSolidBrush(accent);
    let dot_rect = RECT {
        left: pad,
        top: (h - dot) / 2,
        right: pad + dot,
        bottom: (h + dot) / 2,
    };
    FillRect(hdc, &dot_rect, ab);
    // Onde audio temps réel pendant l'écoute.
    let mut text_right = w - pad;
    if matches!(m.state, OverlayState::Listening(_)) {
        let bars = 12usize;
        let bw = (4.0 * sc) as i32;
        let gap = (3.0 * sc) as i32;
        let total = bars as i32 * (bw + gap);
        let x0 = w - pad - total;
        text_right = x0 - gap;
        for (i, lv) in m.levels.iter().rev().take(bars).enumerate() {
            let amp = ((lv * 6.0).min(1.0) * (h as f32 * 0.6)).max(2.0 * sc) as i32;
            let x = x0 + (bars - 1 - i) as i32 * (bw + gap);
            let r = RECT {
                left: x,
                top: (h - amp) / 2,
                right: x + bw,
                bottom: (h + amp) / 2,
            };
            FillRect(hdc, &r, ab);
        }
    }
    let _ = DeleteObject(HGDIOBJ(ab.0));

    let mut lf = LOGFONTW {
        lfHeight: -(14.0 * sc) as i32,
        lfWeight: 600,
        ..Default::default()
    };
    for (i, c) in "Segoe UI".encode_utf16().enumerate() {
        lf.lfFaceName[i] = c;
    }
    let font = CreateFontIndirectW(&lf);
    let old = SelectObject(hdc, HGDIOBJ(font.0));
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, rgb(240, 240, 245));
    let mut tr = RECT {
        left: pad + dot + (10.0 * sc) as i32,
        top: 0,
        right: text_right,
        bottom: h,
    };
    let mut t = wide(&text);
    DrawTextW(hdc, &mut t, &mut tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(font.0));
    drop(m);
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn apply(hwnd: HWND, s: &Shared) {
    let state = s.model.lock().state.clone();
    let visible = !matches!(state, OverlayState::Hidden) && (s.enabled)() && !super::system_state::fullscreen_exclusive();
    if visible {
        place(hwnd, &(s.position)());
        s.model.lock().alpha = 235;
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 235, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_ANIM, 33, None);
    } else {
        let _ = KillTimer(hwnd, TIMER_ANIM);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    let _ = InvalidateRect(hwnd, None, true);
}

unsafe fn on_timer(hwnd: HWND, s: &Shared) {
    let mut hide = false;
    {
        let mut m = s.model.lock();
        let age = m.since.elapsed();
        match m.state {
            OverlayState::Listening(_) => {
                let lv = (s.level)();
                m.levels.push_back(lv);
                while m.levels.len() > 24 {
                    m.levels.pop_front();
                }
            }
            // Succès : visible 600 ms puis fondu de 300 ms.
            OverlayState::Success(_) => {
                if age > Duration::from_millis(600) {
                    let t = (age.as_millis() as f32 - 600.0) / 300.0;
                    let a = (235.0 * (1.0 - t)).max(0.0) as u8;
                    m.alpha = a;
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), a, LWA_ALPHA);
                    if t >= 1.0 {
                        m.state = OverlayState::Hidden;
                        hide = true;
                    }
                }
            }
            OverlayState::Error(_) => {
                if age > Duration::from_secs(3) {
                    m.state = OverlayState::Hidden;
                    hide = true;
                }
            }
            OverlayState::Processing(_) => {
                if age > Duration::from_secs(60) {
                    m.state = OverlayState::Hidden;
                    hide = true;
                }
            }
            OverlayState::Hidden => hide = true,
        }
    }
    if hide {
        apply(hwnd, s);
    } else {
        let _ = InvalidateRect(hwnd, None, false);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(s) = SHARED.get() else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    match msg {
        WM_OVERLAY_UPDATE => {
            apply(hwnd, s);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIM {
                on_timer(hwnd, s);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd, s);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            if s.model.lock().state != OverlayState::Hidden {
                place(hwnd, &(s.position)());
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 == PBT_APMRESUMEAUTOMATIC || wparam.0 == PBT_APMRESUMESUSPEND {
                tracing::info!("Sortie de veille détectée");
                (s.on_resume)();
            }
            LRESULT(1)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub struct OverlayCallbacks {
    pub level: Box<dyn Fn() -> f32 + Send + Sync>,
    pub position: Box<dyn Fn() -> String + Send + Sync>,
    pub enabled: Box<dyn Fn() -> bool + Send + Sync>,
    pub on_resume: Box<dyn Fn() + Send + Sync>,
}

pub fn spawn(cb: OverlayCallbacks) -> OverlayHandle {
    let shared = Arc::new(Shared {
        model: Mutex::new(Model {
            state: OverlayState::Hidden,
            since: Instant::now(),
            levels: VecDeque::new(),
            alpha: 0,
        }),
        level: cb.level,
        position: cb.position,
        enabled: cb.enabled,
        on_resume: cb.on_resume,
        hwnd: AtomicIsize::new(0),
    });
    let _ = SHARED.set(shared.clone());
    std::thread::Builder::new()
        .name("overlay".into())
        .spawn(move || unsafe {
            let hinst = GetModuleHandleW(PCWSTR::null())
                .map(|m| HINSTANCE(m.0))
                .unwrap_or_default();
            let class = w!("DumonVoiceOverlay");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let (w, h) = size();
            let hwnd = match CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                class,
                w!("DumonVoice"),
                WS_POPUP,
                0,
                0,
                w,
                h,
                HWND::default(),
                HMENU::default(),
                hinst,
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("Création de l'overlay impossible : {e}");
                    return;
                }
            };
            shared.hwnd.store(super::system_state::hwnd_to(hwnd), Ordering::SeqCst);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("thread overlay");
    OverlayHandle
}
