//! Comandos Tauri: capa fina entre el frontend y los servicios.
//! El frontend nunca entrega rutas ni nombres de archivo; solo ids internos.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;

use crate::alert::{self, AlertContext, AlertRequest, AlertStarted};
use crate::audio::{PlaybackOrigin, PlaybackStatus};
use crate::error::{AppError, AppResult};
use crate::net::service::{AlertSendResult, DeviceView, NetStatus, PairingView};
use crate::net::store::NetworkSettings;
use crate::settings::Settings;
use crate::songs::{ImportReport, Song};
use crate::Services;

type Svc<'a> = State<'a, Arc<Services>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// "local" en la Fase 1. Fases futuras: "connected".
    pub connection_mode: &'static str,
    pub device_id: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, svc: Svc) -> AppResult<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        connection_mode: "local",
        device_id: svc.net.device_id().to_string(),
    })
}

// ------------------------------------------------------------- canciones

#[tauri::command]
pub fn list_songs(svc: Svc) -> AppResult<Vec<Song>> {
    svc.songs.list()
}

/// Abre el selector nativo (el frontend no maneja rutas) e importa lo elegido.
#[tauri::command]
pub async fn import_songs(app: AppHandle, svc: Svc<'_>) -> AppResult<ImportReport> {
    let services = Arc::clone(&svc);
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("Agregar canciones")
            .add_filter("Audio (MP3, WAV, OGG)", &["mp3", "wav", "ogg"])
            .blocking_pick_files();
        let Some(files) = picked else {
            return Ok(ImportReport { cancelled: true, ..Default::default() });
        };
        let paths: Vec<PathBuf> = files.into_iter().filter_map(|f| f.into_path().ok()).collect();
        Ok(services.songs.import_paths(&paths))
    })
    .await
    .map_err(|e| AppError::System(format!("La importación se interrumpió: {e}")))?
}

#[tauri::command]
pub fn rename_song(svc: Svc, id: String, name: String) -> AppResult<Song> {
    svc.songs.rename(&id, &name)
}

#[tauri::command]
pub fn set_song_enabled(svc: Svc, id: String, enabled: bool) -> AppResult<Song> {
    svc.songs.set_enabled(&id, enabled)
}

#[tauri::command]
pub fn delete_song(svc: Svc, id: String) -> AppResult<()> {
    // Si suena justo esa canción, se detiene antes de borrar el archivo.
    let status = svc.audio.status();
    if status.song_id.as_deref() == Some(id.as_str()) {
        svc.audio.stop()?;
    }
    svc.songs.delete(&id)
}

// ------------------------------------------------------------ reproducción

#[tauri::command]
pub fn play_song(svc: Svc, id: String) -> AppResult<()> {
    if svc.alerts.is_active() {
        return Err(AppError::Alert("No se puede reproducir mientras hay una alerta en curso.".into()));
    }
    let path = svc.songs.audio_path(&id)?;
    let volume = svc.settings.get()?.alert_volume;
    svc.audio.play(path, id, PlaybackOrigin::Preview, volume)
}

#[tauri::command]
pub fn pause_playback(svc: Svc) -> AppResult<()> {
    svc.audio.pause()
}

#[tauri::command]
pub fn resume_playback(svc: Svc) -> AppResult<()> {
    svc.audio.resume()
}

#[tauri::command]
pub fn stop_playback(svc: Svc) -> AppResult<()> {
    svc.audio.stop()
}

#[tauri::command]
pub fn get_playback_status(svc: Svc) -> PlaybackStatus {
    svc.audio.status()
}

// ------------------------------------------------------------ configuración

#[tauri::command]
pub fn get_settings(app: AppHandle, svc: Svc) -> AppResult<Settings> {
    let mut settings = svc.settings.get()?;
    // El estado real del sistema manda sobre lo guardado.
    if let Ok(actual) = app.autolaunch().is_enabled() {
        settings.launch_at_login = actual;
    }
    Ok(settings)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, svc: Svc, settings: Settings) -> AppResult<Settings> {
    crate::background::apply_settings(&app, &svc, settings)
}

/// Envío con el destinatario del atajo (mismas reglas que el atajo global y la bandeja).
#[tauri::command]
pub async fn send_shortcut_alert(app: AppHandle) -> AlertSendResult {
    crate::background::send_from(&app, crate::send::Origin::Local).await
}

#[tauri::command]
pub fn get_background_status(app: AppHandle, svc: Svc) -> crate::background::BackgroundStatus {
    crate::background::status(&app, &svc)
}

/// Salida completa (no confundir con cerrar la ventana).
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    std::thread::spawn(move || crate::background::quit(&app));
}

// ----------------------------------------------------------------- alertas

/// Dispara la alerta local de prueba. `async` para que la creación de
/// ventanas no bloquee el hilo principal (deadlock conocido en Windows).
#[tauri::command]
pub async fn trigger_alert(app: AppHandle, svc: Svc<'_>) -> AppResult<AlertStarted> {
    let services = Arc::clone(&svc);
    let sender_name = services.settings.get()?.user_name;
    alert::start(&app, &services, AlertRequest { sender_name })
}

#[tauri::command]
pub fn close_alert(app: AppHandle, svc: Svc) {
    alert::end(&app, &svc, None);
}

#[tauri::command]
pub fn get_alert_context(svc: Svc) -> Option<AlertContext> {
    svc.alerts.context()
}

#[tauri::command]
pub fn is_alert_active(svc: Svc) -> bool {
    svc.alerts.is_active()
}

// ------------------------------------------------------------- red (Fase 2)
// El frontend nunca abre sockets: todo pasa por estos comandos.

#[tauri::command]
pub fn net_get_status(svc: Svc) -> NetStatus {
    svc.net.status()
}

#[tauri::command]
pub fn net_list_devices(svc: Svc) -> Vec<DeviceView> {
    svc.net.devices()
}

#[tauri::command]
pub fn net_get_pairing(svc: Svc) -> Vec<PairingView> {
    svc.net.pairing()
}

#[tauri::command]
pub fn net_get_settings(svc: Svc) -> NetworkSettings {
    svc.net.settings()
}

#[tauri::command]
pub async fn net_save_settings(svc: Svc<'_>, settings: NetworkSettings) -> AppResult<NetworkSettings> {
    svc.net.save_settings(settings).await
}

#[tauri::command]
pub async fn net_pair(svc: Svc<'_>, device_id: String) -> Result<(), String> {
    svc.net.pair(&device_id).await
}

#[tauri::command]
pub fn net_pairing_decision(svc: Svc, device_id: String, accept: bool) {
    svc.net.pairing_decision(&device_id, accept);
}

#[tauri::command]
pub async fn net_connect(svc: Svc<'_>, device_id: String) -> Result<(), String> {
    svc.net.connect(&device_id).await
}

#[tauri::command]
pub fn net_disconnect(svc: Svc, device_id: String) {
    svc.net.disconnect(&device_id);
}

#[tauri::command]
pub async fn net_forget(svc: Svc<'_>, device_id: String) -> AppResult<()> {
    svc.net.forget(&device_id).await
}

#[tauri::command]
pub async fn net_add_manual(svc: Svc<'_>, host: String, port: u16) -> Result<DeviceView, String> {
    svc.net.add_manual(&host, port).await
}

#[tauri::command]
pub async fn net_send_alert(svc: Svc<'_>, device_id: String) -> Result<AlertSendResult, String> {
    Ok(svc.net.send_alert(&device_id).await)
}

/// Reintenta cargar la identidad (no destructivo): p. ej. tras desbloquear el llavero.
#[tauri::command]
pub async fn net_retry_identity(svc: Svc<'_>) -> Result<NetStatus, String> {
    Ok(svc.net.retry_identity().await)
}

/// Crea una identidad nueva. Solo con la identidad en error y con la confirmación exacta.
#[tauri::command]
pub async fn net_reset_identity(svc: Svc<'_>, confirm: String) -> Result<NetStatus, String> {
    svc.net.reset_identity(&confirm).await
}

#[tauri::command]
pub fn net_rename_device(svc: Svc, device_id: String, alias: Option<String>) -> Result<(), String> {
    svc.net.rename_device(&device_id, alias.as_deref())
}
