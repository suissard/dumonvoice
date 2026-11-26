//! PKI locale auto-générée (`rcgen`) : autorité racine persistante (10 ans) et certificat
//! serveur signé incluant en SAN toutes les IPv4 privées, `localhost`, `127.0.0.1`,
//! `<hostname>.local` et `<mdns_name>.local`. Régénération automatique si l'IP change.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};

pub fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
}

/// IPv4 privées des interfaces réseau actives.
pub fn lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut v: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) if is_private_ipv4(&ip) => Some(ip),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    // Les réseaux domestiques usuels (192.168.x) en premier pour le QR code.
    v.sort_by_key(|ip| if ip.octets()[0] == 192 { 0 } else { 1 });
    v
}

pub fn hostname() -> String {
    gethostname::gethostname()
        .to_string_lossy()
        .to_string()
        .to_lowercase()
}

/// Liste SAN attendue pour un ensemble d'IP.
pub fn desired_sans(ips: &[Ipv4Addr], host: &str, mdns_name: &str) -> Vec<String> {
    let mut s: BTreeSet<String> = BTreeSet::new();
    s.insert("localhost".into());
    s.insert("127.0.0.1".into());
    for ip in ips {
        s.insert(ip.to_string());
    }
    let clean = |n: &str| -> String {
        n.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_lowercase()
    };
    let h = clean(host);
    if !h.is_empty() {
        s.insert(format!("{h}.local"));
    }
    let m = clean(mdns_name);
    if !m.is_empty() {
        s.insert(format!("{m}.local"));
    }
    s.into_iter().collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Meta {
    sans: Vec<String>,
    not_after_unix: i64,
}

pub struct CertPaths {
    pub dir: PathBuf,
}

impl CertPaths {
    pub fn new(app_dir: &Path) -> Self {
        Self {
            dir: app_dir.join("certs"),
        }
    }
    pub fn ca_crt(&self) -> PathBuf {
        self.dir.join("ca.crt")
    }
    pub fn ca_key(&self) -> PathBuf {
        self.dir.join("ca.key")
    }
    pub fn server_crt(&self) -> PathBuf {
        self.dir.join("server.crt")
    }
    pub fn server_key(&self) -> PathBuf {
        self.dir.join("server.key")
    }
    fn meta(&self) -> PathBuf {
        self.dir.join("server.meta.json")
    }
}

fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "DumonVoice Local CA");
    dn.push(DnType::OrganizationName, "DumonVoice (local)");
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    p
}

fn write_private(path: &Path, data: &str) -> std::io::Result<()> {
    std::fs::write(path, data)
}

/// Charge ou crée l'autorité racine. Renvoie (certificat émetteur, clé, PEM du CA).
fn load_or_create_ca(paths: &CertPaths) -> anyhow::Result<(Certificate, KeyPair, String)> {
    std::fs::create_dir_all(&paths.dir)?;
    if let (Ok(key_pem), Ok(crt_pem)) = (
        std::fs::read_to_string(paths.ca_key()),
        std::fs::read_to_string(paths.ca_crt()),
    ) {
        if let Ok(key) = KeyPair::from_pem(&key_pem) {
            // Reconstruit un émetteur équivalent (même DN, même clé) pour signer.
            let issuer = ca_params().self_signed(&key)?;
            return Ok((issuer, key, crt_pem));
        }
        tracing::warn!("Clé CA illisible : régénération de la PKI locale");
    }
    let key = KeyPair::generate()?;
    let mut p = ca_params();
    let now = time::OffsetDateTime::now_utc();
    p.not_before = now - time::Duration::days(1);
    p.not_after = now + time::Duration::days(3650);
    let cert = p.self_signed(&key)?;
    let pem = cert.pem();
    write_private(&paths.ca_key(), &key.serialize_pem())?;
    std::fs::write(paths.ca_crt(), &pem)?;
    // Une nouvelle CA invalide l'ancien certificat serveur.
    let _ = std::fs::remove_file(paths.meta());
    tracing::info!("Autorité de certification locale générée");
    Ok((cert, key, pem))
}

/// Garantit un certificat serveur valide couvrant les SAN courants.
/// Renvoie `(chaîne PEM, clé PEM, régénéré ?)`.
pub fn ensure_server_cert(paths: &CertPaths, sans: &[String]) -> anyhow::Result<(Vec<u8>, Vec<u8>, bool)> {
    let (issuer, ca_key, ca_pem) = load_or_create_ca(paths)?;
    let meta: Meta = std::fs::read_to_string(paths.meta())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let covers = sans.iter().all(|s| meta.sans.contains(s));
    let fresh = meta.not_after_unix - now.unix_timestamp() > 30 * 86_400;
    if covers && fresh {
        if let (Ok(c), Ok(k)) = (std::fs::read(paths.server_crt()), std::fs::read(paths.server_key())) {
            return Ok((c, k, false));
        }
    }
    let mut p = CertificateParams::new(sans.to_vec())?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("DumonVoice {}", hostname()));
    p.distinguished_name = dn;
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
    p.use_authority_key_identifier_extension = true;
    p.not_before = now - time::Duration::days(1);
    // ≤ 398 jours : exigence iOS / Chrome pour les certificats serveur.
    let not_after = now + time::Duration::days(397);
    p.not_after = not_after;
    let key = KeyPair::generate()?;
    let cert = p.signed_by(&key, &issuer, &ca_key)?;
    let chain = format!("{}{}", cert.pem(), ca_pem);
    let key_pem = key.serialize_pem();
    std::fs::write(paths.server_crt(), &chain)?;
    write_private(&paths.server_key(), &key_pem)?;
    let meta = Meta {
        sans: sans.to_vec(),
        not_after_unix: not_after.unix_timestamp(),
    };
    std::fs::write(paths.meta(), serde_json::to_vec_pretty(&meta)?)?;
    tracing::info!("Certificat serveur généré pour {:?}", sans);
    Ok((chain.into_bytes(), key_pem.into_bytes(), true))
}

/// Certificat racine au format DER (installation sur Android / iOS).
pub fn ca_der(paths: &CertPaths) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let pem = std::fs::read_to_string(paths.ca_crt()).ok()?;
    let b64: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges() {
        assert!(is_private_ipv4(&"192.168.1.20".parse().unwrap()));
        assert!(is_private_ipv4(&"10.0.0.2".parse().unwrap()));
        assert!(is_private_ipv4(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ipv4(&"172.31.255.1".parse().unwrap()));
        assert!(!is_private_ipv4(&"172.32.0.1".parse().unwrap()));
        assert!(!is_private_ipv4(&"8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn sans_list() {
        let s = desired_sans(&["192.168.1.5".parse().unwrap()], "PC-BUREAU", "dictee-pc");
        assert!(s.contains(&"192.168.1.5".to_string()));
        assert!(s.contains(&"localhost".to_string()));
        assert!(s.contains(&"127.0.0.1".to_string()));
        assert!(s.contains(&"pc-bureau.local".to_string()));
        assert!(s.contains(&"dictee-pc.local".to_string()));
    }

    #[test]
    fn generates_and_reuses_then_regenerates_on_ip_change() {
        let dir = std::env::temp_dir().join(format!("dv-tls-{}", uuid::Uuid::new_v4()));
        let paths = CertPaths::new(&dir);
        let s1 = desired_sans(&["192.168.1.5".parse().unwrap()], "pc", "dictee-pc");
        let (c1, _, regen1) = ensure_server_cert(&paths, &s1).unwrap();
        assert!(regen1);
        let ca1 = std::fs::read_to_string(paths.ca_crt()).unwrap();
        let (c2, _, regen2) = ensure_server_cert(&paths, &s1).unwrap();
        assert!(!regen2);
        assert_eq!(c1, c2);
        let s2 = desired_sans(&["192.168.1.77".parse().unwrap()], "pc", "dictee-pc");
        let (_, _, regen3) = ensure_server_cert(&paths, &s2).unwrap();
        assert!(regen3);
        // La CA reste stable.
        assert_eq!(std::fs::read_to_string(paths.ca_crt()).unwrap(), ca1);
        assert!(ca_der(&paths).unwrap().len() > 100);
        let _ = std::fs::remove_dir_all(dir);
    }
}
