//! Injecteur Windows : presse-papiers sécurisé (exclusion Win+V / cloud) + `Ctrl+V`
//! (ou combinaison terminal), frappe `SendInput` Unicode (paires de substitution UTF-16),
//! `VK_RETURN` optionnel, restauration différée du presse-papiers d'origine.

use std::thread::sleep;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalFree, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
};

use crate::config::InjectionMethod;
use crate::platform::{paste_combo_for, utf16_units, InjectOptions, InjectionOutcome, PasteCombo};

/// Marqueur `dwExtraInfo` de nos frappes synthétiques (ignorées par notre hook).
pub const INJECT_MAGIC: usize = 0x00D7_0C3E;

const CF_UNICODETEXT: u32 = 13;
const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_INSERT: u16 = 0x2D;
const VK_LWIN: u16 = 0x5B;
const VK_RWIN: u16 = 0x5C;
const VK_V: u16 = 0x56;
const VK_Z: u16 = 0x5A;

/// Formats GDI / non-HGLOBAL qu'on ne sait pas copier octet par octet.
const SKIP_FORMATS: &[u32] = &[2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

fn key_input(vk: u16, up: bool) -> INPUT {
    let extended = matches!(vk, VK_INSERT | 0x2E | 0x21..=0x28 | VK_LWIN | VK_RWIN);
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_MAGIC,
            },
        },
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_MAGIC,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> bool {
    if inputs.is_empty() {
        return true;
    }
    let n = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    n as usize == inputs.len()
}

/// Combinaison de touches : appui des modificateurs, touche, relâchement inverse.
pub fn send_chord(mods: &[u16], key: u16) -> bool {
    let mut v = Vec::new();
    for m in mods {
        v.push(key_input(*m, false));
    }
    v.push(key_input(key, false));
    v.push(key_input(key, true));
    for m in mods.iter().rev() {
        v.push(key_input(*m, true));
    }
    send(&v)
}

pub fn send_key(vk: u16) -> bool {
    send(&[key_input(vk, false), key_input(vk, true)])
}

/// Touche « masque » (vkE8, non assignée) : empêche l'ouverture du menu Démarrer au
/// relâchement de Win après un raccourci Win+….
pub fn send_mask_key() {
    send(&[key_input(0xE8, false), key_input(0xE8, true)]);
}

fn modifier_down() -> bool {
    [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(*vk as i32) } as u16 & 0x8000 != 0)
}

/// Attend (≤ 600 ms) que l'utilisateur relâche les modificateurs physiques, sinon une
/// frappe « a » deviendrait « Ctrl+A ».
fn wait_modifiers_released() {
    for _ in 0..30 {
        if !modifier_down() {
            return;
        }
        sleep(Duration::from_millis(20));
    }
}

pub fn send_unicode_text(text: &str) -> bool {
    let units = utf16_units(text);
    let mut inputs = Vec::with_capacity(units.len() * 2);
    for u in units {
        inputs.push(unicode_input(u, false));
        inputs.push(unicode_input(u, true));
    }
    // Paquets raisonnables pour ne pas saturer la file d'entrée.
    for chunk in inputs.chunks(128) {
        if !send(chunk) {
            return false;
        }
        sleep(Duration::from_millis(2));
    }
    true
}

// ------------------------------------------------------------------ Presse-papiers

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Option<Self> {
        for _ in 0..15 {
            if unsafe { OpenClipboard(HWND::default()) }.is_ok() {
                return Some(ClipboardGuard);
            }
            sleep(Duration::from_millis(15));
        }
        None
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

pub struct ClipboardSnapshot {
    items: Vec<(u32, Vec<u8>)>,
}

fn global_bytes(h: HANDLE) -> Option<Vec<u8>> {
    let hg = HGLOBAL(h.0);
    unsafe {
        let size = GlobalSize(hg);
        if size == 0 || size > MAX_SNAPSHOT_BYTES {
            return None;
        }
        let ptr = GlobalLock(hg) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let data = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(hg);
        Some(data)
    }
}

fn alloc_global(data: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let hg = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)).ok()?;
        let ptr = GlobalLock(hg) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(hg);
            return None;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        let _ = GlobalUnlock(hg);
        Some(hg)
    }
}

fn set_data(format: u32, data: &[u8]) -> bool {
    let Some(hg) = alloc_global(data) else { return false };
    match unsafe { SetClipboardData(format, HANDLE(hg.0)) } {
        Ok(_) => true, // le système possède désormais la mémoire
        Err(_) => {
            let _ = unsafe { GlobalFree(hg) };
            false
        }
    }
}

pub fn save_clipboard() -> Option<ClipboardSnapshot> {
    let _g = ClipboardGuard::open()?;
    let mut items = Vec::new();
    let mut total = 0usize;
    let mut fmt = 0u32;
    loop {
        fmt = unsafe { EnumClipboardFormats(fmt) };
        if fmt == 0 {
            break;
        }
        if SKIP_FORMATS.contains(&fmt) {
            continue;
        }
        if let Ok(h) = unsafe { GetClipboardData(fmt) } {
            if let Some(bytes) = global_bytes(h) {
                total += bytes.len();
                if total > MAX_SNAPSHOT_BYTES {
                    break;
                }
                items.push((fmt, bytes));
            }
        }
    }
    Some(ClipboardSnapshot { items })
}

pub fn restore_clipboard(snap: ClipboardSnapshot) {
    let Some(_g) = ClipboardGuard::open() else { return };
    unsafe {
        let _ = EmptyClipboard();
    }
    for (fmt, data) in &snap.items {
        set_data(*fmt, data);
    }
}

fn exclusion_formats() -> [u32; 3] {
    unsafe {
        [
            RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
            RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
            RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
        ]
    }
}

/// Place du texte `CF_UNICODETEXT`. `private` ajoute les marqueurs d'exclusion de
/// l'historique Win+V et de la synchronisation cloud.
pub fn set_clipboard_text(text: &str, private: bool) -> bool {
    let Some(_g) = ClipboardGuard::open() else { return false };
    unsafe {
        if EmptyClipboard().is_err() {
            return false;
        }
    }
    let mut units = utf16_units(text);
    units.push(0);
    let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
    if !set_data(CF_UNICODETEXT, &bytes) {
        return false;
    }
    if private {
        let zero = 0u32.to_le_bytes();
        for f in exclusion_formats() {
            if f != 0 {
                set_data(f, &zero);
            }
        }
    }
    true
}

/// Lit le texte du presse-papiers (pour « Copier la dernière dictée », diagnostics).
pub fn get_clipboard_text() -> Option<String> {
    let _g = ClipboardGuard::open()?;
    let h = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
    let bytes = global_bytes(h)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16(&units).ok()
}

fn paste(combo: PasteCombo) -> bool {
    match combo {
        PasteCombo::CtrlV => send_chord(&[VK_CONTROL], VK_V),
        PasteCombo::CtrlShiftV => send_chord(&[VK_CONTROL, VK_SHIFT], VK_V),
        PasteCombo::ShiftInsert => send_chord(&[VK_SHIFT], VK_INSERT),
    }
}

pub fn undo() -> bool {
    wait_modifiers_released();
    send_chord(&[VK_CONTROL], VK_Z)
}

pub fn enter() -> bool {
    wait_modifiers_released();
    send_key(VK_RETURN)
}

pub fn backspace() -> bool {
    wait_modifiers_released();
    send_key(VK_BACK)
}

/// Injection complète dans la fenêtre active (bloquant).
pub fn inject(text: &str, process_name: &str, opts: &InjectOptions) -> InjectionOutcome {
    wait_modifiers_released();
    match opts.method {
        InjectionMethod::SendInput => {
            if !send_unicode_text(text) {
                return InjectionOutcome::failed("UIPI_BLOCKED");
            }
            if opts.press_enter_after {
                sleep(Duration::from_millis(50));
                send_key(VK_RETURN);
            }
            InjectionOutcome::ok()
        }
        InjectionMethod::ClipboardPaste => {
            let snapshot = if opts.restore_clipboard { save_clipboard() } else { None };
            if !set_clipboard_text(text, true) {
                // Presse-papiers verrouillé par une autre application : frappe directe.
                if send_unicode_text(text) {
                    return InjectionOutcome::ok();
                }
                return InjectionOutcome::failed("CLIPBOARD_BUSY");
            }
            let seq = unsafe { GetClipboardSequenceNumber() };
            if !paste(paste_combo_for(process_name)) {
                if let Some(s) = snapshot {
                    restore_clipboard(s);
                }
                return InjectionOutcome::failed("UIPI_BLOCKED");
            }
            let delay = Duration::from_millis(opts.paste_delay_ms);
            let mut waited = false;
            if opts.press_enter_after {
                sleep(delay);
                sleep(Duration::from_millis(50));
                send_key(VK_RETURN);
                waited = true;
            }
            let restore = snapshot.map(|s| {
                Box::new(move || {
                    if !waited {
                        sleep(delay);
                    }
                    // L'utilisateur a copié autre chose entre-temps : on ne l'écrase pas.
                    if unsafe { GetClipboardSequenceNumber() } == seq {
                        restore_clipboard(s);
                    }
                }) as crate::platform::RestoreFn
            });
            InjectionOutcome {
                injected: true,
                note: None,
                restore,
            }
        }
    }
}

#[allow(dead_code)]
fn pcwstr(s: &[u16]) -> PCWSTR {
    PCWSTR(s.as_ptr())
}
