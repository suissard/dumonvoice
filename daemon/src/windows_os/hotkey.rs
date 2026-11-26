//! Hook clavier global bas niveau (`WH_KEYBOARD_LL`) + hook souris optionnel
//! (`WH_MOUSE_LL`, boutons latéraux) : Push-to-Talk fiable, touches uniques (Ctrl droit,
//! Verr. Défil, F9…), Échap pour annuler, capture « Cliquer pour définir la touche ».

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP,
    WM_KEYDOWN, WM_KEYUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::injector::{send_mask_key, INJECT_MAGIC};
use crate::hotkey_keys::{
    is_mouse, CaptureResult, HotkeyMatcher, KeyCapture, MatchEvent, VK_ESCAPE, VK_LBUTTON_MIDDLE, VK_XBUTTON1,
    VK_XBUTTON2,
};

#[derive(Debug, Clone)]
pub enum HookEvent {
    Down,
    Up,
    Escape,
    Captured(Option<Vec<String>>),
}

const WM_REINSTALL: u32 = WM_APP + 1;

struct Shared {
    matcher: Mutex<HotkeyMatcher>,
    capture: Mutex<Option<KeyCapture>>,
    /// L'orchestrateur est en écoute (Échap doit annuler).
    listening: Arc<AtomicBool>,
    enabled: AtomicBool,
    need_mouse: AtomicBool,
    tx: UnboundedSender<HookEvent>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

fn handle(vk: u32, down: bool) -> bool {
    let Some(s) = SHARED.get() else { return false };
    {
        let mut cap = s.capture.lock();
        if let Some(c) = cap.as_mut() {
            match c.on_key(vk, down) {
                CaptureResult::Pending => {}
                CaptureResult::Cancelled => {
                    *cap = None;
                    let _ = s.tx.send(HookEvent::Captured(None));
                }
                CaptureResult::Done(keys) => {
                    *cap = None;
                    let _ = s.tx.send(HookEvent::Captured(Some(keys)));
                }
            }
            return true; // rien ne fuit vers l'application pendant la capture
        }
    }
    if vk == VK_ESCAPE && down && s.listening.load(Ordering::SeqCst) {
        let _ = s.tx.send(HookEvent::Escape);
        return true;
    }
    if !s.enabled.load(Ordering::SeqCst) {
        return false;
    }
    let mut m = s.matcher.lock();
    let d = m.on_key(vk, down);
    match d.event {
        MatchEvent::Activated => {
            let _ = s.tx.send(HookEvent::Down);
        }
        MatchEvent::Deactivated => {
            if m.combo_has_meta() {
                send_mask_key();
            }
            let _ = s.tx.send(HookEvent::Up);
        }
        MatchEvent::None => {}
    }
    d.suppress
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if kb.dwExtraInfo != INJECT_MAGIC {
            let msg = wparam.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            if (down || up) && handle(kb.vkCode, down) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let vk = match msg {
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                if (ms.mouseData >> 16) & 0xFFFF == 1 {
                    Some(VK_XBUTTON1)
                } else {
                    Some(VK_XBUTTON2)
                }
            }
            WM_MBUTTONDOWN | WM_MBUTTONUP => Some(VK_LBUTTON_MIDDLE),
            _ => None,
        };
        if let Some(vk) = vk {
            let down = msg == WM_XBUTTONDOWN || msg == WM_MBUTTONDOWN;
            if handle(vk, down) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// Contrôle du hook depuis le reste de l'application.
#[derive(Clone)]
pub struct HotkeyHandle {
    listening: Arc<AtomicBool>,
}

impl HotkeyHandle {
    pub fn set_combo(&self, vks: &[u32], enabled: bool) {
        if let Some(s) = SHARED.get() {
            s.matcher.lock().set_combo(vks);
            s.enabled.store(enabled, Ordering::SeqCst);
            let mouse = vks.iter().any(|v| is_mouse(*v));
            if s.need_mouse.swap(mouse, Ordering::SeqCst) != mouse {
                self.reinstall();
            }
        }
    }

    pub fn start_capture(&self) {
        if let Some(s) = SHARED.get() {
            *s.capture.lock() = Some(KeyCapture::default());
            // La capture des boutons souris nécessite le hook souris.
            s.need_mouse.store(true, Ordering::SeqCst);
            self.reinstall();
        }
    }

    pub fn set_listening(&self, v: bool) {
        self.listening.store(v, Ordering::SeqCst);
    }

    /// Réinstalle les hooks (sortie de veille : Windows peut les avoir retirés).
    pub fn reinstall(&self) {
        if let Some(s) = SHARED.get() {
            s.matcher.lock().reset();
        }
        let tid = THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_REINSTALL, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn install(hinst: HINSTANCE, mouse: bool) -> (Option<HHOOK>, Option<HHOOK>) {
    unsafe {
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hinst, 0).ok();
        let ms = if mouse {
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hinst, 0).ok()
        } else {
            None
        };
        if kb.is_none() {
            tracing::error!("Installation du hook clavier impossible");
        }
        (kb, ms)
    }
}

fn uninstall(h: (Option<HHOOK>, Option<HHOOK>)) {
    unsafe {
        if let Some(k) = h.0 {
            let _ = UnhookWindowsHookEx(k);
        }
        if let Some(m) = h.1 {
            let _ = UnhookWindowsHookEx(m);
        }
    }
}

/// Démarre le thread du hook (avec sa propre boucle de messages).
pub fn spawn(combo: Vec<u32>, enabled: bool, tx: UnboundedSender<HookEvent>) -> HotkeyHandle {
    let listening = Arc::new(AtomicBool::new(false));
    let need_mouse = combo.iter().any(|v| is_mouse(*v));
    let _ = SHARED.set(Shared {
        matcher: Mutex::new(HotkeyMatcher::new(&combo)),
        capture: Mutex::new(None),
        listening: listening.clone(),
        enabled: AtomicBool::new(enabled),
        need_mouse: AtomicBool::new(need_mouse),
        tx,
    });
    std::thread::Builder::new()
        .name("keyboard-hook".into())
        .spawn(|| unsafe {
            THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
            let hinst = GetModuleHandleW(PCWSTR::null())
                .map(|m| HINSTANCE(m.0))
                .unwrap_or_default();
            let mouse = SHARED.get().map(|s| s.need_mouse.load(Ordering::SeqCst)).unwrap_or(false);
            let mut hooks = install(hinst, mouse);
            let mut msg = MSG::default();
            loop {
                let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
                if r.0 <= 0 {
                    break;
                }
                if msg.message == WM_REINSTALL {
                    uninstall(hooks);
                    let mouse = SHARED.get().map(|s| s.need_mouse.load(Ordering::SeqCst)).unwrap_or(false);
                    hooks = install(hinst, mouse);
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            uninstall(hooks);
        })
        .expect("thread hook");
    HotkeyHandle { listening }
}
