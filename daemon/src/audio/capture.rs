//! Capture micro PC (WASAPI via `cpal` sous Windows), conversion vers PCM 16 kHz mono S16.
//!
//! Le flux tourne sur un thread dédié (les `cpal::Stream` ne sont pas `Send`). Le thread
//! surveille le périphérique par défaut et les erreurs de flux : en cas de débranchement d'un
//! casque USB/Bluetooth ou de changement du périphérique par défaut, il reconstruit
//! silencieusement le flux sur le nouveau périphérique (équivalent fonctionnel de
//! `IMMNotificationClient`). Une commande `Restart` est aussi envoyée à la sortie de veille.

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

#[cfg(windows)]
use super::{f32_to_i16, StreamResampler, SAMPLE_RATE};

/// Destination des échantillons capturés (16 kHz mono S16).
pub trait AudioSink: Send + Sync + 'static {
    fn on_audio(&self, samples: &[i16]);
}

pub enum CaptureCmd {
    SetDevice(String),
    Restart,
    SetActive(bool),
    ListDevices(mpsc::Sender<Vec<String>>),
    Shutdown,
}

#[derive(Clone)]
pub struct CaptureHandle {
    tx: mpsc::Sender<CaptureCmd>,
    current: Arc<Mutex<Option<String>>>,
}

impl CaptureHandle {
    pub fn send(&self, cmd: CaptureCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn current_device(&self) -> Option<String> {
        self.current.lock().clone()
    }

    pub fn list_devices(&self) -> Vec<String> {
        let (tx, rx) = mpsc::channel();
        self.send(CaptureCmd::ListDevices(tx));
        rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default()
    }

    /// Poignée inerte (tests, absence de périphérique).
    pub fn dummy() -> Self {
        let (tx, _rx) = mpsc::channel();
        Self {
            tx,
            current: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg(windows)]
fn list_input_devices(host: &cpal::Host) -> Vec<String> {
    host.input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

#[cfg(windows)]
fn pick_device(host: &cpal::Host, wanted: &str) -> Option<cpal::Device> {
    if wanted != "default" && !wanted.is_empty() {
        if let Ok(mut it) = host.input_devices() {
            if let Some(d) = it.find(|d| d.name().map(|n| n == wanted).unwrap_or(false)) {
                return Some(d);
            }
        }
        tracing::warn!("Micro « {wanted} » introuvable, utilisation du périphérique par défaut");
    }
    host.default_input_device()
}

#[cfg(windows)]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sink: Arc<dyn AudioSink>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let mut resampler = StreamResampler::new(config.sample_rate.0, SAMPLE_RATE);
    let mut mono: Vec<f32> = Vec::new();
    let mut res: Vec<f32> = Vec::new();
    let mut pcm: Vec<i16> = Vec::new();
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            mono.clear();
            for frame in data.chunks(channels) {
                let mut acc = 0.0f32;
                for &s in frame {
                    acc += s.to_sample::<f32>();
                }
                mono.push(acc / frame.len() as f32);
            }
            res.clear();
            resampler.process(&mono, &mut res);
            pcm.clear();
            pcm.extend(res.iter().map(|x| f32_to_i16(*x)));
            if !pcm.is_empty() {
                sink.on_audio(&pcm);
            }
        },
        move |e| {
            tracing::warn!("Erreur du flux micro : {e}");
            failed.store(true, Ordering::SeqCst);
        },
        None,
    )
}

#[cfg(windows)]
fn open(
    host: &cpal::Host,
    wanted: &str,
    sink: &Arc<dyn AudioSink>,
    failed: &Arc<AtomicBool>,
) -> Option<(cpal::Stream, String)> {
    let device = pick_device(host, wanted)?;
    let name = device.name().unwrap_or_else(|_| "Micro".into());
    let supported = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Configuration micro indisponible : {e}");
            return None;
        }
    };
    let config: cpal::StreamConfig = supported.config();
    failed.store(false, Ordering::SeqCst);
    let s = sink.clone();
    let f = failed.clone();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, s, f),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, s, f),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, s, f),
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, s, f),
        other => {
            tracing::warn!("Format d'échantillon non supporté : {other:?}");
            return None;
        }
    };
    match stream {
        Ok(st) => {
            if let Err(e) = st.play() {
                tracing::warn!("Démarrage du micro impossible : {e}");
                return None;
            }
            tracing::info!(
                "Capture micro : {name} ({} Hz, {} canaux)",
                config.sample_rate.0,
                config.channels
            );
            Some((st, name))
        }
        Err(e) => {
            tracing::warn!("Ouverture du micro impossible : {e}");
            None
        }
    }
}

/// Démarre le thread de capture.
#[cfg(windows)]
pub fn spawn(sink: Arc<dyn AudioSink>, device: String, active: bool) -> CaptureHandle {
    let (tx, rx) = mpsc::channel::<CaptureCmd>();
    let current = Arc::new(Mutex::new(None));
    let cur = current.clone();
    std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            let host = cpal::default_host();
            let failed = Arc::new(AtomicBool::new(false));
            let mut wanted = device;
            let mut active = active;
            let mut stream: Option<cpal::Stream> = None;
            let mut default_name: Option<String> = None;
            loop {
                if active && stream.is_none() {
                    if let Some((s, name)) = open(&host, &wanted, &sink, &failed) {
                        stream = Some(s);
                        *cur.lock() = Some(name);
                        default_name = host.default_input_device().and_then(|d| d.name().ok());
                    } else {
                        *cur.lock() = None;
                    }
                }
                if !active && stream.is_some() {
                    stream = None;
                    *cur.lock() = None;
                }
                match rx.recv_timeout(Duration::from_secs(3)) {
                    Ok(CaptureCmd::SetDevice(d)) => {
                        wanted = d;
                        stream = None;
                    }
                    Ok(CaptureCmd::Restart) => stream = None,
                    Ok(CaptureCmd::SetActive(a)) => active = a,
                    Ok(CaptureCmd::ListDevices(reply)) => {
                        let _ = reply.send(list_input_devices(&host));
                    }
                    Ok(CaptureCmd::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if failed.load(Ordering::SeqCst) {
                            tracing::info!("Micro perdu : bascule silencieuse");
                            stream = None;
                        } else if active && (wanted == "default" || wanted.is_empty()) {
                            let now = host.default_input_device().and_then(|d| d.name().ok());
                            if now != default_name {
                                tracing::info!("Périphérique par défaut changé : {now:?}");
                                stream = None;
                            }
                        }
                    }
                }
            }
        })
        .expect("thread audio");
    CaptureHandle { tx, current }
}

/// Hors Windows (développement / tests) : pas de capture micro PC.
#[cfg(not(windows))]
pub fn spawn(_sink: Arc<dyn AudioSink>, _device: String, _active: bool) -> CaptureHandle {
    tracing::info!("Capture micro PC désactivée hors Windows");
    let (tx, rx) = mpsc::channel::<CaptureCmd>();
    std::thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                CaptureCmd::ListDevices(reply) => {
                    let _ = reply.send(Vec::new());
                }
                CaptureCmd::Shutdown => break,
                _ => {}
            }
        }
    });
    CaptureHandle {
        tx,
        current: Arc::new(Mutex::new(None)),
    }
}
