//! Routes REST `/api/*` (section 6.4).

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::auth::{cookie_value, Principal};
use super::{AppState, Scope};
use crate::audio::{vad, wake_word};
use crate::downloads;
use crate::engines::whisper::WhisperEngine;
use crate::protocol::{ErrorCode, InjectTextRequest, QuickAction, ServerEvent};

fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "type": "error", "id": null, "code": code, "message": message })),
    )
        .into_response()
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find_map(|h| cookie_value(h, name))
}

/// Protection anti DNS-rebinding du listener local.
fn local_host_ok(headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            let host = h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h);
            host == "127.0.0.1" || host == "localhost" || host == "[::1]"
        })
        .unwrap_or(false)
}

/// Identifie l'appelant à partir des en-têtes (Bearer ou cookie).
pub fn principal(st: &AppState, headers: &HeaderMap, extra_token: Option<&str>) -> Option<Principal> {
    match st.scope {
        Scope::Local => {
            if !local_host_ok(headers) {
                return None;
            }
            let tok = bearer(headers)
                .or_else(|| cookie(headers, "dv_local"))
                .or_else(|| extra_token.map(String::from))?;
            st.ctx.auth.verify_local(&tok).then_some(Principal::Local)
        }
        Scope::Lan => {
            let tok = bearer(headers)
                .or_else(|| cookie(headers, "dv_device"))
                .or_else(|| extra_token.map(String::from))?;
            st.ctx.auth.verify_device(&tok)
        }
    }
}

fn require(st: &AppState, headers: &HeaderMap) -> Result<Principal, Response> {
    principal(st, headers, None)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Authentification requise"))
}

fn require_local(st: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    if st.scope != Scope::Local {
        return Err(err(StatusCode::FORBIDDEN, "LOCAL_ONLY", "Action réservée au PC"));
    }
    require(st, headers).map(|_| ())
}

async fn ollama_version(st: &AppState) -> Option<String> {
    {
        let c = st.ctx.ollama_cache.lock();
        if let Some((at, v)) = c.as_ref() {
            if at.elapsed() < Duration::from_secs(3) {
                return v.clone();
            }
        }
    }
    let base = st.ctx.cfg.read(|c| c.engine.ollama.base_url.clone());
    let v = st.ctx.engines.ollama.health(&base).await;
    *st.ctx.ollama_cache.lock() = Some((tokio::time::Instant::now(), v.clone()));
    v
}

#[derive(Deserialize, Default)]
pub struct StatusQuery {
    #[serde(default)]
    mics: Option<u8>,
}

pub async fn status(State(st): State<AppState>, headers: HeaderMap, Query(q): Query<StatusQuery>) -> Response {
    let p = match require(&st, &headers) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let ctx = &st.ctx;
    let cfg = ctx.cfg.get();
    let snap = ctx.orch.snapshot();
    let ollama = ollama_version(&st).await;
    let mics: Option<Vec<String>> = if q.mics.unwrap_or(0) == 1 {
        let cap = ctx.capture.clone();
        tokio::task::spawn_blocking(move || cap.list_devices()).await.ok()
    } else {
        None
    };
    let catalog: Vec<Value> = downloads::WHISPER_MODELS
        .iter()
        .map(|(profile, file, mb)| json!({ "profile": profile, "file": file, "size_mb": mb }))
        .collect();
    let platform = ctx.orch.platform().clone();
    let gpu = tokio::task::spawn_blocking(move || platform.gpu_info()).await.ok().flatten();
    Json(json!({
        "pc_name": ctx.orch.pc_name,
        "version": env!("CARGO_PKG_VERSION"),
        "enabled": cfg.enabled,
        "state": snap.state,
        "source": snap.source,
        "session_id": snap.session_id,
        "target_window": snap.target_window,
        "error_code": snap.error_code,
        "onboarding_done": cfg.onboarding_done,
        "engine": {
            "active": cfg.engine.active,
            "whisper": {
                "compiled": WhisperEngine::compiled(),
                "model_file": cfg.engine.whisper.model_file,
                "available": ctx.engines.whisper.is_available(&cfg),
                "installed": ctx.engines.whisper.installed_models(),
                "catalog": catalog,
            },
            "ollama": {
                "reachable": ollama.is_some(),
                "version": ollama,
                "base_url": cfg.engine.ollama.base_url,
                "model": cfg.engine.ollama.model,
            }
        },
        "microphone": {
            "current": ctx.capture.current_device(),
            "devices": mics,
        },
        "vad": ctx.orch.vad_backend(),
        "wake_word": {
            "enabled": cfg.triggers.wake_word.enabled,
            "compiled": cfg!(feature = "onnx"),
            "words": wake_word::list_words(&ctx.models_dir),
        },
        "mobile": {
            "enabled": cfg.remote_mobile.enabled,
            "running": ctx.lan_running.load(std::sync::atomic::Ordering::SeqCst),
            "lan_url": ctx.lan_url(),
            "lan_ips": ctx.lan_ips.read().iter().map(|i| i.to_string()).collect::<Vec<_>>(),
            "port": cfg.remote_mobile.lan_port,
            "mdns": format!("{}.local", cfg.remote_mobile.mdns_name),
            "connected_devices": ctx.connected_devices(),
            "paired_count": cfg.remote_mobile.paired_devices.len(),
        },
        "gpu": gpu,
        "hotkey": cfg.triggers.hotkey.keys,
        "client": {
            "scope": if st.scope == Scope::Local { "local" } else { "lan" },
            "device_id": p.device_id(),
        },
    }))
    .into_response()
}

pub async fn get_config(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    Json(st.ctx.cfg.read(|c| c.public_view())).into_response()
}

pub async fn patch_config(State(st): State<AppState>, headers: HeaderMap, Json(patch): Json<Value>) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    match st.ctx.cfg.patch(&patch) {
        Ok(c) => {
            let _ = st.ctx.events.send(ServerEvent::ConfigChanged);
            Json(c.public_view()).into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, "INVALID_CONFIG", &e),
    }
}

pub fn qr_svg(data: &str) -> Option<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .build(),
    )
}

pub async fn pairing_create(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require_local(&st, &headers) {
        return r;
    }
    if !st.ctx.lan_running.load(std::sync::atomic::Ordering::SeqCst) {
        return err(
            StatusCode::CONFLICT,
            "MOBILE_DISABLED",
            "Activez le compagnon téléphone (et vérifiez la connexion Wi-Fi) pour appairer un appareil.",
        );
    }
    let Some(base) = st.ctx.lan_url() else {
        return err(StatusCode::CONFLICT, "NO_LAN", "Aucune adresse réseau local détectée.");
    };
    let code = st.ctx.auth.create_pair_code();
    let url = format!("{base}/remote?pair={code}");
    let port = st.ctx.cfg.read(|c| c.remote_mobile.lan_port);
    let alternatives: Vec<String> = st
        .ctx
        .lan_ips
        .read()
        .iter()
        .map(|ip| format!("https://{ip}:{port}/remote?pair={code}"))
        .collect();
    Json(json!({
        "pair_code": code,
        "url": url,
        "expires_in": super::auth::PAIR_CODE_TTL.as_secs(),
        "qr_svg": qr_svg(&url),
        "alternatives": alternatives,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct ExchangeBody {
    pair_code: String,
    #[serde(default)]
    device_name: String,
}

pub async fn pairing_exchange(State(st): State<AppState>, Json(body): Json<ExchangeBody>) -> Response {
    if st.scope != Scope::Lan {
        return err(StatusCode::FORBIDDEN, "LAN_ONLY", "Appairage uniquement depuis le réseau local");
    }
    match st.ctx.auth.exchange(&body.pair_code, &body.device_name) {
        Some(ex) => {
            let _ = st.ctx.events.send(ServerEvent::DevicesChanged);
            let cookie = format!(
                "dv_device={}; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=315360000",
                ex.device_token
            );
            (
                [(header::SET_COOKIE, cookie)],
                Json(json!({
                    "device_id": ex.device_id,
                    "device_token": ex.device_token,
                    "pc_name": st.ctx.orch.pc_name,
                })),
            )
                .into_response()
        }
        None => err(
            StatusCode::UNAUTHORIZED,
            "INVALID_PAIR_CODE",
            "QR code expiré ou déjà utilisé : générez-en un nouveau sur le PC.",
        ),
    }
}

pub async fn list_devices(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require_local(&st, &headers) {
        return r;
    }
    let connected = st.ctx.connected_devices();
    let devs: Vec<Value> = st.ctx.cfg.read(|c| {
        c.remote_mobile
            .paired_devices
            .iter()
            .map(|d| {
                json!({
                    "id": d.id, "name": d.name, "paired_at": d.paired_at,
                    "connected": connected.contains(&d.id),
                })
            })
            .collect()
    });
    Json(devs).into_response()
}

pub async fn revoke_device(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    if let Err(r) = require_local(&st, &headers) {
        return r;
    }
    if st.ctx.auth.revoke(&id) {
        let _ = st.ctx.events.send(ServerEvent::DeviceRevoked { device_id: id });
        let _ = st.ctx.events.send(ServerEvent::DevicesChanged);
        StatusCode::NO_CONTENT.into_response()
    } else {
        err(StatusCode::NOT_FOUND, "NOT_FOUND", "Appareil inconnu")
    }
}

fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::Busy => StatusCode::CONFLICT,
        ErrorCode::TextTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::DirectTextDisabled | ErrorCode::Unauthorized => StatusCode::FORBIDDEN,
        ErrorCode::Paused => StatusCode::LOCKED,
        ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
        ErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn inject_text(State(st): State<AppState>, headers: HeaderMap, Json(req): Json<InjectTextRequest>) -> Response {
    let p = match require(&st, &headers) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match st.ctx.orch.inject_text(p.device_id().map(String::from), req).await {
        Ok(r) => Json(ServerEvent::InjectionResult(r)).into_response(),
        Err(e) => {
            let code = match &e {
                ServerEvent::Error { code, .. } => *code,
                _ => ErrorCode::BadRequest,
            };
            (status_for(code), Json(e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ActionBody {
    action: QuickAction,
}

pub async fn action(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<ActionBody>) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    match st.ctx.orch.quick_action(b.action).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(code) => (status_for(code), Json(ServerEvent::error(None, code))).into_response(),
    }
}

pub async fn ollama_models(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    let base = st.ctx.cfg.read(|c| c.engine.ollama.base_url.clone());
    match st.ctx.engines.ollama.list_models(&base).await {
        Ok(m) => Json(json!({ "reachable": true, "models": m })).into_response(),
        Err(e) => Json(json!({ "reachable": false, "models": [], "error": e.to_string() })).into_response(),
    }
}

#[derive(Deserialize)]
pub struct DownloadBody {
    /// `whisper | ollama | vad | wakeword`
    kind: String,
    #[serde(default)]
    model: String,
}

pub async fn download(State(st): State<AppState>, headers: HeaderMap, Json(b): Json<DownloadBody>) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    let ctx = st.ctx.clone();
    let label = format!("{}:{}", b.kind, b.model);
    if !ctx.downloads.lock().insert(label.clone()) {
        return err(StatusCode::CONFLICT, "IN_PROGRESS", "Téléchargement déjà en cours");
    }
    let events = ctx.events.clone();
    match b.kind.as_str() {
        "whisper" => {
            let Some(url) = downloads::whisper_url(&b.model) else {
                ctx.downloads.lock().remove(&label);
                return err(StatusCode::BAD_REQUEST, "UNKNOWN_MODEL", "Modèle Whisper inconnu");
            };
            let dest = ctx.models_dir.join(&b.model);
            let file = b.model.clone();
            tokio::spawn(async move {
                let ok = downloads::download_reported(&url, &dest, &label, &events).await;
                ctx.downloads.lock().remove(&label);
                let cfg = ctx.cfg.get();
                if ok && cfg.engine.whisper.model_file == file {
                    let eng = ctx.engines.whisper.clone();
                    let _ = tokio::task::spawn_blocking(move || eng.preload(&cfg)).await;
                }
            });
        }
        "ollama" => {
            let base = ctx.cfg.read(|c| c.engine.ollama.base_url.clone());
            let model = b.model.clone();
            tokio::spawn(async move {
                let ev = events.clone();
                let l2 = label.clone();
                let res = ctx
                    .engines
                    .ollama
                    .pull(&base, &model, move |done, total, _status| {
                        let _ = ev.send(ServerEvent::DownloadProgress {
                            model: l2.clone(),
                            downloaded: done,
                            total,
                            done: false,
                            error: None,
                        });
                    })
                    .await;
                let _ = events.send(ServerEvent::DownloadProgress {
                    model: label.clone(),
                    downloaded: 0,
                    total: None,
                    done: true,
                    error: res.err().map(|e| e.to_string()),
                });
                ctx.downloads.lock().remove(&label);
            });
        }
        "vad" => {
            let dest = ctx.models_dir.join(vad::SILERO_FILE);
            tokio::spawn(async move {
                if downloads::download_reported(vad::SILERO_URL, &dest, &label, &events).await {
                    let dir = ctx.models_dir.clone();
                    if let Ok(m) = tokio::task::spawn_blocking(move || vad::VadModel::load(&dir)).await {
                        ctx.orch.set_vad_model(m);
                    }
                }
                ctx.downloads.lock().remove(&label);
            });
        }
        "wakeword" => {
            let files = wake_word::required_files(&ctx.models_dir, &b.model);
            tokio::spawn(async move {
                let mut ok = true;
                for (path, url) in files {
                    if path.exists() {
                        continue;
                    }
                    match url {
                        Some(u) => ok &= downloads::download_reported(&u, &path, &label, &events).await,
                        None => {
                            ok = false;
                            let _ = events.send(ServerEvent::DownloadProgress {
                                model: label.clone(),
                                downloaded: 0,
                                total: None,
                                done: true,
                                error: Some("Modèle personnalisé absent : importez le fichier .onnx".into()),
                            });
                        }
                    }
                }
                ctx.downloads.lock().remove(&label);
                if ok {
                    let _ = events.send(ServerEvent::DownloadProgress {
                        model: label.clone(),
                        downloaded: 0,
                        total: None,
                        done: true,
                        error: None,
                    });
                    let _ = events.send(ServerEvent::ConfigChanged);
                }
            });
        }
        _ => {
            ctx.downloads.lock().remove(&label);
            return err(StatusCode::BAD_REQUEST, "UNKNOWN_KIND", "Type de téléchargement inconnu");
        }
    }
    (StatusCode::ACCEPTED, Json(json!({ "started": true }))).into_response()
}

pub async fn history(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    Json(st.ctx.orch.history()).into_response()
}

pub async fn hotkey_capture(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require_local(&st, &headers) {
        return r;
    }
    st.ctx.control.start_hotkey_capture();
    (StatusCode::ACCEPTED, Json(json!({ "capturing": true }))).into_response()
}

pub async fn wakewords(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = require(&st, &headers) {
        return r;
    }
    Json(wake_word::list_words(&st.ctx.models_dir)).into_response()
}

#[derive(Deserialize)]
pub struct ImportQuery {
    name: String,
}

pub fn valid_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub async fn import_wakeword(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    if let Err(r) = require_local(&st, &headers) {
        return r;
    }
    if !valid_model_name(&q.name) {
        return err(StatusCode::BAD_REQUEST, "INVALID_NAME", "Nom invalide (a-z, 0-9, _ et -)");
    }
    if body.len() < 1024 || body.len() > 20 * 1024 * 1024 {
        return err(StatusCode::BAD_REQUEST, "INVALID_FILE", "Fichier .onnx invalide");
    }
    let dir = wake_word::wakewords_dir(&st.ctx.models_dir);
    let path = dir.join(format!("{}.onnx", q.name));
    let res = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&path, &body).await
    }
    .await;
    match res {
        Ok(()) => (StatusCode::CREATED, Json(json!({ "name": q.name }))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, "IO", &e.to_string()),
    }
}

pub async fn ca_crt(State(st): State<AppState>) -> Response {
    match super::tls::ca_der(&st.ctx.certs) {
        Some(der) => (
            [
                (header::CONTENT_TYPE, "application/x-x509-ca-cert".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"DumonVoice-CA.crt\"".to_string(),
                ),
            ],
            der,
        )
            .into_response(),
        None => err(StatusCode::NOT_FOUND, "NO_CA", "Certificat non généré (activez le mode mobile)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert!(valid_model_name("hey_dictee"));
        assert!(!valid_model_name("../evil"));
        assert!(!valid_model_name(""));
        assert!(!valid_model_name("Majuscule"));
    }

    #[test]
    fn host_check() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "127.0.0.1:18450".parse().unwrap());
        assert!(local_host_ok(&h));
        h.insert(header::HOST, "localhost:18450".parse().unwrap());
        assert!(local_host_ok(&h));
        h.insert(header::HOST, "evil.com:18450".parse().unwrap());
        assert!(!local_host_ok(&h));
    }

    #[test]
    fn qr_generation() {
        let svg = qr_svg("https://192.168.1.5:18451/remote?pair=abc").unwrap();
        assert!(svg.contains("<svg"));
    }
}
