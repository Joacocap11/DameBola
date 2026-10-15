//! Icono de bandeja (Windows) / barra de menús (macOS).
//!
//! El contenido del menú sale de `model` (función pura, probada); el resto es cableado de Tauri.
//! Las acciones usan las mismas funciones que el resto de la app (`background`, `send`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;

use crate::db::now_ms;
use crate::net::service::DeviceView;
use crate::net::store::NetworkSettings;
use crate::send::resolve_recipient;
use crate::settings::Settings;
use crate::{background, Services};

pub const TRAY_ID: &str = "damebola-tray";
const ID_OPEN: &str = "open";
const ID_SEND: &str = "send";
const ID_MUTE: &str = "mute";
const ID_AUTOSTART: &str = "autostart";
const ID_QUIT: &str = "quit";

/// Cuánto dura "Silenciar alertas" desde la bandeja.
pub const TRAY_MUTE_MS: i64 = 60 * 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    pub status: String,
    pub recipient: String,
    pub send_enabled: bool,
    pub muted: bool,
    pub mute_label: &'static str,
    pub autostart: bool,
    pub tooltip: String,
}

pub fn model(net_state: &str, devices: &[DeviceView], settings: &Settings, net: &NetworkSettings, autostart: bool, now: i64) -> TrayModel {
    let connected = devices.iter().filter(|d| d.pairing == "paired" && d.connection == "connected").count();
    let status_text = match net_state {
        "running" if connected == 0 => "Sin equipos conectados".to_string(),
        "running" if connected == 1 => "Conectado a 1 equipo".to_string(),
        "running" => format!("Conectado a {connected} equipos"),
        "starting" => "Conectando…".to_string(),
        "error" => "Red con error".to_string(),
        _ => "Red detenida".to_string(),
    };
    let (recipient, send_enabled) = match resolve_recipient(devices, settings.shortcut_device_id.as_deref(), "la bandeja") {
        Ok(d) if d.connection == "connected" && net_state == "running" => (format!("Destinatario: {}", d.name), true),
        Ok(d) => (format!("Destinatario: {} (sin conexión)", d.name), false),
        Err(_) if devices.iter().all(|d| d.pairing != "paired") => ("Sin equipos emparejados".to_string(), false),
        Err(_) => ("Elegí un destinatario en Configuración".to_string(), false),
    };
    let muted = !net.remote_alerts_enabled || net.is_muted(now);
    TrayModel {
        tooltip: format!("DameBola — {status_text}"),
        status: status_text,
        recipient,
        send_enabled,
        muted,
        mute_label: if muted { "Reactivar alertas" } else { "Silenciar alertas (1 hora)" },
        autostart,
    }
}

/// Ajustes de red tras pulsar "Silenciar / Reactivar".
pub fn toggled_mute(net: &NetworkSettings, now: i64) -> NetworkSettings {
    if !net.remote_alerts_enabled || net.is_muted(now) {
        NetworkSettings { remote_alerts_enabled: true, muted_until_ms: None, ..net.clone() }
    } else {
        NetworkSettings { muted_until_ms: Some(now + TRAY_MUTE_MS), ..net.clone() }
    }
}

struct Items {
    status: MenuItem<tauri::Wry>,
    recipient: MenuItem<tauri::Wry>,
    send: MenuItem<tauri::Wry>,
    mute: MenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
}

/// Estado gestionado por Tauri: los elementos del menú y si el icono existe.
#[derive(Default)]
pub struct TrayState {
    items: Mutex<Option<Items>>,
    dirty: AtomicBool,
}

impl TrayState {
    pub fn is_active(&self) -> bool {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }
}

/// Crea o quita el icono según los ajustes. Idempotente.
pub fn sync(app: &AppHandle, show: bool) -> Result<(), String> {
    let state = app.state::<TrayState>();
    let exists = state.is_active();
    if show && !exists {
        create(app, &state)?;
        schedule_refresh(app);
    } else if !show && exists {
        let _ = app.remove_tray_by_id(TRAY_ID);
        *state.items.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    Ok(())
}

fn create(app: &AppHandle, state: &TrayState) -> Result<(), String> {
    let err = |e: tauri::Error| format!("No se pudo crear el icono de la bandeja: {e}");
    let status = MenuItem::with_id(app, "status", "DameBola", false, None::<&str>).map_err(err)?;
    let recipient = MenuItem::with_id(app, "recipient", "—", false, None::<&str>).map_err(err)?;
    let open = MenuItem::with_id(app, ID_OPEN, "Abrir DameBola", true, None::<&str>).map_err(err)?;
    let send = MenuItem::with_id(app, ID_SEND, "Enviar DAME BOLA", false, None::<&str>).map_err(err)?;
    let mute = MenuItem::with_id(app, ID_MUTE, "Silenciar alertas (1 hora)", true, None::<&str>).map_err(err)?;
    let autostart = CheckMenuItem::with_id(app, ID_AUTOSTART, "Iniciar con el sistema", true, false, None::<&str>).map_err(err)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Salir completamente", true, None::<&str>).map_err(err)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(app, &[&status, &recipient, &sep().map_err(err)?, &open, &send, &mute, &autostart, &sep().map_err(err)?, &quit]).map_err(err)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("DameBola")
        // macOS: clic abre el menú. Windows: clic izquierdo abre la ventana, derecho el menú.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| background::on_tray_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if !cfg!(target_os = "macos") {
                    background::show_main(tray.app_handle());
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app).map_err(err)?;
    *state.items.lock().unwrap_or_else(|e| e.into_inner()) = Some(Items { status, recipient, send, mute, autostart });
    Ok(())
}

/// Pide actualizar el menú. Se hace en otra tarea (los eventos de red se emiten con locks tomados)
/// y se agrupan varias peticiones seguidas.
pub fn schedule_refresh(app: &AppHandle) {
    let Some(state) = app.try_state::<TrayState>() else { return };
    if state.dirty.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        app.state::<TrayState>().dirty.store(false, Ordering::Release);
        refresh(&app);
    });
}

fn refresh(app: &AppHandle) {
    let Some(services) = app.try_state::<Arc<Services>>() else { return };
    let Ok(settings) = services.settings.get() else { return };
    let autostart = app.autolaunch().is_enabled().unwrap_or(settings.launch_at_login);
    let m = model(services.net.status().state, &services.net.devices(), &settings, &services.net.settings(), autostart, now_ms());
    let state = app.state::<TrayState>();
    let guard = state.items.lock().unwrap_or_else(|e| e.into_inner());
    let Some(items) = guard.as_ref() else { return };
    let _ = items.status.set_text(&m.status);
    let _ = items.recipient.set_text(&m.recipient);
    let _ = items.send.set_enabled(m.send_enabled);
    let _ = items.mute.set_text(m.mute_label);
    let _ = items.autostart.set_checked(m.autostart);
    drop(guard);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(&m.tooltip));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: &'static str) -> &'static str {
        state
    }

    fn device(id: char, pairing: &'static str, connection: &'static str) -> DeviceView {
        DeviceView {
            device_id: id.to_string().repeat(32),
            name: format!("PC-{id}"),
            announced_name: None,
            alias: None,
            os: "windows".into(),
            connection,
            pairing,
            discovered: true,
            addresses: vec![],
            port: None,
            last_seen_ms: None,
            auto_connect: true,
            last_error: None,
        }
    }

    #[test]
    fn send_is_enabled_only_with_a_valid_connected_recipient() {
        let (s, n) = (Settings::default(), NetworkSettings::default());
        let m = model(status("running"), &[device('a', "paired", "connected")], &s, &n, false, 0);
        assert!(m.send_enabled);
        assert_eq!(m.recipient, "Destinatario: PC-a");
        assert_eq!(m.status, "Conectado a 1 equipo");

        let m = model(status("running"), &[device('a', "paired", "disconnected")], &s, &n, false, 0);
        assert!(!m.send_enabled);
        assert!(m.recipient.contains("sin conexión"));

        let m = model(status("running"), &[device('a', "unpaired", "connected")], &s, &n, false, 0);
        assert!(!m.send_enabled);
        assert_eq!(m.recipient, "Sin equipos emparejados");

        let two = [device('a', "paired", "connected"), device('b', "paired", "connected")];
        let m = model(status("running"), &two, &s, &n, false, 0);
        assert!(!m.send_enabled, "varios emparejados sin destinatario elegido");
        assert!(m.recipient.contains("Elegí un destinatario"));
        let chosen = Settings { shortcut_device_id: Some("b".repeat(32)), ..s.clone() };
        let m = model(status("running"), &two, &chosen, &n, false, 0);
        assert!(m.send_enabled);
        assert_eq!(m.recipient, "Destinatario: PC-b");

        let m = model(status("stopped"), &[device('a', "paired", "connected")], &s, &n, false, 0);
        assert!(!m.send_enabled, "con la red detenida no se envía");
    }

    #[test]
    fn mute_toggle_round_trips_and_reenables_disabled_alerts() {
        let n = NetworkSettings::default();
        let muted = toggled_mute(&n, 1_000);
        assert_eq!(muted.muted_until_ms, Some(1_000 + TRAY_MUTE_MS));
        assert!(model(status("running"), &[], &Settings::default(), &muted, false, 2_000).muted);
        assert_eq!(model(status("running"), &[], &Settings::default(), &muted, false, 2_000).mute_label, "Reactivar alertas");
        let back = toggled_mute(&muted, 2_000);
        assert_eq!(back, n);
        let off = NetworkSettings { remote_alerts_enabled: false, ..n.clone() };
        assert!(toggled_mute(&off, 0).remote_alerts_enabled);
        assert_eq!(model(status("running"), &[], &Settings::default(), &n, true, 0).autostart, true);
    }
}
