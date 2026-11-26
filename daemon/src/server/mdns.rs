//! Publication mDNS (`<mdns_name>.local`) pour retrouver le PC même si son IP change.

use std::collections::HashMap;

use mdns_sd::{ServiceDaemon, ServiceInfo};

pub struct MdnsPublisher {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsPublisher {
    pub fn publish(mdns_name: &str, port: u16) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let host = format!("{}.local.", mdns_name.trim_end_matches(".local"));
        let instance = format!("DumonVoice {}", super::tls::hostname());
        let mut props = HashMap::new();
        props.insert("path".to_string(), "/remote".to_string());
        props.insert("app".to_string(), "dumonvoice".to_string());
        let info = ServiceInfo::new("_https._tcp.local.", &instance, &host, "", port, Some(props))?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon.register(info)?;
        tracing::info!("mDNS publié : {host} (port {port})");
        Ok(Self { daemon, fullname })
    }
}

impl Drop for MdnsPublisher {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
