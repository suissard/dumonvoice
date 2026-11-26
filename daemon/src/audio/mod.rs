//! Types audio communs : PCM 16 kHz mono S16, Ring Buffer de pré-enregistrement,
//! rééchantillonnage linéaire, encodage WAV en mémoire.

pub mod capture;
pub mod vad;
pub mod wake_word;

pub const SAMPLE_RATE: u32 = 16_000;
/// Taille du ring buffer de pré-enregistrement (500 ms).
pub const RING_MS: u32 = 500;
/// Portion du ring buffer préfixée à une capture (300 ms).
pub const PREROLL_MS: u32 = 300;

pub fn ms_to_samples(ms: u32) -> usize {
    (SAMPLE_RATE as usize * ms as usize) / 1000
}

pub fn samples_to_ms(n: usize) -> u32 {
    ((n as u64 * 1000) / SAMPLE_RATE as u64) as u32
}

/// Ring buffer circulaire de taille fixe.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    buf: Vec<i16>,
    pos: usize,
    filled: bool,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0; capacity.max(1)],
            pos: 0,
            filled: false,
        }
    }

    pub fn with_ms(ms: u32) -> Self {
        Self::new(ms_to_samples(ms))
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub fn len(&self) -> usize {
        if self.filled {
            self.buf.len()
        } else {
            self.pos
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push_slice(&mut self, data: &[i16]) {
        for &s in data {
            self.buf[self.pos] = s;
            self.pos += 1;
            if self.pos == self.buf.len() {
                self.pos = 0;
                self.filled = true;
            }
        }
    }

    /// Renvoie (dans l'ordre chronologique) au plus les `n` derniers échantillons.
    pub fn last(&self, n: usize) -> Vec<i16> {
        let n = n.min(self.len());
        let cap = self.buf.len();
        let start = (self.pos + cap - n) % cap;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(self.buf[(start + i) % cap]);
        }
        out
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.filled = false;
    }
}

/// Rééchantillonneur linéaire en flux (conserve la phase entre deux blocs).
#[derive(Debug, Clone)]
pub struct StreamResampler {
    from: u32,
    to: u32,
    /// Position fractionnaire de la prochaine sortie, relative à `prev`.
    t: f64,
    prev: Option<f32>,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            from,
            to,
            t: 0.0,
            prev: None,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.from == self.to {
            out.extend_from_slice(input);
            return;
        }
        let step = self.from as f64 / self.to as f64;
        // Signal virtuel : [prev, input...]
        let get = |i: usize, prev: Option<f32>| -> f32 {
            match prev {
                Some(p) => {
                    if i == 0 {
                        p
                    } else {
                        input[i - 1]
                    }
                }
                None => input[i],
            }
        };
        let len = input.len() + usize::from(self.prev.is_some());
        if len < 2 {
            if let Some(&l) = input.last() {
                self.prev = Some(l);
            }
            return;
        }
        let mut t = self.t;
        while t + 1.0 < len as f64 {
            let i = t.floor() as usize;
            let frac = (t - i as f64) as f32;
            let a = get(i, self.prev);
            let b = get(i + 1, self.prev);
            out.push(a + (b - a) * frac);
            t += step;
        }
        // Repositionne t relativement au dernier échantillon qui devient `prev`.
        self.t = t - (len - 1) as f64;
        self.prev = input.last().copied().or(self.prev);
    }
}

/// Rééchantillonnage linéaire d'un bloc complet.
pub fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let mut r = StreamResampler::new(from, to);
    let mut out = Vec::with_capacity(input.len() * to as usize / from.max(1) as usize + 1);
    r.process(input, &mut out);
    out
}

pub fn f32_to_i16(x: f32) -> i16 {
    let v = (x.clamp(-1.0, 1.0) * 32767.0).round();
    v as i16
}

pub fn i16_to_f32(x: i16) -> f32 {
    x as f32 / 32768.0
}

/// Niveau RMS normalisé [0, 1].
pub fn rms_i16(data: &[i16]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let sum: f64 = data.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    (sum / data.len() as f64).sqrt() as f32
}

/// Décode des octets PCM S16LE (trames binaires WebSocket).
pub fn decode_s16le(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Encapsule du PCM 16 kHz mono S16 en WAV standard (en-tête RIFF 44 octets).
pub fn encode_wav(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // taille du bloc fmt
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    v.extend_from_slice(&2u16.to_le_bytes()); // block align
    v.extend_from_slice(&16u16.to_le_bytes()); // bits/sample
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// Mélange multi-canal → mono.
pub fn downmix(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_keeps_latest() {
        let mut r = RingBuffer::new(5);
        r.push_slice(&[1, 2, 3]);
        assert_eq!(r.last(10), vec![1, 2, 3]);
        r.push_slice(&[4, 5, 6, 7]);
        assert_eq!(r.len(), 5);
        assert_eq!(r.last(5), vec![3, 4, 5, 6, 7]);
        assert_eq!(r.last(2), vec![6, 7]);
    }

    #[test]
    fn ring_sizes() {
        let r = RingBuffer::with_ms(RING_MS);
        assert_eq!(r.capacity(), 8000);
        assert_eq!(ms_to_samples(PREROLL_MS), 4800);
    }

    #[test]
    fn resample_48k_to_16k_length() {
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 / 48.0).sin()).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len={}", out.len());
    }

    #[test]
    fn stream_resampler_matches_block() {
        let input: Vec<f32> = (0..4410).map(|i| (i as f32 * 0.01).sin()).collect();
        let block = resample_linear(&input, 44_100, 16_000);
        let mut r = StreamResampler::new(44_100, 16_000);
        let mut out = vec![];
        for chunk in input.chunks(128) {
            r.process(chunk, &mut out);
        }
        assert!((out.len() as i64 - block.len() as i64).abs() <= 1);
        for (a, b) in out.iter().zip(block.iter()) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn conversions() {
        assert_eq!(f32_to_i16(1.0), 32767);
        assert_eq!(f32_to_i16(-1.5), -32767);
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(decode_s16le(&[0x01, 0x00, 0xFF, 0xFF]), vec![1, -1]);
        assert!(rms_i16(&[0; 10]) == 0.0);
        assert!(rms_i16(&[16384; 10]) > 0.49);
    }

    #[test]
    fn wav_header() {
        let w = encode_wav(&[0, 1, 2], 16_000);
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 16_000);
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 6);
    }
}
