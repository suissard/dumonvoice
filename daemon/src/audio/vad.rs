//! Détection d'activité vocale : Silero VAD (ONNX Runtime) avec repli énergétique.
//!
//! * `Vad::speech_ms` : durée vocale effective d'un enregistrement complet (filtre anti
//!   hallucination, seuil 250 ms).
//! * `SilenceTracker` : arrêt automatique après `silence_timeout_ms` de silence.

use std::path::Path;
use std::sync::Arc;

/// Taille de trame Silero à 16 kHz (32 ms).
pub const FRAME: usize = 512;
pub const FRAME_MS: u32 = 32;
pub const MIN_SPEECH_MS: u32 = 250;
pub const SILERO_FILE: &str = "silero_vad.onnx";
pub const SILERO_URL: &str =
    "https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx";

/// VAD énergétique à plancher de bruit adaptatif (repli si Silero indisponible).
#[derive(Debug, Clone)]
pub struct EnergyVad {
    floor: f32,
}

impl Default for EnergyVad {
    fn default() -> Self {
        Self { floor: 0.004 }
    }
}

impl EnergyVad {
    pub fn prob(&mut self, frame: &[i16]) -> f32 {
        let rms = super::rms_i16(frame);
        let speech = rms > (self.floor * 3.5).max(0.012);
        if rms < self.floor {
            self.floor = 0.9 * self.floor + 0.1 * rms.max(0.0005);
        } else if !speech {
            self.floor = 0.995 * self.floor + 0.005 * rms;
        }
        if speech {
            1.0
        } else {
            0.0
        }
    }
}

#[cfg(feature = "onnx")]
mod silero {
    use std::sync::Arc;

    use ort::session::Session;
    use ort::value::Tensor;

    pub struct SileroState {
        session: Arc<Session>,
        state: Vec<f32>,
        context: Vec<f32>,
    }

    pub fn load(path: &std::path::Path) -> Result<Session, ort::Error> {
        Session::builder()?
            .with_intra_threads(1)?
            .commit_from_file(path)
    }

    impl SileroState {
        pub fn new(session: Arc<Session>) -> Self {
            Self {
                session,
                state: vec![0.0; 2 * 128],
                context: vec![0.0; 64],
            }
        }

        pub fn reset(&mut self) {
            self.state = vec![0.0; 2 * 128];
            self.context = vec![0.0; 64];
        }

        pub fn prob(&mut self, frame: &[i16]) -> Result<f32, ort::Error> {
            let mut input: Vec<f32> = Vec::with_capacity(64 + frame.len());
            input.extend_from_slice(&self.context);
            input.extend(frame.iter().map(|s| *s as f32 / 32768.0));
            self.context = input[input.len() - 64..].to_vec();
            let n = input.len();
            let x = Tensor::from_array(([1usize, n], input))?;
            let st = Tensor::from_array(([2usize, 1, 128], self.state.clone()))?;
            let sr = Tensor::from_array(([1usize], vec![16_000i64]))?;
            let outputs = self
                .session
                .run(ort::inputs!["input" => x, "state" => st, "sr" => sr]?)?;
            let (_, p) = outputs["output"].try_extract_raw_tensor::<f32>()?;
            let prob = p.first().copied().unwrap_or(0.0);
            let (_, ns) = outputs["stateN"].try_extract_raw_tensor::<f32>()?;
            if ns.len() == self.state.len() {
                self.state.copy_from_slice(ns);
            }
            Ok(prob)
        }
    }
}

/// Modèle VAD partagé (chargé une fois) ; chaque `Vad` possède son propre état.
#[derive(Clone, Default)]
pub struct VadModel {
    #[cfg(feature = "onnx")]
    silero: Option<Arc<ort::session::Session>>,
}

impl VadModel {
    pub fn energy_only() -> Self {
        Self::default()
    }

    pub fn load(models_dir: &Path) -> Self {
        #[cfg(feature = "onnx")]
        {
            let path = models_dir.join(SILERO_FILE);
            if path.exists() {
                match silero::load(&path) {
                    Ok(s) => {
                        tracing::info!("Silero VAD chargé");
                        return Self {
                            silero: Some(Arc::new(s)),
                        };
                    }
                    Err(e) => tracing::warn!("Silero VAD indisponible ({e}), repli énergétique"),
                }
            } else {
                tracing::info!("Modèle Silero absent : VAD énergétique en attendant le téléchargement");
            }
        }
        let _ = models_dir;
        Self::default()
    }

    pub fn backend_name(&self) -> &'static str {
        #[cfg(feature = "onnx")]
        if self.silero.is_some() {
            return "silero";
        }
        "energy"
    }

    pub fn instance(&self) -> Vad {
        Vad {
            energy: EnergyVad::default(),
            #[cfg(feature = "onnx")]
            silero: self.silero.clone().map(silero::SileroState::new),
        }
    }
}

pub struct Vad {
    energy: EnergyVad,
    #[cfg(feature = "onnx")]
    silero: Option<silero::SileroState>,
}

impl Vad {
    pub fn reset(&mut self) {
        self.energy = EnergyVad::default();
        #[cfg(feature = "onnx")]
        if let Some(s) = self.silero.as_mut() {
            s.reset();
        }
    }

    /// Probabilité de parole d'une trame de `FRAME` échantillons.
    pub fn frame_prob(&mut self, frame: &[i16]) -> f32 {
        #[cfg(feature = "onnx")]
        if let Some(s) = self.silero.as_mut() {
            match s.prob(frame) {
                Ok(p) => return p,
                Err(e) => {
                    tracing::warn!("Erreur Silero ({e}), bascule énergétique");
                    self.silero = None;
                }
            }
        }
        self.energy.prob(frame)
    }

    /// Durée vocale effective (ms) d'un enregistrement complet.
    pub fn speech_ms(&mut self, pcm: &[i16]) -> u32 {
        self.reset();
        let mut frames = 0u32;
        for chunk in pcm.chunks_exact(FRAME) {
            if self.frame_prob(chunk) >= 0.5 {
                frames += 1;
            }
        }
        frames * FRAME_MS
    }
}

/// Arrêt automatique après un silence prolongé suivant de la parole.
pub struct SilenceTracker {
    vad: Vad,
    pending: Vec<i16>,
    speech_ms: u32,
    silence_ms: u32,
    timeout_ms: u32,
    /// Silence maximal toléré avant toute parole (abandon d'un déclenchement accidentel).
    initial_timeout_ms: u32,
    elapsed_ms: u32,
}

impl SilenceTracker {
    pub fn new(vad: Vad, timeout_ms: u32) -> Self {
        Self {
            vad,
            pending: Vec::new(),
            speech_ms: 0,
            silence_ms: 0,
            timeout_ms,
            initial_timeout_ms: 6000,
            elapsed_ms: 0,
        }
    }

    /// Alimente le tracker ; renvoie `true` lorsque la capture doit s'arrêter.
    pub fn feed(&mut self, samples: &[i16]) -> bool {
        self.pending.extend_from_slice(samples);
        let mut stop = false;
        while self.pending.len() >= FRAME {
            let frame: Vec<i16> = self.pending.drain(..FRAME).collect();
            self.elapsed_ms += FRAME_MS;
            if self.vad.frame_prob(&frame) >= 0.5 {
                self.speech_ms += FRAME_MS;
                self.silence_ms = 0;
            } else {
                self.silence_ms += FRAME_MS;
            }
            if self.speech_ms >= 96 && self.silence_ms >= self.timeout_ms {
                stop = true;
            }
            if self.speech_ms < 96 && self.elapsed_ms >= self.initial_timeout_ms {
                stop = true;
            }
        }
        stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: u32, amp: f32) -> Vec<i16> {
        let n = crate::audio::ms_to_samples(ms);
        (0..n)
            .map(|i| ((i as f32 * 0.07).sin() * amp * 32767.0) as i16)
            .collect()
    }

    #[test]
    fn silence_has_no_speech() {
        let mut v = VadModel::energy_only().instance();
        assert_eq!(v.speech_ms(&vec![0i16; 16_000]), 0);
    }

    #[test]
    fn loud_tone_counts_as_speech() {
        let mut v = VadModel::energy_only().instance();
        let mut pcm = vec![0i16; 8000];
        pcm.extend(tone(1000, 0.3));
        let ms = v.speech_ms(&pcm);
        assert!(ms >= 900, "ms={ms}");
        assert!(ms >= MIN_SPEECH_MS);
    }

    #[test]
    fn tracker_stops_after_silence() {
        let mut t = SilenceTracker::new(VadModel::energy_only().instance(), 800);
        assert!(!t.feed(&vec![0i16; 3200]));
        assert!(!t.feed(&tone(600, 0.3)));
        assert!(!t.feed(&vec![0i16; crate::audio::ms_to_samples(500)]));
        assert!(t.feed(&vec![0i16; crate::audio::ms_to_samples(400)]));
    }

    #[test]
    fn tracker_gives_up_without_speech() {
        let mut t = SilenceTracker::new(VadModel::energy_only().instance(), 1500);
        assert!(t.feed(&vec![0i16; crate::audio::ms_to_samples(6100)]));
    }
}
