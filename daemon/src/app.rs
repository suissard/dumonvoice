//! Assemblage de l'application : configuration, orchestrateur, capture audio, moteurs,
//! serveurs, intégration native (hook clavier, overlay, tray) et application à chaud des
//! changements de configuration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

use crate::audio::capture::{self, AudioSink, CaptureCmd};
use crate::audio::vad::{self, VadModel};
use crate::audio::wake_word;
use crate::config::{self, Config, ConfigStore, EngineKind};
use crate::engines::EngineManager;
use crate::fsm::Orchestrator;
use crate::platform::Platform;
use crate::protocol::{FsmState, ServerEvent};
use crate::server::{self, auth::AuthState, tls::CertPaths, AppContext, AppControl, LanServer};
use crate::{downloads, hotkey_keys, EventTx};

pub struct App {
    pub ctx: Arc<AppContext>,
    pub quit: Arc<AtomicBool>,
    pub first_run: bool,
    #[cfg(windows)]
    pub platform: Arc<crate::windows_os::WindowsPlatform>,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

fn init_logging(app_dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_env("DUMONVOICE_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let logs = app_dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let file = tracing_appender::rolling::daily(logs, "dumonvoice.log");
    let (nb, guard) = tracing_appender::non_blocking(file);
    let res = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(nb).with_ansi(false))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .try_init();
    res.ok().map(|_| guard)
}

/// Fichier contenant le jeton local de la session (permet à une seconde instance d'ouvrir
/// les réglages de la première).
fn session_file(app_dir: &std::path::Path) -> PathBuf {
    app_dir.join("session.json")
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    crate::windows_os::system_state::open_url(url);
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn set_autostart(enabled: bool) {
    #[cfg(windows)]
    if let Err(e) = crate::windows_os::system_state::set_autostart(enabled) {
        tracing::warn!("Démarrage automatique : {e}");
    }
    let _ = enabled;
}

struct Control {
    #[cfg(windows)]
    hotkey: crate::windows_os::hotkey::HotkeyHandle,
}

impl AppControl for Control {
    fn start_hotkey_capture(&self) {
        #[cfg(windows)]
        self.hotkey.start_capture();
    }
}

/// `--autostart=0|1` (utilisé par l'installeur) : enregistre le choix sans démarrer.
pub fn apply_cli_autostart(value: bool) -> anyhow::Result<()> {
    let app_dir = config::app_dir();
    std::fs::create_dir_all(&app_dir)?;
    let cfg = ConfigStore::load_or_default(app_dir.join("config.json"));
    cfg.update(|c| c.launch_at_startup = value);
    set_autostart(value);
    Ok(())
}

/// Erreur renvoyée lorsqu'une autre instance tourne déjà.
#[derive(Debug, thiserror::Error)]
#[error("DumonVoice est déjà lancé")]
pub struct AlreadyRunning;

pub async fn start(background: bool) -> anyhow::Result<App> {
    let app_dir = config::app_dir();
    std::fs::create_dir_all(&app_dir)?;
    let log_guard = init_logging(&app_dir);
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cfg = Arc::new(ConfigStore::load_or_default(app_dir.join("config.json")));
    let first_run = !cfg.read(|c| c.onboarding_done);
    let local_port = cfg.read(|c| c.server.local_port);

    // Instance unique : le port local sert de verrou.
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", local_port)).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if let Ok(s) = std::fs::read_to_string(session_file(&app_dir)) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
                        open_url(url);
                    }
                }
            }
            return Err(AlreadyRunning.into());
        }
        Err(e) => return Err(e.into()),
    };

    let (events, _) = broadcast::channel::<ServerEvent>(1024);
    let models_dir = downloads::models_dir(&app_dir);
    std::fs::create_dir_all(&models_dir)?;

    #[cfg(windows)]
    let win_platform = Arc::new(crate::windows_os::WindowsPlatform::new());
    #[cfg(windows)]
    let platform: Arc<dyn Platform> = win_platform.clone();
    #[cfg(not(windows))]
    let platform: Arc<dyn Platform> = Arc::new(crate::platform::NullPlatform::default());

    let md = models_dir.clone();
    let vad_model = tokio::task::spawn_blocking(move || VadModel::load(&md)).await?;
    let engines = Arc::new(EngineManager::new(models_dir.clone(), events.clone()));
    let pc_name = server::tls::hostname().to_uppercase();
    let orch = Orchestrator::new(cfg.clone(), events.clone(), engines.clone(), platform.clone(), vad_model, pc_name);
    orch.spawn_watchdog();

    let (device, enabled) = cfg.read(|c| (c.audio.pc_device_id.clone(), c.enabled));
    let sink: Arc<dyn AudioSink> = orch.clone();
    let capture = capture::spawn(sink, device, enabled);

    // --- Hook clavier (Windows).
    #[cfg(windows)]
    let (hotkey, mut hook_rx) = {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (keys, on) = cfg.read(|c| (c.triggers.hotkey.keys.clone(), c.triggers.hotkey.enabled));
        let vks = hotkey_keys::parse_combo(&keys).unwrap_or_default();
        (crate::windows_os::hotkey::spawn(vks, on, tx), rx)
    };

    let control: Arc<dyn AppControl> = Arc::new(Control {
        #[cfg(windows)]
        hotkey: hotkey.clone(),
    });

    let ctx = Arc::new(AppContext {
        orch: orch.clone(),
        cfg: cfg.clone(),
        events: events.clone(),
        auth: Arc::new(AuthState::new(cfg.clone())),
        engines: engines.clone(),
        capture: capture.clone(),
        app_dir: app_dir.clone(),
        models_dir: models_dir.clone(),
        certs: CertPaths::new(&app_dir),
        control,
        connections: Default::default(),
        downloads: Default::default(),
        lan_ips: Default::default(),
        lan_running: AtomicBool::new(false),
        ollama_cache: Default::default(),
    });

    // Jeton de session pour une éventuelle seconde instance (fichier utilisateur).
    let _ = std::fs::write(
        session_file(&app_dir),
        serde_json::json!({ "url": ctx.local_url(None), "pid": std::process::id() }).to_string(),
    );

    // --- Serveurs.
    let c = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = server::run_local(c, listener).await {
            tracing::error!("Listener local arrêté : {e}");
        }
    });
    let lan: Arc<tokio::sync::Mutex<Option<LanServer>>> = Arc::new(tokio::sync::Mutex::new(None));
    restart_lan(&ctx, &lan).await;

    // --- Mot d'éveil.
    let wake = Arc::new(parking_lot::Mutex::new(None::<WakeRunner>));
    *wake.lock() = start_wake(&ctx);

    // --- Modèles : Silero VAD téléchargé automatiquement (≈ 2 Mo) s'il manque.
    if orch.vad_backend() == "energy" && cfg!(feature = "onnx") {
        let c = ctx.clone();
        tokio::spawn(async move {
            let dest = c.models_dir.join(vad::SILERO_FILE);
            if downloads::download_reported(vad::SILERO_URL, &dest, "vad:silero", &c.events).await {
                let dir = c.models_dir.clone();
                if let Ok(m) = tokio::task::spawn_blocking(move || VadModel::load(&dir)).await {
                    c.orch.set_vad_model(m);
                }
            }
        });
    }
    warm_engines(&ctx);
    set_autostart(cfg.read(|c| c.launch_at_startup));

    // --- Native : overlay HUD + consommateurs d'événements.
    let (resume_tx, mut resume_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    #[cfg(windows)]
    {
        use crate::windows_os::overlay;
        let o1 = orch.clone();
        let c1 = cfg.clone();
        let c2 = cfg.clone();
        let rtx = resume_tx.clone();
        let handle = overlay::spawn(overlay::OverlayCallbacks {
            level: Box::new(move || o1.current_level()),
            position: Box::new(move || c1.read(|c| c.ui.overlay_position.clone())),
            enabled: Box::new(move || c2.read(|c| c.ui.overlay_enabled)),
            on_resume: Box::new(move || {
                let _ = rtx.send(());
            }),
        });
        win_platform.set_overlay(handle.clone());
        spawn_overlay_driver(events.subscribe(), handle);

        // Événements du hook clavier.
        let o = orch.clone();
        let ev = events.clone();
        tokio::spawn(async move {
            use crate::windows_os::hotkey::HookEvent;
            while let Some(e) = hook_rx.recv().await {
                match e {
                    HookEvent::Down => o.on_hotkey_down(),
                    HookEvent::Up => o.on_hotkey_up(),
                    HookEvent::Escape => {
                        o.on_escape();
                    }
                    HookEvent::Captured(keys) => {
                        let (keys, valid, message) = match keys {
                            Some(k) => match hotkey_keys::validate_combo(&k) {
                                Ok(()) => (k, true, None),
                                Err(m) => (k, false, Some(m)),
                            },
                            None => (vec![], false, Some("Capture annulée".into())),
                        };
                        let _ = ev.send(ServerEvent::HotkeyCaptured { keys, valid, message });
                        let _ = ev.send(ServerEvent::ConfigChanged);
                    }
                }
            }
        });
        // L'état « écoute » pilote l'interception d'Échap.
        let mut rx = events.subscribe();
        let hk = hotkey.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ServerEvent::StateChanged { state, .. }) => hk.set_listening(state == FsmState::Listening),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        });
    }
    let _ = &resume_tx;

    // --- Sortie de veille : réinitialisation WASAPI, hooks clavier, réseau.
    {
        let c = ctx.clone();
        let lan = lan.clone();
        #[cfg(windows)]
        let hk = hotkey.clone();
        tokio::spawn(async move {
            while resume_rx.recv().await.is_some() {
                tokio::time::sleep(Duration::from_secs(2)).await;
                c.capture.send(CaptureCmd::Restart);
                #[cfg(windows)]
                hk.reinstall();
                restart_lan(&c, &lan).await;
            }
        });
    }

    // --- Application à chaud des changements de configuration.
    {
        let c = ctx.clone();
        let mut rx = events.subscribe();
        let lan = lan.clone();
        let wake = wake.clone();
        #[cfg(windows)]
        let hk = hotkey.clone();
        tokio::spawn(async move {
            let mut last = c.cfg.get();
            loop {
                match rx.recv().await {
                    Ok(ServerEvent::ConfigChanged) => {}
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
                let new = c.cfg.get();
                if new == last {
                    // Un téléchargement de mot d'éveil peut nécessiter un redémarrage.
                    if new.triggers.wake_word.enabled && wake.lock().is_none() {
                        *wake.lock() = start_wake(&c);
                    }
                    continue;
                }
                apply_config(&c, &last, &new);
                #[cfg(windows)]
                if new.triggers.hotkey != last.triggers.hotkey {
                    let vks = hotkey_keys::parse_combo(&new.triggers.hotkey.keys).unwrap_or_default();
                    hk.set_combo(&vks, new.triggers.hotkey.enabled);
                }
                if new.remote_mobile.enabled != last.remote_mobile.enabled
                    || new.remote_mobile.lan_port != last.remote_mobile.lan_port
                    || new.remote_mobile.mdns_name != last.remote_mobile.mdns_name
                {
                    restart_lan(&c, &lan).await;
                }
                if new.triggers.wake_word != last.triggers.wake_word || new.enabled != last.enabled {
                    let old = wake.lock().take();
                    if let Some(w) = old {
                        w.stop(&c);
                    }
                    *wake.lock() = start_wake(&c);
                }
                last = new;
            }
        });
    }

    let quit = Arc::new(AtomicBool::new(false));
    if first_run && !background {
        open_url(&ctx.local_url(None));
    }
    tracing::info!("DumonVoice prêt");
    Ok(App {
        ctx,
        quit,
        first_run,
        #[cfg(windows)]
        platform: win_platform,
        _log_guard: log_guard,
    })
}

fn apply_config(ctx: &Arc<AppContext>, old: &Config, new: &Config) {
    if new.enabled != old.enabled {
        ctx.orch.set_enabled(new.enabled);
        ctx.capture.send(CaptureCmd::SetActive(new.enabled));
    }
    if new.audio.pc_device_id != old.audio.pc_device_id {
        ctx.capture.send(CaptureCmd::SetDevice(new.audio.pc_device_id.clone()));
    }
    if new.launch_at_startup != old.launch_at_startup {
        set_autostart(new.launch_at_startup);
    }
    if new.engine.whisper != old.engine.whisper {
        ctx.engines.whisper.unload();
    }
    if new.engine != old.engine {
        warm_engines(ctx);
    }
}

/// Préchargement du moteur actif (évite la latence de la première dictée).
fn warm_engines(ctx: &Arc<AppContext>) {
    let cfg = ctx.cfg.get();
    match cfg.engine.active {
        EngineKind::Whisper => {
            let eng = ctx.engines.whisper.clone();
            tokio::task::spawn_blocking(move || eng.preload(&cfg));
        }
        EngineKind::Ollama => {
            let eng = ctx.engines.ollama.clone();
            tokio::spawn(async move { eng.warmup(&cfg.engine.ollama).await });
        }
    }
}

async fn restart_lan(ctx: &Arc<AppContext>, lan: &tokio::sync::Mutex<Option<LanServer>>) {
    let mut g = lan.lock().await;
    if let Some(s) = g.take() {
        s.stop();
        ctx.lan_running.store(false, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if !ctx.cfg.read(|c| c.remote_mobile.enabled) {
        return;
    }
    match server::start_lan(ctx.clone()).await {
        Ok(s) => *g = Some(s),
        Err(e) => {
            tracing::error!("Compagnon mobile indisponible : {e}");
            let _ = ctx.events.send(ServerEvent::notice(
                "error",
                "LAN_START_FAILED",
                format!("Le serveur mobile n'a pas pu démarrer : {e}"),
            ));
        }
    }
}

// ---------------------------------------------------------------------- Mot d'éveil

pub struct WakeRunner {
    stop: Arc<AtomicBool>,
}

impl WakeRunner {
    fn stop(self, ctx: &AppContext) {
        self.stop.store(true, Ordering::SeqCst);
        ctx.orch.set_wake_sender(None);
    }
}

#[cfg(not(feature = "onnx"))]
fn start_wake(ctx: &Arc<AppContext>) -> Option<WakeRunner> {
    if ctx.cfg.read(|c| c.enabled && c.triggers.wake_word.enabled) {
        tracing::warn!("Mot d'éveil indisponible : compilé sans la feature `onnx`");
    }
    None
}

#[cfg(feature = "onnx")]
fn start_wake(ctx: &Arc<AppContext>) -> Option<WakeRunner> {
    let cfg = ctx.cfg.get();
    if !cfg.enabled || !cfg.triggers.wake_word.enabled {
        return None;
    }
    {
        let word = cfg.triggers.wake_word.model.clone();
        let files = wake_word::required_files(&ctx.models_dir, &word);
        let missing: Vec<_> = files.into_iter().filter(|(p, _)| !p.exists()).collect();
        if !missing.is_empty() {
            // Téléchargement automatique des modèles officiels, puis redémarrage.
            let label = format!("wakeword:{word}");
            if ctx.downloads.lock().insert(label.clone()) {
                let c = ctx.clone();
                tokio::spawn(async move {
                    let mut ok = true;
                    for (path, url) in missing {
                        match url {
                            Some(u) => ok &= downloads::download_reported(&u, &path, &label, &c.events).await,
                            None => ok = false,
                        }
                    }
                    c.downloads.lock().remove(&label);
                    if ok {
                        let _ = c.events.send(ServerEvent::ConfigChanged);
                    } else {
                        let _ = c.events.send(ServerEvent::notice(
                            "warn",
                            "WAKEWORD_MISSING",
                            "Modèle de mot d'éveil absent : importez un fichier .onnx",
                        ));
                    }
                });
            }
            return None;
        }
        let models = ctx.models_dir.clone();
        let threshold = wake_word::threshold_from_sensitivity(cfg.triggers.wake_word.sensitivity);
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let orch = ctx.orch.clone();
        let spawned = std::thread::Builder::new()
            .name("wake-word".into())
            .spawn(move || {
                let mut det = match wake_word::WakeWordDetector::load(&models, &word) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("Mot d'éveil « {word} » : {e}");
                        return;
                    }
                };
                tracing::info!("Mot d'éveil actif : « {word} » (seuil {threshold:.2})");
                let mut cooldown = std::time::Instant::now();
                loop {
                    if st.load(Ordering::SeqCst) {
                        break;
                    }
                    match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(samples) => match det.feed(&samples) {
                            Ok(score) if score >= threshold && cooldown.elapsed() > Duration::from_secs(2) => {
                                tracing::info!("Mot d'éveil détecté (score {score:.2})");
                                cooldown = std::time::Instant::now();
                                det.reset();
                                orch.on_wake_word();
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!("Inférence mot d'éveil : {e}");
                            }
                        },
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        if spawned.is_err() {
            return None;
        }
        ctx.orch.set_wake_sender(Some(tx));
        Some(WakeRunner { stop })
    }
}

// ---------------------------------------------------------------------- Overlay

#[cfg(windows)]
fn spawn_overlay_driver(mut rx: broadcast::Receiver<ServerEvent>, handle: crate::windows_os::overlay::OverlayHandle) {
    use crate::protocol::{ErrorCode, SourceKind};
    use crate::windows_os::overlay::{OverlayState, SourceIcon};
    tokio::spawn(async move {
        let mut current = OverlayState::Hidden;
        loop {
            let ev = match rx.recv().await {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let next = match ev {
                ServerEvent::StateChanged { state, source, .. } => match state {
                    FsmState::Listening => Some(OverlayState::Listening(match source {
                        Some(SourceKind::RemoteAudio) => SourceIcon::MobileMic,
                        Some(SourceKind::RemoteText) => SourceIcon::MobileKeyboard,
                        _ => SourceIcon::PcMic,
                    })),
                    FsmState::Transcribing => Some(OverlayState::Processing(
                        if source == Some(SourceKind::RemoteText) { "Nettoyage IA…" } else { "Transcription…" }.into(),
                    )),
                    FsmState::Injecting => Some(if source == Some(SourceKind::RemoteText) {
                        OverlayState::Listening(SourceIcon::MobileKeyboard)
                    } else {
                        OverlayState::Processing("Insertion…".into())
                    }),
                    FsmState::Idle | FsmState::Paused => match current {
                        OverlayState::Listening(_) | OverlayState::Processing(_) => Some(OverlayState::Hidden),
                        _ => None,
                    },
                    FsmState::Error => None,
                },
                ServerEvent::InjectionResult(r) => Some(if r.injected {
                    OverlayState::Success(match r.target_window {
                        Some(w) if !w.is_empty() => format!("Inséré · {w}"),
                        _ => "Inséré".into(),
                    })
                } else {
                    OverlayState::Error("Texte copié : faites Ctrl+V".into())
                }),
                ServerEvent::Error { code, message, .. } => {
                    if code == ErrorCode::EmptyAudio {
                        Some(OverlayState::Hidden)
                    } else {
                        Some(OverlayState::Error(message))
                    }
                }
                ServerEvent::Notice { code, message, .. } if code == "model_loading" => {
                    Some(OverlayState::Processing(message))
                }
                _ => None,
            };
            if let Some(n) = next {
                current = n.clone();
                handle.update(n);
            }
        }
    });
}

impl App {
    /// Boucle native bloquante (System Tray) — thread principal.
    #[cfg(windows)]
    pub fn run_tray(&self) -> anyhow::Result<()> {
        use crate::windows_os::tray::{self, TrayCallbacks, TrayView};
        let c = self.ctx.clone();
        let view = {
            let c = c.clone();
            move || {
                let snap = c.orch.snapshot();
                TrayView {
                    enabled: snap.state != FsmState::Paused,
                    listening: snap.state == FsmState::Listening,
                    busy: matches!(snap.state, FsmState::Transcribing | FsmState::Injecting),
                    engine: c.cfg.read(|x| x.engine.active),
                    phone_connected: !c.connected_devices().is_empty(),
                }
            }
        };
        let c1 = c.clone();
        let c2 = c.clone();
        let c3 = c.clone();
        let c4 = c.clone();
        let c5 = c.clone();
        let quit = self.quit.clone();
        tray::run(TrayCallbacks {
            view: Box::new(view),
            open_settings: Box::new(move || open_url(&c1.local_url(None))),
            show_qr: Box::new(move || open_url(&c2.local_url(Some("mobile")))),
            set_enabled: Box::new(move |on| {
                c3.cfg.update(|x| x.enabled = on);
                let _ = c3.events.send(ServerEvent::ConfigChanged);
            }),
            set_engine: Box::new(move |k| {
                c4.cfg.update(|x| x.engine.active = k);
                let _ = c4.events.send(ServerEvent::ConfigChanged);
            }),
            copy_last: Box::new(move || {
                if let Some(t) = c5.orch.last_text() {
                    c5.orch.platform().copy_to_clipboard(&t);
                }
            }),
            should_quit: Box::new(move || quit.load(Ordering::SeqCst)),
        })
    }

    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::SeqCst);
        self.ctx.capture.send(CaptureCmd::Shutdown);
        let _ = std::fs::remove_file(session_file(&self.ctx.app_dir));
        tracing::info!("Arrêt de DumonVoice");
    }
}
