//! Envío de DAME BOLA iniciado por el usuario sin pasar por React: atajo (local o global) y bandeja.
//!
//! Una sola implementación: elige el destinatario, verifica emparejamiento y conexión, impide
//! envíos simultáneos y delega en `NetworkService::send_alert` (que conserva confirmación `shown`,
//! errores y control de frecuencia del receptor).

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::net::service::{AlertSendResult, DeviceView};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Atajo con la ventana principal enfocada (el frontend solo detecta la tecla).
    Local,
    /// Atajo global registrado en el sistema operativo.
    Global,
    Tray,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Origin::Local | Origin::Global => "el atajo",
            Origin::Tray => "la bandeja",
        }
    }
}

/// Destinatario: el elegido, o el único emparejado. Nunca adivina entre varios.
pub fn resolve_recipient(devices: &[DeviceView], target: Option<&str>, label: &str) -> Result<DeviceView, String> {
    let paired: Vec<&DeviceView> = devices.iter().filter(|d| d.pairing == "paired").collect();
    if let Some(id) = target {
        return paired
            .into_iter()
            .find(|d| d.device_id == id)
            .cloned()
            .ok_or_else(|| format!("El destinatario de {label} ya no está emparejado. Elegí otro en Configuración."));
    }
    match paired.as_slice() {
        [] => Err(format!("Emparejá un dispositivo en Dispositivos antes de usar {label}.")),
        [one] => Ok((*one).clone()),
        _ => Err(format!("Elegí el destinatario de {label} en Configuración: hay varios equipos emparejados.")),
    }
}

/// Un envío a la vez en toda la app (atajo, bandeja y botones comparten el mismo cerrojo).
#[derive(Clone, Default)]
pub struct SendGuard(Arc<AtomicBool>);

pub struct SendPermit(Arc<AtomicBool>);

impl SendGuard {
    pub fn try_acquire(&self) -> Option<SendPermit> {
        self.0.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| SendPermit(Arc::clone(&self.0)))
    }
}

impl Drop for SendPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Resuelve destinatario y estado de conexión; `send` recibe el id y hace el envío real.
pub async fn send_to_recipient<F, Fut>(guard: &SendGuard, devices: &[DeviceView], target: Option<&str>, origin: Origin, send: F) -> AlertSendResult
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = AlertSendResult>,
{
    let Some(_permit) = guard.try_acquire() else {
        return AlertSendResult::fail("error", false, Some("busy"), "Ya hay un envío en curso.");
    };
    let recipient = match resolve_recipient(devices, target, origin.label()) {
        Ok(d) => d,
        Err(message) => return AlertSendResult::fail("error", false, Some("no_recipient"), message),
    };
    if recipient.connection != "connected" {
        return AlertSendResult::fail("offline", false, Some("not_connected"), format!("{} no está conectado. No se guardó ninguna alerta.", recipient.name));
    }
    send(recipient.device_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn device(id: char, pairing: &str, connection: &str) -> DeviceView {
        DeviceView {
            device_id: id.to_string().repeat(32),
            name: format!("PC-{id}"),
            announced_name: None,
            alias: None,
            os: "windows".into(),
            connection: match connection {
                "connected" => "connected",
                "connecting" => "connecting",
                _ => "disconnected",
            },
            pairing: match pairing {
                "paired" => "paired",
                "pending" => "pending",
                _ => "unpaired",
            },
            discovered: true,
            addresses: vec![],
            port: None,
            last_seen_ms: None,
            auto_connect: true,
            last_error: None,
        }
    }

    fn shown() -> AlertSendResult {
        AlertSendResult { status: "shown", accepted: true, reason: None, message: "ok".into() }
    }

    async fn run(guard: &SendGuard, devices: &[DeviceView], target: Option<&str>, calls: &AtomicUsize) -> AlertSendResult {
        send_to_recipient(guard, devices, target, Origin::Global, |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            shown()
        })
        .await
    }

    #[test]
    fn recipient_rules() {
        let a = device('a', "paired", "connected");
        let b = device('b', "paired", "connected");
        let c = device('c', "unpaired", "connected");
        assert_eq!(resolve_recipient(&[a.clone(), c.clone()], None, "x").unwrap().device_id, a.device_id, "único emparejado");
        assert!(resolve_recipient(&[a.clone(), b.clone()], None, "x").unwrap_err().contains("varios"));
        assert_eq!(resolve_recipient(&[a.clone(), b.clone()], Some(&b.device_id), "x").unwrap().device_id, b.device_id);
        assert!(resolve_recipient(&[a.clone(), c.clone()], Some(&c.device_id), "x").is_err(), "un no emparejado nunca es destinatario");
        assert!(resolve_recipient(&[c], None, "x").unwrap_err().contains("Emparejá"));
        assert!(resolve_recipient(&[a], Some(&"z".repeat(32)), "x").unwrap_err().contains("ya no está emparejado"));
    }

    #[tokio::test]
    async fn sends_to_the_connected_paired_recipient_once() {
        let calls = AtomicUsize::new(0);
        let r = run(&SendGuard::default(), &[device('a', "paired", "connected")], None, &calls).await;
        assert_eq!(r.status, "shown");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn offline_or_unpaired_recipient_never_reaches_the_network() {
        let calls = AtomicUsize::new(0);
        let g = SendGuard::default();
        let r = run(&g, &[device('a', "paired", "disconnected")], None, &calls).await;
        assert_eq!((r.status, r.reason.as_deref()), ("offline", Some("not_connected")));
        let r = run(&g, &[device('a', "unpaired", "connected")], None, &calls).await;
        assert_eq!((r.status, r.reason.as_deref()), ("error", Some("no_recipient")));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(g.try_acquire().is_some(), "el cerrojo se libera tras un error");
    }

    #[tokio::test]
    async fn simultaneous_sends_are_rejected_and_the_guard_is_released_afterwards() {
        let g = SendGuard::default();
        let devices = [device('a', "paired", "connected")];
        let calls = AtomicUsize::new(0);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let first = send_to_recipient(&g, &devices, None, Origin::Global, |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            let _ = rx.await;
            shown()
        });
        let second = async {
            tokio::task::yield_now().await;
            let r = run(&g, &devices, None, &calls).await;
            let _ = tx.send(());
            r
        };
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.status, "shown");
        assert_eq!(second.reason.as_deref(), Some("busy"));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "una sola alerta");
        assert_eq!(run(&g, &devices, None, &calls).await.status, "shown", "después se puede volver a enviar");
    }
}
