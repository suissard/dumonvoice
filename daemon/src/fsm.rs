//! Orchestrateur : machine à états finis, verrou exclusif de source, cache d'idempotence,
//! watchdog de flux audio distant, mini-historique (section 6.1 du cahier des charges).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tokio::time::Instant;
use uuid::Uuid;

use crate::audio::capture::AudioSink;
use crate::audio::vad::{SilenceTracker, VadModel, MIN_SPEECH_MS};
use crate::audio::{decode_s16le, ms_to_samples, rms_i16, RingBuffer, PREROLL_MS, RING_MS};
use crate::config::{ConfigStore, TriggerMode};
use crate::engines::{EngineError, SpeechPipeline};
use crate::platform::{effective_method, InjectOptions, Platform, Sound, WindowInfo};
use crate::protocol::{
    ErrorCode, FsmState, InjectTextRequest, InjectionResult, QuickAction, ServerEvent, Source, SourceKind,
};
use crate::EventTx;

/// Watchdog : durée maximale sans paquet audio distant pendant `LISTENING`.
pub const REMOTE_STREAM_TIMEOUT: Duration = Duration::from_secs(2);
/// Durée de rétention du cache d'idempotence.
pub const IDEMPOTENCE_TTL: Duration = Duration::from_secs(60);
/// Durée d'affichage de l'état `ERROR` avant retour à `IDLE`.
pub const ERROR_DISPLAY: Duration = Duration::from_secs(3);
/// Durée maximale d'une dictée (sécurité mémoire).
pub const MAX_SESSION: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub source: SourceKind,
    pub text: String,
    pub injected: bool,
    pub target_window: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub state: FsmState,
    pub source: Option<SourceKind>,
    pub session_id: Option<Uuid>,
    pub target_window: Option<String>,
    pub error_code: Option<ErrorCode>,
}

struct Inner {
    state: FsmState,
    source: Option<Source>,
    session_id: Option<Uuid>,
    target: Option<WindowInfo>,
    started: Option<Instant>,
    ring: RingBuffer,
    recording: Vec<i16>,
    last_packet: Instant,
    tracker_pos: usize,
    error_code: Option<ErrorCode>,
    ducked: bool,
    error_gen: u64,
}

enum Idem {
    InProgress,
    Done(InjectionResult),
}

pub struct Orchestrator {
    pub cfg: Arc<ConfigStore>,
    pub events: EventTx,
    pipeline: Arc<dyn SpeechPipeline>,
    platform: Arc<dyn Platform>,
    vad_model: RwLock<VadModel>,
    inner: Mutex<Inner>,
    tracker: Mutex<Option<SilenceTracker>>,
    idem: Mutex<HashMap<Uuid, (Instant, Idem)>>,
    history: Mutex<VecDeque<HistoryEntry>>,
    last_level: Mutex<Instant>,
    wake_tx: Mutex<Option<std::sync::mpsc::SyncSender<Vec<i16>>>>,
    desktop_clients: AtomicI64,
    level: std::sync::atomic::AtomicU32,
    rt: tokio::runtime::Handle,
    pub pc_name: String,
}

fn engine_error_code(e: &EngineError) -> ErrorCode {
    match e {
        EngineError::Unreachable(_) => ErrorCode::OllamaUnreachable,
        EngineError::Timeout => ErrorCode::Timeout,
        EngineError::ModelMissing(_) | EngineError::Failed(_) => ErrorCode::EngineError,
    }
}

impl Orchestrator {
    pub fn new(
        cfg: Arc<ConfigStore>,
        events: EventTx,
        pipeline: Arc<dyn SpeechPipeline>,
        platform: Arc<dyn Platform>,
        vad_model: VadModel,
        pc_name: String,
    ) -> Arc<Self> {
        let enabled = cfg.read(|c| c.enabled);
        Arc::new(Self {
            cfg,
            events,
            pipeline,
            platform,
            vad_model: RwLock::new(vad_model),
            inner: Mutex::new(Inner {
                state: if enabled { FsmState::Idle } else { FsmState::Paused },
                source: None,
                session_id: None,
                target: None,
                started: None,
                ring: RingBuffer::with_ms(RING_MS),
                recording: Vec::new(),
                last_packet: Instant::now(),
                tracker_pos: 0,
                error_code: None,
                ducked: false,
                error_gen: 0,
            }),
            tracker: Mutex::new(None),
            idem: Mutex::new(HashMap::new()),
            history: Mutex::new(VecDeque::new()),
            last_level: Mutex::new(Instant::now()),
            wake_tx: Mutex::new(None),
            desktop_clients: AtomicI64::new(0),
            level: std::sync::atomic::AtomicU32::new(0),
            rt: tokio::runtime::Handle::current(),
            pc_name,
        })
    }

    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.platform
    }

    /// Dernier niveau audio RMS (overlay HUD).
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn set_vad_model(&self, m: VadModel) {
        *self.vad_model.write() = m;
    }

    pub fn vad_backend(&self) -> &'static str {
        self.vad_model.read().backend_name()
    }

    pub fn set_wake_sender(&self, tx: Option<std::sync::mpsc::SyncSender<Vec<i16>>>) {
        *self.wake_tx.lock() = tx;
    }

    pub fn desktop_client_connected(&self, delta: i64) {
        self.desktop_clients.fetch_add(delta.signum(), Ordering::SeqCst);
    }

    fn emit(&self, e: ServerEvent) {
        let _ = self.events.send(e);
    }

    fn emit_state(&self, g: &Inner) {
        self.emit(ServerEvent::StateChanged {
            state: g.state,
            source: g.source.as_ref().map(|s| s.kind()),
            session_id: g.session_id,
            target_window: g.target.as_ref().map(|t| t.display_name()),
            error_code: g.error_code,
            detail: None,
        });
    }

    pub fn snapshot(&self) -> Snapshot {
        let g = self.inner.lock();
        Snapshot {
            state: g.state,
            source: g.source.as_ref().map(|s| s.kind()),
            session_id: g.session_id,
            target_window: g.target.as_ref().map(|t| t.display_name()),
            error_code: g.error_code,
        }
    }

    pub fn state(&self) -> FsmState {
        self.inner.lock().state
    }

    fn sound(&self, s: Sound) {
        if self.cfg.read(|c| c.audio.sound_feedback) {
            self.platform.play_sound(s);
        }
    }

    fn set_ducking(&self, on: bool) {
        let (enabled, level) = self.cfg.read(|c| (c.audio.duck_other_apps, c.audio.duck_level));
        {
            let mut g = self.inner.lock();
            if on && (!enabled || g.ducked) {
                return;
            }
            if !on && !g.ducked {
                return;
            }
            g.ducked = on;
        }
        let p = self.platform.clone();
        self.rt.spawn_blocking(move || p.set_ducking(on, level));
    }

    // ------------------------------------------------------------------ Pause

    pub fn set_enabled(self: &Arc<Self>, enabled: bool) {
        if !enabled {
            self.cancel(None);
        }
        let mut g = self.inner.lock();
        match (enabled, g.state) {
            (false, _) => {
                g.state = FsmState::Paused;
                g.source = None;
                g.session_id = None;
            }
            (true, FsmState::Paused) => g.state = FsmState::Idle,
            _ => return,
        }
        self.emit_state(&g);
        drop(g);
        self.cfg.update(|c| c.enabled = enabled);
    }

    // ------------------------------------------------------------------ Flux voix

    /// `IDLE → LISTENING` : verrouille la source, capture la fenêtre cible.
    pub fn begin_listening(self: &Arc<Self>, source: Source, session_id: Uuid, auto_stop: bool) -> Result<Uuid, ErrorCode> {
        let target = self.platform.foreground_window();
        let silence_ms = self.cfg.read(|c| c.triggers.wake_word.silence_timeout_ms);
        {
            let mut g = self.inner.lock();
            match g.state {
                FsmState::Paused => return Err(ErrorCode::Paused),
                FsmState::Idle | FsmState::Error => {}
                _ => return Err(ErrorCode::Busy),
            }
            if g.source.is_some() {
                return Err(ErrorCode::Busy);
            }
            let now = Instant::now();
            g.recording = if source == Source::LocalMic {
                g.ring.last(ms_to_samples(PREROLL_MS))
            } else {
                Vec::new()
            };
            g.state = FsmState::Listening;
            g.source = Some(source);
            g.session_id = Some(session_id);
            g.target = target;
            g.started = Some(now);
            g.last_packet = now;
            g.tracker_pos = 0;
            g.error_code = None;
            self.emit_state(&g);
        }
        *self.tracker.lock() = auto_stop.then(|| SilenceTracker::new(self.vad_model.read().instance(), silence_ms));
        self.set_ducking(true);
        self.sound(Sound::Start);
        Ok(session_id)
    }

    /// `LISTENING → IDLE` sans rien injecter.
    pub fn cancel(self: &Arc<Self>, session_id: Option<Uuid>) -> bool {
        {
            let mut g = self.inner.lock();
            if g.state != FsmState::Listening {
                return false;
            }
            if session_id.is_some() && g.session_id != session_id {
                return false;
            }
            g.recording.clear();
            g.state = FsmState::Idle;
            g.source = None;
            g.session_id = None;
            g.target = None;
            self.emit_state(&g);
        }
        *self.tracker.lock() = None;
        self.set_ducking(false);
        true
    }

    /// `LISTENING → TRANSCRIBING` puis traitement asynchrone.
    pub fn finish(self: &Arc<Self>, session_id: Option<Uuid>) -> bool {
        let (pcm, source, sid, target) = {
            let mut g = self.inner.lock();
            if g.state != FsmState::Listening {
                return false;
            }
            if session_id.is_some() && g.session_id != session_id {
                return false;
            }
            g.state = FsmState::Transcribing;
            self.emit_state(&g);
            (
                std::mem::take(&mut g.recording),
                g.source.clone().expect("source"),
                g.session_id.expect("session"),
                g.target.clone(),
            )
        };
        *self.tracker.lock() = None;
        self.set_ducking(false);
        self.sound(Sound::Stop);
        let this = self.clone();
        self.rt.spawn(async move {
            this.process_audio(pcm, source, sid, target).await;
        });
        true
    }

    async fn process_audio(self: Arc<Self>, pcm: Vec<i16>, source: Source, sid: Uuid, target: Option<WindowInfo>) {
        let t0 = Instant::now();
        let mut vad = self.vad_model.read().instance();
        let (speech, pcm) = tokio::task::spawn_blocking(move || {
            let s = vad.speech_ms(&pcm);
            (s, pcm)
        })
        .await
        .unwrap_or((0, Vec::new()));
        if speech < MIN_SPEECH_MS {
            tracing::debug!("Parole effective {speech} ms : ignorée");
            self.emit(ServerEvent::error(Some(sid), ErrorCode::EmptyAudio));
            self.to_idle(sid);
            return;
        }
        let cfg = self.cfg.get();
        match self.pipeline.transcribe(pcm, &cfg).await {
            Ok(text) if !text.trim().is_empty() => {
                self.inject_final(sid, source, text, target, t0, false).await;
            }
            Ok(_) => {
                self.emit(ServerEvent::error(Some(sid), ErrorCode::EmptyAudio));
                self.to_idle(sid);
            }
            Err(e) => {
                tracing::warn!("Transcription échouée : {e}");
                let code = engine_error_code(&e);
                self.enter_error(Some(sid), code, Some(e.to_string()));
            }
        }
    }

    fn to_idle(&self, sid: Uuid) {
        let mut g = self.inner.lock();
        if g.session_id == Some(sid) && g.state != FsmState::Paused {
            g.state = FsmState::Idle;
            g.source = None;
            g.session_id = None;
            g.target = None;
            g.error_code = None;
            self.emit_state(&g);
        }
    }

    /// État `ERROR(code)` transitoire (3 s) puis retour automatique à `IDLE`.
    pub fn enter_error(self: &Arc<Self>, id: Option<Uuid>, code: ErrorCode, detail: Option<String>) {
        let gen = {
            let mut g = self.inner.lock();
            if g.state == FsmState::Paused {
                return;
            }
            g.state = FsmState::Error;
            g.source = None;
            g.session_id = None;
            g.target = None;
            g.error_code = Some(code);
            g.error_gen += 1;
            self.emit(ServerEvent::StateChanged {
                state: FsmState::Error,
                source: None,
                session_id: id,
                target_window: None,
                error_code: Some(code),
                detail: detail.clone(),
            });
            g.error_gen
        };
        self.emit(ServerEvent::Error {
            id,
            code,
            message: detail.unwrap_or_else(|| code.default_message().to_string()),
        });
        self.sound(Sound::Error);
        let this = self.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(ERROR_DISPLAY).await;
            let mut g = this.inner.lock();
            if g.state == FsmState::Error && g.error_gen == gen {
                g.state = FsmState::Idle;
                g.error_code = None;
                this.emit_state(&g);
            }
        });
    }

    /// `INJECTING` : insertion dans la fenêtre capturée, ACK, restauration du presse-papiers.
    async fn inject_final(
        self: &Arc<Self>,
        id: Uuid,
        source: Source,
        text: String,
        target: Option<WindowInfo>,
        t0: Instant,
        press_enter_after: bool,
    ) -> InjectionResult {
        {
            let mut g = self.inner.lock();
            g.state = FsmState::Injecting;
            g.session_id = Some(id);
            g.source = Some(source.clone());
            g.target = target.clone();
            self.emit_state(&g);
        }
        let cfg = self.cfg.get();
        let platform = self.platform.clone();
        let opts = InjectOptions {
            method: effective_method(cfg.injection.method, &text),
            press_enter_after,
            restore_clipboard: cfg.injection.restore_clipboard,
            paste_delay_ms: cfg.injection.paste_delay_ms,
        };
        let check_focus = cfg.injection.check_window_focus;
        let t = text.clone();
        let tgt = target.clone();
        let (injected, note, restore) = tokio::task::spawn_blocking(move || {
            let fg = platform.foreground_window();
            if check_focus {
                if let (Some(t0w), Some(fgw)) = (tgt.as_ref(), fg.as_ref()) {
                    if t0w.hwnd != fgw.hwnd {
                        platform.copy_to_clipboard(&t);
                        return (false, Some("FOCUS_CHANGED_CLIPBOARD_ONLY".to_string()), None);
                    }
                }
            }
            if fg.as_ref().map(|w| w.elevated).unwrap_or(false) && !platform.self_elevated() {
                platform.copy_to_clipboard(&t);
                return (false, Some("ADMIN_WINDOW_CLIPBOARD_ONLY".to_string()), None);
            }
            let out = platform.inject(&t, &opts);
            if !out.injected && out.note.as_deref() == Some("UIPI_BLOCKED") {
                platform.copy_to_clipboard(&t);
                return (false, Some("ADMIN_WINDOW_CLIPBOARD_ONLY".to_string()), None);
            }
            (out.injected, out.note, out.restore)
        })
        .await
        .unwrap_or((false, Some("INJECTION_PANIC".into()), None));

        match note.as_deref() {
            Some("ADMIN_WINDOW_CLIPBOARD_ONLY") => {
                let msg = ErrorCode::AdminWindowFallback.default_message();
                self.emit(ServerEvent::notice("warn", "ADMIN_WINDOW_FALLBACK", msg));
                self.platform.show_toast("DumonVoice", msg);
            }
            Some("FOCUS_CHANGED_CLIPBOARD_ONLY") => {
                let msg = "La fenêtre active a changé : texte copié dans le presse-papiers.";
                self.emit(ServerEvent::notice("warn", "FOCUS_CHANGED", msg));
                self.platform.show_toast("DumonVoice", msg);
            }
            _ => {}
        }

        let result = InjectionResult {
            id,
            source: source.kind(),
            text: text.clone(),
            injected,
            injection_note: note,
            target_window: target.as_ref().map(|t| t.display_name()),
            latency_ms: t0.elapsed().as_millis() as u64,
        };
        self.push_history(&result);
        self.emit(ServerEvent::InjectionResult(result.clone()));
        if !injected && result.injection_note.is_none() {
            self.sound(Sound::Error);
        }
        if let Some(r) = restore {
            let _ = tokio::task::spawn_blocking(r).await;
        }
        self.to_idle(id);
        result
    }

    fn push_history(&self, r: &InjectionResult) {
        let keep = self.cfg.read(|c| c.ui.keep_history_count);
        let mut h = self.history.lock();
        if keep == 0 {
            h.clear();
            return;
        }
        h.push_front(HistoryEntry {
            id: r.id,
            source: r.source,
            text: r.text.clone(),
            injected: r.injected,
            target_window: r.target_window.clone(),
            at: chrono::Utc::now(),
        });
        h.truncate(keep);
    }

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.history.lock().iter().cloned().collect()
    }

    pub fn last_text(&self) -> Option<String> {
        self.history.lock().front().map(|h| h.text.clone())
    }

    // ------------------------------------------------------------------ Déclencheurs PC

    pub fn on_hotkey_down(self: &Arc<Self>) {
        let cfg = self.cfg.get();
        if !cfg.triggers.hotkey.enabled {
            return;
        }
        let listening_local = {
            let g = self.inner.lock();
            g.state == FsmState::Listening && g.source == Some(Source::LocalMic)
        };
        match cfg.triggers.hotkey.mode {
            TriggerMode::PushToTalk => {
                if !listening_local {
                    self.start_local(false);
                }
            }
            TriggerMode::Toggle => {
                if listening_local {
                    self.finish(None);
                } else {
                    self.start_local(cfg.triggers.hotkey.toggle_auto_stop);
                }
            }
        }
    }

    pub fn on_hotkey_up(self: &Arc<Self>) {
        if self.cfg.read(|c| c.triggers.hotkey.mode) == TriggerMode::PushToTalk {
            let local = self.inner.lock().source == Some(Source::LocalMic);
            if local {
                self.finish(None);
            }
        }
    }

    /// `Échap` pendant l'écoute : annulation. Renvoie `true` si la touche a été consommée.
    pub fn on_escape(self: &Arc<Self>) -> bool {
        self.cancel(None)
    }

    pub fn on_wake_word(self: &Arc<Self>) {
        if self.state() != FsmState::Idle {
            return;
        }
        if self.cfg.read(|c| c.triggers.wake_word.pause_during_calls) && self.platform.mic_in_use_by_other_app() {
            tracing::info!("Mot d'éveil ignoré : appel visio en cours");
            return;
        }
        self.start_local(true);
    }

    fn start_local(self: &Arc<Self>, auto_stop: bool) {
        match self.begin_listening(Source::LocalMic, Uuid::new_v4(), auto_stop) {
            Ok(_) => {}
            Err(ErrorCode::Paused) => {}
            Err(code) => {
                self.sound(Sound::Error);
                self.emit(ServerEvent::notice("warn", "BUSY", code.default_message()));
            }
        }
    }

    // ------------------------------------------------------------------ Audio distant

    pub fn remote_session_start(self: &Arc<Self>, device_id: &str, session_id: Uuid, auto_stop: bool) -> Result<(), ErrorCode> {
        self.begin_listening(
            Source::RemoteAudio {
                device_id: device_id.to_string(),
            },
            session_id,
            auto_stop,
        )
        .map(|_| ())
    }

    pub fn remote_audio(self: &Arc<Self>, device_id: &str, bytes: &[u8]) {
        let mut g = self.inner.lock();
        if g.state != FsmState::Listening {
            return;
        }
        match &g.source {
            Some(Source::RemoteAudio { device_id: d }) if d == device_id => {}
            _ => return,
        }
        let samples = decode_s16le(bytes);
        g.recording.extend_from_slice(&samples);
        g.last_packet = Instant::now();
        self.level.store(rms_i16(&samples).to_bits(), Ordering::Relaxed);
    }

    fn owns_remote(&self, device_id: &str, session_id: Option<Uuid>) -> bool {
        let g = self.inner.lock();
        matches!(&g.source, Some(Source::RemoteAudio { device_id: d }) if d == device_id)
            && (session_id.is_none() || g.session_id == session_id)
    }

    pub fn remote_session_stop(self: &Arc<Self>, device_id: &str, session_id: Uuid) -> bool {
        self.owns_remote(device_id, Some(session_id)) && self.finish(Some(session_id))
    }

    pub fn remote_session_cancel(self: &Arc<Self>, device_id: &str, session_id: Uuid) -> bool {
        self.owns_remote(device_id, Some(session_id)) && self.cancel(Some(session_id))
    }

    /// Déconnexion du téléphone pendant une écoute : finalisation propre ou annulation.
    pub fn remote_disconnected(self: &Arc<Self>, device_id: &str) {
        if !self.owns_remote(device_id, None) {
            return;
        }
        let enough = {
            let g = self.inner.lock();
            g.state == FsmState::Listening && g.recording.len() >= ms_to_samples(1000)
        };
        if enough {
            self.finish(None);
        } else {
            self.cancel(None);
        }
    }

    // ------------------------------------------------------------------ Texte direct

    /// Flux texte direct mobile (idempotent par `message_id`).
    pub async fn inject_text(self: &Arc<Self>, device_id: Option<String>, req: InjectTextRequest) -> Result<InjectionResult, ServerEvent> {
        let t0 = Instant::now();
        let id = req.message_id;
        let cfg = self.cfg.get();
        if device_id.is_some() && !cfg.remote_mobile.allow_direct_text {
            return Err(ServerEvent::error(Some(id), ErrorCode::DirectTextDisabled));
        }
        let max = cfg.remote_mobile.max_text_length;
        if req.text.chars().count() > max || req.text.len() > max.saturating_mul(4) {
            return Err(ServerEvent::Error {
                id: Some(id),
                code: ErrorCode::TextTooLarge,
                message: format!("Texte trop long (max {max} caractères)."),
            });
        }
        if req.text.trim().is_empty() {
            return Err(ServerEvent::error(Some(id), ErrorCode::BadRequest));
        }
        // Idempotence.
        loop {
            {
                let mut idem = self.idem.lock();
                // None = libre ; Some(None) = en cours ; Some(Some(r)) = déjà traité.
                let status: Option<Option<InjectionResult>> = match idem.get(&id) {
                    Some((_, Idem::Done(r))) => Some(Some(r.clone())),
                    Some((at, Idem::InProgress)) if at.elapsed() <= Duration::from_secs(30) => Some(None),
                    _ => None,
                };
                match status {
                    Some(Some(r)) => return Ok(r),
                    Some(None) => {}
                    None => {
                        idem.insert(id, (Instant::now(), Idem::InProgress));
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }

        let source = Source::RemoteText {
            device_id: device_id.clone().unwrap_or_else(|| "local".into()),
        };
        let target = self.platform.foreground_window();
        let refine = req.refine_with_ollama;
        {
            let mut g = self.inner.lock();
            let busy = match g.state {
                FsmState::Paused => Some(ErrorCode::Paused),
                FsmState::Idle | FsmState::Error if g.source.is_none() => None,
                _ => Some(ErrorCode::Busy),
            };
            if let Some(code) = busy {
                drop(g);
                self.idem.lock().remove(&id);
                return Err(ServerEvent::error(Some(id), code));
            }
            g.state = if refine { FsmState::Transcribing } else { FsmState::Injecting };
            g.source = Some(source.clone());
            g.session_id = Some(id);
            g.target = target.clone();
            g.error_code = None;
            self.emit_state(&g);
        }
        self.sound(Sound::Received);
        let mut text = req.text.clone();
        if refine {
            match self.pipeline.refine(&text, &cfg).await {
                Ok(t) => text = t,
                Err(e) => tracing::info!("Nettoyage IA indisponible ({e}), texte brut injecté"),
            }
        }
        let result = self
            .inject_final(id, source, text, target, t0, req.press_enter_after)
            .await;
        self.idem.lock().insert(id, (Instant::now(), Idem::Done(result.clone())));
        Ok(result)
    }

    pub async fn quick_action(self: &Arc<Self>, action: QuickAction) -> Result<(), ErrorCode> {
        match self.state() {
            FsmState::Idle | FsmState::Error => {}
            FsmState::Paused => return Err(ErrorCode::Paused),
            _ => return Err(ErrorCode::Busy),
        }
        let p = self.platform.clone();
        let ok = tokio::task::spawn_blocking(move || p.quick_action(action))
            .await
            .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err(ErrorCode::InjectionFailed)
        }
    }

    // ------------------------------------------------------------------ Watchdog

    /// À appeler périodiquement (100 ms) : watchdog de flux, VAD auto-stop, purge.
    pub fn tick(self: &Arc<Self>) {
        let now = Instant::now();
        let mut action: Option<bool> = None; // Some(true)=finish, Some(false)=cancel
        let mut new_samples = Vec::new();
        let sid;
        {
            let mut g = self.inner.lock();
            sid = g.session_id;
            if g.state == FsmState::Listening {
                let remote = matches!(g.source, Some(Source::RemoteAudio { .. }));
                if remote && now.duration_since(g.last_packet) > REMOTE_STREAM_TIMEOUT {
                    tracing::warn!("Watchdog : flux audio mobile interrompu");
                    action = Some(g.recording.len() >= ms_to_samples(500));
                } else if g.started.map(|s| now.duration_since(s) > MAX_SESSION).unwrap_or(false) {
                    action = Some(true);
                } else if g.tracker_pos < g.recording.len() {
                    new_samples = g.recording[g.tracker_pos..].to_vec();
                    g.tracker_pos = g.recording.len();
                }
            }
        }
        if action.is_none() && !new_samples.is_empty() {
            let mut t = self.tracker.lock();
            if let Some(tr) = t.as_mut() {
                if tr.feed(&new_samples) {
                    action = Some(true);
                }
            }
        }
        match action {
            Some(true) => {
                self.finish(sid);
            }
            Some(false) => {
                self.cancel(sid);
            }
            None => {}
        }
        self.idem
            .lock()
            .retain(|_, (at, _)| now.duration_since(*at) < IDEMPOTENCE_TTL);
    }

    pub fn spawn_watchdog(self: &Arc<Self>) {
        let this = self.clone();
        self.rt.spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(100));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                iv.tick().await;
                this.tick();
            }
        });
    }
}

impl AudioSink for Orchestrator {
    fn on_audio(&self, samples: &[i16]) {
        let idle = {
            let mut g = self.inner.lock();
            g.ring.push_slice(samples);
            if g.state == FsmState::Listening && g.source == Some(Source::LocalMic) {
                g.recording.extend_from_slice(samples);
                self.level.store(rms_i16(samples).to_bits(), Ordering::Relaxed);
            }
            g.state == FsmState::Idle
        };
        if self.desktop_clients.load(Ordering::Relaxed) > 0 {
            let mut last = self.last_level.lock();
            if last.elapsed() >= Duration::from_millis(50) {
                *last = Instant::now();
                self.emit(ServerEvent::AudioLevel { rms: rms_i16(samples) });
            }
        }
        if idle {
            if let Some(tx) = self.wake_tx.lock().as_ref() {
                let _ = tx.try_send(samples.to_vec());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::platform::NullPlatform;
    use async_trait::async_trait;

    struct MockPipeline {
        text: String,
        fail: bool,
    }

    #[async_trait]
    impl SpeechPipeline for MockPipeline {
        async fn transcribe(&self, _pcm: Vec<i16>, _cfg: &Config) -> Result<String, EngineError> {
            if self.fail {
                Err(EngineError::Unreachable("down".into()))
            } else {
                Ok(self.text.clone())
            }
        }
        async fn refine(&self, text: &str, _cfg: &Config) -> Result<String, EngineError> {
            if self.fail {
                Err(EngineError::Timeout)
            } else {
                Ok(format!("{}.", text.trim()))
            }
        }
    }

    fn setup(fail: bool) -> (Arc<Orchestrator>, Arc<NullPlatform>, tokio::sync::broadcast::Receiver<ServerEvent>) {
        let (tx, rx) = tokio::sync::broadcast::channel(256);
        let platform = Arc::new(NullPlatform::default());
        let o = Orchestrator::new(
            Arc::new(ConfigStore::in_memory(Config::default())),
            tx,
            Arc::new(MockPipeline {
                text: "Bonjour le monde".into(),
                fail,
            }),
            platform.clone(),
            VadModel::energy_only(),
            "PC-TEST".into(),
        );
        (o, platform, rx)
    }

    fn req(text: &str) -> InjectTextRequest {
        InjectTextRequest {
            message_id: Uuid::new_v4(),
            text: text.into(),
            press_enter_after: false,
            refine_with_ollama: false,
        }
    }

    fn speech(ms: u32) -> Vec<i16> {
        (0..ms_to_samples(ms))
            .map(|i| ((i as f32 * 0.07).sin() * 0.3 * 32767.0) as i16)
            .collect()
    }

    async fn wait_idle(o: &Arc<Orchestrator>) {
        for _ in 0..200 {
            if o.state() == FsmState::Idle {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("pas revenu à IDLE : {:?}", o.state());
    }

    #[tokio::test]
    async fn direct_text_is_idempotent() {
        let (o, p, _rx) = setup(false);
        let r = req("Salut 👍");
        let a = o.inject_text(Some("dev".into()), r.clone()).await.unwrap();
        let b = o.inject_text(Some("dev".into()), r.clone()).await.unwrap();
        assert!(a.injected);
        assert_eq!(a.id, b.id);
        assert_eq!(p.injected.lock().len(), 1);
        assert_eq!(o.state(), FsmState::Idle);
        assert_eq!(o.history().len(), 1);
    }

    #[tokio::test]
    async fn press_enter_and_refine() {
        let (o, p, _rx) = setup(false);
        let mut r = req("bonjour");
        r.press_enter_after = true;
        r.refine_with_ollama = true;
        let res = o.inject_text(Some("dev".into()), r).await.unwrap();
        assert_eq!(res.text, "bonjour.");
        assert_eq!(p.injected.lock()[0], "bonjour.⏎");
    }

    #[tokio::test]
    async fn refine_failure_falls_back_to_raw() {
        let (o, p, _rx) = setup(true);
        let mut r = req("texte brut");
        r.refine_with_ollama = true;
        let res = o.inject_text(Some("dev".into()), r).await.unwrap();
        assert_eq!(res.text, "texte brut");
        assert_eq!(p.injected.lock()[0], "texte brut");
    }

    #[tokio::test]
    async fn busy_when_listening_and_retry_possible() {
        let (o, p, _rx) = setup(false);
        o.begin_listening(Source::LocalMic, Uuid::new_v4(), false).unwrap();
        let r = req("pendant la dictée");
        let err = o.inject_text(Some("dev".into()), r.clone()).await.unwrap_err();
        assert!(matches!(err, ServerEvent::Error { code: ErrorCode::Busy, .. }));
        assert!(p.injected.lock().is_empty());
        // Une seconde source audio est aussi refusée.
        assert_eq!(
            o.remote_session_start("dev", Uuid::new_v4(), false).unwrap_err(),
            ErrorCode::Busy
        );
        o.cancel(None);
        // Le même message_id peut être renvoyé après un BUSY.
        assert!(o.inject_text(Some("dev".into()), r).await.unwrap().injected);
    }

    #[tokio::test]
    async fn text_limits_and_permissions() {
        let (o, _p, _rx) = setup(false);
        let big = "a".repeat(20_001);
        let e = o.inject_text(Some("dev".into()), req(&big)).await.unwrap_err();
        assert!(matches!(e, ServerEvent::Error { code: ErrorCode::TextTooLarge, .. }));
        o.cfg.update(|c| c.remote_mobile.allow_direct_text = false);
        let e = o.inject_text(Some("dev".into()), req("x")).await.unwrap_err();
        assert!(matches!(e, ServerEvent::Error { code: ErrorCode::DirectTextDisabled, .. }));
        // Le bac à sable local reste autorisé.
        assert!(o.inject_text(None, req("x")).await.is_ok());
    }

    #[tokio::test]
    async fn voice_flow_local_mic() {
        let (o, p, _rx) = setup(false);
        o.on_hotkey_down();
        assert_eq!(o.state(), FsmState::Listening);
        o.on_audio(&speech(800));
        o.on_hotkey_up();
        wait_idle(&o).await;
        assert_eq!(p.injected.lock().as_slice(), ["Bonjour le monde"]);
    }

    #[tokio::test]
    async fn silence_is_ignored() {
        let (o, p, mut rx) = setup(false);
        o.on_hotkey_down();
        o.on_audio(&vec![0i16; 16_000]);
        o.on_hotkey_up();
        wait_idle(&o).await;
        assert!(p.injected.lock().is_empty());
        let mut saw_empty = false;
        while let Ok(e) = rx.try_recv() {
            if matches!(e, ServerEvent::Error { code: ErrorCode::EmptyAudio, .. }) {
                saw_empty = true;
            }
        }
        assert!(saw_empty);
    }

    #[tokio::test]
    async fn escape_cancels() {
        let (o, p, _rx) = setup(false);
        o.on_hotkey_down();
        o.on_audio(&speech(800));
        assert!(o.on_escape());
        assert_eq!(o.state(), FsmState::Idle);
        o.on_hotkey_up();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(p.injected.lock().is_empty());
    }

    #[tokio::test]
    async fn preroll_is_prefixed() {
        let (o, _p, _rx) = setup(false);
        o.on_audio(&vec![7i16; 16_000]);
        o.begin_listening(Source::LocalMic, Uuid::new_v4(), false).unwrap();
        let g = o.inner.lock();
        assert_eq!(g.recording.len(), ms_to_samples(PREROLL_MS));
        assert!(g.recording.iter().all(|s| *s == 7));
    }

    #[tokio::test]
    async fn toggle_mode() {
        let (o, p, _rx) = setup(false);
        o.cfg.update(|c| c.triggers.hotkey.mode = TriggerMode::Toggle);
        o.on_hotkey_down();
        o.on_hotkey_up();
        assert_eq!(o.state(), FsmState::Listening);
        o.on_audio(&speech(600));
        o.on_hotkey_down();
        wait_idle(&o).await;
        assert_eq!(p.injected.lock().len(), 1);
    }

    #[tokio::test]
    async fn remote_audio_flow_and_watchdog() {
        let (o, p, _rx) = setup(false);
        let sid = Uuid::new_v4();
        o.remote_session_start("phone", sid, false).unwrap();
        let bytes: Vec<u8> = speech(800).iter().flat_map(|s| s.to_le_bytes()).collect();
        o.remote_audio("phone", &bytes);
        // Un autre appareil ne peut pas injecter d'audio dans la session.
        o.remote_audio("intrus", &bytes);
        assert!(!o.remote_session_stop("intrus", sid));
        assert!(o.remote_session_stop("phone", sid));
        wait_idle(&o).await;
        assert_eq!(p.injected.lock().len(), 1);

        // Watchdog : plus de paquets pendant > 2 s.
        tokio::time::pause();
        let sid = Uuid::new_v4();
        o.remote_session_start("phone", sid, false).unwrap();
        o.remote_audio("phone", &bytes);
        tokio::time::advance(Duration::from_millis(2100)).await;
        o.tick();
        assert_ne!(o.state(), FsmState::Listening);
        tokio::time::resume();
        wait_idle(&o).await;
        assert_eq!(p.injected.lock().len(), 2);
    }

    #[tokio::test]
    async fn engine_failure_enters_error_then_idle() {
        let (o, p, _rx) = setup(true);
        o.on_hotkey_down();
        o.on_audio(&speech(800));
        o.on_hotkey_up();
        for _ in 0..100 {
            if o.state() == FsmState::Error {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(o.state(), FsmState::Error);
        assert_eq!(o.snapshot().error_code, Some(ErrorCode::OllamaUnreachable));
        // Une nouvelle dictée reste possible pendant l'affichage de l'erreur.
        assert!(o.begin_listening(Source::LocalMic, Uuid::new_v4(), false).is_ok());
        assert!(p.injected.lock().is_empty());
    }

    #[tokio::test]
    async fn focus_change_copies_to_clipboard() {
        let (o, p, _rx) = setup(false);
        let w = |h: isize| WindowInfo {
            hwnd: h,
            title: format!("W{h}"),
            process_name: "x.exe".into(),
            elevated: false,
        };
        *p.window_sequence.lock() = vec![w(1), w(2)];
        o.on_hotkey_down();
        o.on_audio(&speech(800));
        o.on_hotkey_up();
        wait_idle(&o).await;
        assert!(p.injected.lock().is_empty());
        assert_eq!(p.clipboard.lock().as_deref(), Some("Bonjour le monde"));
        let h = o.history();
        assert_eq!(h[0].injected, false);
    }

    #[tokio::test]
    async fn admin_window_fallback() {
        let (o, p, _rx) = setup(false);
        *p.window.lock() = Some(WindowInfo {
            hwnd: 5,
            title: "Administrateur : PowerShell".into(),
            process_name: "powershell.exe".into(),
            elevated: true,
        });
        let r = o.inject_text(Some("dev".into()), req("whoami")).await.unwrap();
        assert!(!r.injected);
        assert_eq!(r.injection_note.as_deref(), Some("ADMIN_WINDOW_CLIPBOARD_ONLY"));
        assert_eq!(p.clipboard.lock().as_deref(), Some("whoami"));
    }

    #[tokio::test]
    async fn pause_blocks_everything() {
        let (o, _p, _rx) = setup(false);
        o.set_enabled(false);
        assert_eq!(o.state(), FsmState::Paused);
        let e = o.inject_text(Some("dev".into()), req("x")).await.unwrap_err();
        assert!(matches!(e, ServerEvent::Error { code: ErrorCode::Paused, .. }));
        o.on_hotkey_down();
        assert_eq!(o.state(), FsmState::Paused);
        o.set_enabled(true);
        assert_eq!(o.state(), FsmState::Idle);
    }

    #[tokio::test]
    async fn quick_actions() {
        let (o, p, _rx) = setup(false);
        o.quick_action(QuickAction::Enter).await.unwrap();
        o.quick_action(QuickAction::UndoLast).await.unwrap();
        assert_eq!(p.actions.lock().as_slice(), [QuickAction::Enter, QuickAction::UndoLast]);
    }
}
