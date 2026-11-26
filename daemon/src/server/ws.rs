//! WebSocket `/ws` : messages JSON + trames binaires PCM S16LE 16 kHz (mobile → PC).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};

use super::auth::Principal;
use super::rest::principal;
use super::{AppContext, AppState, Scope};
use crate::protocol::{ClientMessage, ErrorCode, ServerEvent};

/// Fermeture si aucun message (ping inclus) pendant ce délai.
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

fn origin_ok(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) else {
        return true; // clients non-navigateur (Raccourcis iOS, scripts)
    };
    match st.scope {
        Scope::Local => {
            let port = st.ctx.local_port();
            origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
        }
        Scope::Lan => origin.starts_with("https://"),
    }
}

pub async fn ws_handler(State(st): State<AppState>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !origin_ok(&st, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Sur le listener local, le cookie HttpOnly authentifie dès l'upgrade.
    let pre = principal(&st, &headers, None);
    if st.scope == Scope::Local && pre.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.max_message_size(512 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, st, pre))
}

fn to_text(e: &ServerEvent) -> Option<Message> {
    serde_json::to_string(e).ok().map(Message::Text)
}

async fn handle_socket(socket: WebSocket, st: AppState, pre: Option<Principal>) {
    let ctx = st.ctx.clone();
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(512);
    let writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            let close = matches!(m, Message::Close(_));
            if sink.send(m).await.is_err() || close {
                break;
            }
        }
    });

    // Abonnement avant l'authentification pour ne rien manquer.
    let mut events = ctx.events.subscribe();

    // --- Authentification (premier message `auth` obligatoire sur le LAN).
    let principal = match pre {
        Some(p) => p,
        None => {
            let first = tokio::time::timeout(AUTH_TIMEOUT, stream.next()).await;
            let p = match first {
                Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str::<ClientMessage>(&t) {
                    Ok(ClientMessage::Auth { token, .. }) => ctx.auth.verify_device(&token),
                    _ => None,
                },
                _ => None,
            };
            match p {
                Some(p) => p,
                None => {
                    if let Some(m) = to_text(&ServerEvent::error(None, ErrorCode::Unauthorized)) {
                        let _ = out_tx.send(m).await;
                    }
                    let _ = out_tx.send(Message::Close(None)).await;
                    let _ = writer.await;
                    return;
                }
            }
        }
    };
    let is_local = principal == Principal::Local;
    let source_id = principal.source_id();
    let device_id = principal.device_id().map(String::from);

    register(&ctx, &source_id, 1);
    if is_local {
        ctx.orch.desktop_client_connected(1);
    }
    if device_id.is_some() {
        let _ = ctx.events.send(ServerEvent::DevicesChanged);
    }
    let auth_ok = ServerEvent::AuthOk {
        pc_name: ctx.orch.pc_name.clone(),
        state: ctx.orch.state(),
        device_id: device_id.clone(),
    };
    if let Some(m) = to_text(&auth_ok) {
        let _ = out_tx.send(m).await;
    }
    // État courant immédiatement.
    let snap = ctx.orch.snapshot();
    if let Some(m) = to_text(&ServerEvent::StateChanged {
        state: snap.state,
        source: snap.source,
        session_id: snap.session_id,
        target_window: snap.target_window,
        error_code: snap.error_code,
        detail: None,
    }) {
        let _ = out_tx.send(m).await;
    }

    // --- Diffusion des événements vers ce client.
    let fwd_tx = out_tx.clone();
    let my_dev = device_id.clone();
    let forwarder = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(ServerEvent::DeviceRevoked { device_id }) => {
                    if my_dev.as_deref() == Some(device_id.as_str()) {
                        if let Some(m) = to_text(&ServerEvent::error(None, ErrorCode::Unauthorized)) {
                            let _ = fwd_tx.send(m).await;
                        }
                        let _ = fwd_tx.send(Message::Close(None)).await;
                        break;
                    }
                }
                Ok(e) => {
                    if e.is_internal() || (!is_local && e.desktop_only()) {
                        continue;
                    }
                    if let Some(m) = to_text(&e) {
                        // Pas de blocage si le client est lent : on saute les niveaux audio.
                        if matches!(e, ServerEvent::AudioLevel { .. }) {
                            let _ = fwd_tx.try_send(m);
                        } else if fwd_tx.send(m).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("Client WS en retard de {n} événements");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // --- Boucle de réception.
    loop {
        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
        let msg = match next {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => {
                tracing::info!("WebSocket {source_id} inactif : fermeture (heartbeat manquant)");
                break;
            }
        };
        // Révocation : l'appareil n'est plus autorisé.
        if let Some(d) = &device_id {
            if ctx.cfg.read(|c| !c.remote_mobile.paired_devices.iter().any(|p| &p.id == d)) {
                break;
            }
        }
        match msg {
            Message::Binary(bytes) => ctx.orch.remote_audio(&source_id, &bytes),
            Message::Text(t) => match serde_json::from_str::<ClientMessage>(&t) {
                Ok(m) => handle_message(&ctx, &principal, &source_id, m, &out_tx).await,
                Err(e) => {
                    let ev = ServerEvent::Error {
                        id: None,
                        code: ErrorCode::BadRequest,
                        message: format!("Message invalide : {e}"),
                    };
                    if let Some(m) = to_text(&ev) {
                        let _ = out_tx.send(m).await;
                    }
                }
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    // --- Nettoyage.
    forwarder.abort();
    ctx.orch.remote_disconnected(&source_id);
    register(&ctx, &source_id, -1);
    if is_local {
        ctx.orch.desktop_client_connected(-1);
    }
    if device_id.is_some() {
        let _ = ctx.events.send(ServerEvent::DevicesChanged);
    }
    drop(out_tx);
    let _ = writer.await;
}

fn register(ctx: &AppContext, id: &str, delta: i64) {
    let mut g = ctx.connections.lock();
    let e = g.entry(id.to_string()).or_insert(0);
    if delta > 0 {
        *e += 1;
    } else {
        *e = e.saturating_sub(1);
    }
    if *e == 0 {
        g.remove(id);
    }
}

async fn send(out: &mpsc::Sender<Message>, e: &ServerEvent) {
    if let Some(m) = to_text(e) {
        let _ = out.send(m).await;
    }
}

async fn handle_message(
    ctx: &Arc<AppContext>,
    principal: &Principal,
    source_id: &str,
    msg: ClientMessage,
    out: &mpsc::Sender<Message>,
) {
    match msg {
        ClientMessage::Auth { .. } => {
            send(
                out,
                &ServerEvent::AuthOk {
                    pc_name: ctx.orch.pc_name.clone(),
                    state: ctx.orch.state(),
                    device_id: principal.device_id().map(String::from),
                },
            )
            .await;
        }
        ClientMessage::Ping { ts } => send(out, &ServerEvent::Pong { ts }).await,
        ClientMessage::SessionStart {
            session_id,
            audio_format,
            auto_stop_vad,
            ..
        } => {
            if let Some(f) = &audio_format {
                if f.encoding != "pcm_s16le" || f.sample_rate != 16_000 || f.channels != 1 {
                    send(
                        out,
                        &ServerEvent::Error {
                            id: Some(session_id),
                            code: ErrorCode::BadRequest,
                            message: "Format audio attendu : pcm_s16le 16000 Hz mono".into(),
                        },
                    )
                    .await;
                    return;
                }
            }
            if let Err(code) = ctx.orch.remote_session_start(source_id, session_id, auto_stop_vad) {
                send(out, &ServerEvent::error(Some(session_id), code)).await;
            }
        }
        ClientMessage::SessionStop { session_id } => {
            ctx.orch.remote_session_stop(source_id, session_id);
        }
        ClientMessage::SessionCancel { session_id } => {
            ctx.orch.remote_session_cancel(source_id, session_id);
        }
        ClientMessage::InjectText(req) => {
            // Tâche dédiée : la réception (heartbeat, audio) n'est jamais bloquée.
            let orch = ctx.orch.clone();
            let out = out.clone();
            let dev = principal.device_id().map(String::from);
            tokio::spawn(async move {
                match orch.inject_text(dev, req).await {
                    Ok(r) => send(&out, &ServerEvent::InjectionResult(r)).await,
                    Err(e) => send(&out, &e).await,
                }
            });
        }
        ClientMessage::Action { action } => {
            let orch = ctx.orch.clone();
            let out = out.clone();
            tokio::spawn(async move {
                if let Err(code) = orch.quick_action(action).await {
                    send(&out, &ServerEvent::error(None, code)).await;
                }
            });
        }
    }
}
