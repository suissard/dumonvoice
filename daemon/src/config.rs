//! Chargement / sauvegarde atomique de `config.json` (`%APPDATA%\DumonVoice\config.json`).
//!
//! Le schéma suit la section 6.6 du cahier des charges. Tous les champs ont une valeur par
//! défaut : un fichier partiel ou ancien est donc toujours lisible.

use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APP_NAME: &str = "DumonVoice";
pub const CONFIG_VERSION: u32 = 1;

/// Répertoire de données de l'application (`%APPDATA%\DumonVoice` sous Windows).
pub fn app_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("DUMONVOICE_HOME") {
        return PathBuf::from(custom);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_NAME)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub enabled: bool,
    pub launch_at_startup: bool,
    pub onboarding_done: bool,
    pub language: String,
    pub triggers: Triggers,
    pub remote_mobile: RemoteMobile,
    pub engine: EngineConfig,
    pub audio: AudioConfig,
    pub injection: InjectionConfig,
    pub ui: UiConfig,
    pub server: ServerConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            enabled: true,
            launch_at_startup: true,
            onboarding_done: false,
            language: "fr".into(),
            triggers: Triggers::default(),
            remote_mobile: RemoteMobile::default(),
            engine: EngineConfig::default(),
            audio: AudioConfig::default(),
            injection: InjectionConfig::default(),
            ui: UiConfig::default(),
            server: ServerConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Triggers {
    pub hotkey: HotkeyConfig,
    pub wake_word: WakeWordConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HotkeyConfig {
    pub enabled: bool,
    pub keys: Vec<String>,
    pub mode: TriggerMode,
    /// En mode Toggle : arrêt automatique après `wake_word.silence_timeout_ms` de silence.
    pub toggle_auto_stop: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keys: vec!["ControlLeft".into(), "Space".into()],
            mode: TriggerMode::PushToTalk,
            toggle_auto_stop: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WakeWordConfig {
    pub enabled: bool,
    pub model: String,
    pub sensitivity: f32,
    pub silence_timeout_ms: u32,
    pub pause_during_calls: bool,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "hey_jarvis".into(),
            sensitivity: 0.65,
            silence_timeout_ms: 1500,
            pause_during_calls: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub paired_at: chrono::DateTime<chrono::Utc>,
}

impl Default for PairedDevice {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            token_hash: String::new(),
            paired_at: chrono::Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RemoteMobile {
    pub enabled: bool,
    pub allow_direct_text: bool,
    pub max_text_length: usize,
    pub lan_port: u16,
    pub mdns_name: String,
    pub paired_devices: Vec<PairedDevice>,
}

impl Default for RemoteMobile {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_direct_text: true,
            max_text_length: 20_000,
            lan_port: 18451,
            mdns_name: "dictee-pc".into(),
            paired_devices: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Whisper,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EngineConfig {
    pub active: EngineKind,
    /// Pipeline hybride : passe la sortie de Whisper dans un modèle texte Ollama.
    pub post_process_with_ollama: bool,
    pub whisper: WhisperConfig,
    pub ollama: OllamaConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            active: EngineKind::Whisper,
            post_process_with_ollama: false,
            whisper: WhisperConfig::default(),
            ollama: OllamaConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhisperProfile {
    Fast,
    Balanced,
    Accurate,
}

impl WhisperProfile {
    pub fn default_model_file(self) -> &'static str {
        match self {
            WhisperProfile::Fast => "ggml-base-q5_1.bin",
            WhisperProfile::Balanced => "ggml-small-q5_1.bin",
            WhisperProfile::Accurate => "ggml-large-v3-turbo-q5_0.bin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WhisperConfig {
    pub profile: WhisperProfile,
    pub model_file: String,
    pub use_gpu: bool,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            profile: WhisperProfile::Balanced,
            model_file: WhisperProfile::Balanced.default_model_file().into(),
            use_gpu: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OllamaApiStyle {
    /// `POST /api/chat` (API native Ollama, audio passé dans `images`).
    Native,
    /// `POST /v1/chat/completions` (compatibilité OpenAI, `input_audio`).
    Openai,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OllamaConfig {
    pub base_url: String,
    pub model: String,
    /// Modèle texte utilisé pour `refine_text` (vide = même modèle que `model`).
    pub text_model: String,
    pub api_style: OllamaApiStyle,
    pub keep_alive: String,
    pub clean_dictation: bool,
    pub custom_system_prompt: Option<String>,
    pub fallback_to_whisper: bool,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".into(),
            model: "lfm3-audio".into(),
            text_model: "lfm3".into(),
            api_style: OllamaApiStyle::Native,
            keep_alive: "30m".into(),
            clean_dictation: true,
            custom_system_prompt: None,
            fallback_to_whisper: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioConfig {
    pub pc_device_id: String,
    pub duck_other_apps: bool,
    pub duck_level: f32,
    pub sound_feedback: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            pc_device_id: "default".into(),
            duck_other_apps: true,
            duck_level: 0.15,
            sound_feedback: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InjectionMethod {
    ClipboardPaste,
    SendInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InjectionConfig {
    pub method: InjectionMethod,
    pub restore_clipboard: bool,
    pub paste_delay_ms: u64,
    pub check_window_focus: bool,
    pub custom_dictionary: std::collections::BTreeMap<String, String>,
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            method: InjectionMethod::ClipboardPaste,
            restore_clipboard: true,
            paste_delay_ms: 150,
            check_window_focus: true,
            custom_dictionary: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiConfig {
    /// `top_left | top_center | top_right | bottom_left | bottom_center | bottom_right`
    pub overlay_position: String,
    pub overlay_enabled: bool,
    /// Taille du mini-historique en RAM (0 = désactivé, max 10).
    pub keep_history_count: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            overlay_position: "bottom_center".into(),
            overlay_enabled: true,
            keep_history_count: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ServerConfig {
    pub local_port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { local_port: 18450 }
    }
}

pub const OVERLAY_POSITIONS: &[&str] = &[
    "top_left",
    "top_center",
    "top_right",
    "bottom_left",
    "bottom_center",
    "bottom_right",
];

impl Config {
    /// Valide les bornes (appelé avant toute sauvegarde issue de l'API).
    pub fn validate(&self) -> Result<(), String> {
        let ww = &self.triggers.wake_word;
        if !(800..=3000).contains(&ww.silence_timeout_ms) {
            return Err("silence_timeout_ms doit être compris entre 800 et 3000".into());
        }
        if !(0.0..=1.0).contains(&ww.sensitivity) {
            return Err("sensitivity doit être comprise entre 0 et 1".into());
        }
        if !(0.0..=1.0).contains(&self.audio.duck_level) {
            return Err("duck_level doit être compris entre 0 et 1".into());
        }
        if self.remote_mobile.max_text_length == 0 || self.remote_mobile.max_text_length > 200_000 {
            return Err("max_text_length invalide".into());
        }
        if self.remote_mobile.lan_port == self.server.local_port {
            return Err("Les ports local et LAN doivent être différents".into());
        }
        if self.remote_mobile.lan_port < 1024 || self.server.local_port < 1024 {
            return Err("Les ports doivent être >= 1024".into());
        }
        if self.injection.paste_delay_ms > 2000 {
            return Err("paste_delay_ms doit être <= 2000".into());
        }
        if self.ui.keep_history_count > 10 {
            return Err("keep_history_count doit être <= 10".into());
        }
        if !OVERLAY_POSITIONS.contains(&self.ui.overlay_position.as_str()) {
            return Err("overlay_position invalide".into());
        }
        if self.triggers.hotkey.enabled {
            crate::hotkey_keys::validate_combo(&self.triggers.hotkey.keys)?;
        }
        if !self.engine.ollama.base_url.starts_with("http://")
            && !self.engine.ollama.base_url.starts_with("https://")
        {
            return Err("base_url Ollama invalide".into());
        }
        Ok(())
    }

    /// Vue publique : sans les empreintes de jetons.
    pub fn public_view(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(devs) = v
            .pointer_mut("/remote_mobile/paired_devices")
            .and_then(|d| d.as_array_mut())
        {
            for d in devs {
                if let Some(o) = d.as_object_mut() {
                    o.remove("token_hash");
                }
            }
        }
        v
    }
}

/// Fusion JSON Merge Patch (RFC 7386).
pub fn merge_patch(target: &mut Value, patch: &Value) {
    if let Value::Object(p) = patch {
        if !target.is_object() {
            *target = Value::Object(Default::default());
        }
        let t = target.as_object_mut().expect("object");
        for (k, v) in p {
            if v.is_null() {
                t.remove(k);
            } else {
                merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    } else {
        *target = patch.clone();
    }
}

/// Stockage thread-safe de la configuration avec persistance atomique.
pub struct ConfigStore {
    path: PathBuf,
    inner: RwLock<Config>,
}

impl ConfigStore {
    pub fn load_or_default(path: PathBuf) -> Self {
        let cfg = match std::fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str::<Config>(&s) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("config.json illisible ({e}), sauvegarde et valeurs par défaut");
                    let _ = std::fs::copy(&path, path.with_extension("json.bak"));
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        };
        let store = Self {
            path,
            inner: RwLock::new(cfg),
        };
        if let Err(e) = store.save() {
            tracing::warn!("Impossible d'écrire config.json : {e}");
        }
        store
    }

    pub fn in_memory(cfg: Config) -> Self {
        Self {
            path: PathBuf::new(),
            inner: RwLock::new(cfg),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self) -> Config {
        self.inner.read().clone()
    }

    pub fn read<R>(&self, f: impl FnOnce(&Config) -> R) -> R {
        f(&self.inner.read())
    }

    /// Modification arbitraire puis sauvegarde.
    pub fn update<R>(&self, f: impl FnOnce(&mut Config) -> R) -> R {
        let r = {
            let mut g = self.inner.write();
            f(&mut g)
        };
        if let Err(e) = self.save() {
            tracing::warn!("Sauvegarde config échouée : {e}");
        }
        r
    }

    /// Applique un patch JSON partiel validé. Les appareils appairés ne peuvent pas être
    /// modifiés par cette voie (seulement via l'API d'appairage).
    pub fn patch(&self, patch: &Value) -> Result<Config, String> {
        let mut g = self.inner.write();
        let mut current = serde_json::to_value(&*g).map_err(|e| e.to_string())?;
        let mut patch = patch.clone();
        if let Some(rm) = patch.get_mut("remote_mobile").and_then(|v| v.as_object_mut()) {
            rm.remove("paired_devices");
        }
        if let Some(o) = patch.as_object_mut() {
            o.remove("version");
        }
        merge_patch(&mut current, &patch);
        let new_cfg: Config = serde_json::from_value(current).map_err(|e| e.to_string())?;
        new_cfg.validate()?;
        *g = new_cfg.clone();
        drop(g);
        self.save().map_err(|e| e.to_string())?;
        Ok(new_cfg)
    }

    /// Écriture atomique : fichier temporaire puis renommage.
    pub fn save(&self) -> std::io::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_vec_pretty(&*self.inner.read())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn parses_spec_example() {
        let s = r#"{
          "version": 1, "enabled": true, "launch_at_startup": true, "language": "fr",
          "triggers": {
            "hotkey": { "enabled": true, "keys": ["ControlLeft", "Space"], "mode": "push_to_talk" },
            "wake_word": { "enabled": false, "model": "hey_dictee", "sensitivity": 0.65,
                           "silence_timeout_ms": 1500, "pause_during_calls": true }
          },
          "remote_mobile": { "enabled": true, "allow_direct_text": true, "max_text_length": 20000,
            "lan_port": 18451, "mdns_name": "dictee-pc",
            "paired_devices": [{ "id": "dev_01", "name": "Mon Téléphone",
               "token_hash": "abc", "paired_at": "2026-11-26T10:00:00Z" }] },
          "engine": { "active": "ollama",
            "whisper": { "profile": "balanced", "model_file": "ggml-small-q5_1.bin", "use_gpu": true },
            "ollama": { "base_url": "http://127.0.0.1:11434", "model": "lfm3-audio", "keep_alive": "30m",
              "clean_dictation": true, "custom_system_prompt": null, "fallback_to_whisper": true } },
          "audio": { "pc_device_id": "default", "duck_other_apps": true, "duck_level": 0.15, "sound_feedback": true },
          "injection": { "method": "clipboard_paste", "restore_clipboard": true, "paste_delay_ms": 150,
            "check_window_focus": true, "custom_dictionary": {} },
          "ui": { "overlay_position": "bottom_center", "keep_history_count": 10 }
        }"#;
        let c: Config = serde_json::from_str(s).unwrap();
        assert_eq!(c.engine.active, EngineKind::Ollama);
        assert_eq!(c.remote_mobile.paired_devices.len(), 1);
        assert_eq!(c.triggers.hotkey.mode, TriggerMode::PushToTalk);
        c.validate().unwrap();
    }

    #[test]
    fn public_view_strips_token_hashes() {
        let mut c = Config::default();
        c.remote_mobile.paired_devices.push(PairedDevice {
            id: "d".into(),
            name: "n".into(),
            token_hash: "secret".into(),
            paired_at: chrono::Utc::now(),
        });
        let v = c.public_view();
        assert!(!v.to_string().contains("secret"));
        assert_eq!(v["remote_mobile"]["paired_devices"][0]["id"], "d");
    }

    #[test]
    fn patch_merges_and_validates() {
        let store = ConfigStore::in_memory(Config::default());
        let c = store
            .patch(&json!({"triggers": {"wake_word": {"silence_timeout_ms": 2000}}}))
            .unwrap();
        assert_eq!(c.triggers.wake_word.silence_timeout_ms, 2000);
        assert_eq!(c.triggers.hotkey.keys, vec!["ControlLeft", "Space"]);
        assert!(store
            .patch(&json!({"triggers": {"wake_word": {"silence_timeout_ms": 100}}}))
            .is_err());
        // Valeur précédente conservée après un patch invalide.
        assert_eq!(store.get().triggers.wake_word.silence_timeout_ms, 2000);
    }

    #[test]
    fn patch_cannot_inject_devices() {
        let store = ConfigStore::in_memory(Config::default());
        store
            .patch(&json!({"remote_mobile": {"paired_devices": [{"id": "x", "token_hash": "y"}]}}))
            .unwrap();
        assert!(store.get().remote_mobile.paired_devices.is_empty());
    }

    #[test]
    fn atomic_save_roundtrip() {
        let dir = std::env::temp_dir().join(format!("dv-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.json");
        let store = ConfigStore::load_or_default(path.clone());
        store.update(|c| c.language = "en".into());
        let store2 = ConfigStore::load_or_default(path);
        assert_eq!(store2.get().language, "en");
        let _ = std::fs::remove_dir_all(dir);
    }
}
