//! Ajustes persistentes (guardados como JSON en la tabla `settings`).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::error::{AppError, AppResult};

const KEY_APP: &str = "app";

pub const DEFAULT_DURATION_SECS: u32 = 15;
pub const MIN_DURATION_SECS: u32 = 3;
pub const MAX_DURATION_SECS: u32 = 120;
const MAX_USER_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Nombre con el que se identificará quien envía una alerta (fases futuras).
    pub user_name: String,
    /// 0..=100
    pub alert_volume: u8,
    pub alert_duration_secs: u32,
    pub sound_enabled: bool,
    pub animations_enabled: bool,
    pub launch_at_login: bool,
    /// F12 en la ventana principal; nunca es un atajo global.
    pub shortcut_enabled: bool,
    /// Combinación canónica (ver `shortcut.rs`); F12 por defecto.
    pub shortcut_key: String,
    /// None elige el único equipo emparejado; varios requieren selección.
    pub shortcut_device_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: "Compañero".into(),
            alert_volume: 80,
            alert_duration_secs: DEFAULT_DURATION_SECS,
            sound_enabled: true,
            animations_enabled: true,
            launch_at_login: false,
            shortcut_enabled: true,
            shortcut_key: crate::shortcut::DEFAULT_SHORTCUT.into(),
            shortcut_device_id: None,
        }
    }
}

impl Settings {
    /// Valida y normaliza. Rechaza valores fuera de rango en lugar de
    /// corregirlos en silencio.
    pub fn validated(mut self) -> AppResult<Self> {
        self.user_name = self.user_name.trim().to_string();
        if self.user_name.is_empty() {
            return Err(AppError::Validation("El nombre de usuario no puede estar vacío.".into()));
        }
        if self.user_name.chars().count() > MAX_USER_NAME_CHARS {
            return Err(AppError::Validation(format!(
                "El nombre de usuario no puede superar {MAX_USER_NAME_CHARS} caracteres."
            )));
        }
        if self.alert_volume > 100 {
            return Err(AppError::Validation("El volumen debe estar entre 0 y 100.".into()));
        }
        if !(MIN_DURATION_SECS..=MAX_DURATION_SECS).contains(&self.alert_duration_secs) {
            return Err(AppError::Validation(format!(
                "La duración debe estar entre {MIN_DURATION_SECS} y {MAX_DURATION_SECS} segundos."
            )));
        }
        self.shortcut_key = crate::shortcut::normalize(&self.shortcut_key).map_err(AppError::Validation)?;
        if self.shortcut_device_id.as_ref().is_some_and(|id| !crate::net::protocol::is_device_id(id)) {
            return Err(AppError::Validation("El destinatario del atajo no es válido.".into()));
        }
        Ok(self)
    }
}

pub struct SettingsStore {
    db: Arc<Database>,
}

impl SettingsStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn read(&self, key: &str) -> AppResult<Option<String>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    fn write(&self, key: &str, value: &str) -> AppResult<()> {
        self.db.lock().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get(&self) -> AppResult<Settings> {
        match self.read(KEY_APP)? {
            Some(json) => {
                let mut settings: Settings = serde_json::from_str(&json).unwrap_or_default();
                // Un valor guardado no válido (editado a mano, versión futura) vuelve a F12.
                settings.shortcut_key = crate::shortcut::normalize(&settings.shortcut_key).unwrap_or_else(|_| crate::shortcut::DEFAULT_SHORTCUT.into());
                Ok(settings)
            }
            None => Ok(Settings::default()),
        }
    }

    pub fn save(&self, settings: Settings) -> AppResult<Settings> {
        let settings = settings.validated()?;
        let json = serde_json::to_string(&settings)
            .map_err(|e| AppError::System(format!("No se pudieron serializar los ajustes: {e}")))?;
        self.write(KEY_APP, &json)?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SettingsStore {
        SettingsStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn defaults_when_nothing_saved() {
        let s = store().get().unwrap();
        assert_eq!(s.alert_duration_secs, 15);
        assert!(s.sound_enabled);
    }

    #[test]
    fn persists_and_reloads() {
        let st = store();
        let mut s = st.get().unwrap();
        s.user_name = "  Ana ".into();
        s.alert_volume = 35;
        st.save(s).unwrap();
        let back = st.get().unwrap();
        assert_eq!(back.user_name, "Ana");
        assert_eq!(back.alert_volume, 35);
    }

    #[test]
    fn rejects_out_of_range_values() {
        let st = store();
        let mut s = Settings::default();
        s.alert_volume = 101;
        assert!(st.save(s.clone()).is_err());
        s.alert_volume = 50;
        s.alert_duration_secs = 1;
        assert!(st.save(s.clone()).is_err());
        s.alert_duration_secs = 15;
        s.user_name = "   ".into();
        assert!(st.save(s).is_err());
    }

    #[test]
    fn phase2_settings_keep_preferences_when_shortcut_fields_are_missing() {
        let old = r#"{"userName":"Ana","alertVolume":35,"soundEnabled":false,"launchAtLogin":true}"#;
        let loaded: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(loaded.user_name, "Ana");
        assert_eq!(loaded.alert_volume, 35);
        assert!(!loaded.sound_enabled);
        assert!(loaded.launch_at_login);
        assert!(loaded.shortcut_enabled);
        assert_eq!(loaded.shortcut_device_id, None);
    }

    #[test]
    fn shortcut_target_and_disabled_state_persist() {
        let st = store();
        let mut s = st.get().unwrap();
        s.shortcut_enabled = false;
        s.shortcut_device_id = Some("a".repeat(32));
        st.save(s.clone()).unwrap();
        assert_eq!(st.get().unwrap(), s);
        s.shortcut_device_id = Some("not-an-id".into());
        assert!(st.save(s).is_err());
    }

    /// Instalación de la versión 0.1.1 (atajo F12 fijo, sin `shortcutKey`): conserva todo.
    #[test]
    fn upgrading_from_the_fixed_f12_version_keeps_f12_state_and_recipient() {
        let st = store();
        let id = "b".repeat(32);
        let old = format!(r#"{{"userName":"Ana","alertVolume":35,"alertDurationSecs":20,"soundEnabled":false,"animationsEnabled":true,"launchAtLogin":false,"shortcutEnabled":false,"shortcutDeviceId":"{id}"}}"#);
        st.write(KEY_APP, &old).unwrap();
        let s = st.get().unwrap();
        assert_eq!(s.shortcut_key, "F12");
        assert!(!s.shortcut_enabled, "el estado desactivado se conserva");
        assert_eq!(s.shortcut_device_id.as_deref(), Some(id.as_str()));
        assert_eq!((s.user_name.as_str(), s.alert_volume, s.alert_duration_secs, s.sound_enabled), ("Ana", 35, 20, false));
        // Guardar sin tocar el atajo sigue en F12.
        assert_eq!(st.save(s).unwrap().shortcut_key, "F12");
    }

    #[test]
    fn shortcut_key_is_validated_normalized_and_persisted() {
        let st = store();
        let mut s = st.get().unwrap();
        assert_eq!(s.shortcut_key, "F12");
        s.shortcut_key = "shift+ctrl+b".into();
        let saved = st.save(s.clone()).unwrap();
        assert_eq!(saved.shortcut_key, "Ctrl+Shift+B", "se guarda la forma canónica");
        assert_eq!(st.get().unwrap().shortcut_key, "Ctrl+Shift+B");

        for bad in ["B", "Ctrl+C", "Space", "F25", "Alt+F4", "", "Ctrl+Alt+B"] {
            s.shortcut_key = bad.into();
            assert!(st.save(s.clone()).is_err(), "{bad}");
        }
        assert_eq!(st.get().unwrap().shortcut_key, "Ctrl+Shift+B", "un intento inválido no pisa el atajo guardado");
    }

    #[test]
    fn replacing_the_shortcut_leaves_other_settings_untouched() {
        let st = store();
        let mut s = st.get().unwrap();
        s.user_name = "Ana".into();
        s.shortcut_device_id = Some("c".repeat(32));
        st.save(s.clone()).unwrap();
        s.shortcut_key = "F8".into();
        st.save(s.clone()).unwrap();
        let back = st.get().unwrap();
        assert_eq!((back.shortcut_key.as_str(), back.user_name.as_str()), ("F8", "Ana"));
        assert_eq!(back.shortcut_device_id, s.shortcut_device_id);
    }

    #[test]
    fn invalid_stored_shortcut_falls_back_to_f12() {
        let st = store();
        st.write(KEY_APP, r#"{"userName":"Ana","shortcutKey":"Backspace"}"#).unwrap();
        assert_eq!(st.get().unwrap().shortcut_key, "F12");
    }
}
