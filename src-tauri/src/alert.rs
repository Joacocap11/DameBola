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
pub const ALREADY_ACTIVE_MSG: &str = "Ya hay una alerta en curso.";

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

    /// Reserva el turno de alerta (una sola a la vez). Devuelve (generación, última canción).
    fn begin(&self) -> AppResult<(u64, Option<String>)> {
        let mut inner = self.lock();
        if inner.active {
            return Err(AppError::Alert(ALREADY_ACTIVE_MSG.into()));
        }
        inner.active = true;
        inner.generation += 1;
        Ok((inner.generation, inner.last_song_id.clone()))
    }

    /// Libera el turno. Con `expected_generation` solo actúa si coincide con la alerta en curso.
    /// Devuelve `false` si no había nada que terminar (idempotente).
    fn finish(&self, expected_generation: Option<u64>) -> bool {
        let mut inner = self.lock();
        if !inner.active || expected_generation.is_some_and(|g| g != inner.generation) {
            return false;
        }
        inner.active = false;
        inner.context = None;
        inner.generation += 1;
        true
    }
}

// ------------------------------------------------------------------------------------------
// Apertura de ventanas
//
// Significado exacto de «mostrada» (lo que confirma `ALERT_ACK stage=shown` al emisor):
// **todas** las ventanas de alerta (una por monitor) se crearon, se posicionaron y se pidió
// mostrarlas sin error del sistema. Si CUALQUIERA falla, la alerta se cancela por completo: se destruyen
// las que ya se habían abierto (sin ventanas huérfanas), no suena audio y el resultado es un error de
// visualización. NO garantiza que una persona la haya visto (pantalla apagada, usuario ausente, otra
// aplicación a pantalla completa...). El foco de la primera ventana es «mejor esfuerzo».
// ------------------------------------------------------------------------------------------

/// Una ventana de alerta ya creada.
pub trait WindowHandle {
    fn show(&self) -> Result<(), String>;
    /// Mejor esfuerzo: un fallo de foco no cancela la alerta (los botones siguen funcionando).
    fn focus(&self);
    fn destroy(&self);
}

/// Origen de ventanas (Tauri en producción; simulado en las pruebas).
pub trait Surface {
    type Handle: WindowHandle;
    fn monitor_count(&self) -> Result<usize, String>;
    fn open(&self, index: usize) -> Result<Self::Handle, String>;
}

/// Abre una ventana por monitor con la política «todo o nada». Ver el comentario de arriba.
fn open_all<S: Surface>(surface: &S) -> AppResult<Vec<S::Handle>> {
    let count = surface.monitor_count().map_err(|e| AppError::Alert(format!("No se pudieron listar los monitores: {e}")))?;
    if count == 0 {
        return Err(AppError::Alert("No se detectó ningún monitor.".into()));
    }
    let destroy_all = |windows: &[S::Handle]| windows.iter().for_each(WindowHandle::destroy);
    let mut windows: Vec<S::Handle> = Vec::with_capacity(count);
    for index in 0..count {
        match surface.open(index) {
            Ok(w) => windows.push(w),
            Err(e) => {
                destroy_all(&windows);
                return Err(AppError::Alert(format!("No se pudo abrir la ventana de alerta del monitor {}: {e}", index + 1)));
            }
        }
    }
    for (index, w) in windows.iter().enumerate() {
        if let Err(e) = w.show() {
            destroy_all(&windows);
            return Err(AppError::Alert(format!("No se pudo mostrar la ventana de alerta del monitor {}: {e}", index + 1)));
        }
    }
    if let Some(first) = windows.first() {
        first.focus(); // para que Escape funcione
    }
    Ok(windows)
}

struct TauriSurface {
    app: AppHandle,
    monitors: Vec<Monitor>,
}

impl TauriSurface {
    fn new(app: &AppHandle) -> AppResult<Self> {
        let mut monitors = app
            .available_monitors()
            .map_err(|e| AppError::Alert(format!("No se pudieron listar los monitores: {e}")))?;
        if monitors.is_empty() {
            monitors.extend(app.primary_monitor().ok().flatten());
        }
        Ok(Self { app: app.clone(), monitors })
    }
}

impl WindowHandle for WebviewWindow {
    fn show(&self) -> Result<(), String> {
        WebviewWindow::show(self).map_err(|e| e.to_string())
    }
    fn focus(&self) {
        let _ = self.set_focus();
    }
    fn destroy(&self) {
        let _ = WebviewWindow::destroy(self);
    }
}

impl Surface for TauriSurface {
    type Handle = WebviewWindow;
    fn monitor_count(&self) -> Result<usize, String> {
        Ok(self.monitors.len())
    }
    fn open(&self, index: usize) -> Result<WebviewWindow, String> {
        let monitor = self.monitors.get(index).ok_or("monitor inexistente")?;
        build_window(&self.app, index, monitor).map_err(|e| e.to_string())
    }
}

pub fn start(app: &AppHandle, services: &Arc<Services>, request: AlertRequest) -> AppResult<AlertStarted> {
    let (generation, last_song_id) = services.alerts.begin()?;

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

    // Todo o nada: si falla una ventana no hay audio ni alerta (ver `open_all`).
    let windows = open_all(&TauriSurface::new(app)?)?;
    let monitors = windows.len();

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
    if !services.alerts.finish(expected_generation) {
        return;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    #[derive(Default, Debug)]
    struct World {
        alive: BTreeSet<usize>,
        opened_ever: Vec<usize>,
        destroyed: Vec<usize>,
        shown: Vec<usize>,
        focused: Vec<usize>,
    }

    struct Fake {
        monitors: Result<usize, String>,
        fail_open: Option<usize>,
        fail_show: Option<usize>,
        world: Rc<RefCell<World>>,
    }

    #[derive(Debug)]
    struct FakeWindow {
        index: usize,
        fail_show: bool,
        world: Rc<RefCell<World>>,
    }

    impl WindowHandle for FakeWindow {
        fn show(&self) -> Result<(), String> {
            if self.fail_show {
                return Err("show falló".into());
            }
            self.world.borrow_mut().shown.push(self.index);
            Ok(())
        }
        fn focus(&self) {
            self.world.borrow_mut().focused.push(self.index);
        }
        fn destroy(&self) {
            let mut w = self.world.borrow_mut();
            w.alive.remove(&self.index);
            w.destroyed.push(self.index);
        }
    }

    impl Surface for Fake {
        type Handle = FakeWindow;
        fn monitor_count(&self) -> Result<usize, String> {
            self.monitors.clone()
        }
        fn open(&self, index: usize) -> Result<FakeWindow, String> {
            if self.fail_open == Some(index) {
                return Err("build falló".into());
            }
            let mut w = self.world.borrow_mut();
            w.alive.insert(index);
            w.opened_ever.push(index);
            Ok(FakeWindow { index, fail_show: self.fail_show == Some(index), world: self.world.clone() })
        }
    }

    fn fake(monitors: usize, fail_open: Option<usize>, fail_show: Option<usize>) -> (Fake, Rc<RefCell<World>>) {
        let world = Rc::new(RefCell::new(World::default()));
        (Fake { monitors: Ok(monitors), fail_open, fail_show, world: world.clone() }, world)
    }

    #[test]
    fn every_monitor_gets_a_shown_window_and_the_first_one_takes_focus() {
        let (surface, world) = fake(3, None, None);
        let windows = open_all(&surface).unwrap();
        assert_eq!(windows.len(), 3);
        let w = world.borrow();
        assert_eq!(w.shown, vec![0, 1, 2]);
        assert_eq!(w.focused, vec![0]);
        assert!(w.destroyed.is_empty());
        assert_eq!(w.alive.len(), 3);
    }

    #[test]
    fn a_failure_on_the_first_window_fails_the_alert_and_leaves_nothing_open() {
        let (surface, world) = fake(3, Some(0), None);
        let err = open_all(&surface).unwrap_err().to_string();
        assert!(err.contains("monitor 1"), "{err}");
        let w = world.borrow();
        assert!(w.alive.is_empty() && w.opened_ever.is_empty() && w.shown.is_empty());
    }

    #[test]
    fn a_failure_on_a_secondary_window_destroys_the_ones_already_opened() {
        let (surface, world) = fake(3, Some(2), None);
        let err = open_all(&surface).unwrap_err().to_string();
        assert!(err.contains("monitor 3"), "{err}");
        let w = world.borrow();
        assert_eq!(w.opened_ever, vec![0, 1]);
        assert!(w.alive.is_empty(), "sin ventanas huérfanas: {:?}", w.alive);
        assert!(w.shown.is_empty(), "no se muestra nada si la alerta no puede completarse");
    }

    #[test]
    fn a_show_failure_after_opening_everything_also_tears_everything_down() {
        for failing in [0, 1, 2] {
            let (surface, world) = fake(3, None, Some(failing));
            let err = open_all(&surface).unwrap_err().to_string();
            assert!(err.contains(&format!("monitor {}", failing + 1)), "{err}");
            assert!(world.borrow().alive.is_empty(), "show falla en {failing}");
            assert!(world.borrow().focused.is_empty(), "no se enfoca una alerta cancelada");
        }
    }

    #[test]
    fn no_monitors_or_a_listing_error_is_an_error_not_a_silent_success() {
        let (mut surface, world) = fake(0, None, None);
        assert!(open_all(&surface).is_err());
        surface.monitors = Err("sin pantalla".into());
        assert!(open_all(&surface).unwrap_err().to_string().contains("monitores"));
        assert!(world.borrow().alive.is_empty());
    }

    #[test]
    fn a_single_monitor_works() {
        let (surface, _) = fake(1, None, None);
        assert_eq!(open_all(&surface).unwrap().len(), 1);
    }

    #[test]
    fn only_one_alert_at_a_time_and_a_failed_attempt_releases_the_turn() {
        let m = AlertManager::default();
        let (gen1, _) = m.begin().unwrap();
        assert!(matches!(m.begin(), Err(AppError::Alert(msg)) if msg == ALREADY_ACTIVE_MSG), "no se acumulan alertas");
        // La apertura de ventanas falló: se termina la alerta y el turno queda libre.
        assert!(m.finish(Some(gen1)));
        assert!(!m.is_active());
        assert!(!m.finish(Some(gen1)), "terminar es idempotente");
        assert!(m.begin().is_ok(), "la siguiente alerta puede empezar");
    }

    #[test]
    fn a_stale_timer_cannot_end_a_newer_alert() {
        let m = AlertManager::default();
        let (old, _) = m.begin().unwrap();
        assert!(m.finish(None));
        let (new, _) = m.begin().unwrap();
        assert_ne!(old, new);
        assert!(!m.finish(Some(old)), "el temporizador viejo no cierra la alerta nueva");
        assert!(m.is_active());
        assert!(m.finish(Some(new)));
    }
}
