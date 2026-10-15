//! Registro del atajo GLOBAL de envío.
//!
//! Solo se registra UNA combinación concreta mediante la API del sistema operativo
//! (`tauri-plugin-global-shortcut`); nunca se observa el teclado en general.
//!
//! Transacción de cambio (`apply`): registrar la nueva → si funciona, liberar la anterior →
//! si falla, la anterior sigue registrada y no cambia nada.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::settings::{Settings, ShortcutScope};
use crate::shortcut;

/// Frontera con el sistema operativo (se sustituye en pruebas).
pub trait ShortcutBackend: Send + Sync {
    /// `plugin_text` es el formato de `shortcut::to_plugin_string`.
    fn register(&self, plugin_text: &str) -> Result<(), String>;
    fn unregister(&self, plugin_text: &str);
    fn unregister_all(&self);
}

#[derive(Default)]
struct State {
    /// Forma canónica registrada ahora (`Ctrl+Shift+B`).
    active: Option<String>,
    /// Último fallo de registro, para mostrarlo en Configuración.
    error: Option<String>,
}

pub struct GlobalShortcuts {
    backend: Arc<dyn ShortcutBackend>,
    os: &'static str,
    state: Mutex<State>,
}

/// Combinación global deseada según los ajustes (`None` = ninguna).
pub fn desired(settings: &Settings) -> Option<String> {
    (settings.shortcut_enabled && settings.shortcut_scope == ShortcutScope::Global).then(|| settings.shortcut_key.clone())
}

impl GlobalShortcuts {
    pub fn new(backend: Arc<dyn ShortcutBackend>, os: &'static str) -> Self {
        Self { backend, os, state: Mutex::new(State::default()) }
    }

    fn st(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn active(&self) -> Option<String> {
        self.st().active.clone()
    }

    pub fn error(&self) -> Option<String> {
        self.st().error.clone()
    }

    /// Reglas del sistema operativo para el modo global. No toca el registro.
    pub fn validate(&self, settings: &Settings) -> Result<(), String> {
        match desired(settings) {
            Some(key) => shortcut::validate_global(&key, self.os),
            None => Ok(()),
        }
    }

    /// Deja registrada exactamente `wanted` (o nada). En error conserva lo registrado antes.
    pub fn apply(&self, wanted: Option<&str>) -> Result<(), String> {
        let mut st = self.st();
        if st.active.as_deref() == wanted {
            st.error = None;
            return Ok(());
        }
        if let Some(key) = wanted {
            if let Err(e) = shortcut::validate_global(key, self.os) {
                st.error = Some(e.clone());
                return Err(e);
            }
            if let Err(detail) = self.backend.register(&shortcut::to_plugin_string(key)) {
                let message = format!(
                    "No se pudo registrar {key} como atajo global: otro programa o el sistema ya lo usa. Elegí otra combinación. ({detail})"
                );
                st.error = Some(message.clone());
                return Err(message);
            }
        }
        if let Some(old) = st.active.take() {
            self.backend.unregister(&shortcut::to_plugin_string(&old));
        }
        st.active = wanted.map(String::from);
        st.error = None;
        Ok(())
    }

    /// Libera todo (salida completa).
    pub fn release_all(&self) {
        let mut st = self.st();
        self.backend.unregister_all();
        st.active = None;
    }
}

/// Descarta la repetición de tecla: mientras se mantenga pulsada, una sola pulsación cuenta.
/// Si la liberación nunca llega, una pulsación nueva tras `REARM` vuelve a valer.
pub struct PressGate {
    down: bool,
    last: Option<Instant>,
}

const REARM: Duration = Duration::from_millis(1500);

impl PressGate {
    pub fn new() -> Self {
        Self { down: false, last: None }
    }

    /// `true` si esta pulsación debe disparar el envío.
    pub fn press(&mut self, now: Instant) -> bool {
        let held = self.down && self.last.is_some_and(|t| now.saturating_duration_since(t) <= REARM);
        self.down = true;
        self.last = Some(now);
        !held
    }

    pub fn release(&mut self) {
        self.down = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct Fake {
        registered: Mutex<HashSet<String>>,
        taken: Mutex<HashSet<String>>,
    }

    impl ShortcutBackend for Fake {
        fn register(&self, t: &str) -> Result<(), String> {
            if self.taken.lock().unwrap().contains(t) {
                return Err("ocupado".into());
            }
            self.registered.lock().unwrap().insert(t.into());
            Ok(())
        }
        fn unregister(&self, t: &str) {
            self.registered.lock().unwrap().remove(t);
        }
        fn unregister_all(&self) {
            self.registered.lock().unwrap().clear();
        }
    }

    fn setup(os: &'static str) -> (Arc<Fake>, GlobalShortcuts) {
        let fake = Arc::new(Fake::default());
        (Arc::clone(&fake), GlobalShortcuts::new(fake, os))
    }

    fn regs(f: &Fake) -> Vec<String> {
        let mut v: Vec<String> = f.registered.lock().unwrap().iter().cloned().collect();
        v.sort();
        v
    }

    #[test]
    fn nothing_is_registered_until_global_scope_is_chosen() {
        let mut s = Settings::default();
        assert_eq!((s.shortcut_key.as_str(), desired(&s)), ("F12", None), "F12 local por defecto no se registra");
        s.shortcut_scope = ShortcutScope::Global;
        assert_eq!(desired(&s).as_deref(), Some("F12"));
        s.shortcut_enabled = false;
        assert_eq!(desired(&s), None, "desactivado nunca es global");
    }

    #[test]
    fn busy_shortcut_reports_error_and_keeps_the_previous_one() {
        let (fake, g) = setup("windows");
        g.apply(Some("Ctrl+Shift+B")).unwrap();
        fake.taken.lock().unwrap().insert("Ctrl+Shift+K".into());
        let err = g.apply(Some("Ctrl+Shift+K")).unwrap_err();
        assert!(err.contains("Ctrl+Shift+K"));
        assert_eq!(g.active().as_deref(), Some("Ctrl+Shift+B"));
        assert_eq!(regs(&fake), ["Ctrl+Shift+B"], "la anterior sigue registrada");
        assert!(g.error().is_some());
        g.apply(Some("Ctrl+Shift+B")).unwrap();
        assert!(g.error().is_none());
    }

    #[test]
    fn changing_shortcut_registers_new_then_releases_old() {
        let (fake, g) = setup("windows");
        g.apply(Some("Ctrl+Shift+B")).unwrap();
        g.apply(Some("Alt+F9")).unwrap();
        assert_eq!(regs(&fake), ["Alt+F9"]);
        assert_eq!(g.active().as_deref(), Some("Alt+F9"));
    }

    #[test]
    fn switching_back_to_local_releases_everything() {
        let (fake, g) = setup("macos");
        g.apply(Some("Meta+Shift+B")).unwrap();
        assert_eq!(regs(&fake), ["Super+Shift+B"]);
        g.apply(None).unwrap();
        assert!(regs(&fake).is_empty());
        assert_eq!(g.active(), None);
    }

    #[test]
    fn os_reserved_combinations_are_never_registered() {
        let (fake, g) = setup("windows");
        assert!(g.apply(Some("Meta+B")).is_err());
        assert!(regs(&fake).is_empty());
        let (fake, g) = setup("macos");
        assert!(g.apply(Some("Meta+Q")).is_err());
        assert!(regs(&fake).is_empty());
    }

    #[test]
    fn release_all_frees_the_shortcut_on_exit() {
        let (fake, g) = setup("windows");
        g.apply(Some("Ctrl+Shift+B")).unwrap();
        g.release_all();
        assert!(regs(&fake).is_empty());
        assert_eq!(g.active(), None);
    }

    #[test]
    fn key_repeat_counts_once_and_a_new_press_after_release_counts_again() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut gate = PressGate::new();
        assert!(gate.press(ms(0)));
        // Repetición del sistema operativo: nunca dispara mientras no se suelte.
        for n in (500..4000).step_by(30) {
            assert!(!gate.press(ms(n)), "repetición a {n} ms");
        }
        gate.release();
        assert!(gate.press(ms(4100)));
    }

    #[test]
    fn lost_release_event_does_not_lock_the_shortcut_forever() {
        let t0 = Instant::now();
        let mut gate = PressGate::new();
        assert!(gate.press(t0));
        assert!(!gate.press(t0 + Duration::from_millis(800)));
        assert!(gate.press(t0 + Duration::from_millis(800 + 1600)));
    }
}
