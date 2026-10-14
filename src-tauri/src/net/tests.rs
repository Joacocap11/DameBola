//! Pruebas de integración: dos (o tres) instancias reales del servicio de red
//! en 127.0.0.1 con puertos distintos, TLS + WebSocket de verdad.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::identity::{self, MemoryStore};
use super::protocol::*;
use super::service::*;
use super::store::{NetStore, NetworkSettings};
use super::tls::{self, PeerPin};
use crate::db::Database;

// ---------------------------------------------------------------- utilidades

struct NullEvents;
impl NetEvents for NullEvents {
    fn status(&self, _: &NetStatus) {}
    fn devices(&self, _: &[DeviceView]) {}
    fn pairing(&self, _: &[PairingView]) {}
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ok,
    Busy,
    Fail,
}

struct Alerts {
    count: AtomicUsize,
    mode: Mutex<Mode>,
    senders: Mutex<Vec<String>>,
}

impl Alerts {
    fn new() -> Arc<Self> {
        Arc::new(Self { count: AtomicUsize::new(0), mode: Mutex::new(Mode::Ok), senders: Mutex::new(vec![]) })
    }
    fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

impl AlertHandler for Alerts {
    fn show(&self, sender_name: &str) -> Result<(), RemoteAlertError> {
        match *self.mode.lock().unwrap() {
            Mode::Ok => {
                self.count.fetch_add(1, Ordering::SeqCst);
                self.senders.lock().unwrap().push(sender_name.to_string());
                Ok(())
            }
            Mode::Busy => Err(RemoteAlertError::Busy),
            Mode::Fail => Err(RemoteAlertError::Failed("boom".into())),
        }
    }
}

fn fast() -> NetTimings {
    NetTimings {
        connect_timeout: Duration::from_secs(2),
        hello_timeout: Duration::from_secs(3),
        ping_interval: Duration::from_millis(200),
        idle_timeout: Duration::from_secs(2),
        ack_timeout: Duration::from_secs(3),
        pairing_timeout: Duration::from_secs(5),
        maintenance_tick: Duration::from_millis(100),
        iface_check: Duration::from_secs(5),
        backoff_base: Duration::from_millis(100),
        backoff_max: Duration::from_millis(400),
        resume_gap: Duration::from_secs(20),
        discovery_ttl: Duration::from_secs(600),
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Node {
    svc: NetworkService,
    alerts: Arc<Alerts>,
    store: NetStore,
    port: u16,
}

impl Node {
    fn id(&self) -> String {
        self.svc.device_id().to_string()
    }
}

async fn node(name: &str) -> Node {
    let store = NetStore::new(Arc::new(Database::open_in_memory().unwrap()));
    let port = free_port();
    store.save_settings(NetworkSettings { port, ..Default::default() }).unwrap();
    let ident = identity::load_or_create(&store, &MemoryStore::default()).unwrap();
    let alerts = Alerts::new();
    let name = name.to_string();
    let svc = NetworkService::new(
        ident,
        store.clone(),
        Arc::new(NullEvents),
        alerts.clone(),
        NetConfig {
            app_version: "0.2.0-test".into(),
            os: "macos",
            bind_override: Some(vec!["127.0.0.1".parse().unwrap()]),
            mdns: false,
            timings: fast(),
            name: Arc::new(move || name.clone()),
        },
    )
    .unwrap();
    svc.start().await;
    assert_eq!(svc.listening_port(), Some(port));
    Node { svc, alerts, store, port }
}

async fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("tiempo agotado esperando: {what}");
}

/// Empareja a (iniciador) con b (receptor) de punta a punta, verificando el código.
async fn pair(a: &Node, b: &Node) {
    let dev = a.svc.add_manual("127.0.0.1", b.port).await.expect("add_manual");
    assert_eq!(dev.device_id, b.id());
    assert_eq!(dev.pairing, "unpaired");
    a.svc.pair(&b.id()).await.expect("pair");
    wait_for("solicitud en el receptor", || b.svc.pairing().iter().any(|p| p.device_id == a.id() && p.state == "active")).await;
    let pa = a.svc.pairing().into_iter().find(|p| p.device_id == b.id()).unwrap();
    let pb = b.svc.pairing().into_iter().find(|p| p.device_id == a.id()).unwrap();
    assert_eq!(pa.code, pb.code, "ambos equipos deben mostrar el mismo código");
    assert_eq!((pa.role, pb.role), ("initiator", "responder"));
    b.svc.pairing_decision(&a.id(), true);
    a.svc.pairing_decision(&b.id(), true);
    wait_for("emparejado en ambos", || {
        a.store.get_paired(&b.id()).unwrap().is_some() && b.store.get_paired(&a.id()).unwrap().is_some()
    })
    .await;
    wait_for("conexión entre ambos", || a.svc.connection_count() == 1 && b.svc.connection_count() == 1).await;
}

// -------------------------------------------------------------- emparejamiento

#[tokio::test]
async fn pairing_needs_both_users_and_stores_pinned_identities() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    let on_a = a.store.get_paired(&b.id()).unwrap().unwrap();
    let on_b = b.store.get_paired(&a.id()).unwrap().unwrap();
    assert_eq!(on_a.fingerprint, identity::hex(&b.svc.fingerprint()));
    assert_eq!(on_b.fingerprint, identity::hex(&a.svc.fingerprint()));
    assert_eq!(on_a.name, "Beto");
    let views = a.svc.devices();
    assert!(views.iter().any(|d| d.device_id == b.id() && d.pairing == "paired" && d.connection == "connected"));
}

#[tokio::test]
async fn pairing_is_not_completed_if_the_receiver_rejects() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    a.svc.add_manual("127.0.0.1", b.port).await.unwrap();
    a.svc.pair(&b.id()).await.unwrap();
    wait_for("solicitud", || b.svc.pairing().iter().any(|p| p.state == "active")).await;
    b.svc.pairing_decision(&a.id(), false);
    wait_for("rechazo visto por el iniciador", || a.svc.pairing().iter().any(|p| p.state == "rejected")).await;
    assert!(a.store.get_paired(&b.id()).unwrap().is_none());
    assert!(b.store.get_paired(&a.id()).unwrap().is_none());
}

#[tokio::test]
async fn pairing_is_not_completed_if_the_initiator_says_codes_differ() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    a.svc.add_manual("127.0.0.1", b.port).await.unwrap();
    a.svc.pair(&b.id()).await.unwrap();
    wait_for("solicitud", || b.svc.pairing().iter().any(|p| p.state == "active")).await;
    b.svc.pairing_decision(&a.id(), true);
    a.svc.pairing_decision(&b.id(), false); // "no coincide"
    wait_for("cancelado en el receptor", || b.svc.pairing().iter().any(|p| p.state == "rejected")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(a.store.get_paired(&b.id()).unwrap().is_none());
    assert!(b.store.get_paired(&a.id()).unwrap().is_none(), "sin confirmación del iniciador no se guarda nada");
}

#[tokio::test]
async fn unconfirmed_pairing_expires() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    a.svc.add_manual("127.0.0.1", b.port).await.unwrap();
    a.svc.pair(&b.id()).await.unwrap();
    wait_for("expira", || a.svc.pairing().iter().any(|p| p.state == "expired")).await;
    assert!(a.store.get_paired(&b.id()).unwrap().is_none());
}

// ---------------------------------------------------------------------- alertas

#[tokio::test]
async fn a_can_alert_b_and_b_can_alert_a() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;

    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!((r.status, r.accepted), ("shown", true), "{r:?}");
    assert_eq!(b.alerts.count(), 1);
    assert_eq!(b.alerts.senders.lock().unwrap().as_slice(), ["Ana"], "el remitente sale de la identidad autenticada");

    let r = b.svc.send_alert(&a.id()).await;
    assert_eq!(r.status, "shown", "{r:?}");
    assert_eq!(a.alerts.count(), 1);
    assert_eq!(b.alerts.count(), 1, "cada alerta se muestra solo en el destinatario");
}

#[tokio::test]
async fn unpaired_device_cannot_trigger_alerts() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    // A nunca se emparejó con B pero intenta mandar una alerta cruda.
    let id = a.svc.test_connect(b.port, None).await.unwrap();
    assert_eq!(id, b.id());
    assert!(a.svc.test_send_raw(&id, Message::new(Body::AlertRequest(Empty {}))));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(b.alerts.count(), 0);
    // Y a nivel de API, ni siquiera se permite enviar a un no emparejado.
    let r = a.svc.send_alert(&id).await;
    assert_eq!(r.status, "error");
    assert_eq!(b.alerts.count(), 0);
}

#[tokio::test]
async fn repeated_unauthorized_requests_get_the_connection_dropped() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    let id = a.svc.test_connect(b.port, None).await.unwrap();
    for _ in 0..5 {
        a.svc.test_send_raw(&id, Message::new(Body::AlertRequest(Empty {})));
    }
    wait_for("conexión cerrada por abuso", || b.svc.connection_count() == 0).await;
    assert_eq!(b.alerts.count(), 0);
}

#[tokio::test]
async fn revoked_device_loses_authorization_on_both_sides() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    a.svc.forget(&b.id()).await.unwrap();
    wait_for("B se entera de la revocación", || b.store.get_paired(&a.id()).unwrap().is_none()).await;
    assert!(a.store.get_paired(&b.id()).unwrap().is_none());
    let r = b.svc.send_alert(&a.id()).await;
    assert_eq!(r.status, "error");
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!(r.status, "error");
    assert_eq!((a.alerts.count(), b.alerts.count()), (0, 0));
    // No se reconectan solos: ya no son dispositivos emparejados.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(a.svc.connection_count(), 0);
}

#[tokio::test]
async fn receiver_rejects_alerts_from_a_peer_that_was_silently_unpaired() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    // B borra el registro sin avisar (p. ej. base restaurada): A aún cree estar emparejado.
    b.store.remove_paired(&a.id()).unwrap();
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!(r.status, "rejected", "{r:?}");
    assert_eq!(r.reason.as_deref(), Some("unauthorized"));
    assert!(!r.accepted);
    assert_eq!(b.alerts.count(), 0);
}

#[tokio::test]
async fn duplicate_alert_requests_run_only_once() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    let msg = Message::new(Body::AlertRequest(Empty {}));
    assert!(a.svc.test_send_raw(&b.id(), msg.clone()));
    assert!(a.svc.test_send_raw(&b.id(), msg.clone()));
    assert!(a.svc.test_send_raw(&b.id(), msg));
    wait_for("primera alerta", || b.alerts.count() >= 1).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(b.alerts.count(), 1, "el mismo id no se procesa dos veces");
}

#[tokio::test]
async fn rate_limit_allows_one_alert_per_cooldown_per_sender() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    assert_eq!(a.svc.send_alert(&b.id()).await.status, "shown");
    let second = a.svc.send_alert(&b.id()).await;
    assert_eq!((second.status, second.reason.as_deref()), ("rejected", Some("ratelimited")), "{second:?}");
    assert_eq!(b.alerts.count(), 1);
    // Otro remitente (B → A) no se ve afectado.
    assert_eq!(b.svc.send_alert(&a.id()).await.status, "shown");
}

#[tokio::test]
async fn receiver_controls_disable_mute_busy_and_failures() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    let base = b.svc.settings();

    b.svc.save_settings(NetworkSettings { remote_alerts_enabled: false, ..base.clone() }).await.unwrap();
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!((r.status, r.reason.as_deref()), ("rejected", Some("disabled")));

    let until = chrono_like_now() + 60_000;
    b.svc.save_settings(NetworkSettings { muted_until_ms: Some(until), ..base.clone() }).await.unwrap();
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!((r.status, r.reason.as_deref()), ("rejected", Some("muted")));

    b.svc.save_settings(base).await.unwrap();
    *b.alerts.mode.lock().unwrap() = Mode::Busy;
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!((r.status, r.reason.as_deref()), ("rejected", Some("busy")));

    *b.alerts.mode.lock().unwrap() = Mode::Fail;
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!((r.status, r.accepted), ("rejected", true), "aceptada pero no mostrada ≠ entregada: {r:?}");
    assert_eq!(b.alerts.count(), 0);
}

fn chrono_like_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

#[tokio::test]
async fn sending_to_a_disconnected_device_reports_offline() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    b.svc.save_settings(NetworkSettings { enabled: false, ..b.svc.settings() }).await.unwrap();
    wait_for("A pierde la conexión", || a.svc.connection_count() == 0).await;
    let r = a.svc.send_alert(&b.id()).await;
    assert_eq!(r.status, "offline");
    assert!(!r.accepted);
    assert_eq!(b.svc.status().state, "stopped");
}

// ---------------------------------------------------------------- conexión

#[tokio::test]
async fn reconnects_automatically_after_the_connection_drops() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    let before_a = a.svc.connection_ids();
    let before_b = b.svc.connection_ids();
    a.svc.drop_all_connections();
    // Una reconexión rápida puede ocurrir entre dos sondeos de 25 ms. Exigir
    // conexiones nuevas en ambos extremos comprueba el reemplazo sin depender
    // de observar ese estado desconectado transitorio (carrera en Windows).
    wait_for("se reconecta sola con conexiones nuevas", || {
        a.svc.connection_count() == 1 && b.svc.connection_count() == 1
            && a.svc.connection_ids() != before_a && b.svc.connection_ids() != before_b
    }).await;
    assert_eq!(a.svc.send_alert(&b.id()).await.status, "shown");
}

#[tokio::test]
async fn user_disconnect_stops_auto_reconnect_until_connect_is_pressed() {
    let (a, b) = (node("Ana").await, node("Beto").await);
    pair(&a, &b).await;
    a.svc.disconnect(&b.id());
    wait_for("desconectado", || a.svc.connection_count() == 0 && b.svc.connection_count() == 0).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(a.svc.connection_count(), 0, "no debe reconectar solo");
    a.svc.connect(&b.id()).await.unwrap();
    wait_for("reconectado", || a.svc.connection_count() == 1).await;
}

#[tokio::test]
async fn a_paired_device_is_only_trusted_with_its_own_certificate() {
    let (a, b, impostor) = (node("Ana").await, node("Beto").await, node("Impostor").await);
    pair(&a, &b).await;
    // El impostor escucha en otro puerto; A espera el certificado de B.
    let err = a.svc.test_connect(impostor.port, Some(b.svc.fingerprint())).await.unwrap_err();
    assert!(err.starts_with("tls"), "{err}");
    // Y el verdadero B sí es aceptado con su fingerprint.
    assert!(a.svc.test_connect(b.port, Some(b.svc.fingerprint())).await.is_ok());
}

#[tokio::test]
async fn manual_connection_validates_input_and_reports_clear_errors() {
    let a = node("Ana").await;
    let e = a.svc.add_manual("no-es-ip", 38473).await.unwrap_err();
    assert!(e.contains("dirección IP válida"));
    let e = a.svc.add_manual("8.8.8.8", 38473).await.unwrap_err();
    assert!(e.contains("red local"), "no se debe conectar hacia Internet: {e}");
    let e = a.svc.add_manual("127.0.0.1", 0).await.unwrap_err();
    assert!(e.contains("puerto"));
    let e = a.svc.add_manual("127.0.0.1", free_port()).await.unwrap_err();
    assert!(e.contains("rechazada") || e.contains("tiempo agotado"), "{e}");
    let e = a.svc.add_manual("127.0.0.1", a.port).await.unwrap_err();
    assert!(e.contains("mismo equipo"), "no debe mostrarse a sí mismo como remoto: {e}");
}

#[tokio::test]
async fn disabling_the_network_stops_listening_and_clears_state() {
    let a = node("Ana").await;
    a.svc.save_settings(NetworkSettings { enabled: false, ..a.svc.settings() }).await.unwrap();
    assert_eq!(a.svc.status().state, "stopped");
    let port = a.port;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err(), "el puerto debe cerrarse");
    a.svc.save_settings(NetworkSettings { enabled: true, ..a.svc.settings() }).await.unwrap();
    assert_eq!(a.svc.status().state, "running");
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", a.port)).await.is_ok());
}

// --------------------------------------------------------- protocolo inválido

type RawWs = tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;

async fn raw_client(port: u16) -> (RawWs, identity::Identity) {
    let store = NetStore::new(Arc::new(Database::open_in_memory().unwrap()));
    let ident = identity::load_or_create(&store, &MemoryStore::default()).unwrap();
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let cfg = tls::client_config(&ident, PeerPin::AcceptAny).unwrap();
    let tls = TlsConnector::from(cfg).connect(rustls::pki_types::ServerName::try_from(tls::SERVER_NAME).unwrap(), tcp).await.unwrap();
    let (ws, _) = tokio_tungstenite::client_async("wss://damebola.local/", tls).await.unwrap();
    (ws, ident)
}

/// ¿El servidor cerró la conexión en breve?
async fn closed_soon(ws: &mut RawWs) -> bool {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(WsMessage::Close(_))) => return true,
                Some(Ok(_)) => continue, // HELLO del servidor, PING, etc.
            }
        }
    })
    .await
    .unwrap_or(false)
}

fn valid_hello(ident: &identity::Identity) -> Message {
    Message::new(Body::Hello(Hello { device_id: ident.device_id(), name: "Raw".into(), os: "macos".into(), app_version: "1".into() }))
}

#[tokio::test]
async fn server_drops_connections_that_send_garbage() {
    let b = node("Beto").await;
    let (mut ws, _) = raw_client(b.port).await;
    ws.send(WsMessage::text("esto no es json")).await.unwrap();
    assert!(closed_soon(&mut ws).await);
}

#[tokio::test]
async fn server_drops_oversized_messages() {
    let b = node("Beto").await;
    let (mut ws, ident) = raw_client(b.port).await;
    ws.send(WsMessage::text(valid_hello(&ident).encode())).await.unwrap();
    ws.send(WsMessage::text("x".repeat(MAX_MESSAGE_BYTES * 2))).await.unwrap();
    assert!(closed_soon(&mut ws).await);
}

#[tokio::test]
async fn server_requires_hello_before_anything_else() {
    let b = node("Beto").await;
    let (mut ws, _) = raw_client(b.port).await;
    ws.send(WsMessage::text(Message::new(Body::AlertRequest(Empty {})).encode())).await.unwrap();
    assert!(closed_soon(&mut ws).await);
    assert_eq!(b.alerts.count(), 0);
}

#[tokio::test]
async fn server_rejects_a_hello_that_claims_someone_elses_id() {
    let b = node("Beto").await;
    let (mut ws, _) = raw_client(b.port).await;
    let forged = Message::new(Body::Hello(Hello { device_id: "a".repeat(32), name: "Falso".into(), os: "macos".into(), app_version: "1".into() }));
    ws.send(WsMessage::text(forged.encode())).await.unwrap();
    assert!(closed_soon(&mut ws).await);
    assert!(b.svc.devices().is_empty(), "la identidad falsa no debe aparecer");
}

#[tokio::test]
async fn server_rejects_unknown_types_and_extra_fields() {
    let b = node("Beto").await;
    let (mut ws, ident) = raw_client(b.port).await;
    ws.send(WsMessage::text(valid_hello(&ident).encode())).await.unwrap();
    ws.send(WsMessage::text(r#"{"v":1,"id":"00000000-0000-0000-0000-000000000000","type":"RUN","body":{"cmd":"rm -rf /"}}"#)).await.unwrap();
    assert!(closed_soon(&mut ws).await);
}

#[tokio::test]
async fn server_rejects_binary_frames() {
    let b = node("Beto").await;
    let (mut ws, ident) = raw_client(b.port).await;
    ws.send(WsMessage::text(valid_hello(&ident).encode())).await.unwrap();
    ws.send(WsMessage::binary(vec![1, 2, 3])).await.unwrap();
    assert!(closed_soon(&mut ws).await);
}

// ------------------------------------------------------------------------ mDNS

/// Requiere una interfaz LAN real y multicast: se ejecuta a mano con
/// `cargo test -- --ignored mdns`.
#[tokio::test]
#[ignore]
async fn mdns_discovers_the_other_instance_and_expires_it_when_it_leaves() {
    let ips = lan_addresses();
    let Some(ip) = ips.first().copied() else { return };
    let make = |name: &'static str| async move {
        let store = NetStore::new(Arc::new(Database::open_in_memory().unwrap()));
        store.save_settings(NetworkSettings { port: free_port(), ..Default::default() }).unwrap();
        let ident = identity::load_or_create(&store, &MemoryStore::default()).unwrap();
        let svc = NetworkService::new(
            ident,
            store,
            Arc::new(NullEvents),
            Alerts::new(),
            NetConfig { app_version: "0.2.0".into(), os: "macos", bind_override: Some(vec![ip]), mdns: true, timings: fast(), name: Arc::new(move || name.to_string()) },
        )
        .unwrap();
        svc.start().await;
        svc
    };
    let (a, b) = (make("Ana").await, make("Beto").await);
    wait_for("A ve a B", || a.devices().iter().any(|d| d.device_id == b.device_id() && d.discovered)).await;
    assert!(!a.devices().iter().any(|d| d.device_id == a.device_id()), "no se muestra a sí mismo");
    b.stop().await;
    wait_for("B desaparece", || !a.devices().iter().any(|d| d.device_id == b.device_id() && d.discovered)).await;
}
