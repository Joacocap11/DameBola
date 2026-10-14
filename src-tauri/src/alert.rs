//! Gestión de las ventanas de alerta (una por monitor) y su ciclo de vida.
//!
//! Garantías:
//! - Solo una alerta activa a la vez (`start` falla si ya hay una).
//! - El audio se inicia UNA vez, desde el motor de audio, no desde las ventanas.
//! - `end` es idempotente: se puede llamar desde el botón, Escape, el temporizador
//!   o el cierre manual de una ventana sin efectos duplicados.
//! - Las ventanas son temporales y se destruyen al terminar; no se toca el
//!   escritorio ni las ventanas de otras aplicaciones.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::audio::PlaybackOrigin;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::Services;

pub const ALERT_LABEL_PREFIX: &str = "alert-";
pub const EVENT_ALERT_STATE: &str = "alert://state";

pub fn is_alert_label(label: &str) -> bool {
    label.starts_with(ALERT_LABEL_PREFIX)
}

/// Quién dispara la alerta. En la Fase 1 siempre es el usuario local; en
/// fases futuras una alerta remota usará el mismo punto de entrada con el
/// nombre del remitente.
#[derive(Debug, Clone)]
pub struct AlertRequest {
    pub sender_name: String,
}

/// Datos que consumen las ventanas de alerta al montarse.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertContext {
    pub sender_name: String,
    pub duration_secs: u32,
    pub animations_enabled: bool,
    pub song_name: Option<String>,
    pub ends_at_ms: i64,
}

/// Resultado de iniciar una alerta, para informar a la ventana principal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertStarted {
    pub monitors: usize,
    pub song_name: Option<String>,
    /// Si el audio falló, la alerta igual se muestra; aquí va el motivo.
    pub audio_error: Option<String>,
}

#[derive(Default)]
struct Inner {
    active: bool,
    generation: u64,
    context: Option<AlertContext>,
    last_song_id: Option<String>,
}

#[derive(Default)]
pub struct AlertManager {
    inner: Mutex<Inner>,
}

impl AlertManager {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_active(&self) -> bool {
        self.lock().active
    }

    pub fn context(&self) -> Option<AlertContext> {
        self.lock().context.clone()
    }
}

pub fn start(app: &AppHandle, services: &Arc<Services>, request: AlertRequest) -> AppResult<AlertStarted> {
    let manager = &services.alerts;
    let (generation, last_song_id) = {
        let mut inner = manager.lock();
        if inner.active {
            return Err(AppError::Alert("Ya hay una alerta en curso.".into()));
        }
        inner.active = true;
        inner.generation += 1;
        (inner.generation, inner.last_song_id.clone())
    };

    match start_inner(app, services, request, generation, last_song_id) {
        Ok(started) => Ok(started),
        Err(e) => {
            end(app, services, Some(generation));
            Err(e)
        }
    }
}

fn start_inner(
    app: &AppHandle,
    services: &Arc<Services>,
    request: AlertRequest,
    generation: u64,
    last_song_id: Option<String>,
) -> AppResult<AlertStarted> {
    let settings = services.settings.get()?;

    // Selección de canción. Un fallo aquí NO impide mostrar la alerta.
    let mut audio_error = None;
    let song = if settings.sound_enabled {
        match services.songs.pick_random(last_song_id.as_deref()) {
            Ok(song) => song,
            Err(e) => {
                audio_error = Some(e.to_string());
                None
            }
        }
    } else {
        None
    };

    let context = AlertContext {
        sender_name: request.sender_name,
        duration_secs: settings.alert_duration_secs,
        animations_enabled: settings.animations_enabled,
        song_name: song.as_ref().map(|s| s.name.clone()),
        ends_at_ms: now_ms() + i64::from(settings.alert_duration_secs) * 1000,
    };
    {
        // El contexto debe existir antes de crear las ventanas: lo piden al montarse.
        let mut inner = services.alerts.lock();
        inner.context = Some(context.clone());
        if let Some(s) = &song {
            inner.last_song_id = Some(s.id.clone());
        }
    }

    let monitors = create_windows(app)?;

    if let Some(song) = &song {
        let played = services
            .songs
            .audio_path(&song.id)
            .and_then(|path| services.audio.play(path, song.id.clone(), PlaybackOrigin::Alert, settings.alert_volume));
        if let Err(e) = played {
            audio_error = Some(e.to_string());
        }
    }

    spawn_timer(app.clone(), Arc::clone(services), generation, Duration::from_secs(u64::from(settings.alert_duration_secs)));
    let _ = app.emit(EVENT_ALERT_STATE, true);

    Ok(AlertStarted { monitors, song_name: context.song_name, audio_error })
}

/// Termina la alerta. Si `expected_generation` es `Some`, solo actúa si
/// coincide con la alerta en curso (evita que un temporizador viejo cierre
/// una alerta nueva).
pub fn end(app: &AppHandle, services: &Arc<Services>, expected_generation: Option<u64>) {
    {
        let mut inner = services.alerts.lock();
        if !inner.active || expected_generation.is_some_and(|g| g != inner.generation) {
            return;
        }
        inner.active = false;
        inner.context = None;
        inner.generation += 1;
    }
    // Lock liberado: destruir ventanas dispara eventos que reentran en `end`.
    for (label, window) in app.webview_windows() {
        if is_alert_label(&label) {
            let _ = window.destroy();
        }
    }
    let _ = services.audio.stop_origin(PlaybackOrigin::Alert);
    let _ = app.emit(EVENT_ALERT_STATE, false);
}

fn spawn_timer(app: AppHandle, services: Arc<Services>, generation: u64, duration: Duration) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            {
                let inner = services.alerts.lock();
                if !inner.active || inner.generation != generation {
                    return; // ya terminó por otra vía
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        end(&app, &services, Some(generation));
    });
}

/// Crea una ventana por monitor. Devuelve cuántas se crearon.
fn create_windows(app: &AppHandle) -> AppResult<usize> {
    let mut monitors = app
        .available_monitors()
        .map_err(|e| AppError::Alert(format!("No se pudieron listar los monitores: {e}")))?;
    if monitors.is_empty() {
        monitors.extend(app.primary_monitor().ok().flatten());
    }
    if monitors.is_empty() {
        return Err(AppError::Alert("No se detectó ningún monitor.".into()));
    }

    let mut windows: Vec<WebviewWindow> = Vec::with_capacity(monitors.len());
    for (i, monitor) in monitors.iter().enumerate() {
        match build_window(app, i, monitor) {
            Ok(w) => windows.push(w),
            Err(e) => {
                for w in &windows {
                    let _ = w.destroy();
                }
                return Err(e);
            }
        }
    }

    for w in &windows {
        let _ = w.show();
    }
    if let Some(first) = windows.first() {
        // La primera ventana toma el foco para que Escape funcione.
        let _ = first.set_focus();
    }
    Ok(windows.len())
}

fn build_window(app: &AppHandle, index: usize, monitor: &Monitor) -> AppResult<WebviewWindow> {
    let label = format!("{ALERT_LABEL_PREFIX}{index}");
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("DameBola - ¡Alerta!")
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible_on_all_workspaces(true)
        .accept_first_mouse(true)
        .focused(index == 0)
        .visible(false)
        .build()
        .map_err(|e| AppError::Alert(format!("No se pudo crear la ventana de alerta: {e}")))?;

    place_on_monitor(&window, monitor)?;
    Ok(window)
}

/// Cubre el monitor completo con una ventana sin bordes (NO pantalla
/// completa nativa: en macOS esa crearía un Space nuevo por monitor).
fn place_on_monitor(window: &WebviewWindow, monitor: &Monitor) -> AppResult<()> {
    let err = |e: tauri::Error| AppError::Alert(format!("No se pudo posicionar la ventana: {e}"));
    let pos = monitor.position();
    let size = monitor.size();

    #[cfg(target_os = "macos")]
    {
        // En macOS las coordenadas del monitor se convierten con SU factor de escala.
        let scale = monitor.scale_factor();
        window.set_position(pos.to_logical::<f64>(scale)).map_err(err)?;
        window.set_size(size.to_logical::<f64>(scale)).map_err(err)?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows: píxeles físicos del escritorio virtual. Con DPIs distintos
        // entre monitores hay que reposicionar tras cambiar el tamaño.
        window.set_position(*pos).map_err(err)?;
        window.set_size(*size).map_err(err)?;
        window.set_position(*pos).map_err(err)?;
    }
    Ok(())
}
