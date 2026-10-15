//! Ciclo de vida de aplicación residente: ocultar al cerrar, bandeja, salida completa,
//! aplicación transaccional de ajustes y envío desde Rust (atajo global y bandeja).
//!
//! Cerrar la ventana ≠ salir. Solo `quit` (menú de la bandeja, Cmd+Q, cerrar con
//! "mantener en segundo plano" desactivado) termina el proceso.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Window};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::{DialogExt as _, MessageDialogKind};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};

use crate::error::{AppError, AppResult};
use crate::hotkey::{self, ShortcutBackend};
use crate::net::service::{current_os, AlertSendResult};
use crate::send::{send_to_recipient, Origin};
use crate::settings::Settings;
use crate::tray::{self, TrayState};
use crate::{alert, Services};

pub const MAIN_LABEL: &str = "main";
pub const EVENT_SEND_RESULT: &str = "send://result";
pub const EVENT_SETTINGS_CHANGED: &str = "settings://changed";
pub const EVENT_NET_SETTINGS: &str = "net://settings";
/// Argumento con el que el sistema inicia la app al abrir sesión.
pub const AUTOSTART_ARG: &str = "--autostart";

pub const BACKGROUND_NOTICE: &str = "DameBola seguirá funcionando en segundo plano.\nPodés abrirlo desde la bandeja del sistema.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Ocultar la ventana; el proceso, la red y los atajos siguen vivos.
    Hide,
    /// Salir completamente.
    Quit,
}

/// Solo se oculta si hay una bandeja desde la cual reabrir y salir.
pub fn close_action(keep_running: bool, tray_active: bool) -> CloseAction {
    if keep_running && tray_active {
        CloseAction::Hide
    } else {
        CloseAction::Quit
    }
}

/// ¿La ventana principal debe empezar visible? Solo se oculta al iniciar con el sistema.
pub fn start_visible(settings: &Settings, launched_by_autostart: bool, tray_active: bool) -> bool {
    !(launched_by_autostart && settings.start_minimized && tray_active)
}

/// Muestra, restaura, trae al frente y enfoca la ventana principal.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Cierre de la ventana principal. Devuelve `true` si el cierre debe cancelarse (se oculta).
pub fn on_main_close_requested(window: &Window) -> bool {
    let app = window.app_handle();
    let (Some(services), Some(tray)) = (app.try_state::<Arc<Services>>(), app.try_state::<TrayState>()) else {
        return false;
    };
    let settings = services.settings.get().unwrap_or_default();
    match close_action(settings.keep_running_on_close, tray.is_active()) {
        CloseAction::Hide => {
            let _ = window.hide();
            if !settings.background_notice_shown && services.settings.mark_background_notice_shown().is_ok() {
                app.dialog().message(BACKGROUND_NOTICE).title("DameBola").kind(MessageDialogKind::Info).show(|_| {});
            }
            true
        }
        CloseAction::Quit => {
            let app = app.clone();
            // Fuera del manejador de eventos de la ventana.
            std::thread::spawn(move || quit(&app));
            true
        }
    }
}

/// ¿Debe impedirse que el proceso termine porque ya no quedan ventanas?
pub fn should_prevent_exit(code: Option<i32>, keep_running: bool, tray_active: bool, windows: usize, quitting: bool) -> bool {
    code.is_none() && !quitting && windows == 0 && close_action(keep_running, tray_active) == CloseAction::Hide
}

pub fn exit_requested_should_be_prevented(app: &AppHandle, code: Option<i32>) -> bool {
    let (Some(services), Some(tray)) = (app.try_state::<Arc<Services>>(), app.try_state::<TrayState>()) else {
        return false;
    };
    let keep = services.settings.get().map(|s| s.keep_running_on_close).unwrap_or(true);
    should_prevent_exit(code, keep, tray.is_active(), app.webview_windows().len(), services.quitting.load(Ordering::Acquire))
}

/// Salida completa: detiene red, audio, alertas y atajos globales, quita el icono y cierra el proceso.
pub fn quit(app: &AppHandle) {
    shutdown(app);
    app.exit(0);
}

/// Idempotente: se llama desde `quit` y desde `RunEvent::Exit`.
pub fn shutdown(app: &AppHandle) {
    let Some(services) = app.try_state::<Arc<Services>>() else { return };
    let services = Arc::clone(&services);
    if services.quitting.swap(true, Ordering::AcqRel) {
        return;
    }
    services.hotkeys.release_all();
    alert::end(app, &services, None);
    let _ = services.audio.stop();
    tauri::async_runtime::block_on(services.net.stop());
    let _ = tray::sync(app, false);
}

/// Acciones del menú de la bandeja.
pub fn on_tray_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        "quit" => {
            let app = app.clone();
            std::thread::spawn(move || quit(&app));
        }
        "send" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                send_from(&app, Origin::Tray).await;
            });
        }
        "mute" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let services = app.state::<Arc<Services>>();
                let next = tray::toggled_mute(&services.net.settings(), crate::db::now_ms());
                if services.net.save_settings(next.clone()).await.is_ok() {
                    let _ = app.emit(EVENT_NET_SETTINGS, next);
                }
                tray::schedule_refresh(&app);
            });
        }
        "autostart" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let services = Arc::clone(&app.state::<Arc<Services>>());
                let outcome = services.settings.get().and_then(|mut s| {
                    s.launch_at_login = !app.autolaunch().is_enabled().unwrap_or(s.launch_at_login);
                    apply_settings(&app, &services, s)
                });
                if let Err(e) = outcome {
                    let _ = app.emit(EVENT_SEND_RESULT, SendEvent { origin: "tray", result: AlertSendResult::fail("error", false, None, e.to_string()) });
                }
                tray::schedule_refresh(&app);
            });
        }
        _ => {}
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SendEvent {
    origin: &'static str,
    #[serde(flatten)]
    result: AlertSendResult,
}

/// Envío de DAME BOLA decidido en Rust (no depende de que React esté montado).
pub async fn send_from(app: &AppHandle, origin: Origin) -> AlertSendResult {
    let services = Arc::clone(&app.state::<Arc<Services>>());
    let result = match services.settings.get() {
        Ok(settings) => {
            let devices = services.net.devices();
            let net = &services.net;
            send_to_recipient(&services.send_guard, &devices, settings.shortcut_device_id.as_deref(), origin, |id| async move { net.send_alert(&id).await }).await
        }
        Err(e) => AlertSendResult::fail("error", false, None, e.to_string()),
    };
    if origin != Origin::Local {
        let name = if origin == Origin::Tray { "tray" } else { "global" };
        let _ = app.emit(EVENT_SEND_RESULT, SendEvent { origin: name, result: result.clone() });
    }
    result
}

/// Evento del atajo global. Solo `Pressed` envía; la repetición de tecla se descarta.
pub fn on_global_event(app: &AppHandle, state: ShortcutState) {
    let Some(services) = app.try_state::<Arc<Services>>() else { return };
    let mut gate = services.press_gate.lock().unwrap_or_else(|e| e.into_inner());
    match state {
        ShortcutState::Released => gate.release(),
        ShortcutState::Pressed => {
            if !gate.press(Instant::now()) {
                return;
            }
            drop(gate);
            // Defensa: si el modo global ya no está activo no se envía nada.
            if services.hotkeys.active().is_none() || services.quitting.load(Ordering::Acquire) {
                return;
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                send_from(&app, Origin::Global).await;
            });
        }
    }
}

/// Backend real del registro de atajos globales.
pub struct PluginShortcuts(pub AppHandle);

impl ShortcutBackend for PluginShortcuts {
    fn register(&self, plugin_text: &str) -> Result<(), String> {
        self.0.global_shortcut().on_shortcut(plugin_text, |app, _shortcut, event| on_global_event(app, event.state())).map_err(|e| e.to_string())
    }
    fn unregister(&self, plugin_text: &str) {
        let _ = self.0.global_shortcut().unregister(plugin_text);
    }
    fn unregister_all(&self) {
        let _ = self.0.global_shortcut().unregister_all();
    }
}

/// Aplica y guarda ajustes de forma transaccional: si algo falla, el estado anterior se conserva.
/// Orden: atajo global → bandeja → inicio con el sistema → SQLite.
pub fn apply_settings(app: &AppHandle, services: &Arc<Services>, incoming: Settings) -> AppResult<Settings> {
    let incoming = incoming.validated()?;
    let previous = services.settings.get()?;
    let wanted = hotkey::desired(&incoming);
    let shortcut_changed = wanted != hotkey::desired(&previous);
    let previous_active = services.hotkeys.active();

    services.hotkeys.validate(&incoming).map_err(AppError::Validation)?;
    let undo_shortcut = || {
        let _ = services.hotkeys.apply(previous_active.as_deref());
    };
    if shortcut_changed {
        services.hotkeys.apply(wanted.as_deref()).map_err(AppError::Validation)?;
    } else {
        // Sin cambios en el atajo: reintento sin bloquear el guardado de otros ajustes
        // (p. ej. si otra app lo ocupó desde el inicio; el error queda visible en Configuración).
        let _ = services.hotkeys.apply(wanted.as_deref());
    }

    let tray_was_active = app.state::<TrayState>().is_active();
    if let Err(e) = tray::sync(app, incoming.show_tray_icon) {
        undo_shortcut();
        return Err(AppError::System(e));
    }
    let undo_tray = || {
        let _ = tray::sync(app, tray_was_active);
    };

    let autostart = app.autolaunch();
    let autostart_was = autostart.is_enabled().unwrap_or(false);
    if incoming.launch_at_login != autostart_was {
        let result = if incoming.launch_at_login { autostart.enable() } else { autostart.disable() };
        if let Err(e) = result {
            undo_tray();
            undo_shortcut();
            return Err(AppError::System(format!("No se pudo cambiar el inicio con el sistema: {e}")));
        }
    }

    match services.settings.save(incoming) {
        Ok(saved) => {
            services.net.refresh_announcement(); // el nombre visible puede haber cambiado
            let _ = app.emit(EVENT_SETTINGS_CHANGED, &saved);
            tray::schedule_refresh(app);
            Ok(saved)
        }
        Err(e) => {
            if autostart.is_enabled().unwrap_or(false) != autostart_was {
                let _ = if autostart_was { autostart.enable() } else { autostart.disable() };
            }
            undo_tray();
            undo_shortcut();
            Err(e)
        }
    }
}

/// Estado para la pantalla de Configuración.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundStatus {
    /// Combinación global registrada ahora mismo.
    pub global_shortcut: Option<String>,
    /// Por qué no se pudo registrar el atajo global (ocupado, reservado…).
    pub global_error: Option<String>,
    pub tray_active: bool,
    pub os: &'static str,
}

pub fn status(app: &AppHandle, services: &Services) -> BackgroundStatus {
    BackgroundStatus {
        global_shortcut: services.hotkeys.active(),
        global_error: services.hotkeys.error(),
        tray_active: app.state::<TrayState>().is_active(),
        os: current_os(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_hides_only_with_background_enabled_and_a_tray_to_reopen_from() {
        assert_eq!(close_action(true, true), CloseAction::Hide);
        assert_eq!(close_action(false, true), CloseAction::Quit, "con segundo plano desactivado cerrar termina la app");
        assert_eq!(close_action(true, false), CloseAction::Quit, "sin bandeja nunca se esconde la app");
        assert_eq!(close_action(false, false), CloseAction::Quit);
    }

    #[test]
    fn last_window_closing_does_not_end_the_process_in_background_mode() {
        assert!(should_prevent_exit(None, true, true, 0, false));
        assert!(!should_prevent_exit(Some(0), true, true, 0, false), "salida explícita");
        assert!(!should_prevent_exit(None, true, true, 1, false), "Cmd+Q con ventanas abiertas se respeta");
        assert!(!should_prevent_exit(None, false, true, 0, false));
        assert!(!should_prevent_exit(None, true, true, 0, true), "ya se está saliendo");
    }

    #[test]
    fn hidden_start_only_when_launched_at_login_and_asked_for() {
        let mut s = Settings::default();
        assert!(start_visible(&s, true, true), "por defecto la ventana se muestra");
        s.start_minimized = true;
        assert!(!start_visible(&s, true, true));
        assert!(start_visible(&s, false, true), "abrir a mano siempre muestra la ventana");
        assert!(start_visible(&s, true, false), "sin bandeja no se puede ocultar");
    }
}
