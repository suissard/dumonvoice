//! Mot d'éveil local via OpenWakeWord (ONNX Runtime).
//!
//! Pipeline OpenWakeWord : audio 16 kHz → `melspectrogram.onnx` → `embedding_model.onnx`
//! (fenêtres de 76 trames mel, pas de 8) → modèle de mot d'éveil (16 embeddings) → score.
//! Les modèles de mots se trouvent dans `models/wakewords/*.onnx` (modèles fournis
//! téléchargeables + import de modèles personnalisés).

use std::path::{Path, PathBuf};

pub const CHUNK: usize = 1280; // 80 ms
pub const OWW_RELEASE: &str = "https://github.com/dscripka/openWakeWord/releases/download/v0.5.1";
pub const MEL_FILE: &str = "melspectrogram.onnx";
pub const EMB_FILE: &str = "embedding_model.onnx";

/// Mots d'éveil pré-configurés (modèles officiels OpenWakeWord).
pub const BUILTIN_WORDS: &[(&str, &str)] = &[
    ("hey_jarvis", "hey_jarvis_v0.1.onnx"),
    ("alexa", "alexa_v0.1.onnx"),
    ("hey_mycroft", "hey_mycroft_v0.1.onnx"),
];

pub fn wakewords_dir(models_dir: &Path) -> PathBuf {
    models_dir.join("wakewords")
}

/// Fichier du modèle de mot d'éveil `name` (intégré ou importé).
pub fn word_model_path(models_dir: &Path, name: &str) -> PathBuf {
    let file = BUILTIN_WORDS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| f.to_string())
        .unwrap_or_else(|| format!("{name}.onnx"));
    wakewords_dir(models_dir).join(file)
}

/// Liste des mots disponibles : intégrés + modèles `.onnx` importés par l'utilisateur.
pub fn list_words(models_dir: &Path) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = BUILTIN_WORDS
        .iter()
        .map(|(n, f)| {
            serde_json::json!({
                "name": n, "builtin": true,
                "installed": wakewords_dir(models_dir).join(f).exists()
            })
        })
        .collect();
    if let Ok(rd) = std::fs::read_dir(wakewords_dir(models_dir)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("onnx") {
                continue;
            }
            let file = p.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            if BUILTIN_WORDS.iter().any(|(_, f)| *f == file) {
                continue;
            }
            let name = file.trim_end_matches(".onnx").to_string();
            out.push(serde_json::json!({"name": name, "builtin": false, "installed": true}));
        }
    }
    out
}

/// Seuil de détection dérivé de la sensibilité (plus sensible = seuil plus bas).
pub fn threshold_from_sensitivity(sensitivity: f32) -> f32 {
    (1.0 - sensitivity).clamp(0.15, 0.9)
}

#[cfg(feature = "onnx")]
pub use imp::WakeWordDetector;

#[cfg(feature = "onnx")]
mod imp {
    use std::collections::VecDeque;
    use std::path::Path;

    use ort::session::Session;
    use ort::value::Tensor;

    use super::*;

    pub struct WakeWordDetector {
        mel: Session,
        emb: Session,
        word: Session,
        raw: VecDeque<f32>,
        pending: Vec<f32>,
        mel_frames: VecDeque<[f32; 32]>,
        embeddings: VecDeque<Vec<f32>>,
        chunks_seen: u32,
        word_inputs: usize,
    }

    fn load(path: &Path) -> anyhow::Result<Session> {
        Ok(Session::builder()?
            .with_intra_threads(1)?
            .commit_from_file(path)?)
    }

    impl WakeWordDetector {
        pub fn load(models_dir: &Path, word: &str) -> anyhow::Result<Self> {
            let mel = load(&models_dir.join("wakewords").join(MEL_FILE))?;
            let emb = load(&models_dir.join("wakewords").join(EMB_FILE))?;
            let word_path = word_model_path(models_dir, word);
            let word = load(&word_path)?;
            // Nombre d'embeddings attendus par le modèle de mot (16 pour les modèles officiels).
            let word_inputs = 16;
            let mut mel_frames = VecDeque::new();
            for _ in 0..76 {
                mel_frames.push_back([1.0f32; 32]);
            }
            Ok(Self {
                mel,
                emb,
                word,
                raw: VecDeque::new(),
                pending: Vec::new(),
                mel_frames,
                embeddings: VecDeque::new(),
                chunks_seen: 0,
                word_inputs,
            })
        }

        pub fn reset(&mut self) {
            self.raw.clear();
            self.pending.clear();
            self.embeddings.clear();
            self.chunks_seen = 0;
        }

        /// Alimente le détecteur ; renvoie le score maximal observé dans ce bloc.
        pub fn feed(&mut self, samples: &[i16]) -> anyhow::Result<f32> {
            // OpenWakeWord attend des valeurs int16 converties en float (non normalisées).
            self.pending.extend(samples.iter().map(|s| *s as f32));
            let mut best = 0.0f32;
            while self.pending.len() >= CHUNK {
                let chunk: Vec<f32> = self.pending.drain(..CHUNK).collect();
                for s in chunk {
                    self.raw.push_back(s);
                }
                while self.raw.len() > CHUNK + 480 {
                    self.raw.pop_front();
                }
                if self.raw.len() < CHUNK + 480 {
                    continue;
                }
                let score = self.process_chunk()?;
                self.chunks_seen += 1;
                // Les premières prédictions (tampons non remplis) sont ignorées.
                if self.chunks_seen > 20 {
                    best = best.max(score);
                }
            }
            Ok(best)
        }

        fn process_chunk(&mut self) -> anyhow::Result<f32> {
            let audio: Vec<f32> = self.raw.iter().copied().collect();
            let n = audio.len();
            let input = Tensor::from_array(([1usize, n], audio))?;
            let mel_name = self.mel.inputs[0].name.clone();
            let mel_out = self.mel.outputs[0].name.clone();
            let out = self.mel.run(ort::inputs![mel_name.as_str() => input]?)?;
            let (_, mel) = out[mel_out.as_str()].try_extract_raw_tensor::<f32>()?;
            for frame in mel.chunks_exact(32) {
                let mut f = [0f32; 32];
                for (i, v) in frame.iter().enumerate() {
                    f[i] = v / 10.0 + 2.0;
                }
                self.mel_frames.push_back(f);
            }
            while self.mel_frames.len() > 76 {
                self.mel_frames.pop_front();
            }
            let mut window = Vec::with_capacity(76 * 32);
            for f in &self.mel_frames {
                window.extend_from_slice(f);
            }
            let emb_in = Tensor::from_array(([1usize, 76, 32, 1], window))?;
            let emb_name = self.emb.inputs[0].name.clone();
            let emb_out = self.emb.outputs[0].name.clone();
            let out = self.emb.run(ort::inputs![emb_name.as_str() => emb_in]?)?;
            let (_, e) = out[emb_out.as_str()].try_extract_raw_tensor::<f32>()?;
            self.embeddings.push_back(e.to_vec());
            while self.embeddings.len() > self.word_inputs {
                self.embeddings.pop_front();
            }
            if self.embeddings.len() < self.word_inputs {
                return Ok(0.0);
            }
            let dim = self.embeddings[0].len();
            let mut feats = Vec::with_capacity(self.word_inputs * dim);
            for e in &self.embeddings {
                feats.extend_from_slice(e);
            }
            let w_in = Tensor::from_array(([1usize, self.word_inputs, dim], feats))?;
            let w_name = self.word.inputs[0].name.clone();
            let w_out = self.word.outputs[0].name.clone();
            let out = self.word.run(ort::inputs![w_name.as_str() => w_in]?)?;
            let (_, s) = out[w_out.as_str()].try_extract_raw_tensor::<f32>()?;
            Ok(s.first().copied().unwrap_or(0.0))
        }
    }
}

/// Fichiers nécessaires au mot d'éveil `name` : (fichier local, URL de téléchargement).
pub fn required_files(models_dir: &Path, name: &str) -> Vec<(PathBuf, Option<String>)> {
    let dir = wakewords_dir(models_dir);
    let mut v = vec![
        (dir.join(MEL_FILE), Some(format!("{OWW_RELEASE}/{MEL_FILE}"))),
        (dir.join(EMB_FILE), Some(format!("{OWW_RELEASE}/{EMB_FILE}"))),
    ];
    let builtin = BUILTIN_WORDS.iter().find(|(n, _)| *n == name);
    let path = word_model_path(models_dir, name);
    v.push((path, builtin.map(|(_, f)| format!("{OWW_RELEASE}/{f}"))));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds() {
        assert!((threshold_from_sensitivity(0.65) - 0.35).abs() < 1e-6);
        assert_eq!(threshold_from_sensitivity(1.0), 0.15);
        assert_eq!(threshold_from_sensitivity(0.0), 0.9);
    }

    #[test]
    fn model_paths() {
        let d = Path::new("/m");
        assert!(word_model_path(d, "alexa").ends_with("wakewords/alexa_v0.1.onnx"));
        assert!(word_model_path(d, "hey_dictee").ends_with("wakewords/hey_dictee.onnx"));
        let req = required_files(d, "hey_dictee");
        assert_eq!(req.len(), 3);
        assert!(req[2].1.is_none());
        assert!(required_files(d, "hey_jarvis")[2].1.as_ref().unwrap().ends_with("hey_jarvis_v0.1.onnx"));
    }
}
