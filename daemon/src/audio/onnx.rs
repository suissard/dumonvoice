//! Environnement ONNX Runtime partagé et chargement de sessions (Silero VAD, OpenWakeWord).

use std::path::Path;
use std::sync::OnceLock;

use ort::environment::Environment;
use ort::session::Session;

static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();

fn environment() -> Result<&'static Environment, String> {
    ENV.get_or_init(|| ort::init().with_name("dumonvoice").build().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| e.clone())
}

/// Charge un modèle ONNX (un seul thread d'inférence : très faible empreinte CPU).
pub fn load_session(path: &Path) -> Result<Session, String> {
    let env = environment()?;
    let builder = Session::builder(env).map_err(|e| e.to_string())?;
    let mut builder = builder.with_intra_threads(1).map_err(|e| e.to_string())?;
    builder.commit_from_file(path).map_err(|e| e.to_string())
}
