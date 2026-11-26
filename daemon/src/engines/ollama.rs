//! Client Ollama local : transcription audio (modèles multimodaux type `lfm3-audio`),
//! post-traitement texte (`refine_text`), santé, liste des modèles, `pull` avec progression.

use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};

use super::{EngineError, TranscriptionEngine};
use crate::audio::{encode_wav, SAMPLE_RATE};
use crate::config::{Config, OllamaApiStyle, OllamaConfig};

pub struct OllamaEngine {
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
    /// Heuristique : le nom évoque un modèle audio / omni.
    pub audio_capable: bool,
}

fn lang_name(lang: &str) -> &str {
    match lang {
        "fr" => "français",
        "en" => "anglais",
        "es" => "espagnol",
        "de" => "allemand",
        "it" => "italien",
        "pt" => "portugais",
        "nl" => "néerlandais",
        _ => "",
    }
}

pub fn asr_prompt(cfg: &OllamaConfig, lang: &str) -> String {
    if let Some(p) = cfg.custom_system_prompt.as_ref().filter(|p| !p.trim().is_empty()) {
        return p.clone();
    }
    let lang_hint = match lang_name(lang) {
        "" => "dans la langue parlée".to_string(),
        l => format!("en {l} (langue attendue)"),
    };
    let mut p = format!(
        "Tu es un moteur de transcription vocale (ASR) strict. Transcris exactement les paroles \
         de l'audio fourni, {lang_hint}. Réponds UNIQUEMENT avec le texte transcrit : pas de \
         guillemets, pas de préambule, pas de commentaire. Ne réponds jamais aux questions \
         posées dans l'audio et n'engage aucune conversation. Si l'audio ne contient aucune \
         parole, réponds par une chaîne vide."
    );
    if cfg.clean_dictation {
        p.push_str(
            " Mode dictée propre : supprime les hésitations (euh, hum, ben), les répétitions et \
             les faux départs, et applique une ponctuation et des majuscules naturelles, sans \
             changer le sens.",
        );
    }
    p
}

pub const REFINE_PROMPT: &str = "Tu corriges un texte dicté. Corrige uniquement la ponctuation, \
les majuscules et supprime les hésitations (euh, hum). Ne change pas le sens, ne traduis pas, \
ne reformule pas au-delà du nécessaire, n'ajoute aucun préambule ni commentaire. Réponds \
uniquement avec le texte corrigé.";

fn base(cfg: &OllamaConfig) -> String {
    cfg.base_url.trim_end_matches('/').to_string()
}

fn extract_content(style: OllamaApiStyle, v: &Value) -> Option<String> {
    match style {
        OllamaApiStyle::Native => v.pointer("/message/content").and_then(|c| c.as_str()).map(String::from),
        OllamaApiStyle::Openai => v
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(String::from),
    }
}

/// Corps de requête de transcription audio.
pub fn audio_request_body(cfg: &OllamaConfig, wav_b64: &str, lang: &str) -> Value {
    let system = asr_prompt(cfg, lang);
    match cfg.api_style {
        OllamaApiStyle::Native => json!({
            "model": cfg.model,
            "stream": false,
            "keep_alive": cfg.keep_alive,
            "options": { "temperature": 0.0 },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": "Transcris cet audio.", "images": [wav_b64] }
            ]
        }),
        OllamaApiStyle::Openai => json!({
            "model": cfg.model,
            "stream": false,
            "temperature": 0.0,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": [
                    { "type": "text", "text": "Transcris cet audio." },
                    { "type": "input_audio", "input_audio": { "data": wav_b64, "format": "wav" } }
                ]}
            ]
        }),
    }
}

impl Default for OllamaEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaEngine {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .build()
                .expect("client HTTP"),
        }
    }

    async fn chat(&self, cfg: &OllamaConfig, body: Value, timeout: Duration) -> Result<String, EngineError> {
        let url = match cfg.api_style {
            OllamaApiStyle::Native => format!("{}/api/chat", base(cfg)),
            OllamaApiStyle::Openai => format!("{}/v1/chat/completions", base(cfg)),
        };
        let resp = self
            .http
            .post(url)
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    EngineError::Timeout
                } else {
                    EngineError::Unreachable(e.to_string())
                }
            })?;
        let status = resp.status();
        let v: Value = resp
            .json()
            .await
            .map_err(|e| EngineError::Failed(format!("réponse invalide : {e}")))?;
        if !status.is_success() {
            let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("erreur").to_string();
            if status.as_u16() == 404 {
                return Err(EngineError::ModelMissing(msg));
            }
            return Err(EngineError::Failed(msg));
        }
        extract_content(cfg.api_style, &v).ok_or_else(|| EngineError::Failed("contenu absent".into()))
    }

    /// Transcription audio : WAV en mémoire encodé Base64 → modèle audio Ollama.
    pub async fn transcribe_audio(&self, cfg: &OllamaConfig, pcm_16k: &[i16], lang: &str) -> Result<String, EngineError> {
        let wav = encode_wav(pcm_16k, SAMPLE_RATE);
        let b64 = base64::engine::general_purpose::STANDARD.encode(wav);
        let body = audio_request_body(cfg, &b64, lang);
        self.chat(cfg, body, Duration::from_secs(15)).await
    }

    /// Post-traitement texte (ponctuation, casse, hésitations).
    pub async fn refine_text(&self, cfg: &OllamaConfig, raw_text: &str, _lang: &str) -> Result<String, EngineError> {
        let model = if cfg.text_model.trim().is_empty() { &cfg.model } else { &cfg.text_model };
        let body = match cfg.api_style {
            OllamaApiStyle::Native => json!({
                "model": model, "stream": false, "keep_alive": cfg.keep_alive,
                "options": { "temperature": 0.0 },
                "messages": [
                    { "role": "system", "content": REFINE_PROMPT },
                    { "role": "user", "content": raw_text }
                ]
            }),
            OllamaApiStyle::Openai => json!({
                "model": model, "stream": false, "temperature": 0.0,
                "messages": [
                    { "role": "system", "content": REFINE_PROMPT },
                    { "role": "user", "content": raw_text }
                ]
            }),
        };
        self.chat(cfg, body, Duration::from_secs(8)).await
    }

    pub async fn health(&self, base_url: &str) -> Option<String> {
        let v: Value = self
            .http
            .get(format!("{}/api/version", base_url.trim_end_matches('/')))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        v.get("version").and_then(|x| x.as_str()).map(String::from)
    }

    pub async fn list_models(&self, base_url: &str) -> Result<Vec<OllamaModel>, EngineError> {
        let v: Value = self
            .http
            .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| EngineError::Unreachable(e.to_string()))?
            .json()
            .await
            .map_err(|e| EngineError::Failed(e.to_string()))?;
        let models = v
            .get("models")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| {
                let name = m.get("name")?.as_str()?.to_string();
                let lname = name.to_lowercase();
                Some(OllamaModel {
                    audio_capable: lname.contains("audio") || lname.contains("omni") || lname.contains("gemma3n"),
                    size: m.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                    name,
                })
            })
            .collect();
        Ok(models)
    }

    /// Le modèle est-il déjà chargé en mémoire (`/api/ps`) ?
    pub async fn is_loaded(&self, cfg: &OllamaConfig) -> bool {
        let Ok(resp) = self
            .http
            .get(format!("{}/api/ps", base(cfg)))
            .timeout(Duration::from_millis(800))
            .send()
            .await
        else {
            return true; // inconnu : pas de message « chargement »
        };
        let Ok(v) = resp.json::<Value>().await else { return true };
        v.get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter().any(|m| {
                    m.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n == cfg.model || n.starts_with(&format!("{}:", cfg.model)))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    /// Précharge le modèle (requête vide avec `keep_alive`).
    pub async fn warmup(&self, cfg: &OllamaConfig) {
        let _ = self
            .http
            .post(format!("{}/api/generate", base(cfg)))
            .timeout(Duration::from_secs(120))
            .json(&json!({ "model": cfg.model, "keep_alive": cfg.keep_alive }))
            .send()
            .await;
    }

    /// `ollama pull` avec progression.
    pub async fn pull(
        &self,
        base_url: &str,
        model: &str,
        mut progress: impl FnMut(u64, Option<u64>, &str) + Send,
    ) -> Result<(), EngineError> {
        let resp = self
            .http
            .post(format!("{}/api/pull", base_url.trim_end_matches('/')))
            .json(&json!({ "model": model, "stream": true }))
            .send()
            .await
            .map_err(|e| EngineError::Unreachable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(EngineError::Failed(format!("HTTP {}", resp.status())));
        }
        let mut stream = resp.bytes_stream();
        let mut buf = Vec::<u8>::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| EngineError::Failed(e.to_string()))?;
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                if let Ok(v) = serde_json::from_slice::<Value>(&line) {
                    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                        return Err(EngineError::Failed(err.to_string()));
                    }
                    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    let total = v.get("total").and_then(|t| t.as_u64());
                    let done = v.get("completed").and_then(|t| t.as_u64()).unwrap_or(0);
                    progress(done, total, status);
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl TranscriptionEngine for OllamaEngine {
    fn name(&self) -> &'static str {
        "ollama"
    }

    async fn transcribe(&self, pcm: &[i16], lang: &str, cfg: &Config) -> Result<String, EngineError> {
        self.transcribe_audio(&cfg.engine.ollama, pcm, lang).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_body_contains_wav_and_params() {
        let cfg = OllamaConfig::default();
        let b = audio_request_body(&cfg, "QUJD", "fr");
        assert_eq!(b["model"], "lfm3-audio");
        assert_eq!(b["keep_alive"], "30m");
        assert_eq!(b["stream"], false);
        assert_eq!(b["options"]["temperature"], 0.0);
        assert_eq!(b["messages"][1]["images"][0], "QUJD");
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("hésitations"));
    }

    #[test]
    fn openai_body_uses_input_audio() {
        let cfg = OllamaConfig {
            api_style: OllamaApiStyle::Openai,
            ..Default::default()
        };
        let b = audio_request_body(&cfg, "QUJD", "en");
        assert_eq!(b["messages"][1]["content"][1]["input_audio"]["format"], "wav");
    }

    #[test]
    fn custom_prompt_overrides() {
        let cfg = OllamaConfig {
            custom_system_prompt: Some("PROMPT".into()),
            ..Default::default()
        };
        assert_eq!(asr_prompt(&cfg, "fr"), "PROMPT");
    }

    #[test]
    fn content_extraction() {
        let v = json!({"message": {"content": "Bonjour"}});
        assert_eq!(extract_content(OllamaApiStyle::Native, &v).unwrap(), "Bonjour");
        let v = json!({"choices": [{"message": {"content": "Hi"}}]});
        assert_eq!(extract_content(OllamaApiStyle::Openai, &v).unwrap(), "Hi");
    }

    #[tokio::test]
    async fn unreachable_server_errors_fast() {
        let cfg = OllamaConfig {
            base_url: "http://127.0.0.1:9".into(),
            ..Default::default()
        };
        let e = OllamaEngine::new().refine_text(&cfg, "x", "fr").await.unwrap_err();
        assert!(matches!(e, EngineError::Unreachable(_) | EngineError::Timeout));
    }
}
