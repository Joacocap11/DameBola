//! Descubrimiento mDNS (`_damebola._tcp.local.`) con `mdns-sd`.
//!
//! Se anuncia únicamente: id público, nombre visible, sistema operativo,
//! versión de protocolo y puerto. Nada sensible.

use std::net::IpAddr;
use std::sync::Mutex;

use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc::UnboundedSender;

use super::protocol::{is_device_id, sanitize_name, PROTOCOL_VERSION};

pub const SERVICE_TYPE: &str = "_damebola._tcp.local.";

#[derive(Debug, Clone)]
pub struct Announce {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub port: u16,
    /// Solo las direcciones en las que realmente se escucha.
    pub ips: Vec<IpAddr>,
}

#[derive(Debug, Clone)]
pub struct Found {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub addrs: Vec<IpAddr>,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    Found(Found),
    Lost(String),
}

pub struct Discovery {
    daemon: ServiceDaemon,
    registered: Mutex<Option<String>>,
}

impl Discovery {
    pub fn start(announce: &Announce, tx: UnboundedSender<DiscoveryEvent>) -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let me = Self { daemon, registered: Mutex::new(None) };
        me.restrict_interfaces(&announce.ips)?;
        me.announce(announce)?;

        let rx = me.daemon.browse(SERVICE_TYPE).map_err(|e| e.to_string())?;
        std::thread::Builder::new()
            .name("damebola-mdns".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    let out = match event {
                        ServiceEvent::ServiceResolved(info) => parse_resolved(&info),
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            fullname.split('.').next().filter(|id| is_device_id(id)).map(|id| DiscoveryEvent::Lost(id.to_string()))
                        }
                        _ => None,
                    };
                    if let Some(ev) = out {
                        if tx.send(ev).is_err() {
                            break;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(me)
    }

    /// Solo se usan las interfaces donde escuchamos (no VPN/públicas).
    fn restrict_interfaces(&self, ips: &[IpAddr]) -> Result<(), String> {
        self.daemon.disable_interface(IfKind::All).map_err(|e| e.to_string())?;
        for ip in ips {
            self.daemon.enable_interface(IfKind::Addr(*ip)).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn announce(&self, a: &Announce) -> Result<(), String> {
        let props = [
            ("id", a.device_id.as_str()),
            ("name", a.name.as_str()),
            ("os", a.os.as_str()),
            ("v", &PROTOCOL_VERSION.to_string()),
        ];
        let host = format!("{}.local.", a.device_id);
        let info = ServiceInfo::new(SERVICE_TYPE, &a.device_id, &host, &a.ips[..], a.port, &props[..]).map_err(|e| e.to_string())?;
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info).map_err(|e| e.to_string())?;
        *self.registered.lock().unwrap_or_else(|e| e.into_inner()) = Some(fullname);
        Ok(())
    }

    /// Cambió la IP, el puerto o el nombre visible.
    pub fn reannounce(&self, a: &Announce) -> Result<(), String> {
        if let Some(name) = self.registered.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = self.daemon.unregister(&name);
        }
        self.restrict_interfaces(&a.ips)?;
        self.announce(a)
    }

    pub fn stop(&self) {
        if let Some(name) = self.registered.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = self.daemon.unregister(&name);
        }
        let _ = self.daemon.shutdown();
    }
}

fn parse_resolved(info: &mdns_sd::ResolvedService) -> Option<DiscoveryEvent> {
    let device_id = info.get_property_val_str("id").filter(|id| is_device_id(id))?.to_string();
    let name = sanitize_name(info.get_property_val_str("name")?)?;
    let os = match info.get_property_val_str("os")? {
        o @ ("macos" | "windows" | "linux" | "other") => o.to_string(),
        _ => "other".to_string(),
    };
    if info.get_property_val_str("v")?.parse::<u32>().ok()? != PROTOCOL_VERSION {
        return None;
    }
    let addrs: Vec<IpAddr> = info.get_addresses().iter().map(|a| a.to_ip_addr()).filter(IpAddr::is_ipv4).collect();
    if addrs.is_empty() {
        return None;
    }
    Some(DiscoveryEvent::Found(Found { device_id, name, os, addrs, port: info.get_port() }))
}
