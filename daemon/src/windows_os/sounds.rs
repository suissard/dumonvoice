//! Retours sonores doux et courts, synthétisés en mémoire (aucun fichier externe).

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

use crate::platform::Sound;

const RATE: u32 = 22_050;

fn tone(segments: &[(f32, u32)], volume: f32) -> Vec<u8> {
    let mut pcm: Vec<i16> = Vec::new();
    for (freq, ms) in segments {
        let n = (RATE * ms / 1000) as usize;
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            // Enveloppe douce (attaque/relâchement 8 ms) pour éviter les clics.
            let a = (i as f32 / (RATE as f32 * 0.008)).min(1.0);
            let r = ((n - i) as f32 / (RATE as f32 * 0.008)).min(1.0);
            let s = (2.0 * std::f32::consts::PI * freq * t).sin() * volume * a * r;
            pcm.push((s * 32767.0) as i16);
        }
    }
    crate::audio::encode_wav(&pcm, RATE)
}

fn data(sound: Sound) -> &'static [u8] {
    static START: OnceLock<Vec<u8>> = OnceLock::new();
    static STOP: OnceLock<Vec<u8>> = OnceLock::new();
    static RECV: OnceLock<Vec<u8>> = OnceLock::new();
    static ERR: OnceLock<Vec<u8>> = OnceLock::new();
    match sound {
        Sound::Start => START.get_or_init(|| tone(&[(660.0, 45), (880.0, 70)], 0.18)),
        Sound::Stop => STOP.get_or_init(|| tone(&[(880.0, 45), (660.0, 70)], 0.16)),
        Sound::Received => RECV.get_or_init(|| tone(&[(990.0, 50)], 0.12)),
        Sound::Error => ERR.get_or_init(|| tone(&[(220.0, 90), (180.0, 120)], 0.2)),
    }
}

pub fn play(sound: Sound) {
    let bytes = data(sound);
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(bytes.as_ptr() as *const u16),
            HMODULE::default(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}
