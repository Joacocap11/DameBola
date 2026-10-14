//! Servicio de red P2P: servidor y cliente WebSocket sobre TLS 1.3 con
//! autenticación mutua y pinning de identidad, emparejamiento verificado,
//! heartbeats, reconexión y alertas remotas. No depende de Tauri.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::antispam::{DedupCache, RateLimiter, TokenBucket};
use super::discovery::{Announce, Discovery, DiscoveryEvent, Found};
use super::identity::{device_id_from_fingerprint, format_fingerprint, hex, Fingerprint, Identity, KeyStorage};
use super::protocol::*;
use super::store::{NetStore, NetworkSettings, PairedDevice};
use super::tls::{self, PeerPin, SERVER_NAME};
use crate::error::AppResult;

// ------------------------------------------------------------------ DTOs

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetStatus {
    pub enabled: bool,
    pub state: &'static str,
    pub error: Option<String>,
    pub device_id: String,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub port: u16,
    pub addresses: Vec<String>,
    pub mdns: &'static str,
    pub mdns_error: Option<String>,
    pub key_storage: KeyStorage,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub connection: &'static str,
    pub pairing: &'static str,
    pub discovered: bool,
    pub addresses: Vec<String>,
    pub port: Option<u16>,
    pub last_seen_ms: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub code: String,
    pub role: &'static str,
    pub local_decision: &'static str,
    pub peer_decision: &'static str,
    pub state: &'static str,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AlertSendResult {
    /// shown | rejected | offline | timeout | error
    pub status: &'static str,
    /// El destinatario validó y aceptó antes de mostrar.
    pub accepted: bool,
    pub reason: Option<String>,
    pub message: String,
}

impl AlertSendResult {
    fn fail(status: &'static str, accepted: bool, reason: Option<&str>, message: impl Into<String>) -> Self {
        Self { status, accepted, reason: reason.map(String::from), message: message.into() }
    }
}

// --------------------------------------------------------------- contratos

pub trait NetEvents: Send + Sync {
    fn status(&self, status: &NetStatus);
    fn devices(&self, devices: &[DeviceView]);
    fn pairing(&self, pairing: &[PairingView]);
}

#[derive(Debug)]
pub enum RemoteAlertError {
    Busy,
    Failed(String),
}

/// Punto de integración con el sistema de alertas de la Fase 1.
/// Debe devolver `Ok` solo cuando las ventanas ya se mostraron.
pub trait AlertHandler: Send + Sync {
    fn show(&self, sender_name: &str) -> Result<(), RemoteAlertError>;
}

#[derive(Clone)]
pub struct NetTimings {
    pub connect_timeout: Duration,
    pub hello_timeout: Duration,
    pub ping_interval: Duration,
    pub idle_timeout: Duration,
    pub ack_timeout: Duration,
    pub pairing_timeout: Duration,
    pub maintenance_tick: Duration,
    pub iface_check: Duration,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// Un salto de reloj mayor que esto entre ticks indica suspensión/reanudación.
    pub resume_gap: Duration,
    pub discovery_ttl: Duration,
}

impl Default for NetTimings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            hello_timeout: Duration::from_secs(10),
            ping_interval: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(30),
            ack_timeout: Duration::from_secs(15),
            pairing_timeout: Duration::from_secs(120),
            maintenance_tick: Duration::from_secs(2),
            iface_check: Duration::from_secs(5),
            backoff_base: Duration::from_secs(2),
            backoff_max: Duration::from_secs(60),
            resume_gap: Duration::from_secs(20),
            discovery_ttl: Duration::from_secs(600),
        }
    }
}

pub struct NetConfig {
    pub app_version: String,
    pub os: &'static str,
    /// `None` = interfaces LAN reales. `Some` = fijar (pruebas).
    pub bind_override: Option<Vec<IpAddr>>,
    pub mdns: bool,
    pub timings: NetTimings,
    pub name: Arc<dyn Fn() -> String + Send + Sync>,
}

pub fn current_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => "other",
    }
}

// ------------------------------------------------------------------ estado

const MAX_CONNS: usize = 32;
const MAX_HANDSHAKES: usize = 16;
const MAX_PAIRING_SESSIONS: usize = 3;
const PAIRING_REQUEST_COOLDOWN: Duration = Duration::from_secs(10);
const MAX_STRIKES: u32 = 3;

struct Conn {
    fp: Fingerprint,
    addr: SocketAddr,
    initiator_local: bool,
    tx: mpsc::Sender<Message>,
    cancel: CancellationToken,
    hello: Option<Hello>,
    sas: String,
    strikes: u32,
}

#[derive(Clone)]
struct Discovered {
    name: String,
    os: String,
    addrs: Vec<IpAddr>,
    port: u16,
    last_seen_ms: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PairState {
    Active,
    Completed,
    Rejected,
    Expired,
}

struct Pairing {
    name: String,
    os: String,
    code: String,
    initiator: bool,
    conn_id: u64,
    fp: Fingerprint,
    addr: SocketAddr,
    local_accepted: bool,
    peer_accepted: bool,
    state: PairState,
    message: Option<String>,
    started: Instant,
    remove_at: Option<Instant>,
}

struct Backoff {
    failures: u32,
    next_at: Instant,
}

struct Running {
    cancel: CancellationToken,
    discovery: Option<Arc<Discovery>>,
    listeners: HashMap<IpAddr, CancellationToken>,
}

struct State {
    settings: NetworkSettings,
    running: Option<Running>,
    start_error: Option<String>,
    mdns_error: Option<String>,
    addresses: Vec<IpAddr>,
    bound_port: Option<u16>,
    conns: HashMap<u64, Conn>,
    discovered: HashMap<String, Discovered>,
    pairing: HashMap<String, Pairing>,
    connecting: HashSet<String>,
    errors: HashMap<String, String>,
    backoff: HashMap<String, Backoff>,
    pending_alerts: HashMap<Uuid, (u64, mpsc::UnboundedSender<Body>)>,
    rate: RateLimiter<String>,
    dedup: DedupCache<(String, Uuid), Body>,
    pairing_requests: HashMap<String, Instant>,
    /// Dispositivos que el usuario desconectó a propósito: no se reconectan solos.
    paused: HashSet<String>,
}

struct Inner {
    identity: Identity,
    device_id: String,
    store: NetStore,
    events: Arc<dyn NetEvents>,
    alerts: Arc<dyn AlertHandler>,
    cfg: NetConfig,
    state: Mutex<State>,
    next_conn: AtomicU64,
    handshakes: Arc<Semaphore>,
}

#[derive(Clone)]
pub struct NetworkService {
    inner: Arc<Inner>,
}

struct SessionMeta {
    conn_id: u64,
    cancel: CancellationToken,
    ready: Option<oneshot::Sender<Result<String, String>>>,
    rx: mpsc::Receiver<Message>,
}

struct SessionCtx {
    hello_done: bool,
    ready: Option<oneshot::Sender<Result<String, String>>>,
}

enum Flow {
    Continue,
    Close,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Direcciones IPv4 de LAN (privadas o link-local) de interfaces activas.
pub fn lan_addresses() -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && i.is_oper_up())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) if v4.is_private() || v4.is_link_local() => Some(IpAddr::V4(v4)),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Regla de desempate cuando hay dos conexiones con el mismo dispositivo:
/// se conserva la iniciada por el dispositivo de id menor (ambos lados
/// llegan a la misma conclusión). Devuelve `true` si la nueva gana.
fn new_connection_wins(local_id: &str, peer_id: &str, new_initiator_local: bool, old_initiator_local: bool) -> bool {
    let local_is_lower = local_id < peer_id;
    let new_preferred = new_initiator_local == local_is_lower;
    let old_preferred = old_initiator_local == local_is_lower;
    // Si ambas son (o no son) la preferida, gana la más nueva.
    new_preferred || !old_preferred
}

fn backoff_delay(base: Duration, max: Duration, failures: u32) -> Duration {
    let exp = base.saturating_mul(1u32 << failures.min(10));
    let capped = exp.min(max);
    capped + capped.mul_f64(fastrand::f64() * 0.25)
}

fn reject_message(r: RejectReason) -> &'static str {
    match r {
        RejectReason::Unauthorized => "El otro equipo no te tiene emparejado. Volvé a emparejar los dispositivos.",
        RejectReason::RateLimited => "Esperá unos segundos: el otro equipo limita la frecuencia de alertas.",
        RejectReason::Muted => "El otro equipo silenció las alertas por un rato.",
        RejectReason::Disabled => "El otro equipo desactivó las alertas remotas.",
        RejectReason::Busy => "El otro equipo ya está mostrando una alerta.",
        RejectReason::Failed => "El otro equipo no pudo mostrar la alerta.",
    }
}

impl NetworkService {
    pub fn new(identity: Identity, store: NetStore, events: Arc<dyn NetEvents>, alerts: Arc<dyn AlertHandler>, cfg: NetConfig) -> AppResult<Self> {
        let settings = store.settings()?;
        let device_id = identity.device_id();
        let state = State {
            settings,
            running: None,
            start_error: None,
            mdns_error: None,
            addresses: vec![],
            bound_port: None,
            conns: HashMap::new(),
            discovered: HashMap::new(),
            pairing: HashMap::new(),
            connecting: HashSet::new(),
            errors: HashMap::new(),
            backoff: HashMap::new(),
            pending_alerts: HashMap::new(),
            rate: RateLimiter::new(),
            dedup: DedupCache::new(256, Duration::from_secs(600)),
            pairing_requests: HashMap::new(),
            paused: HashSet::new(),
        };
        Ok(Self {
            inner: Arc::new(Inner {
                identity,
                device_id,
                store,
                events,
                alerts,
                cfg,
                state: Mutex::new(state),
                next_conn: AtomicU64::new(1),
                handshakes: Arc::new(Semaphore::new(MAX_HANDSHAKES)),
            }),
        })
    }

    // ----------------------------------------------------------- consultas

    pub fn status(&self) -> NetStatus {
        self.inner.status()
    }

    pub fn devices(&self) -> Vec<DeviceView> {
        self.inner.devices()
    }

    pub fn pairing(&self) -> Vec<PairingView> {
        self.inner.pairing_views()
    }

    pub fn settings(&self) -> NetworkSettings {
        self.inner.st().settings.clone()
    }

    pub fn device_id(&self) -> &str {
        &self.inner.device_id
    }

    pub fn listening_port(&self) -> Option<u16> {
        self.inner.st().bound_port
    }

    // ------------------------------------------------------------ ciclo de vida

    /// Arranca si la red está habilitada en los ajustes.
    pub async fn start(&self) {
        if self.inner.st().settings.enabled {
            Inner::start_network(&self.inner).await;
        }
        self.inner.emit_all();
    }

    pub async fn stop(&self) {
        self.inner.stop_network();
        self.inner.emit_all();
    }

    pub async fn save_settings(&self, new: NetworkSettings) -> AppResult<NetworkSettings> {
        let saved = self.inner.store.save_settings(new)?;
        let restart = {
            let mut st = self.inner.st();
            let restart = st.settings.enabled != saved.enabled || st.settings.port != saved.port;
            st.settings = saved.clone();
            restart
        };
        if restart {
            self.inner.stop_network();
            if saved.enabled {
                Inner::start_network(&self.inner).await;
            }
        }
        self.inner.emit_all();
        Ok(saved)
    }

    /// El nombre visible cambió: re-anunciar.
    pub fn refresh_announcement(&self) {
        self.inner.reannounce();
    }

    // ------------------------------------------------------------ acciones

    pub async fn add_manual(&self, host: &str, port: u16) -> Result<DeviceView, String> {
        Inner::add_manual(&self.inner, host, port).await
    }

    pub async fn connect(&self, device_id: &str) -> Result<(), String> {
        Inner::connect_device(&self.inner, device_id, true).await
    }

    pub fn disconnect(&self, device_id: &str) {
        self.inner.close_device(device_id);
        self.inner.emit_all();
    }

    pub async fn pair(&self, device_id: &str) -> Result<(), String> {
        Inner::start_pairing(&self.inner, device_id).await
    }

    pub fn pairing_decision(&self, device_id: &str, accept: bool) {
        self.inner.pairing_decision(device_id, accept);
    }

    /// Revoca el emparejamiento (avisa al otro equipo si hay conexión).
    pub async fn forget(&self, device_id: &str) -> AppResult<()> {
        self.inner.forget(device_id).await
    }

    pub async fn send_alert(&self, device_id: &str) -> AlertSendResult {
        Inner::send_alert(&self.inner, device_id).await
    }

    /// Cancela todas las conexiones (pruebas de reconexión).
    #[cfg(test)]
    pub fn drop_all_connections(&self) {
        let conns: Vec<CancellationToken> = self.inner.st().conns.values().map(|c| c.cancel.clone()).collect();
        for c in conns {
            c.cancel();
        }
    }

    #[cfg(test)]
    pub fn connection_count(&self) -> usize {
        self.inner.st().conns.values().filter(|c| c.hello.is_some()).count()
    }
}

// ------------------------------------------------------------ implementación

impl Inner {
    fn st(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn local_name(&self) -> String {
        sanitize_name(&(self.cfg.name)()).unwrap_or_else(|| "Dispositivo".into())
    }

    fn status(&self) -> NetStatus {
        let st = self.st();
        let running = st.running.is_some();
        NetStatus {
            enabled: st.settings.enabled,
            state: if !st.settings.enabled {
                "stopped"
            } else if st.start_error.is_some() {
                "error"
            } else if running {
                "running"
            } else {
                "starting"
            },
            error: st.start_error.clone(),
            device_id: self.device_id.clone(),
            device_name: self.local_name(),
            os: self.cfg.os.to_string(),
            fingerprint: format_fingerprint(self.identity.fingerprint()),
            port: st.bound_port.unwrap_or(st.settings.port),
            addresses: st.addresses.iter().map(|a| a.to_string()).collect(),
            mdns: if !self.cfg.mdns || !running {
                "disabled"
            } else if st.mdns_error.is_some() {
                "error"
            } else {
                "active"
            },
            mdns_error: st.mdns_error.clone(),
            key_storage: self.identity.storage(),
        }
    }

    fn devices(&self) -> Vec<DeviceView> {
        let paired = self.store.list_paired().unwrap_or_default();
        let st = self.st();
        let mut ids: Vec<String> = paired.iter().map(|p| p.device_id.clone()).collect();
        for id in st.discovered.keys().chain(st.pairing.keys()) {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        for c in st.conns.values() {
            if let Some(h) = &c.hello {
                if !ids.contains(&h.device_id) {
                    ids.push(h.device_id.clone());
                }
            }
        }
        let mut out: Vec<DeviceView> = ids
            .into_iter()
            .map(|id| {
                let p = paired.iter().find(|p| p.device_id == id);
                let d = st.discovered.get(&id);
                let live = st.conns.values().find(|c| c.hello.as_ref().is_some_and(|h| h.device_id == id));
                let pairing_active = st.pairing.get(&id).is_some_and(|s| s.state == PairState::Active);
                let name = p
                    .map(|p| p.name.clone())
                    .or_else(|| live.and_then(|c| c.hello.as_ref().map(|h| h.name.clone())))
                    .or_else(|| d.map(|d| d.name.clone()))
                    .or_else(|| st.pairing.get(&id).map(|s| s.name.clone()))
                    .unwrap_or_else(|| "Dispositivo".into());
                let os = p
                    .map(|p| p.os.clone())
                    .or_else(|| live.and_then(|c| c.hello.as_ref().map(|h| h.os.clone())))
                    .or_else(|| d.map(|d| d.os.clone()))
                    .unwrap_or_else(|| "other".into());
                let mut addresses: Vec<String> = d.map(|d| d.addrs.iter().map(|a| a.to_string()).collect()).unwrap_or_default();
                if let Some(a) = p.and_then(|p| p.last_address.clone()) {
                    if !addresses.contains(&a) {
                        addresses.push(a);
                    }
                }
                let port = d.map(|d| d.port).or_else(|| p.and_then(|p| p.last_port));
                DeviceView {
                    connection: if live.is_some() {
                        "connected"
                    } else if st.connecting.contains(&id) {
                        "connecting"
                    } else {
                        "disconnected"
                    },
                    pairing: if pairing_active {
                        "pending"
                    } else if p.is_some() {
                        "paired"
                    } else {
                        "unpaired"
                    },
                    discovered: d.is_some(),
                    last_seen_ms: live.map(|_| now_ms()).or_else(|| d.map(|d| d.last_seen_ms)).or_else(|| p.and_then(|p| p.last_seen_at)),
                    last_error: st.errors.get(&id).cloned(),
                    device_id: id,
                    name,
                    os,
                    addresses,
                    port,
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.device_id.cmp(&b.device_id)));
        out
    }

    fn pairing_views(&self) -> Vec<PairingView> {
        let st = self.st();
        let mut v: Vec<PairingView> = st
            .pairing
            .iter()
            .map(|(id, p)| PairingView {
                device_id: id.clone(),
                name: p.name.clone(),
                os: p.os.clone(),
                code: p.code.clone(),
                role: if p.initiator { "initiator" } else { "responder" },
                local_decision: if p.local_accepted { "accepted" } else { "pending" },
                peer_decision: if p.peer_accepted { "accepted" } else { "pending" },
                state: match p.state {
                    PairState::Active => "active",
                    PairState::Completed => "completed",
                    PairState::Rejected => "rejected",
                    PairState::Expired => "expired",
                },
                message: p.message.clone(),
            })
            .collect();
        v.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        v
    }

    fn emit_all(&self) {
        self.events.status(&self.status());
        self.events.devices(&self.devices());
        self.events.pairing(&self.pairing_views());
    }

    fn emit_devices(&self) {
        self.events.devices(&self.devices());
    }

    fn emit_pairing(&self) {
        self.events.pairing(&self.pairing_views());
        self.events.devices(&self.devices());
    }

    // -------------------------------------------------------- arranque / parada

    async fn start_network(this: &Arc<Self>) {
        if this.st().running.is_some() {
            return;
        }
        let cancel = CancellationToken::new();
        let addresses = this.cfg.bind_override.clone().unwrap_or_else(lan_addresses);
        let configured_port = this.st().settings.port;
        {
            let mut st = this.st();
            st.start_error = None;
            st.mdns_error = None;
            st.running = Some(Running { cancel: cancel.clone(), discovery: None, listeners: HashMap::new() });
        }
        let bind_error = Self::reconcile_listeners(this, &addresses, configured_port, &cancel).await;
        let (listening, port) = {
            let st = this.st();
            (st.running.as_ref().is_some_and(|r| !r.listeners.is_empty()), st.bound_port.unwrap_or(configured_port))
        };
        if !listening {
            let msg = bind_error.unwrap_or_else(|| "No hay una red local disponible para escuchar.".into());
            let mut st = this.st();
            // Sin interfaces LAN todavía: se queda "running" para reintentar cuando aparezca una.
            if !addresses.is_empty() {
                st.start_error = Some(msg);
            }
        }

        if this.cfg.mdns {
            let (tx, rx) = mpsc::unbounded_channel();
            match Discovery::start(&this.announce(&addresses, port), tx) {
                Ok(d) => {
                    if let Some(r) = this.st().running.as_mut() {
                        r.discovery = Some(Arc::new(d));
                    }
                    tokio::spawn(Self::discovery_loop(this.clone(), rx, cancel.clone()));
                }
                Err(e) => this.st().mdns_error = Some(e),
            }
        }
        tokio::spawn(Self::maintenance_loop(this.clone(), cancel));
    }

    fn announce(&self, ips: &[IpAddr], port: u16) -> Announce {
        Announce { device_id: self.device_id.clone(), name: self.local_name(), os: self.cfg.os.to_string(), port, ips: ips.to_vec() }
    }

    fn stop_network(&self) {
        let (running, conns) = {
            let mut st = self.st();
            let conns: Vec<CancellationToken> = st.conns.values().map(|c| c.cancel.clone()).collect();
            st.conns.clear();
            st.discovered.clear();
            st.connecting.clear();
            st.backoff.clear();
            st.pairing.clear();
            st.pending_alerts.clear();
            st.addresses.clear();
            st.bound_port = None;
            st.start_error = None;
            st.mdns_error = None;
            (st.running.take(), conns)
        };
        for c in conns {
            c.cancel();
        }
        if let Some(r) = running {
            r.cancel.cancel();
            for (_, l) in r.listeners {
                l.cancel();
            }
            if let Some(d) = r.discovery {
                d.stop();
            }
        }
    }

    fn reannounce(&self) {
        let (disc, ips, port) = {
            let st = self.st();
            (st.running.as_ref().and_then(|r| r.discovery.clone()), st.addresses.clone(), st.bound_port.unwrap_or(st.settings.port))
        };
        if let Some(d) = disc {
            if let Err(e) = d.reannounce(&self.announce(&ips, port)) {
                self.st().mdns_error = Some(e);
            }
        }
    }

    /// Abre/cierra listeners para que coincidan con `addresses`. Devuelve el último error de bind.
    async fn reconcile_listeners(this: &Arc<Self>, addresses: &[IpAddr], port: u16, cancel: &CancellationToken) -> Option<String> {
        let (existing, mut bound_port): (Vec<IpAddr>, u16) = {
            let st = this.st();
            (
                st.running.as_ref().map(|r| r.listeners.keys().copied().collect()).unwrap_or_default(),
                st.bound_port.unwrap_or(port),
            )
        };
        let mut last_error = None;
        for ip in existing.iter().filter(|ip| !addresses.contains(ip)) {
            if let Some(t) = this.st().running.as_mut().and_then(|r| r.listeners.remove(ip)) {
                t.cancel();
            }
        }
        for ip in addresses.iter().filter(|ip| !existing.contains(ip)) {
            match Self::bind_with_retry(SocketAddr::new(*ip, bound_port)).await {
                Ok(listener) => {
                    if bound_port == 0 {
                        bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                    }
                    let token = cancel.child_token();
                    if let Some(r) = this.st().running.as_mut() {
                        r.listeners.insert(*ip, token.clone());
                    }
                    tokio::spawn(Self::accept_loop(this.clone(), listener, token));
                }
                Err(e) => {
                    last_error = Some(if e.kind() == std::io::ErrorKind::AddrInUse {
                        format!("El puerto {bound_port} ya está en uso. Elegí otro en la configuración de red.")
                    } else {
                        format!("No se pudo escuchar en {ip}:{bound_port}: {e}")
                    })
                }
            }
        }
        let mut st = this.st();
        st.addresses = st.running.as_ref().map(|r| addresses.iter().copied().filter(|ip| r.listeners.contains_key(ip)).collect()).unwrap_or_default();
        st.bound_port = Some(bound_port);
        last_error
    }

    /// Tras desactivar/reactivar la red el listener anterior puede tardar unos ms en soltar el puerto.
    async fn bind_with_retry(addr: SocketAddr) -> std::io::Result<TcpListener> {
        let mut last = None;
        for _ in 0..10 {
            match TcpListener::bind(addr).await {
                Ok(l) => return Ok(l),
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && addr.port() != 0 => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::AddrInUse)))
    }

    async fn accept_loop(this: Arc<Self>, listener: TcpListener, cancel: CancellationToken) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                accepted = listener.accept() => match accepted {
                    Ok((stream, addr)) => {
                        let Ok(permit) = this.handshakes.clone().try_acquire_owned() else { continue };
                        if this.st().conns.len() >= MAX_CONNS { continue; }
                        let this = this.clone();
                        let conn_cancel = cancel.child_token();
                        tokio::spawn(async move {
                            let _permit = permit; // limita handshakes simultáneos
                            this.handle_inbound(stream, addr, conn_cancel).await;
                        });
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
                }
            }
        }
    }

    fn ws_config() -> WebSocketConfig {
        WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE_BYTES)).max_frame_size(Some(MAX_MESSAGE_BYTES))
    }

    async fn handle_inbound(self: Arc<Self>, stream: TcpStream, addr: SocketAddr, cancel: CancellationToken) {
        let _ = stream.set_nodelay(true);
        let Ok(server_cfg) = tls::server_config(&self.identity) else { return };
        let acceptor = TlsAcceptor::from(server_cfg);
        let handshake = async {
            let tls = acceptor.accept(stream).await.map_err(|e| e.to_string())?;
            let (_, conn) = tls.get_ref();
            let peer = tls::peer_fingerprint(conn.peer_certificates().map(|c| c.to_vec()).as_deref()).ok_or("sin certificado")?;
            let sas = tls::pairing_code(conn, &peer, self.identity.fingerprint()).map_err(|e| e.to_string())?;
            let ws = tokio_tungstenite::accept_async_with_config(tls, Some(Self::ws_config())).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((ws, peer, sas))
        };
        let Ok(Ok((ws, peer, sas))) = tokio::time::timeout(self.cfg.timings.hello_timeout, handshake).await else { return };
        let conn_id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let rx = self.register_conn(conn_id, peer, addr, false, sas, cancel.clone());
        self.run_session(ws, SessionMeta { conn_id, cancel, ready: None, rx }).await;
    }

    /// Conecta a `addr` y espera a que el otro extremo envíe su HELLO válido.
    async fn connect_outbound(self: &Arc<Self>, addr: SocketAddr, pin: PeerPin, cancel: CancellationToken) -> Result<String, String> {
        let t = &self.cfg.timings;
        let tcp = tokio::time::timeout(t.connect_timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| "timeout".to_string())?
            .map_err(|e| e.to_string())?;
        let _ = tcp.set_nodelay(true);
        let cfg = tls::client_config(&self.identity, pin).map_err(|e| e.to_string())?;
        let connector = TlsConnector::from(cfg);
        let name = rustls::pki_types::ServerName::try_from(SERVER_NAME).map_err(|e| e.to_string())?;
        let tls = tokio::time::timeout(t.connect_timeout, connector.connect(name, tcp))
            .await
            .map_err(|_| "timeout".to_string())?
            .map_err(|e| format!("tls: {e}"))?;
        let (_, conn) = tls.get_ref();
        let peer = tls::peer_fingerprint(conn.peer_certificates().map(|c| c.to_vec()).as_deref()).ok_or("sin certificado")?;
        let sas = tls::pairing_code(conn, self.identity.fingerprint(), &peer).map_err(|e| e.to_string())?;
        let (ws, _) = tokio::time::timeout(
            t.connect_timeout,
            tokio_tungstenite::client_async_with_config("wss://damebola.local/", tls, Some(Self::ws_config())),
        )
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|e| format!("ws: {e}"))?;

        let conn_id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let rx = self.register_conn(conn_id, peer, addr, true, sas, cancel.clone());
        let (ready_tx, ready_rx) = oneshot::channel();
        let this = self.clone();
        tokio::spawn(async move { this.run_session(ws, SessionMeta { conn_id, cancel, ready: Some(ready_tx), rx }).await });
        match tokio::time::timeout(t.hello_timeout, ready_rx).await {
            Ok(Ok(r)) => r,
            _ => Err("timeout".into()),
        }
    }

    fn register_conn(&self, conn_id: u64, fp: Fingerprint, addr: SocketAddr, initiator_local: bool, sas: String, cancel: CancellationToken) -> mpsc::Receiver<Message> {
        let (tx, rx) = mpsc::channel(32);
        self.st().conns.insert(
            conn_id,
            Conn { fp, addr, initiator_local, tx, cancel, hello: None, sas, strikes: 0 },
        );
        rx
    }

    // ------------------------------------------------------------- sesión

    async fn run_session<S>(self: Arc<Self>, mut ws: WebSocketStream<S>, meta: SessionMeta)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let SessionMeta { conn_id, cancel, ready, mut rx } = meta;
        let t = self.cfg.timings.clone();
        let mut ctx = SessionCtx { hello_done: false, ready };
        let hello = Message::new(Body::Hello(Hello {
            device_id: self.device_id.clone(),
            name: self.local_name(),
            os: self.cfg.os.to_string(),
            app_version: self.cfg.app_version.clone(),
        }));
        let write = |m: &Message| WsMessage::text(m.encode());
        let alive = ws.send(write(&hello)).await.is_ok();

        let mut bucket = TokenBucket::new(30, 10.0, Instant::now());
        let hello_deadline = tokio::time::sleep(t.hello_timeout);
        tokio::pin!(hello_deadline);
        let mut ping = tokio::time::interval(t.ping_interval);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;
        let mut last_rx = Instant::now();

        while alive {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = &mut hello_deadline, if !ctx.hello_done => break,
                _ = ping.tick() => {
                    if last_rx.elapsed() > t.idle_timeout { break; }
                    if ctx.hello_done && ws.send(write(&Message::new(Body::Ping(Empty {})))).await.is_err() { break; }
                }
                out = rx.recv() => match out {
                    Some(m) => {
                        let sent = tokio::time::timeout(Duration::from_secs(5), ws.send(write(&m))).await;
                        if !matches!(sent, Ok(Ok(()))) { break; }
                    }
                    None => break,
                },
                frame = ws.next() => match frame {
                    Some(Ok(WsMessage::Text(text))) => {
                        last_rx = Instant::now();
                        if !bucket.try_take(last_rx) { break; }
                        match Message::decode(text.as_str()) {
                            Ok(msg) => if matches!(self.handle(conn_id, msg, &mut ctx).await, Flow::Close) { break },
                            Err(_) => break,
                        }
                    }
                    Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_))) => last_rx = Instant::now(),
                    _ => break,
                },
            }
        }
        // Entrega mensajes finales pendientes (p. ej. UNPAIR / PAIR_REJECT) antes de cerrar.
        while let Ok(m) = rx.try_recv() {
            if tokio::time::timeout(Duration::from_secs(1), ws.send(write(&m))).await.is_err() {
                break;
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), ws.close(None)).await;
        // Cierre ordenado: se leen los datos pendientes hasta que el otro extremo cierre.
        // Cerrar un socket con datos sin leer provoca un RST (sobre todo en Windows) y el
        // otro lado podría perder el último mensaje (PAIR_REJECT/UNPAIR).
        let _ = tokio::time::timeout(Duration::from_millis(500), async { while ws.next().await.is_some_and(|r| r.is_ok()) {} }).await;
        self.cleanup_conn(conn_id, &mut ctx);
    }

    fn cleanup_conn(&self, conn_id: u64, ctx: &mut SessionCtx) {
        let mut expired = vec![];
        {
            let mut st = self.st();
            st.conns.remove(&conn_id);
            st.pending_alerts.retain(|_, (c, _)| *c != conn_id);
            for (id, p) in st.pairing.iter_mut() {
                if p.conn_id == conn_id && p.state == PairState::Active {
                    p.state = PairState::Expired;
                    p.message = Some("Se cerró la conexión antes de terminar el emparejamiento.".into());
                    expired.push(id.clone());
                }
            }
        }
        if let Some(r) = ctx.ready.take() {
            let _ = r.send(Err("La conexión se cerró antes de completar el saludo.".into()));
        }
        for id in expired {
            self.schedule_pairing_cleanup(id, Duration::from_secs(6));
        }
        self.emit_pairing();
    }

    fn send_body(&self, conn_id: u64, body: Body) {
        self.send_message(conn_id, Message::new(body));
    }

    fn send_message(&self, conn_id: u64, msg: Message) {
        let tx = self.st().conns.get(&conn_id).map(|c| c.tx.clone());
        if let Some(tx) = tx {
            let _ = tx.try_send(msg);
        }
    }

    fn conn_for_device(&self, device_id: &str) -> Option<u64> {
        self.st().conns.iter().find(|(_, c)| c.hello.as_ref().is_some_and(|h| h.device_id == device_id)).map(|(id, _)| *id)
    }

    fn authorize(&self, fp: &Fingerprint) -> Option<PairedDevice> {
        self.store.paired_by_fingerprint(&hex(fp)).ok().flatten()
    }

    async fn handle(self: &Arc<Self>, conn_id: u64, msg: Message, ctx: &mut SessionCtx) -> Flow {
        let Some((fp, addr, initiator_local, hello)) = self.st().conns.get(&conn_id).map(|c| (c.fp, c.addr, c.initiator_local, c.hello.clone())) else {
            return Flow::Close;
        };
        match msg.body {
            Body::Hello(h) => {
                if ctx.hello_done || h.device_id != device_id_from_fingerprint(&fp) {
                    return Flow::Close; // el id declarado debe coincidir con el certificado
                }
                if h.device_id == self.device_id {
                    if let Some(r) = ctx.ready.take() {
                        let _ = r.send(Err("Ese es este mismo equipo.".into()));
                    }
                    return Flow::Close;
                }
                ctx.hello_done = true;
                let device_id = h.device_id.clone();
                let keep = self.register_hello(conn_id, fp, h.clone(), initiator_local, addr);
                if let Some(r) = ctx.ready.take() {
                    let _ = r.send(Ok(device_id.clone()));
                }
                self.emit_devices();
                if keep { Flow::Continue } else { Flow::Close }
            }
            _ if hello.is_none() => Flow::Close, // nada antes del HELLO
            Body::Ping(_) => {
                self.send_message(conn_id, Message::new(Body::Pong(Pong { reply_to: msg.id })));
                Flow::Continue
            }
            Body::Pong(_) => Flow::Continue,
            Body::PairRequest(_) => self.on_pair_request(conn_id, fp, addr, hello.as_ref()),
            Body::PairAccept(_) => self.on_pair_accept(conn_id, hello.as_ref()),
            Body::PairConfirm(_) => self.on_pair_confirm(conn_id, hello.as_ref()),
            Body::PairReject(r) => {
                self.on_pair_reject(conn_id, hello.as_ref(), r.reason);
                Flow::Close
            }
            Body::AlertRequest(_) => self.on_alert_request(conn_id, msg.id, &fp).await,
            Body::AlertAck(a) => self.route_alert_reply(conn_id, &fp, a.reply_to, Body::AlertAck(a)),
            Body::AlertReject(a) => self.route_alert_reply(conn_id, &fp, a.reply_to, Body::AlertReject(a)),
            Body::Unpair(_) => {
                if let Some(p) = self.authorize(&fp) {
                    let _ = self.store.remove_paired(&p.device_id);
                    self.st().pairing.remove(&p.device_id);
                    self.emit_pairing();
                }
                Flow::Close
            }
        }
    }

    /// Registra el HELLO y resuelve conexiones duplicadas. `false` = esta conexión pierde.
    fn register_hello(&self, conn_id: u64, fp: Fingerprint, h: Hello, initiator_local: bool, addr: SocketAddr) -> bool {
        let name = sanitize_name(&h.name).unwrap_or_else(|| "Dispositivo".into());
        let mut losers: Vec<CancellationToken> = vec![];
        let keep = {
            let mut st = self.st();
            let duplicates: Vec<u64> = st
                .conns
                .iter()
                .filter(|(id, c)| **id != conn_id && c.hello.as_ref().is_some_and(|x| x.device_id == h.device_id))
                .map(|(id, _)| *id)
                .collect();
            let mut keep_new = true;
            for old_id in duplicates {
                let pairing_on_old = st.pairing.get(&h.device_id).is_some_and(|p| p.conn_id == old_id && p.state == PairState::Active);
                let old_init = st.conns.get(&old_id).map(|c| c.initiator_local).unwrap_or(false);
                if pairing_on_old || !new_connection_wins(&self.device_id, &h.device_id, initiator_local, old_init) {
                    keep_new = false;
                } else if let Some(old) = st.conns.remove(&old_id) {
                    losers.push(old.cancel);
                }
            }
            if keep_new {
                if let Some(c) = st.conns.get_mut(&conn_id) {
                    c.hello = Some(Hello { name: name.clone(), ..h.clone() });
                }
                st.errors.remove(&h.device_id);
                st.backoff.remove(&h.device_id);
            } else if let Some(c) = st.conns.remove(&conn_id) {
                losers.push(c.cancel);
            }
            keep_new
        };
        for l in losers {
            l.cancel();
        }
        if keep {
            if let Some(p) = self.authorize(&fp) {
                let address = initiator_local.then(|| (addr.ip().to_string(), addr.port()));
                let _ = self.store.touch_paired(&p.device_id, &name, &h.os, address);
            }
        }
        keep
    }

    // --------------------------------------------------------- emparejamiento

    fn on_pair_request(&self, conn_id: u64, fp: Fingerprint, addr: SocketAddr, hello: Option<&Hello>) -> Flow {
        let Some(h) = hello else { return Flow::Close };
        let busy = |this: &Self| {
            this.send_body(conn_id, Body::PairReject(PairReject { reason: PairRejectReason::Busy }));
            Flow::Continue
        };
        {
            let mut st = self.st();
            let active = st.pairing.values().filter(|p| p.state == PairState::Active).count();
            let exists = st.pairing.get(&h.device_id).is_some_and(|p| p.state == PairState::Active);
            let recent = st.pairing_requests.get(&h.device_id).is_some_and(|t| t.elapsed() < PAIRING_REQUEST_COOLDOWN);
            if exists || recent || active >= MAX_PAIRING_SESSIONS {
                drop(st);
                return busy(self);
            }
            st.pairing_requests.insert(h.device_id.clone(), Instant::now());
            let code = st.conns.get(&conn_id).map(|c| c.sas.clone()).unwrap_or_default();
            st.pairing.insert(
                h.device_id.clone(),
                Pairing {
                    name: h.name.clone(),
                    os: h.os.clone(),
                    code,
                    initiator: false,
                    conn_id,
                    fp,
                    addr,
                    local_accepted: false,
                    peer_accepted: false,
                    state: PairState::Active,
                    message: None,
                    started: Instant::now(),
                    remove_at: None,
                },
            );
        }
        self.emit_pairing();
        Flow::Continue
    }

    fn on_pair_accept(&self, conn_id: u64, hello: Option<&Hello>) -> Flow {
        let Some(h) = hello else { return Flow::Close };
        let both = {
            let mut st = self.st();
            match st.pairing.get_mut(&h.device_id) {
                Some(p) if p.state == PairState::Active && p.initiator && p.conn_id == conn_id && !p.peer_accepted => {
                    p.peer_accepted = true;
                    p.local_accepted
                }
                _ => return Flow::Close,
            }
        };
        if both {
            self.finalize_pairing(&h.device_id);
        } else {
            self.emit_pairing();
        }
        Flow::Continue
    }

    fn on_pair_confirm(&self, conn_id: u64, hello: Option<&Hello>) -> Flow {
        let Some(h) = hello else { return Flow::Close };
        {
            let mut st = self.st();
            match st.pairing.get_mut(&h.device_id) {
                // Solo es válido si ya aceptamos nosotros y fuimos el receptor.
                Some(p) if p.state == PairState::Active && !p.initiator && p.conn_id == conn_id && p.local_accepted => p.peer_accepted = true,
                _ => return Flow::Close,
            }
        }
        self.finalize_pairing(&h.device_id);
        Flow::Continue
    }

    fn on_pair_reject(&self, conn_id: u64, hello: Option<&Hello>, reason: PairRejectReason) {
        let Some(h) = hello else { return };
        {
            let mut st = self.st();
            if let Some(p) = st.pairing.get_mut(&h.device_id) {
                if p.state == PairState::Active && p.conn_id == conn_id {
                    p.state = PairState::Rejected;
                    p.message = Some(
                        match reason {
                            PairRejectReason::Declined => "El otro equipo rechazó el emparejamiento.",
                            PairRejectReason::Mismatch => "El otro equipo indicó que los códigos no coinciden. Cancelado por seguridad.",
                            PairRejectReason::Busy => "El otro equipo está ocupado con otro emparejamiento. Probá de nuevo en unos segundos.",
                        }
                        .into(),
                    );
                }
            }
        }
        self.schedule_pairing_cleanup(h.device_id.clone(), Duration::from_secs(6));
        self.emit_pairing();
    }

    fn finalize_pairing(&self, device_id: &str) {
        let (device, conn_id, initiator) = {
            let mut st = self.st();
            let Some(p) = st.pairing.get_mut(device_id) else { return };
            p.state = PairState::Completed;
            p.message = Some("Dispositivo emparejado.".into());
            let d = PairedDevice {
                device_id: device_id.to_string(),
                name: p.name.clone(),
                os: p.os.clone(),
                fingerprint: hex(&p.fp),
                last_address: p.initiator.then(|| p.addr.ip().to_string()),
                last_port: p.initiator.then_some(p.addr.port()),
                paired_at: now_ms(),
                last_seen_at: Some(now_ms()),
            };
            (d, p.conn_id, p.initiator)
        };
        match self.store.upsert_paired(&device) {
            Ok(()) => {
                if initiator {
                    self.send_body(conn_id, Body::PairConfirm(Empty {}));
                }
            }
            Err(e) => {
                if let Some(p) = self.st().pairing.get_mut(device_id) {
                    p.state = PairState::Rejected;
                    p.message = Some(format!("No se pudo guardar el emparejamiento: {e}"));
                }
            }
        }
        self.schedule_pairing_cleanup(device_id.to_string(), Duration::from_secs(6));
        self.emit_pairing();
    }

    /// Marca la sesión para que el ciclo de mantenimiento la retire pasado `delay`.
    fn schedule_pairing_cleanup(&self, device_id: String, delay: Duration) {
        if let Some(p) = self.st().pairing.get_mut(&device_id) {
            p.remove_at = Some(Instant::now() + delay);
        }
    }
}

impl Inner {
    async fn start_pairing(this: &Arc<Self>, device_id: &str) -> Result<(), String> {
        if this.st().running.is_none() {
            return Err("La red está desactivada.".into());
        }
        if this.store.get_paired(device_id).ok().flatten().is_some() {
            return Err("Ese dispositivo ya está emparejado.".into());
        }
        if this.st().pairing.get(device_id).is_some_and(|p| p.state == PairState::Active) {
            return Err("Ya hay un emparejamiento en curso con ese dispositivo.".into());
        }
        if this.conn_for_device(device_id).is_none() {
            Self::connect_device(this, device_id, true).await?;
        }
        let conn_id = this.conn_for_device(device_id).ok_or("No se pudo establecer la conexión.")?;
        {
            let mut st = this.st();
            let Some(c) = st.conns.get(&conn_id) else { return Err("La conexión se cerró.".into()) };
            let (Some(h), fp, addr, sas) = (c.hello.clone(), c.fp, c.addr, c.sas.clone()) else { return Err("La conexión se cerró.".into()) };
            st.pairing.insert(
                device_id.to_string(),
                Pairing {
                    name: h.name,
                    os: h.os,
                    code: sas,
                    initiator: true,
                    conn_id,
                    fp,
                    addr,
                    local_accepted: false,
                    peer_accepted: false,
                    state: PairState::Active,
                    message: None,
                    started: Instant::now(),
                    remove_at: None,
                },
            );
        }
        this.send_body(conn_id, Body::PairRequest(Empty {}));
        this.emit_pairing();
        Ok(())
    }

    fn pairing_decision(&self, device_id: &str, accept: bool) {
        enum Next {
            Nothing,
            Finalize,
            SendAccept(u64),
            Reject(u64, PairRejectReason),
        }
        let next = {
            let mut st = self.st();
            let Some(p) = st.pairing.get_mut(device_id) else { return };
            if p.state != PairState::Active {
                return;
            }
            if !accept {
                p.state = PairState::Rejected;
                p.message = Some("Cancelaste el emparejamiento.".into());
                Next::Reject(p.conn_id, if p.initiator { PairRejectReason::Mismatch } else { PairRejectReason::Declined })
            } else if p.local_accepted {
                Next::Nothing
            } else {
                p.local_accepted = true;
                if p.initiator {
                    if p.peer_accepted { Next::Finalize } else { Next::Nothing }
                } else {
                    Next::SendAccept(p.conn_id)
                }
            }
        };
        match next {
            Next::Nothing => {}
            Next::Finalize => {
                self.finalize_pairing(device_id);
                return;
            }
            Next::SendAccept(conn_id) => self.send_body(conn_id, Body::PairAccept(Empty {})),
            Next::Reject(conn_id, reason) => {
                self.send_body(conn_id, Body::PairReject(PairReject { reason }));
                self.schedule_pairing_cleanup(device_id.to_string(), Duration::from_secs(6));
                // Si el dispositivo no está emparejado, la conexión ya no sirve.
                if self.store.get_paired(device_id).ok().flatten().is_none() {
                    // El mensaje en cola se entrega antes de cerrar (ver `run_session`).
                    let token = self.st().conns.get(&conn_id).map(|c| c.cancel.clone());
                    if let Some(c) = token {
                        c.cancel();
                    }
                }
            }
        }
        self.emit_pairing();
    }

    // ----------------------------------------------------------------- alertas

    async fn on_alert_request(self: &Arc<Self>, conn_id: u64, msg_id: Uuid, fp: &Fingerprint) -> Flow {
        let reject = |this: &Self, reason: RejectReason| {
            this.send_body(conn_id, Body::AlertReject(AlertReject { reply_to: msg_id, reason }));
        };
        let Some(peer) = self.authorize(fp) else {
            let strikes = {
                let mut st = self.st();
                st.conns.get_mut(&conn_id).map(|c| { c.strikes += 1; c.strikes }).unwrap_or(MAX_STRIKES)
            };
            reject(self, RejectReason::Unauthorized);
            return if strikes >= MAX_STRIKES { Flow::Close } else { Flow::Continue };
        };

        let key = (peer.device_id.clone(), msg_id);
        let now = Instant::now();
        {
            let mut st = self.st();
            if let Some(previous) = st.dedup.check_or_insert(key.clone(), now) {
                // Duplicado: se repite la respuesta original, sin volver a mostrar nada.
                drop(st);
                if let Some(body) = previous {
                    self.send_message(conn_id, Message { id: Uuid::new_v4(), body });
                }
                return Flow::Continue;
            }
        }

        let early = {
            let st = self.st();
            if !st.settings.remote_alerts_enabled {
                Some(RejectReason::Disabled)
            } else if st.settings.is_muted(now_ms()) {
                Some(RejectReason::Muted)
            } else if st.rate.check(&peer.device_id, now, Duration::from_secs(u64::from(st.settings.alert_cooldown_secs))).is_err() {
                Some(RejectReason::RateLimited)
            } else {
                None
            }
        };
        if let Some(reason) = early {
            let body = Body::AlertReject(AlertReject { reply_to: msg_id, reason });
            self.st().dedup.set_response(&key, body.clone());
            self.send_body(conn_id, body);
            return Flow::Continue;
        }

        // Aceptada: avisa y ejecuta el sistema de alertas de la Fase 1.
        self.send_body(conn_id, Body::AlertAck(AlertAck { reply_to: msg_id, stage: AckStage::Accepted }));
        let handler = self.alerts.clone();
        let sender = peer.name.clone();
        let outcome = tokio::task::spawn_blocking(move || handler.show(&sender)).await;
        let body = match outcome {
            Ok(Ok(())) => {
                self.st().rate.record(peer.device_id.clone(), Instant::now());
                Body::AlertAck(AlertAck { reply_to: msg_id, stage: AckStage::Shown })
            }
            Ok(Err(RemoteAlertError::Busy)) => Body::AlertReject(AlertReject { reply_to: msg_id, reason: RejectReason::Busy }),
            _ => Body::AlertReject(AlertReject { reply_to: msg_id, reason: RejectReason::Failed }),
        };
        self.st().dedup.set_response(&key, body.clone());
        self.send_body(conn_id, body);
        Flow::Continue
    }

    fn route_alert_reply(&self, conn_id: u64, fp: &Fingerprint, reply_to: Uuid, body: Body) -> Flow {
        if self.authorize(fp).is_none() {
            return Flow::Close;
        }
        let terminal = matches!(&body, Body::AlertReject(_) | Body::AlertAck(AlertAck { stage: AckStage::Shown, .. }));
        let mut st = self.st();
        // Solo la conexión a la que enviamos la solicitud puede responderla.
        if let Some((owner, tx)) = st.pending_alerts.get(&reply_to) {
            if *owner == conn_id {
                let _ = tx.send(body);
                if terminal {
                    st.pending_alerts.remove(&reply_to);
                }
            }
        }
        Flow::Continue
    }

    async fn send_alert(this: &Arc<Self>, device_id: &str) -> AlertSendResult {
        let Some(paired) = this.store.get_paired(device_id).ok().flatten() else {
            return AlertSendResult::fail("error", false, Some("not_paired"), "Ese dispositivo no está emparejado.");
        };
        let conn_id = {
            let st = this.st();
            st.conns
                .iter()
                .find(|(_, c)| c.hello.as_ref().is_some_and(|h| h.device_id == device_id) && hex(&c.fp) == paired.fingerprint)
                .map(|(id, _)| *id)
        };
        let Some(conn_id) = conn_id else {
            return AlertSendResult::fail(
                "offline",
                false,
                None,
                format!("{} no está conectado. No se guardan alertas para después: probá de nuevo cuando se reconecte.", paired.name),
            );
        };
        let msg = Message::new(Body::AlertRequest(Empty {}));
        let (ptx, mut prx) = mpsc::unbounded_channel();
        this.st().pending_alerts.insert(msg.id, (conn_id, ptx));
        let id = msg.id;
        this.send_message(conn_id, msg);

        let deadline = tokio::time::Instant::now() + this.cfg.timings.ack_timeout;
        let mut accepted = false;
        let result = loop {
            match tokio::time::timeout_at(deadline, prx.recv()).await {
                Ok(Some(Body::AlertAck(a))) => match a.stage {
                    AckStage::Accepted => accepted = true,
                    AckStage::Shown => {
                        break AlertSendResult::fail("shown", true, None, format!("La alerta se mostró en {}.", paired.name));
                    }
                },
                Ok(Some(Body::AlertReject(r))) => {
                    break AlertSendResult::fail("rejected", accepted, Some(&format!("{:?}", r.reason).to_lowercase()), reject_message(r.reason));
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    break AlertSendResult::fail("offline", accepted, None, "Se perdió la conexión antes de recibir la confirmación. No se sabe si la alerta se mostró.");
                }
                Err(_) => {
                    break AlertSendResult::fail("timeout", accepted, None, "El otro equipo no confirmó a tiempo. No se puede asegurar que la alerta se haya mostrado.");
                }
            }
        };
        this.st().pending_alerts.remove(&id);
        result
    }

    // ------------------------------------------------------------- conexiones

    fn describe_error(raw: &str, pinned: bool, port: u16) -> String {
        let r = raw.to_lowercase();
        if r.contains("timeout") {
            format!("No hubo respuesta (tiempo agotado). Verificá que el otro equipo tenga DameBola abierto con la red activada, que estén en la misma red y que el firewall permita el puerto TCP {port}.")
        } else if r.contains("refused") {
            "Conexión rechazada: DameBola no está escuchando en esa dirección y puerto. Revisá el puerto y que la red esté activada en el otro equipo.".into()
        } else if r.contains("unreachable") || r.contains("no route") {
            "No se puede llegar a esa dirección desde esta red.".into()
        } else if r.starts_with("tls") && pinned {
            "La identidad del dispositivo no coincide con la que emparejaste. Puede ser otro equipo o una reinstalación: olvidá el dispositivo y volvé a emparejarlo.".into()
        } else if r.starts_with("tls") || r.starts_with("ws") {
            "No se pudo establecer una conexión segura con ese equipo (¿es DameBola?).".into()
        } else {
            raw.to_string()
        }
    }

    /// Conecta a un dispositivo conocido (descubierto o emparejado).
    async fn connect_device(this: &Arc<Self>, device_id: &str, user: bool) -> Result<(), String> {
        let cancel = {
            let mut st = this.st();
            let Some(r) = st.running.as_ref() else { return Err("La red está desactivada.".into()) };
            let cancel = r.cancel.child_token();
            if user {
                st.paused.remove(device_id);
            }
            if st.connecting.contains(device_id) || st.conns.values().any(|c| c.hello.as_ref().is_some_and(|h| h.device_id == device_id)) {
                return Ok(());
            }
            st.connecting.insert(device_id.to_string());
            cancel
        };
        this.emit_devices();

        let paired = this.store.get_paired(device_id).ok().flatten();
        let mut candidates: Vec<SocketAddr> = vec![];
        {
            let st = this.st();
            if let Some(d) = st.discovered.get(device_id) {
                candidates.extend(d.addrs.iter().map(|a| SocketAddr::new(*a, d.port)));
            }
        }
        if let Some(p) = &paired {
            if let (Some(a), Some(port)) = (p.last_address.as_ref().and_then(|a| a.parse::<IpAddr>().ok()), p.last_port) {
                let sa = SocketAddr::new(a, port);
                if !candidates.contains(&sa) {
                    candidates.push(sa);
                }
            }
        }
        let pin = match &paired {
            Some(p) => hex_to_fp(&p.fingerprint).map(PeerPin::Expect).unwrap_or(PeerPin::AcceptAny),
            None => PeerPin::AcceptAny,
        };
        let pinned = matches!(pin, PeerPin::Expect(_));

        let mut result = Err("No conozco la dirección de ese dispositivo. Agregalo manualmente con su IP y puerto.".to_string());
        for addr in candidates {
            match this.connect_outbound(addr, pin, cancel.clone()).await {
                Ok(id) if id == device_id => {
                    result = Ok(());
                    break;
                }
                Ok(_) => result = Err("En esa dirección respondió otro dispositivo.".into()),
                Err(e) => result = Err(Self::describe_error(&e, pinned, addr.port())),
            }
        }

        {
            let mut st = this.st();
            st.connecting.remove(device_id);
            match &result {
                Ok(()) => {
                    st.errors.remove(device_id);
                    st.backoff.remove(device_id);
                }
                Err(e) => {
                    st.errors.insert(device_id.to_string(), e.clone());
                    let failures = st.backoff.get(device_id).map(|b| b.failures + 1).unwrap_or(1);
                    let delay = backoff_delay(this.cfg.timings.backoff_base, this.cfg.timings.backoff_max, failures);
                    st.backoff.insert(device_id.to_string(), Backoff { failures, next_at: Instant::now() + delay });
                }
            }
        }
        this.emit_devices();
        result
    }

    /// Corta las conexiones con un dispositivo y evita reconectar solo.
    fn close_device(&self, device_id: &str) {
        let mut st = self.st();
        st.paused.insert(device_id.to_string());
        let ids: Vec<u64> = st.conns.iter().filter(|(_, c)| c.hello.as_ref().is_some_and(|h| h.device_id == device_id)).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(c) = st.conns.remove(&id) {
                c.cancel.cancel();
            }
        }
    }

    async fn forget(&self, device_id: &str) -> AppResult<()> {
        let conn = self.conn_for_device(device_id);
        if let Some(conn_id) = conn {
            // Se encola el UNPAIR y se cierra: `run_session` lo entrega antes de cerrar.
            self.send_body(conn_id, Body::Unpair(Empty {}));
        }
        self.store.remove_paired(device_id)?;
        {
            let mut st = self.st();
            st.pairing.remove(device_id);
            st.errors.remove(device_id);
            st.backoff.remove(device_id);
            st.paused.remove(device_id);
            if let Some(conn_id) = conn {
                if let Some(c) = st.conns.get(&conn_id) {
                    c.cancel.cancel();
                }
            }
        }
        self.emit_all();
        Ok(())
    }

    async fn add_manual(this: &Arc<Self>, host: &str, port: u16) -> Result<DeviceView, String> {
        let ip: IpAddr = host.trim().parse().map_err(|_| "Ingresá una dirección IP válida (por ejemplo 192.168.1.20).".to_string())?;
        if !is_lan_ip(&ip) {
            return Err("Solo se admiten direcciones de la red local (privadas o locales), no de Internet.".into());
        }
        if port == 0 {
            return Err("El puerto debe estar entre 1 y 65535.".into());
        }
        let cancel = {
            let st = this.st();
            match st.running.as_ref() {
                Some(r) => r.cancel.child_token(),
                None => return Err("La red está desactivada.".into()),
            }
        };
        let addr = SocketAddr::new(ip, port);
        // Si ya conocemos el dispositivo emparejado en esa dirección, se fija su identidad.
        let pin = this
            .store
            .list_paired()
            .ok()
            .and_then(|l| l.into_iter().find(|p| p.last_address.as_deref() == Some(&ip.to_string()) && p.last_port == Some(port)))
            .and_then(|p| hex_to_fp(&p.fingerprint))
            .map(PeerPin::Expect)
            .unwrap_or(PeerPin::AcceptAny);
        let device_id = this
            .connect_outbound(addr, pin, cancel)
            .await
            .map_err(|e| Self::describe_error(&e, matches!(pin, PeerPin::Expect(_)), port))?;

        let (name, os) = {
            let st = this.st();
            let h = st.conns.values().find_map(|c| c.hello.clone().filter(|h| h.device_id == device_id));
            h.map(|h| (h.name, h.os)).unwrap_or_else(|| ("Dispositivo".into(), "other".into()))
        };
        let paired = this.store.get_paired(&device_id).ok().flatten().is_some();
        this.st().discovered.insert(device_id.clone(), Discovered { name, os, addrs: vec![ip], port, last_seen_ms: now_ms() });
        if !paired {
            // Solo era una sonda: no se deja una conexión sin autoridad abierta.
            this.close_device(&device_id);
            this.st().paused.remove(&device_id);
        }
        this.emit_devices();
        this.devices().into_iter().find(|d| d.device_id == device_id).ok_or_else(|| "El dispositivo no está disponible.".to_string())
    }

    // ------------------------------------------------------------ mantenimiento

    async fn discovery_loop(this: Arc<Self>, mut rx: mpsc::UnboundedReceiver<DiscoveryEvent>, cancel: CancellationToken) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                ev = rx.recv() => match ev {
                    None => return,
                    Some(DiscoveryEvent::Found(f)) => this.on_found(f),
                    Some(DiscoveryEvent::Lost(id)) => {
                        this.st().discovered.remove(&id);
                        this.emit_devices();
                    }
                }
            }
        }
    }

    fn on_found(&self, f: Found) {
        if f.device_id == self.device_id {
            return; // este mismo equipo
        }
        let first_addr = f.addrs.first().map(|a| (a.to_string(), f.port));
        {
            let mut st = self.st();
            st.discovered.insert(
                f.device_id.clone(),
                Discovered { name: f.name.clone(), os: f.os.clone(), addrs: f.addrs, port: f.port, last_seen_ms: now_ms() },
            );
            // Apareció (o cambió de IP): reintentar de inmediato.
            st.backoff.remove(&f.device_id);
        }
        if self.store.get_paired(&f.device_id).ok().flatten().is_some() {
            let _ = self.store.touch_paired(&f.device_id, &f.name, &f.os, first_addr);
        }
        self.emit_devices();
    }

    async fn maintenance_loop(this: Arc<Self>, cancel: CancellationToken) {
        let t = this.cfg.timings.clone();
        let mut last_wall = now_ms();
        let mut last_iface = Instant::now();
        let mut last_scan: Vec<IpAddr> = this.st().addresses.clone();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(t.maintenance_tick) => {}
            }
            // Suspensión/reanudación: los temporizadores de tokio se congelan, el reloj no.
            let wall = now_ms();
            let gap = Duration::from_millis((wall - last_wall).max(0) as u64);
            last_wall = wall;
            if gap > t.maintenance_tick + t.resume_gap {
                Self::on_resume(&this, &cancel).await;
                last_scan.clear();
            }
            // Cambios de IP / interfaces.
            if this.cfg.bind_override.is_none() && last_iface.elapsed() >= t.iface_check {
                last_iface = Instant::now();
                let now = lan_addresses();
                if now != last_scan {
                    last_scan = now.clone();
                    let port = { let st = this.st(); st.bound_port.unwrap_or(st.settings.port) };
                    let err = Self::reconcile_listeners(&this, &now, port, &cancel).await;
                    this.st().start_error = if now.is_empty() { None } else { err.filter(|_| this.st().addresses.is_empty()) };
                    this.reannounce();
                    this.st().backoff.clear();
                    this.emit_all();
                }
            }
            this.sweep_pairing();
            this.sweep_discovered();
            Self::reconnect_pass(&this);
        }
    }

    /// El equipo volvió de suspensión: las conexiones y sockets pueden estar rotos.
    async fn on_resume(this: &Arc<Self>, cancel: &CancellationToken) {
        let (conns, listeners) = {
            let mut st = this.st();
            st.backoff.clear();
            let conns: Vec<CancellationToken> = st.conns.drain().map(|(_, c)| c.cancel).collect();
            let listeners: Vec<CancellationToken> = st.running.as_mut().map(|r| r.listeners.drain().map(|(_, t)| t).collect()).unwrap_or_default();
            (conns, listeners)
        };
        for c in conns.into_iter().chain(listeners) {
            c.cancel();
        }
        let addrs = this.cfg.bind_override.clone().unwrap_or_else(lan_addresses);
        let port = { let st = this.st(); st.bound_port.unwrap_or(st.settings.port) };
        let _ = Self::reconcile_listeners(this, &addrs, port, cancel).await;
        this.reannounce();
        this.emit_all();
    }

    fn sweep_pairing(&self) {
        let timeout = self.cfg.timings.pairing_timeout;
        let mut cancel: Vec<CancellationToken> = vec![];
        let changed = {
            let mut st = self.st();
            let now = Instant::now();
            let mut changed = false;
            let expired: Vec<String> = st.pairing.iter().filter(|(_, p)| p.state == PairState::Active && now.duration_since(p.started) > timeout).map(|(id, _)| id.clone()).collect();
            for id in expired {
                let conn_id = st.pairing.get(&id).map(|p| p.conn_id);
                if let Some(c) = conn_id.and_then(|cid| st.conns.get(&cid)) {
                    cancel.push(c.cancel.clone());
                }
                if let Some(p) = st.pairing.get_mut(&id) {
                    p.state = PairState::Expired;
                    p.message = Some("El emparejamiento venció por falta de confirmación.".into());
                    p.remove_at = Some(now + Duration::from_secs(6));
                    changed = true;
                }
            }
            let before = st.pairing.len();
            st.pairing.retain(|_, p| p.remove_at.is_none_or(|t| t > now));
            changed || st.pairing.len() != before
        };
        for c in cancel {
            c.cancel();
        }
        if changed {
            self.emit_pairing();
        }
    }

    fn sweep_discovered(&self) {
        let ttl = self.cfg.timings.discovery_ttl.as_millis() as i64;
        let changed = {
            let mut st = self.st();
            let live: HashSet<String> = st.conns.values().filter_map(|c| c.hello.as_ref().map(|h| h.device_id.clone())).collect();
            let before = st.discovered.len();
            let now = now_ms();
            st.discovered.retain(|id, d| live.contains(id) || now - d.last_seen_ms < ttl);
            st.discovered.len() != before
        };
        if changed {
            self.emit_devices();
        }
    }

    /// Reconexión automática con backoff exponencial a dispositivos emparejados.
    fn reconnect_pass(this: &Arc<Self>) {
        let paired = this.store.list_paired().unwrap_or_default();
        let now = Instant::now();
        let due: Vec<String> = {
            let st = this.st();
            if st.addresses.is_empty() && this.cfg.bind_override.is_none() {
                return; // sin red local no hay a dónde conectar
            }
            paired
                .iter()
                .filter(|p| {
                    !st.paused.contains(&p.device_id)
                        && !st.connecting.contains(&p.device_id)
                        && !st.conns.values().any(|c| c.hello.as_ref().is_some_and(|h| h.device_id == p.device_id))
                        && st.backoff.get(&p.device_id).is_none_or(|b| b.next_at <= now)
                        && (st.discovered.contains_key(&p.device_id) || p.last_address.is_some())
                })
                .map(|p| p.device_id.clone())
                .collect()
        };
        for id in due {
            let this = this.clone();
            tokio::spawn(async move {
                let _ = Self::connect_device(&this, &id, false).await;
            });
        }
    }
}

fn hex_to_fp(s: &str) -> Option<Fingerprint> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

/// Solo direcciones de red local: nunca se conecta hacia Internet.
fn is_lan_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

#[cfg(test)]
impl NetworkService {
    /// Abre una conexión de prueba hacia 127.0.0.1:`port` y devuelve el id remoto.
    pub async fn test_connect(&self, port: u16, pin: Option<Fingerprint>) -> Result<String, String> {
        let cancel = self.inner.st().running.as_ref().map(|r| r.cancel.child_token()).ok_or("detenido")?;
        let addr = SocketAddr::new("127.0.0.1".parse().unwrap(), port);
        self.inner.connect_outbound(addr, pin.map(PeerPin::Expect).unwrap_or(PeerPin::AcceptAny), cancel).await
    }

    /// Envía un mensaje crudo por la conexión viva con `device_id`.
    pub fn test_send_raw(&self, device_id: &str, msg: Message) -> bool {
        match self.inner.conn_for_device(device_id) {
            Some(c) => {
                self.inner.send_message(c, msg);
                true
            }
            None => false,
        }
    }

    pub fn fingerprint(&self) -> Fingerprint {
        *self.inner.identity.fingerprint()
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn tie_break_keeps_the_connection_started_by_the_lower_id() {
        // local < peer: la preferida es la que inicia el local.
        assert!(new_connection_wins("a", "b", true, false));
        assert!(!new_connection_wins("a", "b", false, true));
        // local > peer: la preferida es la que inicia el peer.
        assert!(new_connection_wins("b", "a", false, true));
        assert!(!new_connection_wins("b", "a", true, false));
        // Ambas del mismo origen: gana la más nueva.
        assert!(new_connection_wins("a", "b", true, true));
        assert!(new_connection_wins("a", "b", false, false));
    }

    #[test]
    fn both_sides_agree_on_which_connection_survives() {
        // La conexión X la inicia A. Vista desde A (local=a) y desde B (local=b).
        let (a, b) = ("a", "b");
        let y_started_by_a = true; // Y la inicia B en el otro sentido => no A
        let from_a = new_connection_wins(a, b, y_started_by_a, !y_started_by_a);
        let from_b = new_connection_wins(b, a, !y_started_by_a, y_started_by_a);
        assert_eq!(from_a, from_b);
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        let (base, max) = (Duration::from_secs(2), Duration::from_secs(60));
        let d1 = backoff_delay(base, max, 1);
        let d3 = backoff_delay(base, max, 3);
        let d20 = backoff_delay(base, max, 20);
        assert!(d1 >= Duration::from_secs(4) && d1 <= Duration::from_secs(5));
        assert!(d3 >= Duration::from_secs(16));
        assert!(d20 >= max && d20 <= max.mul_f64(1.25));
    }

    #[test]
    fn manual_addresses_must_be_local() {
        assert!(is_lan_ip(&"192.168.1.5".parse().unwrap()));
        assert!(is_lan_ip(&"10.1.2.3".parse().unwrap()));
        assert!(is_lan_ip(&"169.254.1.1".parse().unwrap()));
        assert!(is_lan_ip(&"fd00::1".parse().unwrap()));
        assert!(!is_lan_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_lan_ip(&"2001:4860:4860::8888".parse().unwrap()));
    }

    #[test]
    fn hex_roundtrip() {
        let fp = [7u8; 32];
        assert_eq!(hex_to_fp(&hex(&fp)), Some(fp));
        assert_eq!(hex_to_fp("zz"), None);
    }
}
