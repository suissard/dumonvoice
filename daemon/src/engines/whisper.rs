//! Moteur Whisper intégré (`whisper.cpp` via `whisper-rs`).
//!
//! L'accélération matérielle est choisie à la compilation (features `whisper-cuda`,
//! `whisper-vulkan`) ; à l'exécution `use_gpu` permet de la désactiver. Sans GPU,
//! whisper.cpp utilise automatiquement les optimisations CPU (AVX2) et les modèles quantifiés.

use std::path::PathBuf;

use async_trait::async_trait;

use super::{EngineError, TranscriptionEngine};
use crate::config::Config;

pub struct WhisperEngine {
    models_dir: PathBuf,
    #[cfg(feature = "whisper")]
    ctx: parking_lot::Mutex<Option<(String, bool, std::sync::Arc<whisper_rs::WhisperContext>)>>,
}

impl WhisperEngine {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            #[cfg(feature = "whisper")]
            ctx: parking_lot::Mutex::new(None),
        }
    }

    pub fn model_path(&self, cfg: &Config) -> PathBuf {
        self.models_dir.join(&cfg.engine.whisper.model_file)
    }

    pub fn compiled() -> bool {
        cfg!(feature = "whisper")
    }

    /// Le moteur est utilisable (compilé et modèle présent).
    pub fn is_available(&self, cfg: &Config) -> bool {
        Self::compiled() && self.model_path(cfg).exists()
    }

    pub fn installed_models(&self) -> Vec<String> {
        std::fs::read_dir(&self.models_dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n.starts_with("ggml-") && n.ends_with(".bin"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Libère le modèle (changement de profil).
    pub fn unload(&self) {
        #[cfg(feature = "whisper")]
        {
            *self.ctx.lock() = None;
        }
    }

    #[cfg(feature = "whisper")]
    fn context(&self, cfg: &Config) -> Result<std::sync::Arc<whisper_rs::WhisperContext>, EngineError> {
        let file = cfg.engine.whisper.model_file.clone();
        let gpu = cfg.engine.whisper.use_gpu;
        let mut g = self.ctx.lock();
        if let Some((f, gp, c)) = g.as_ref() {
            if *f == file && *gp == gpu {
                return Ok(c.clone());
            }
        }
        let path = self.models_dir.join(&file);
        if !path.exists() {
            return Err(EngineError::ModelMissing(file));
        }
        let mut params = whisper_rs::WhisperContextParameters::default();
        params.use_gpu(gpu);
        let ctx = whisper_rs::WhisperContext::new_with_params(&path.to_string_lossy(), params)
            .map_err(|e| EngineError::Failed(format!("chargement Whisper : {e:?}")))?;
        let ctx = std::sync::Arc::new(ctx);
        *g = Some((file, gpu, ctx.clone()));
        tracing::info!("Modèle Whisper chargé : {}", path.display());
        Ok(ctx)
    }

    /// Précharge le modèle en arrière-plan (évite la latence de la première dictée).
    pub fn preload(&self, cfg: &Config) {
        #[cfg(feature = "whisper")]
        if self.is_available(cfg) {
            if let Err(e) = self.context(cfg) {
                tracing::warn!("Préchargement Whisper : {e}");
            }
        }
        let _ = cfg;
    }

    #[cfg(feature = "whisper")]
    fn run(ctx: &whisper_rs::WhisperContext, pcm: &[i16], lang: &str, prompt: &str) -> Result<String, EngineError> {
        use whisper_rs::{FullParams, SamplingStrategy};
        let audio: Vec<f32> = pcm.iter().map(|s| *s as f32 / 32768.0).collect();
        let mut state = ctx
            .create_state()
            .map_err(|e| EngineError::Failed(format!("{e:?}")))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8) as i32)
            .unwrap_or(4);
        params.set_n_threads(threads);
        params.set_language(Some(if lang.is_empty() { "auto" } else { lang }));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        if !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }
        state
            .full(params, &audio)
            .map_err(|e| EngineError::Failed(format!("{e:?}")))?;
        let n = state
            .full_n_segments()
            .map_err(|e| EngineError::Failed(format!("{e:?}")))?;
        let mut out = String::new();
        for i in 0..n {
            if let Ok(seg) = state.full_get_segment_text(i) {
                out.push_str(&seg);
            }
        }
        Ok(out.trim().to_string())
    }
}

impl WhisperEngine {
    #[cfg(feature = "whisper")]
    async fn transcribe_impl(&self, pcm: &[i16], lang: &str, cfg: &Config) -> Result<String, EngineError> {
        let ctx = self.context(cfg)?;
        let pcm = pcm.to_vec();
        let lang = if lang == "auto" { String::new() } else { lang.to_string() };
        // Le dictionnaire personnalisé sert d'amorce (noms propres, termes métier).
        let prompt: String = cfg
            .injection
            .custom_dictionary
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        tokio::task::spawn_blocking(move || Self::run(&ctx, &pcm, &lang, &prompt))
            .await
            .map_err(|e| EngineError::Failed(e.to_string()))?
    }

    #[cfg(not(feature = "whisper"))]
    async fn transcribe_impl(&self, _pcm: &[i16], _lang: &str, _cfg: &Config) -> Result<String, EngineError> {
        Err(EngineError::ModelMissing(
            "moteur Whisper non compilé (feature `whisper`)".into(),
        ))
    }
}

#[async_trait]
impl TranscriptionEngine for WhisperEngine {
    fn name(&self) -> &'static str {
        "whisper"
    }

    async fn transcribe(&self, pcm: &[i16], lang: &str, cfg: &Config) -> Result<String, EngineError> {
        self.transcribe_impl(pcm, lang, cfg).await
    }
}
