//! Moteurs de transcription : trait `TranscriptionEngine`, gestionnaire avec repli
//! automatique (Ollama → Whisper), post-traitement commun.

pub mod ollama;
pub mod whisper;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::config::{Config, EngineKind};
use crate::protocol::ServerEvent;
use crate::EventTx;

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum EngineError {
    #[error("moteur injoignable : {0}")]
    Unreachable(String),
    #[error("délai dépassé")]
    Timeout,
    #[error("modèle absent : {0}")]
    ModelMissing(String),
    #[error("échec : {0}")]
    Failed(String),
}

/// Interface commune d'un moteur de transcription audio → texte.
#[async_trait]
pub trait TranscriptionEngine: Send + Sync {
    fn name(&self) -> &'static str;
    async fn transcribe(&self, pcm: &[i16], lang: &str, cfg: &Config) -> Result<String, EngineError>;
}

/// Pipeline IA vu par l'orchestrateur (permet de simuler les moteurs dans les tests).
#[async_trait]
pub trait SpeechPipeline: Send + Sync {
    async fn transcribe(&self, pcm: Vec<i16>, cfg: &Config) -> Result<String, EngineError>;
    async fn refine(&self, text: &str, cfg: &Config) -> Result<String, EngineError>;
}

pub struct EngineManager {
    pub ollama: Arc<ollama::OllamaEngine>,
    pub whisper: Arc<whisper::WhisperEngine>,
    events: EventTx,
}

impl EngineManager {
    pub fn new(models_dir: PathBuf, events: EventTx) -> Self {
        Self {
            ollama: Arc::new(ollama::OllamaEngine::new()),
            whisper: Arc::new(whisper::WhisperEngine::new(models_dir)),
            events,
        }
    }

    fn notice(&self, level: &str, code: &str, msg: &str) {
        let _ = self.events.send(ServerEvent::notice(level, code, msg));
    }

    async fn whisper_then_refine(&self, pcm: &[i16], cfg: &Config) -> Result<String, EngineError> {
        let text = self.whisper.transcribe(pcm, &cfg.language, cfg).await?;
        if cfg.engine.post_process_with_ollama && !text.trim().is_empty() {
            return Ok(self.refine(&text, cfg).await.unwrap_or(text));
        }
        Ok(text)
    }
}

#[async_trait]
impl SpeechPipeline for EngineManager {
    async fn transcribe(&self, pcm: Vec<i16>, cfg: &Config) -> Result<String, EngineError> {
        let raw = match cfg.engine.active {
            EngineKind::Whisper => self.whisper_then_refine(&pcm, cfg).await?,
            EngineKind::Ollama => {
                if !self.ollama.is_loaded(&cfg.engine.ollama).await {
                    self.notice("info", "model_loading", "Chargement du modèle…");
                }
                let res = tokio::time::timeout(
                    Duration::from_secs(15),
                    self.ollama.transcribe_audio(&cfg.engine.ollama, &pcm, &cfg.language),
                )
                .await
                .unwrap_or(Err(EngineError::Timeout));
                match res {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Ollama audio : {e}");
                        if cfg.engine.ollama.fallback_to_whisper && self.whisper.is_available(cfg) {
                            self.notice("warn", "fallback_whisper", "Ollama indisponible : repli sur Whisper");
                            self.whisper.transcribe(&pcm, &cfg.language, cfg).await?
                        } else {
                            return Err(match e {
                                EngineError::Timeout => EngineError::Timeout,
                                other => EngineError::Unreachable(other.to_string()),
                            });
                        }
                    }
                }
            }
        };
        Ok(clean_output(&raw, &cfg.injection.custom_dictionary))
    }

    async fn refine(&self, text: &str, cfg: &Config) -> Result<String, EngineError> {
        let res = tokio::time::timeout(
            Duration::from_secs(8),
            self.ollama.refine_text(&cfg.engine.ollama, text, &cfg.language),
        )
        .await
        .unwrap_or(Err(EngineError::Timeout))?;
        let out = clean_output(&res, &cfg.injection.custom_dictionary);
        if out.is_empty() {
            return Err(EngineError::Failed("réponse vide".into()));
        }
        Ok(out)
    }
}

/// Hallucinations classiques de Whisper sur du silence ou du bruit.
const HALLUCINATIONS: &[&str] = &[
    "sous-titres réalisés par la communauté d'amara.org",
    "sous-titres réalisés para la communauté d'amara.org",
    "sous-titrage st' 501",
    "sous-titrage société radio-canada",
    "merci d'avoir regardé cette vidéo",
    "merci d'avoir regardé",
    "thank you for watching",
    "thanks for watching",
    "[musique]",
    "(musique)",
    "[blank_audio]",
    "[silence]",
    "...",
];

fn is_hallucination(text: &str) -> bool {
    let t = text
        .trim()
        .trim_end_matches(['.', '!', ' '])
        .to_lowercase();
    if t.is_empty() {
        return true;
    }
    HALLUCINATIONS
        .iter()
        .any(|h| t == h.trim_end_matches('.') || t.starts_with("sous-titres réalisés"))
}

/// Retire les guillemets enveloppant toute la réponse.
fn strip_wrapping_quotes(s: &str) -> &str {
    const PAIRS: &[(char, char)] = &[('"', '"'), ('«', '»'), ('“', '”'), ('\'', '\''), ('`', '`')];
    let t = s.trim();
    for (a, b) in PAIRS {
        if t.chars().count() >= 2 && t.starts_with(*a) && t.ends_with(*b) {
            let inner = &t[a.len_utf8()..t.len() - b.len_utf8()];
            // Pas de guillemet interne du même type : enveloppe certaine.
            if !inner.contains(*a) && !inner.contains(*b) {
                return inner.trim();
            }
        }
    }
    t
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Remplacement insensible à la casse, sur mots entiers.
pub fn apply_dictionary(text: &str, dict: &BTreeMap<String, String>) -> String {
    let mut out = text.to_string();
    for (from, to) in dict {
        if from.trim().is_empty() {
            continue;
        }
        let lower_from: Vec<char> = from.to_lowercase().chars().collect();
        let chars: Vec<char> = out.chars().collect();
        let lower: Vec<char> = out.to_lowercase().chars().collect();
        if lower.len() != chars.len() {
            // Casse changeant la longueur (rare) : remplacement exact uniquement.
            out = out.replace(from.as_str(), to);
            continue;
        }
        let mut res = String::with_capacity(out.len());
        let mut i = 0;
        while i < chars.len() {
            let end = i + lower_from.len();
            let matches = end <= chars.len()
                && lower[i..end] == lower_from[..]
                && (i == 0 || !is_word_char(chars[i - 1]))
                && (end == chars.len() || !is_word_char(chars[end]));
            if matches {
                res.push_str(to);
                i = end;
            } else {
                res.push(chars[i]);
                i += 1;
            }
        }
        out = res;
    }
    out
}

/// Nettoyage post-inférence commun : trim, guillemets parasites, dictionnaire, hallucinations.
pub fn clean_output(raw: &str, dict: &BTreeMap<String, String>) -> String {
    let mut t = raw.trim().to_string();
    for prefix in ["Transcription :", "Transcription:", "Texte corrigé :", "Texte :"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim().to_string();
        }
    }
    let t = strip_wrapping_quotes(&t).to_string();
    if is_hallucination(&t) {
        return String::new();
    }
    apply_dictionary(&t, dict).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_quotes_and_prefix() {
        let d = BTreeMap::new();
        assert_eq!(clean_output("  \"Bonjour à tous.\"  ", &d), "Bonjour à tous.");
        assert_eq!(clean_output("« Salut »", &d), "Salut");
        assert_eq!(clean_output("Transcription : Salut", &d), "Salut");
        assert_eq!(clean_output("Il a dit \"oui\" puis \"non\"", &d), "Il a dit \"oui\" puis \"non\"");
    }

    #[test]
    fn filters_hallucinations() {
        let d = BTreeMap::new();
        assert_eq!(clean_output("Sous-titres réalisés par la communauté d'Amara.org", &d), "");
        assert_eq!(clean_output("Merci d'avoir regardé !", &d), "");
        assert_eq!(clean_output("[BLANK_AUDIO]", &d), "");
        assert_eq!(clean_output("Merci d'avoir regardé le dossier, Paul.", &d), "Merci d'avoir regardé le dossier, Paul.");
    }

    #[test]
    fn dictionary_whole_words_case_insensitive() {
        let mut d = BTreeMap::new();
        d.insert("dumon voice".to_string(), "DumonVoice".to_string());
        d.insert("rust".to_string(), "Rust".to_string());
        assert_eq!(
            apply_dictionary("J'aime Dumon Voice et rust, pas rustique.", &d),
            "J'aime DumonVoice et Rust, pas rustique."
        );
    }
}
