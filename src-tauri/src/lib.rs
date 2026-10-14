mod alert;
mod audio;
mod commands;
mod db;
mod error;
mod net;
mod selection;
mod settings;
mod shortcut;
mod songs;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::alert::{AlertManager, AlertRequest};
use crate::audio::AudioPlayer;
use crate::db::Database;
use crate::error::AppError;
use crate::net::identity;
use crate::net::service::{
    current_os, AlertHandler, DeviceView, NetConfig, NetEvents, NetStatus, NetTimings, NetworkService, PairingView, RemoteAlertError,
};
use crate::net::store::NetStore;
use crate::settings::SettingsStore;
use crate::songs::SongService;

/// Servicios compartidos de la aplicación (estado gestionado por Tauri).
pub struct Services {
    pub songs: SongService,
    pub settings: SettingsStore,
    pub audio: AudioPlayer,
    pub alerts: AlertManager,
    pub net: NetworkService,
}

/// Publica los cambios de red al frontend (payload completo en cada evento).
struct TauriNetEvents(tauri::AppHandle);

impl NetEvents for TauriNetEvents {
    fn status(&self, s: &NetStatus) {
        let _ = self.0.emit("net://status", s);
    }
    fn devices(&self, d: &[DeviceView]) {
        let _ = self.0.emit("net://devices", d);
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            // Directorio de datos del sistema operativo:
            //   macOS:   ~/Library/Application Support/com.damebola.app
            //   Windows: %APPDATA%\com.damebola.app
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db = Arc::new(Database::open(&data_dir.join("damebola.db"))?);

            let handle = app.handle().clone();
            let audio = AudioPlayer::spawn(move |status| {
                let _ = handle.emit("playback://status", status);
            });

            // Identidad de red: certificado en SQLite, clave privada en el llavero del SO.
            let net_store = NetStore::new(Arc::clone(&db));
            let secrets = identity::open_secret_store(&app.config().identifier, data_dir.join("device.key"));
            let device_identity = identity::load_or_create(&net_store, secrets.as_ref())?;
            let name_source = SettingsStore::new(Arc::clone(&db));
            let net = NetworkService::new(
                device_identity,
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
                },
            )?;

            app.manage(Arc::new(Services {
                songs: SongService::new(Arc::clone(&db), data_dir.join("audio"))?,
                settings: SettingsStore::new(db),
                audio,
                alerts: AlertManager::default(),
                net: net.clone(),
            }));
            tauri::async_runtime::spawn(async move { net.start().await });
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
        ])
        .run(tauri::generate_context!())
        .expect("error al ejecutar DameBola");
}
