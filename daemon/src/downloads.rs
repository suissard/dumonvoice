//! Téléchargement de modèles (Whisper, Silero VAD, OpenWakeWord) avec progression temps réel.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::protocol::ServerEvent;
use crate::EventTx;

pub const WHISPER_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Catalogue des modèles Whisper proposés (profil, fichier, taille approximative en Mo).
pub const WHISPER_MODELS: &[(&str, &str, u32)] = &[
    ("fast", "ggml-tiny-q5_1.bin", 31),
    ("fast", "ggml-base-q5_1.bin", 57),
    ("balanced", "ggml-small-q5_1.bin", 181),
    ("balanced", "ggml-medium-q5_0.bin", 514),
    ("accurate", "ggml-large-v3-turbo-q5_0.bin", 547),
];

pub fn whisper_url(file: &str) -> Option<String> {
    WHISPER_MODELS
        .iter()
        .any(|(_, f, _)| *f == file)
        .then(|| format!("{WHISPER_BASE_URL}/{file}"))
}

pub fn models_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("models")
}

/// Télécharge `url` vers `dest` (fichier `.part` puis renommage atomique).
pub async fn download(url: &str, dest: &Path, label: &str, events: &EventTx) -> anyhow::Result<()> {
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    let resp = client.get(url).send().await?.error_for_status()?;
    let total = resp.content_length();
    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last = Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if last.elapsed() > Duration::from_millis(250) {
            last = Instant::now();
            let _ = events.send(ServerEvent::DownloadProgress {
                model: label.into(),
                downloaded,
                total,
                done: false,
                error: None,
            });
        }
    }
    file.flush().await?;
    drop(file);
    if let Some(t) = total {
        if downloaded != t {
            let _ = tokio::fs::remove_file(&tmp).await;
            anyhow::bail!("téléchargement incomplet ({downloaded}/{t} octets)");
        }
    }
    tokio::fs::rename(&tmp, dest).await?;
    let _ = events.send(ServerEvent::DownloadProgress {
        model: label.into(),
        downloaded,
        total,
        done: true,
        error: None,
    });
    Ok(())
}

/// Variante qui publie l'erreur éventuelle dans la progression.
pub async fn download_reported(url: &str, dest: &Path, label: &str, events: &EventTx) -> bool {
    match download(url, dest, label, events).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Téléchargement {label} échoué : {e}");
            let _ = events.send(ServerEvent::DownloadProgress {
                model: label.into(),
                downloaded: 0,
                total: None,
                done: true,
                error: Some(e.to_string()),
            });
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_urls() {
        assert_eq!(
            whisper_url("ggml-small-q5_1.bin").unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin"
        );
        assert!(whisper_url("../../etc/passwd").is_none());
    }
}
