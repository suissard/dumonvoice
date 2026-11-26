//! Abstraction de la plateforme (injection de texte, fenêtre active, sons, ducking…).
//! Implémentée par `windows_os::WindowsPlatform` ; `NullPlatform` sert aux tests et au
//! développement hors Windows.

use parking_lot::Mutex;

use crate::config::InjectionMethod;
use crate::protocol::QuickAction;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowInfo {
    /// `HWND` sous forme entière.
    pub hwnd: isize,
    pub title: String,
    pub process_name: String,
    /// La fenêtre appartient à un processus au niveau d'intégrité élevé (Admin).
    pub elevated: bool,
}

impl WindowInfo {
    pub fn display_name(&self) -> String {
        if !self.title.is_empty() {
            // Les titres du type « fichier.rs - Projet - Visual Studio Code » : on garde la
            // dernière partie (nom de l'application).
            let last = self.title.rsplit(" - ").next().unwrap_or(&self.title);
            last.trim().to_string()
        } else {
            self.process_name.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteCombo {
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

/// Terminaux connus et combinaison de collage adaptée.
pub fn paste_combo_for(process_name: &str) -> PasteCombo {
    let p = process_name.to_ascii_lowercase();
    match p.as_str() {
        "windowsterminal.exe" | "alacritty.exe" | "wezterm-gui.exe" => PasteCombo::CtrlShiftV,
        "cmd.exe" | "powershell.exe" | "pwsh.exe" | "mintty.exe" | "conhost.exe" => {
            PasteCombo::ShiftInsert
        }
        _ => PasteCombo::CtrlV,
    }
}

/// Le texte impose-t-il le presse-papiers (> 200 caractères, multi-lignes, hors BMP) ?
pub fn requires_clipboard(text: &str) -> bool {
    text.chars().count() > 200 || text.contains('\n') || text.chars().any(|c| (c as u32) > 0xFFFF)
}

pub fn effective_method(preferred: InjectionMethod, text: &str) -> InjectionMethod {
    if requires_clipboard(text) {
        InjectionMethod::ClipboardPaste
    } else {
        preferred
    }
}

/// Encodage UTF-16 pour `SendInput` (`KEYEVENTF_UNICODE`), paires de substitution incluses.
pub fn utf16_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[derive(Debug, Clone)]
pub struct InjectOptions {
    pub method: InjectionMethod,
    pub press_enter_after: bool,
    pub restore_clipboard: bool,
    pub paste_delay_ms: u64,
}

pub type RestoreFn = Box<dyn FnOnce() + Send>;

pub struct InjectionOutcome {
    pub injected: bool,
    pub note: Option<String>,
    /// Restauration différée du presse-papiers (exécutée après l'envoi de l'ACK).
    pub restore: Option<RestoreFn>,
}

impl InjectionOutcome {
    pub fn ok() -> Self {
        Self {
            injected: true,
            note: None,
            restore: None,
        }
    }

    pub fn failed(note: &str) -> Self {
        Self {
            injected: false,
            note: Some(note.into()),
            restore: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Start,
    Stop,
    Received,
    Error,
}

pub trait Platform: Send + Sync + 'static {
    fn foreground_window(&self) -> Option<WindowInfo>;
    /// Injection bloquante dans la fenêtre active.
    fn inject(&self, text: &str, opts: &InjectOptions) -> InjectionOutcome;
    fn copy_to_clipboard(&self, text: &str) -> bool;
    fn quick_action(&self, action: QuickAction) -> bool;
    /// Notre propre processus est-il élevé ?
    fn self_elevated(&self) -> bool {
        false
    }
    fn set_ducking(&self, _on: bool, _level: f32) {}
    fn play_sound(&self, _sound: Sound) {}
    /// Une autre application capture-t-elle le micro (appel visio) ?
    fn mic_in_use_by_other_app(&self) -> bool {
        false
    }
    /// Jeu / application plein écran exclusif en cours ?
    fn fullscreen_exclusive(&self) -> bool {
        false
    }
    fn show_toast(&self, _title: &str, _message: &str) {}
    fn gpu_info(&self) -> Option<String> {
        None
    }
}

/// Plateforme factice : journalise et mémorise les injections (tests, Linux/macOS).
#[derive(Default)]
pub struct NullPlatform {
    pub injected: Mutex<Vec<String>>,
    pub clipboard: Mutex<Option<String>>,
    pub actions: Mutex<Vec<QuickAction>>,
    pub window: Mutex<Option<WindowInfo>>,
    /// Liste de fenêtres renvoyées successivement par `foreground_window` (tests).
    pub window_sequence: Mutex<Vec<WindowInfo>>,
    pub inject_delay_ms: Mutex<u64>,
}

impl Platform for NullPlatform {
    fn foreground_window(&self) -> Option<WindowInfo> {
        let mut seq = self.window_sequence.lock();
        if !seq.is_empty() {
            let w = seq.remove(0);
            *self.window.lock() = Some(w.clone());
            return Some(w);
        }
        self.window.lock().clone().or_else(|| {
            Some(WindowInfo {
                hwnd: 1,
                title: "Bac à sable".into(),
                process_name: "test.exe".into(),
                elevated: false,
            })
        })
    }

    fn inject(&self, text: &str, opts: &InjectOptions) -> InjectionOutcome {
        let d = *self.inject_delay_ms.lock();
        if d > 0 {
            std::thread::sleep(std::time::Duration::from_millis(d));
        }
        let mut t = text.to_string();
        if opts.press_enter_after {
            t.push_str("⏎");
        }
        tracing::info!("[NullPlatform] injection : {t}");
        self.injected.lock().push(t);
        InjectionOutcome::ok()
    }

    fn copy_to_clipboard(&self, text: &str) -> bool {
        *self.clipboard.lock() = Some(text.to_string());
        true
    }

    fn quick_action(&self, action: QuickAction) -> bool {
        self.actions.lock().push(action);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_forced_for_long_multiline_emoji() {
        assert!(!requires_clipboard("Bonjour à tous"));
        assert!(requires_clipboard("ligne 1\nligne 2"));
        assert!(requires_clipboard("Super 👍"));
        assert!(requires_clipboard(&"a".repeat(201)));
        assert!(!requires_clipboard(&"é".repeat(200)));
        assert_eq!(
            effective_method(InjectionMethod::SendInput, "ok 👍"),
            InjectionMethod::ClipboardPaste
        );
        assert_eq!(
            effective_method(InjectionMethod::SendInput, "ok"),
            InjectionMethod::SendInput
        );
    }

    #[test]
    fn surrogate_pairs() {
        let u = utf16_units("a👍");
        assert_eq!(u.len(), 3);
        assert_eq!(u[1], 0xD83D);
        assert_eq!(u[2], 0xDC4D);
    }

    #[test]
    fn terminal_detection() {
        assert_eq!(paste_combo_for("WindowsTerminal.exe"), PasteCombo::CtrlShiftV);
        assert_eq!(paste_combo_for("cmd.exe"), PasteCombo::ShiftInsert);
        assert_eq!(paste_combo_for("PWSH.EXE"), PasteCombo::ShiftInsert);
        assert_eq!(paste_combo_for("Code.exe"), PasteCombo::CtrlV);
    }

    #[test]
    fn display_names() {
        let w = WindowInfo {
            title: "main.rs - dumonvoice - Visual Studio Code".into(),
            ..Default::default()
        };
        assert_eq!(w.display_name(), "Visual Studio Code");
        let w = WindowInfo {
            process_name: "notepad.exe".into(),
            ..Default::default()
        };
        assert_eq!(w.display_name(), "notepad.exe");
    }
}
