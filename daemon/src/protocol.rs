//! Contrat de messages WebSocket / REST (section 6.4 du cahier des charges).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FsmState {
    Paused,
    Idle,
    Listening,
    Transcribing,
    Injecting,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    LocalMic,
    RemoteAudio,
    RemoteText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    LocalMic,
    RemoteAudio { device_id: String },
    RemoteText { device_id: String },
}

impl Source {
    pub fn kind(&self) -> SourceKind {
        match self {
            Source::LocalMic => SourceKind::LocalMic,
            Source::RemoteAudio { .. } => SourceKind::RemoteAudio,
            Source::RemoteText { .. } => SourceKind::RemoteText,
        }
    }

    pub fn device_id(&self) -> Option<&str> {
        match self {
            Source::LocalMic => None,
            Source::RemoteAudio { device_id } | Source::RemoteText { device_id } => Some(device_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Busy,
    TextTooLarge,
    OllamaUnreachable,
    AdminWindowFallback,
    EmptyAudio,
    Timeout,
    Paused,
    DirectTextDisabled,
    Unauthorized,
    BadRequest,
    EngineError,
    NoMicrophone,
    InjectionFailed,
}

impl ErrorCode {
    pub fn default_message(self) -> &'static str {
        match self {
            ErrorCode::Busy => "Une dictée est déjà en cours sur le PC.",
            ErrorCode::TextTooLarge => "Texte trop long.",
            ErrorCode::OllamaUnreachable => "Ollama est injoignable.",
            ErrorCode::AdminWindowFallback => {
                "Fenêtre Admin détectée : texte copié, faites Ctrl+V"
            }
            ErrorCode::EmptyAudio => "Aucune parole détectée.",
            ErrorCode::Timeout => "Délai dépassé.",
            ErrorCode::Paused => "La dictée est en pause sur le PC.",
            ErrorCode::DirectTextDisabled => "L'envoi de texte direct est désactivé sur le PC.",
            ErrorCode::Unauthorized => "Appareil non autorisé.",
            ErrorCode::BadRequest => "Requête invalide.",
            ErrorCode::EngineError => "Erreur du moteur de transcription.",
            ErrorCode::NoMicrophone => "Aucun micro disponible sur le PC.",
            ErrorCode::InjectionFailed => "L'injection du texte a échoué.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioFormat {
    pub encoding: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InjectTextRequest {
    pub message_id: Uuid,
    pub text: String,
    #[serde(default)]
    pub press_enter_after: bool,
    #[serde(default)]
    pub refine_with_ollama: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickAction {
    Enter,
    UndoLast,
    Backspace,
}

/// Messages Client → Daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Auth {
        #[serde(default)]
        token: String,
        #[serde(default)]
        client_kind: String,
    },
    SessionStart {
        session_id: Uuid,
        #[serde(default)]
        audio_format: Option<AudioFormat>,
        #[serde(default)]
        trigger_mode: Option<String>,
        #[serde(default)]
        auto_stop_vad: bool,
    },
    SessionStop {
        session_id: Uuid,
    },
    SessionCancel {
        session_id: Uuid,
    },
    InjectText(InjectTextRequest),
    Action {
        action: QuickAction,
    },
    Ping {
        #[serde(default)]
        ts: i64,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InjectionResult {
    pub id: Uuid,
    pub source: SourceKind,
    pub text: String,
    pub injected: bool,
    pub injection_note: Option<String>,
    pub target_window: Option<String>,
    pub latency_ms: u64,
}

/// Messages Daemon → Clients.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    AuthOk {
        pc_name: String,
        state: FsmState,
        device_id: Option<String>,
    },
    StateChanged {
        state: FsmState,
        source: Option<SourceKind>,
        session_id: Option<Uuid>,
        target_window: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_code: Option<ErrorCode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    AudioLevel {
        rms: f32,
    },
    InjectionResult(InjectionResult),
    Error {
        id: Option<Uuid>,
        code: ErrorCode,
        message: String,
    },
    Pong {
        ts: i64,
    },
    /// Notification courte (toast PC + mobile) : chargement de modèle, repli, etc.
    Notice {
        level: String,
        code: String,
        message: String,
    },
    DownloadProgress {
        model: String,
        downloaded: u64,
        total: Option<u64>,
        done: bool,
        error: Option<String>,
    },
    HotkeyCaptured {
        keys: Vec<String>,
        valid: bool,
        message: Option<String>,
    },
    ConfigChanged,
    DevicesChanged,
    /// Interne : un appareil révoqué doit être déconnecté (non diffusé aux clients).
    #[serde(skip)]
    DeviceRevoked {
        device_id: String,
    },
}

impl ServerEvent {
    pub fn error(id: Option<Uuid>, code: ErrorCode) -> Self {
        ServerEvent::Error {
            id,
            code,
            message: code.default_message().to_string(),
        }
    }

    pub fn notice(level: &str, code: &str, message: impl Into<String>) -> Self {
        ServerEvent::Notice {
            level: level.into(),
            code: code.into(),
            message: message.into(),
        }
    }

    /// Événements réservés à la PWA Desktop (vumètre PC, capture de touche).
    pub fn desktop_only(&self) -> bool {
        matches!(
            self,
            ServerEvent::AudioLevel { .. }
                | ServerEvent::HotkeyCaptured { .. }
                | ServerEvent::DownloadProgress { .. }
        )
    }

    pub fn is_internal(&self) -> bool {
        matches!(self, ServerEvent::DeviceRevoked { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_spec_client_messages() {
        let m: ClientMessage = serde_json::from_str(
            r#"{ "type": "auth", "token": "abc", "client_kind": "mobile" }"#,
        )
        .unwrap();
        assert!(matches!(m, ClientMessage::Auth { .. }));
        let m: ClientMessage = serde_json::from_str(
            r#"{"type": "session_start","session_id": "550e8400-e29b-41d4-a716-446655440000",
                "audio_format": { "encoding": "pcm_s16le", "sample_rate": 16000, "channels": 1 },
                "trigger_mode": "push_to_talk","auto_stop_vad": false}"#,
        )
        .unwrap();
        assert!(matches!(m, ClientMessage::SessionStart { .. }));
        let m: ClientMessage = serde_json::from_str(
            r#"{"type": "inject_text","message_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
                "text": "Bonjour 👍","press_enter_after": false,"refine_with_ollama": false}"#,
        )
        .unwrap();
        match m {
            ClientMessage::InjectText(r) => assert_eq!(r.text, "Bonjour 👍"),
            _ => panic!(),
        }
        let m: ClientMessage = serde_json::from_str(r#"{ "type": "ping", "ts": 1732150000000 }"#).unwrap();
        assert_eq!(m, ClientMessage::Ping { ts: 1732150000000 });
    }

    #[test]
    fn serialize_server_events() {
        let e = ServerEvent::InjectionResult(InjectionResult {
            id: Uuid::nil(),
            source: SourceKind::RemoteText,
            text: "x".into(),
            injected: true,
            injection_note: None,
            target_window: Some("Visual Studio Code".into()),
            latency_ms: 28,
        });
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "injection_result");
        assert_eq!(v["source"], "remote_text");
        assert_eq!(v["latency_ms"], 28);
        let e = ServerEvent::error(None, ErrorCode::Busy);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "BUSY");
        let e = ServerEvent::StateChanged {
            state: FsmState::Injecting,
            source: Some(SourceKind::RemoteText),
            session_id: None,
            target_window: None,
            error_code: None,
            detail: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["state"], "INJECTING");
        assert!(v.get("error_code").is_none());
    }
}
