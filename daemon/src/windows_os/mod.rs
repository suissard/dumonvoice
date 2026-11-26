//! Intégration Windows native.

pub mod audio_session;
pub mod hotkey;
pub mod injector;
pub mod overlay;
pub mod sounds;
pub mod system_state;
pub mod tray;

use std::sync::OnceLock;

use crate::platform::{InjectOptions, InjectionOutcome, Platform, Sound, WindowInfo};
use crate::protocol::QuickAction;

pub struct WindowsPlatform {
    ducker: audio_session::Ducker,
    self_elevated: bool,
    overlay: OnceLock<overlay::OverlayHandle>,
    gpu: OnceLock<Option<String>>,
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            ducker: audio_session::Ducker::default(),
            self_elevated: system_state::self_elevated(),
            overlay: OnceLock::new(),
            gpu: OnceLock::new(),
        }
    }

    pub fn set_overlay(&self, h: overlay::OverlayHandle) {
        let _ = self.overlay.set(h);
    }
}

impl Platform for WindowsPlatform {
    fn foreground_window(&self) -> Option<WindowInfo> {
        system_state::foreground_window()
    }

    fn inject(&self, text: &str, opts: &InjectOptions) -> InjectionOutcome {
        let process = system_state::foreground_window()
            .map(|w| w.process_name)
            .unwrap_or_default();
        injector::inject(text, &process, opts)
    }

    fn copy_to_clipboard(&self, text: &str) -> bool {
        // Copie « normale » : l'utilisateur doit pouvoir faire Ctrl+V lui-même.
        injector::set_clipboard_text(text, false)
    }

    fn quick_action(&self, action: QuickAction) -> bool {
        match action {
            QuickAction::Enter => injector::enter(),
            QuickAction::UndoLast => injector::undo(),
            QuickAction::Backspace => injector::backspace(),
        }
    }

    fn self_elevated(&self) -> bool {
        self.self_elevated
    }

    fn set_ducking(&self, on: bool, level: f32) {
        self.ducker.set(on, level);
    }

    fn play_sound(&self, sound: Sound) {
        sounds::play(sound);
    }

    fn mic_in_use_by_other_app(&self) -> bool {
        audio_session::mic_in_use_by_other_app()
    }

    fn fullscreen_exclusive(&self) -> bool {
        system_state::fullscreen_exclusive()
    }

    fn show_toast(&self, _title: &str, message: &str) {
        if let Some(o) = self.overlay.get() {
            o.update(overlay::OverlayState::Error(message.to_string()));
        }
    }

    fn gpu_info(&self) -> Option<String> {
        self.gpu.get_or_init(system_state::gpu_info).clone()
    }
}
