//! Icône System Tray et menu minimaliste (thread principal, boucle de messages Win32).

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, SetTimer, TranslateMessage, MSG};

use crate::config::EngineKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayView {
    pub enabled: bool,
    pub listening: bool,
    pub busy: bool,
    pub engine: EngineKind,
    pub phone_connected: bool,
}

pub struct TrayCallbacks {
    pub view: Box<dyn Fn() -> TrayView>,
    pub open_settings: Box<dyn Fn()>,
    pub show_qr: Box<dyn Fn()>,
    pub set_enabled: Box<dyn Fn(bool)>,
    pub set_engine: Box<dyn Fn(EngineKind)>,
    pub copy_last: Box<dyn Fn()>,
    pub should_quit: Box<dyn Fn() -> bool>,
}

/// Icône 32×32 générée : pastille colorée + micro stylisé.
fn make_icon(color: [u8; 3]) -> Option<Icon> {
    const N: u32 = 32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let i = ((y * N + x) * 4) as usize;
            let (fx, fy) = (x as f32 - c, y as f32 - c);
            let d = (fx * fx + fy * fy).sqrt();
            if d <= 15.5 {
                let a = ((15.5 - d).min(1.0) * 255.0) as u8;
                let mut px = [color[0], color[1], color[2], a];
                // Corps du micro (capsule) et pied.
                let in_capsule = fx.abs() <= 4.0 && (-9.0..=3.0).contains(&fy) && {
                    let cy = if fy < -5.0 { fy + 5.0 } else if fy > -1.0 { fy + 1.0 } else { 0.0 };
                    (fx * fx + cy * cy).sqrt() <= 4.0
                };
                let in_arc = (fy >= -1.0 && fy <= 5.0) && {
                    let r = (fx * fx + (fy + 1.0).powi(2)).sqrt();
                    (6.0..=7.6).contains(&r) && fy >= -1.0
                };
                let in_stem = fx.abs() <= 1.0 && (6.0..=9.0).contains(&fy);
                let in_base = fx.abs() <= 4.5 && (9.0..=10.5).contains(&fy);
                if in_capsule || in_arc || in_stem || in_base {
                    px = [255, 255, 255, a];
                }
                rgba[i..i + 4].copy_from_slice(&px);
            }
        }
    }
    Icon::from_rgba(rgba, N, N).ok()
}

fn icon_for(v: &TrayView) -> Option<Icon> {
    let color = if !v.enabled {
        [120, 120, 128]
    } else if v.listening {
        [220, 38, 38]
    } else if v.busy {
        [217, 119, 6]
    } else {
        [37, 99, 235]
    };
    make_icon(color)
}

fn tooltip(v: &TrayView) -> String {
    let mut t = String::from("DumonVoice — ");
    t.push_str(if !v.enabled {
        "en pause"
    } else if v.listening {
        "écoute…"
    } else if v.busy {
        "traitement…"
    } else {
        "prêt"
    });
    if v.phone_connected {
        t.push_str(" · téléphone connecté");
    }
    t
}

/// Boucle bloquante du System Tray (à appeler depuis le thread principal).
pub fn run(cb: TrayCallbacks) -> anyhow::Result<()> {
    let mut view = (cb.view)();
    let menu = Menu::new();
    let toggle = MenuItem::new(if view.enabled { "Mettre en pause" } else { "Activer" }, true, None);
    let settings = MenuItem::new("Ouvrir les réglages", true, None);
    let qr = MenuItem::new("Afficher le QR code mobile", true, None);
    let engine_menu = Submenu::new("Changer de moteur", true);
    let eng_whisper = CheckMenuItem::new("Classique (Whisper intégré)", true, view.engine == EngineKind::Whisper, None);
    let eng_ollama = CheckMenuItem::new("Ollama (IA locale)", true, view.engine == EngineKind::Ollama, None);
    engine_menu.append_items(&[&eng_whisper, &eng_ollama])?;
    let copy = MenuItem::new("Copier la dernière dictée", true, None);
    let quit = MenuItem::new("Quitter", true, None);
    menu.append_items(&[
        &toggle,
        &PredefinedMenuItem::separator(),
        &settings,
        &qr,
        &engine_menu,
        &copy,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip(&view));
    if let Some(i) = icon_for(&view) {
        builder = builder.with_icon(i);
    }
    let tray = builder.build()?;

    unsafe {
        SetTimer(HWND::default(), 0, 250, None);
    }
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) };
        if r.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == *quit.id() {
                return Ok(());
            } else if ev.id == *toggle.id() {
                (cb.set_enabled)(!view.enabled);
            } else if ev.id == *settings.id() {
                (cb.open_settings)();
            } else if ev.id == *qr.id() {
                (cb.show_qr)();
            } else if ev.id == *copy.id() {
                (cb.copy_last)();
            } else if ev.id == *eng_whisper.id() {
                (cb.set_engine)(EngineKind::Whisper);
            } else if ev.id == *eng_ollama.id() {
                (cb.set_engine)(EngineKind::Ollama);
            }
        }
        if (cb.should_quit)() {
            return Ok(());
        }
        let v = (cb.view)();
        if v != view {
            if v.enabled != view.enabled {
                toggle.set_text(if v.enabled { "Mettre en pause" } else { "Activer" });
            }
            eng_whisper.set_checked(v.engine == EngineKind::Whisper);
            eng_ollama.set_checked(v.engine == EngineKind::Ollama);
            let _ = tray.set_icon(icon_for(&v));
            let _ = tray.set_tooltip(Some(tooltip(&v)));
            view = v;
        }
    }
    Ok(())
}
