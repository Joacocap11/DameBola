mod alert;
mod audio;
mod commands;
mod db;
mod error;
mod selection;
mod settings;
mod songs;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::alert::AlertManager;
use crate::audio::AudioPlayer;
use crate::db::Database;
use crate::settings::SettingsStore;
use crate::songs::SongService;

/// Servicios compartidos de la aplicación (estado gestionado por Tauri).
pub struct Services {
    pub songs: SongService,
    pub settings: SettingsStore,
    pub audio: AudioPlayer,
    pub alerts: AlertManager,
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

            app.manage(Arc::new(Services {
                songs: SongService::new(Arc::clone(&db), data_dir.join("audio"))?,
                settings: SettingsStore::new(db),
                audio,
                alerts: AlertManager::default(),
            }));
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
        ])
        .run(tauri::generate_context!())
        .expect("error al ejecutar DameBola");
}
