mod alert;
mod audio;
mod background;
mod commands;
mod db;
mod error;
mod hotkey;
mod instance;
mod net;
mod selection;
mod send;
mod settings;
mod shortcut;
mod songs;
mod tray;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};

use crate::alert::{AlertManager, AlertRequest};
use crate::audio::AudioPlayer;
use crate::db::Database;
use crate::error::AppError;
use crate::hotkey::{GlobalShortcuts, PressGate};
use crate::net::identity;
use crate::net::service::{
    current_os, AlertHandler, DeviceView, NetConfig, NetEvents, NetStatus, NetTimings, NetworkService, PairingView, RemoteAlertError, StoreIdentityProvider,
};
use crate::net::store::NetStore;
use crate::send::SendGuard;
use crate::settings::SettingsStore;
use crate::songs::SongService;

/// Servicios compartidos de la aplicación (estado gestionado por Tauri).
pub struct Services {
    pub songs: SongService,
    pub settings: SettingsStore,
    pub audio: AudioPlayer,
    pub alerts: AlertManager,
    pub net: NetworkService,
    /// Atajo global registrado en el sistema operativo (transaccional).
    pub hotkeys: GlobalShortcuts,
    /// Un envío a la vez (atajo local, global y bandeja).
    pub send_guard: SendGuard,
    /// Descarta la repetición de tecla del atajo global.
    pub press_gate: Mutex<PressGate>,
    /// `true` cuando comenzó la salida completa.
    pub quitting: AtomicBool,
}

/// Publica los cambios de red al frontend (payload completo en cada evento).
struct TauriNetEvents(tauri::AppHandle);

impl NetEvents for TauriNetEvents {
    fn status(&self, s: &NetStatus) {
        let _ = self.0.emit("net://status", s);
        tray::schedule_refresh(&self.0);
    }
    fn devices(&self, d: &[DeviceView]) {
        let _ = self.0.emit("net://devices", d);
        tray::schedule_refresh(&self.0);
    }
    fn pairing(&self, p: &[PairingView]) {
        let _ = self.0.emit("net://pairing", p);
    }
}

/// Una alerta remota reutiliza EXACTAMENTE el mismo sistema que la prueba local.
struct AppAlertHandler(tauri::AppHandle);

impl AlertHandler for AppAlertHandler {
    fn show(&self, sender_name: &str) -> Result<(), RemoteAlertError> {
        let services = self.0.state::<Arc<Services>>();
        match alert::start(&self.0, &services, AlertRequest { sender_name: sender_name.to_string() }) {
            Ok(_) => Ok(()),
            Err(AppError::Alert(m)) if m == alert::ALREADY_ACTIVE_MSG => Err(RemoteAlertError::Busy),
            Err(e) => Err(RemoteAlertError::Failed(e.to_string())),
        }
    }
}

pub fn run() {
    let mut builder = tauri::Builder::default();
    // Una sola instancia: un segundo lanzamiento solo trae al frente la ventana de la primera.
    // Las instancias de desarrollo con carpeta de datos propia (DAMEBOLA_DATA_DIR) no se bloquean entre sí.
    let isolated_dev = std::env::var(instance::ENV_DATA_DIR).is_ok_and(|v| !v.trim().is_empty());
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    if !isolated_dev {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| background::show_main(app)));
    }
    let _ = isolated_dev;
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![background::AUTOSTART_ARG])))
        .setup(|app| {
            // Directorio de datos del sistema operativo:
            //   macOS:   ~/Library/Application Support/com.damebola.app
            //   Windows: %APPDATA%\com.damebola.app
            // Las instancias de desarrollo deben usar su propia carpeta (DAMEBOLA_DATA_DIR); ver `instance.rs`.
            let runtime = instance::RuntimeEnv::resolve(app.path().app_data_dir()?, |k| std::env::var(k).ok());
            for var in &runtime.ignored {
                eprintln!("Aviso: {var} se ignora porque no se indicó DAMEBOLA_DATA_DIR (protege la identidad de la instalación real).");
            }
            // Una sola instancia por carpeta de datos (misma base, misma identidad, mismo puerto).
            let lock = instance::InstanceLock::acquire(&runtime.data_dir)?;
            app.manage(lock);
            let data_dir = runtime.data_dir.clone();
            let db = Arc::new(Database::open(&data_dir.join("damebola.db"))?);

            let handle = app.handle().clone();
            let audio = AudioPlayer::spawn(move |status| {
                let _ = handle.emit("playback://status", status);
            });

            // Identidad de red: certificado en SQLite, clave privada en el llavero del SO (o en archivo).
            // Si la identidad tiene un problema NO se regenera: la red queda detenida con un error visible.
            let net_store = NetStore::new(Arc::clone(&db));
            let backends = identity::open_secret_backends(&app.config().identifier, &data_dir, runtime.force_file_keys);
            let provider = Arc::new(StoreIdentityProvider { store: net_store.clone(), backends });
            let name_source = SettingsStore::new(Arc::clone(&db));
            let net = NetworkService::new(
                provider,
                net_store,
                Arc::new(TauriNetEvents(app.handle().clone())),
                Arc::new(AppAlertHandler(app.handle().clone())),
                NetConfig {
                    app_version: app.package_info().version.to_string(),
                    os: current_os(),
                    bind_override: None,
                    mdns: true,
                    timings: NetTimings::default(),
                    name: Arc::new(move || name_source.get().map(|s| s.user_name).unwrap_or_default()),
                    allow_loopback: runtime.allow_loopback,
                },
            )?;

            app.manage(tray::TrayState::default());
            let services = Arc::new(Services {
                songs: SongService::new(Arc::clone(&db), data_dir.join("audio"))?,
                settings: SettingsStore::new(db),
                audio,
                alerts: AlertManager::default(),
                net: net.clone(),
                hotkeys: GlobalShortcuts::new(Arc::new(background::PluginShortcuts(app.handle().clone())), current_os()),
                send_guard: SendGuard::default(),
                press_gate: Mutex::new(PressGate::new()),
                quitting: AtomicBool::new(false),
            });
            app.manage(Arc::clone(&services));
            tauri::async_runtime::spawn(async move { net.start().await });

            // Segundo plano: bandeja, atajo global (solo si el usuario lo activó) y ventana inicial.
            let settings = services.settings.get().unwrap_or_default();
            if settings.show_tray_icon {
                if let Err(e) = tray::sync(app.handle(), true) {
                    eprintln!("Aviso: {e}");
                }
            }
            match services.hotkeys.apply(hotkey::desired(&settings).as_deref()) {
                Ok(()) => {
                    if let Some(key) = services.hotkeys.active() {
                        eprintln!("Atajo global registrado: {key}");
                    }
                }
                // No impide arrancar: el error queda visible en Configuración y el modo global queda inactivo.
                Err(e) => eprintln!("Aviso: {e}"),
            }
            // Reescribe la entrada de inicio con el argumento actual (versiones anteriores no lo tenían).
            let autostart = app.autolaunch();
            if autostart.is_enabled().unwrap_or(false) {
                let _ = autostart.enable();
            }
            let by_autostart = std::env::args().any(|a| a == background::AUTOSTART_ARG);
            if background::start_visible(&settings, by_autostart, app.state::<tray::TrayState>().is_active()) {
                background::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Si el usuario cierra una ventana de alerta por su cuenta
            // (Alt+F4, Cmd+W), se termina toda la alerta.
            if matches!(event, WindowEvent::Destroyed) && alert::is_alert_label(window.label()) {
                let app = window.app_handle();
                let services = app.state::<Arc<Services>>();
                alert::end(app, &services, None);
            }
            // Cerrar la ventana principal no es salir: se oculta (o se sale, según los ajustes).
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == background::MAIN_LABEL && background::on_main_close_requested(window) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_songs,
            commands::import_songs,
            commands::rename_song,
            commands::set_song_enabled,
            commands::delete_song,
            commands::play_song,
            commands::pause_playback,
            commands::resume_playback,
            commands::stop_playback,
            commands::get_playback_status,
            commands::get_settings,
            commands::save_settings,
            commands::trigger_alert,
            commands::close_alert,
            commands::get_alert_context,
            commands::is_alert_active,
            commands::net_get_status,
            commands::net_list_devices,
            commands::net_get_pairing,
            commands::net_get_settings,
            commands::net_save_settings,
            commands::net_pair,
            commands::net_pairing_decision,
            commands::net_connect,
            commands::net_disconnect,
            commands::net_forget,
            commands::net_add_manual,
            commands::net_send_alert,
            commands::net_retry_identity,
            commands::net_reset_identity,
            commands::net_rename_device,
            commands::send_shortcut_alert,
            commands::get_background_status,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error al construir DameBola");

    app.run(|app, event| match event {
        // Sin ventanas abiertas el proceso sigue vivo (segundo plano); Cmd+Q y "Salir" sí terminan.
        RunEvent::ExitRequested { api, code, .. } => {
            if background::exit_requested_should_be_prevented(app, code) {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => background::shutdown(app),
        // macOS: clic en el icono del Dock con la ventana oculta.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => background::show_main(app),
        _ => {}
    });
}
