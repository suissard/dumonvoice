//! Authentification : jeton local (PWA Desktop) et appairage QR Code des smartphones.
//!
//! * `pair_code` : 32 octets aléatoires base64url, valide 120 s, usage unique.
//! * `device_token` : 32 octets aléatoires, stocké uniquement haché (SHA-256) dans
//!   `config.json`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use parking_lot::Mutex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::config::{ConfigStore, PairedDevice};

pub const PAIR_CODE_TTL: Duration = Duration::from_secs(120);

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub fn sha256_hex(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Comparaison en temps constant.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// PWA Desktop sur `127.0.0.1`.
    Local,
    /// Smartphone appairé.
    Device { id: String, name: String },
}

impl Principal {
    pub fn device_id(&self) -> Option<&str> {
        match self {
            Principal::Local => None,
            Principal::Device { id, .. } => Some(id),
        }
    }

    /// Identifiant de source audio / texte.
    pub fn source_id(&self) -> String {
        self.device_id().unwrap_or("local").to_string()
    }
}

pub struct AuthState {
    pub local_token: String,
    pair_codes: Mutex<HashMap<String, Instant>>,
    cfg: Arc<ConfigStore>,
}

pub struct Exchanged {
    pub device_id: String,
    pub device_token: String,
}

impl AuthState {
    pub fn new(cfg: Arc<ConfigStore>) -> Self {
        Self {
            local_token: random_token(32),
            pair_codes: Mutex::new(HashMap::new()),
            cfg,
        }
    }

    pub fn verify_local(&self, token: &str) -> bool {
        !token.is_empty() && ct_eq(token, &self.local_token)
    }

    pub fn create_pair_code(&self) -> String {
        let code = random_token(32);
        let mut g = self.pair_codes.lock();
        let now = Instant::now();
        g.retain(|_, exp| *exp > now);
        g.insert(code.clone(), now + PAIR_CODE_TTL);
        code
    }

    /// Échange un code d'appairage (usage unique) contre un jeton d'appareil permanent.
    pub fn exchange(&self, code: &str, device_name: &str) -> Option<Exchanged> {
        let valid = {
            let mut g = self.pair_codes.lock();
            match g.remove(code) {
                Some(exp) => exp > Instant::now(),
                None => false,
            }
        };
        if !valid {
            return None;
        }
        let token = random_token(32);
        let id = format!("dev_{}", &sha256_hex(&random_token(16))[..10]);
        let name: String = device_name.trim().chars().take(60).collect();
        let name = if name.is_empty() { "Téléphone".to_string() } else { name };
        let dev = PairedDevice {
            id: id.clone(),
            name,
            token_hash: sha256_hex(&token),
            paired_at: chrono::Utc::now(),
        };
        self.cfg.update(|c| c.remote_mobile.paired_devices.push(dev));
        Some(Exchanged {
            device_id: id,
            device_token: token,
        })
    }

    pub fn verify_device(&self, token: &str) -> Option<Principal> {
        if token.is_empty() {
            return None;
        }
        let h = sha256_hex(token);
        self.cfg.read(|c| {
            c.remote_mobile
                .paired_devices
                .iter()
                .find(|d| ct_eq(&d.token_hash, &h))
                .map(|d| Principal::Device {
                    id: d.id.clone(),
                    name: d.name.clone(),
                })
        })
    }

    pub fn revoke(&self, id: &str) -> bool {
        self.cfg.update(|c| {
            let before = c.remote_mobile.paired_devices.len();
            c.remote_mobile.paired_devices.retain(|d| d.id != id);
            before != c.remote_mobile.paired_devices.len()
        })
    }
}

/// Extrait la valeur d'un cookie de l'en-tête `Cookie`.
pub fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn auth() -> AuthState {
        AuthState::new(Arc::new(ConfigStore::in_memory(Config::default())))
    }

    #[test]
    fn pairing_is_single_use() {
        let a = auth();
        let code = a.create_pair_code();
        assert_eq!(code.len(), 43); // 32 octets base64url sans padding
        let ex = a.exchange(&code, "Pixel").unwrap();
        assert!(a.exchange(&code, "Pixel").is_none());
        let p = a.verify_device(&ex.device_token).unwrap();
        assert_eq!(p.device_id(), Some(ex.device_id.as_str()));
        // Le jeton n'est stocké que haché.
        let cfg = a.cfg.get();
        assert_ne!(cfg.remote_mobile.paired_devices[0].token_hash, ex.device_token);
        assert!(a.verify_device("faux").is_none());
    }

    #[tokio::test]
    async fn pair_code_expires() {
        tokio::time::pause();
        let a = auth();
        let code = a.create_pair_code();
        tokio::time::advance(PAIR_CODE_TTL + Duration::from_secs(1)).await;
        assert!(a.exchange(&code, "x").is_none());
    }

    #[test]
    fn revoke_device() {
        let a = auth();
        let ex = a.exchange(&a.create_pair_code(), "iPhone").unwrap();
        assert!(a.revoke(&ex.device_id));
        assert!(a.verify_device(&ex.device_token).is_none());
        assert!(!a.revoke(&ex.device_id));
    }

    #[test]
    fn local_token_and_cookies() {
        let a = auth();
        assert!(a.verify_local(&a.local_token.clone()));
        assert!(!a.verify_local(""));
        assert!(!a.verify_local("nope"));
        assert_eq!(cookie_value("a=1; dv_local=xyz; b=2", "dv_local").as_deref(), Some("xyz"));
        assert_eq!(cookie_value("a=1", "dv_local"), None);
    }
}
