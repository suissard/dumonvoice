//! DumonVoice — dictée vocale universelle 100 % locale pour Windows,
//! avec compagnon mobile PWA (micro du téléphone et clavier / dictée native).
#![recursion_limit = "256"]

pub mod app;
pub mod audio;
pub mod config;
pub mod downloads;
pub mod engines;
pub mod fsm;
pub mod hotkey_keys;
pub mod platform;
pub mod protocol;
pub mod server;
#[cfg(windows)]
pub mod windows_os;

/// Bus d'événements diffusés à l'overlay et à tous les clients WebSocket.
pub type EventTx = tokio::sync::broadcast::Sender<protocol::ServerEvent>;
