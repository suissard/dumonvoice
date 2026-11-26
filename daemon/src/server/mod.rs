//! Serveur web embarqué : double listener (HTTP local `127.0.0.1:18450` + HTTPS LAN
//! `0.0.0.0:18451`), PWA embarquée, API REST et WebSocket.

pub mod auth;
pub mod mdns;
pub mod rest;
pub mod tls;
pub mod ws;

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use parking_lot::{Mutex, RwLock};
use rust_embed::RustEmbed;

use crate::audio::capture::CaptureHandle;
use crate::config::ConfigStore;
use crate::engines::EngineManager;
use crate::fsm::Orchestrator;
use crate::EventTx;
use auth::AuthState;
use tls::CertPaths;

#[derive(RustEmbed)]
#[folder = "../pwa/dist"]
#[allow_missing = true]
struct Assets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `http://127.0.0.1` : PWA Desktop.
    Local,
    /// `https://<ip LAN>` : compagnon mobile.
    Lan,
}

/// Actions natives déclenchées depuis l'API (implémentées par le binaire).
pub trait AppControl: Send + Sync {
    /// Active la capture de la prochaine combinaison de touches (hook clavier).
    fn start_hotkey_capture(&self);
}

pub struct NoControl;
impl AppControl for NoControl {
    fn start_hotkey_capture(&self) {}
}

pub struct AppContext {
    pub orch: Arc<Orchestrator>,
    pub cfg: Arc<ConfigStore>,
    pub events: EventTx,
    pub auth: Arc<AuthState>,
    pub engines: Arc<EngineManager>,
    pub capture: CaptureHandle,
    pub app_dir: PathBuf,
    pub models_dir: PathBuf,
    pub certs: CertPaths,
    pub control: Arc<dyn AppControl>,
    /// Appareils connectés en WebSocket (id → nombre de connexions).
    pub connections: Mutex<HashMap<String, usize>>,
    pub downloads: Mutex<HashSet<String>>,
    pub lan_ips: RwLock<Vec<Ipv4Addr>>,
    pub lan_running: std::sync::atomic::AtomicBool,
    pub ollama_cache: Mutex<Option<(tokio::time::Instant, Option<String>)>>,
}

impl AppContext {
    pub fn local_port(&self) -> u16 {
        self.cfg.read(|c| c.server.local_port)
    }

    pub fn lan_url(&self) -> Option<String> {
        let ip = self.lan_ips.read().first().copied()?;
        let port = self.cfg.read(|c| c.remote_mobile.lan_port);
        Some(format!("https://{ip}:{port}"))
    }

    pub fn local_url(&self, view: Option<&str>) -> String {
        let mut u = format!("http://127.0.0.1:{}/?auth={}", self.local_port(), self.auth.local_token);
        if let Some(v) = view {
            u.push_str("&view=");
            u.push_str(v);
        }
        u
    }

    pub fn connected_devices(&self) -> Vec<String> {
        self.connections
            .lock()
            .iter()
            .filter(|(k, n)| **n > 0 && k.as_str() != "local")
            .map(|(k, _)| k.clone())
            .collect()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub ctx: Arc<AppContext>,
    pub scope: Scope,
}

pub fn router(ctx: Arc<AppContext>, scope: Scope) -> Router {
    Router::new()
        .route("/api/status", get(rest::status))
        .route("/api/config", get(rest::get_config).patch(rest::patch_config))
        .route("/api/pairing/create", post(rest::pairing_create))
        .route("/api/pairing/exchange", post(rest::pairing_exchange))
        .route("/api/pairing/devices", get(rest::list_devices))
        .route("/api/pairing/devices/:id", delete(rest::revoke_device))
        .route("/api/remote/inject-text", post(rest::inject_text))
        .route("/api/remote/action", post(rest::action))
        .route("/api/engines/ollama/models", get(rest::ollama_models))
        .route("/api/engines/download", post(rest::download))
        .route("/api/history", get(rest::history))
        .route("/api/hotkey/capture", post(rest::hotkey_capture))
        .route("/api/wakewords", get(rest::wakewords))
        .route("/api/wakewords/import", post(rest::import_wakeword))
        .route("/ca.crt", get(rest::ca_crt))
        .route("/ws", get(ws::ws_handler))
        .fallback(static_handler)
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(AppState { ctx, scope })
}

const FALLBACK_HTML: &str = "<!doctype html><meta charset=utf-8><title>DumonVoice</title>\
<p style='font-family:sans-serif'>DumonVoice fonctionne, mais l'interface PWA n'a pas été \
embarquée dans ce binaire (lancez <code>npm run build</code> dans <code>pwa/</code> puis recompilez).</p>";

fn security_headers(h: &mut HeaderMap) {
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: blob:; connect-src 'self' ws: wss:; worker-src 'self'; \
             manifest-src 'self'; frame-ancestors 'none'",
        ),
    );
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
}

async fn static_handler(
    State(st): State<AppState>,
    uri: Uri,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    // Ouverture depuis le System Tray : `/?auth=<local_token>` → cookie HttpOnly.
    if st.scope == Scope::Local {
        if let Some(tok) = q.get("auth") {
            if st.ctx.auth.verify_local(tok) {
                let view = q
                    .get("view")
                    .filter(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                    .map(|v| format!("/#{v}"))
                    .unwrap_or_else(|| "/".into());
                let cookie = format!("dv_local={tok}; HttpOnly; SameSite=Strict; Path=/");
                let mut r = Response::builder()
                    .status(StatusCode::FOUND)
                    .header(header::LOCATION, view)
                    .header(header::SET_COOKIE, cookie)
                    .header(header::CACHE_CONTROL, "no-store")
                    .body(Body::empty())
                    .unwrap();
                security_headers(r.headers_mut());
                return r;
            }
        }
    }
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (file, served) = match Assets::get(path) {
        Some(f) => (Some(f), path.to_string()),
        // Routes SPA (`/remote`, `/setup-ca`…) → index.html
        None if !path.contains('.') => (Assets::get("index.html"), "index.html".into()),
        None => (None, String::new()),
    };
    let mut resp = match file {
        Some(f) => {
            let mime = mime_guess::from_path(&served).first_or_octet_stream();
            let cache = if served.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, cache)
                .body(Body::from(f.data.into_owned()))
                .unwrap()
        }
        None if served == "index.html" || path == "index.html" => Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(FALLBACK_HTML))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    };
    if served.ends_with("sw.js") {
        resp.headers_mut()
            .insert("Service-Worker-Allowed", HeaderValue::from_static("/"));
    }
    security_headers(resp.headers_mut());
    resp
}

/// Listener local (toujours actif).
pub async fn run_local(ctx: Arc<AppContext>, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    if let Ok(addr) = listener.local_addr() {
        tracing::info!("Listener local : http://{addr}");
    }
    let app = router(ctx, Scope::Local);
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

/// Listener LAN HTTPS (actif si `remote_mobile.enabled`).
pub struct LanServer {
    handle: axum_server::Handle,
    _mdns: Option<mdns::MdnsPublisher>,
    watcher: tokio::task::JoinHandle<()>,
    pub port: u16,
}

impl LanServer {
    pub fn stop(self) {
        self.watcher.abort();
        self.handle.graceful_shutdown(Some(Duration::from_secs(1)));
    }
}

fn current_sans(ctx: &AppContext) -> (Vec<Ipv4Addr>, Vec<String>) {
    let ips = tls::lan_ipv4s();
    let mdns = ctx.cfg.read(|c| c.remote_mobile.mdns_name.clone());
    let sans = tls::desired_sans(&ips, &tls::hostname(), &mdns);
    (ips, sans)
}

pub async fn start_lan(ctx: Arc<AppContext>) -> anyhow::Result<LanServer> {
    let (port, mdns_name) = ctx.cfg.read(|c| (c.remote_mobile.lan_port, c.remote_mobile.mdns_name.clone()));
    let (ips, sans) = current_sans(&ctx);
    *ctx.lan_ips.write() = ips;
    let certs_dir = ctx.app_dir.clone();
    let s = sans.clone();
    let (cert, key, _) =
        tokio::task::spawn_blocking(move || tls::ensure_server_cert(&CertPaths::new(&certs_dir), &s)).await??;
    let tls_cfg = RustlsConfig::from_pem(cert, key).await?;
    let handle = axum_server::Handle::new();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let app = router(ctx.clone(), Scope::Lan);
    let server = axum_server::bind_rustls(addr, tls_cfg.clone())
        .handle(handle.clone())
        .serve(app.into_make_service());
    tokio::spawn(async move {
        if let Err(e) = server.await {
            tracing::error!("Listener LAN arrêté : {e}");
        }
    });
    match tokio::time::timeout(Duration::from_secs(5), handle.listening()).await {
        Ok(Some(a)) => tracing::info!("Listener LAN : https://{a}"),
        _ => anyhow::bail!("impossible d'écouter sur le port {port}"),
    }
    let mdns = match mdns::MdnsPublisher::publish(&mdns_name, port) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!("mDNS indisponible : {e}");
            None
        }
    };
    // Surveillance réseau : nouvelle IP LAN → régénération du certificat SAN à la volée.
    let c2 = ctx.clone();
    let watcher = tokio::spawn(async move {
        let mut last = sans;
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let (ips, sans) = current_sans(&c2);
            *c2.lan_ips.write() = ips;
            if sans == last {
                continue;
            }
            last = sans.clone();
            let dir = c2.app_dir.clone();
            let res = tokio::task::spawn_blocking(move || tls::ensure_server_cert(&CertPaths::new(&dir), &sans)).await;
            if let Ok(Ok((cert, key, true))) = res {
                if let Err(e) = tls_cfg.reload_from_pem(cert, key).await {
                    tracing::warn!("Rechargement TLS échoué : {e}");
                } else {
                    tracing::info!("Certificat LAN rechargé (changement d'IP)");
                }
            }
        }
    });
    ctx.lan_running.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(LanServer {
        handle,
        _mdns: mdns,
        watcher,
        port,
    })
}
