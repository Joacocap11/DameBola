//! Persistencia de la red en SQLite: certificado público, dispositivos
//! emparejados y ajustes de red. La clave privada NO se guarda aquí.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{now_ms, Database};
use crate::error::{AppError, AppResult};

pub const DEFAULT_PORT: u16 = 38473;
pub const MIN_PORT: u16 = 1024;
pub const MIN_COOLDOWN_SECS: u32 = 5;
pub const MAX_COOLDOWN_SECS: u32 = 3600;
const KEY_NETWORK: &str = "network";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub enabled: bool,
    pub port: u16,
    pub remote_alerts_enabled: bool,
    /// Epoch ms hasta el cual las alertas remotas están silenciadas.
    pub muted_until_ms: Option<i64>,
    pub alert_cooldown_secs: u32,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            // La red arranca activa, pero sin dispositivos emparejados no
            // se acepta nada excepto solicitudes de emparejamiento explícitas.
            enabled: true,
            port: DEFAULT_PORT,
            remote_alerts_enabled: true,
            muted_until_ms: None,
            alert_cooldown_secs: 15,
        }
    }
}

impl NetworkSettings {
    pub fn validated(self) -> AppResult<Self> {
        if self.port < MIN_PORT {
            return Err(AppError::Validation(format!("El puerto debe estar entre {MIN_PORT} y 65535.")));
        }
        if !(MIN_COOLDOWN_SECS..=MAX_COOLDOWN_SECS).contains(&self.alert_cooldown_secs) {
            return Err(AppError::Validation(format!(
                "El intervalo entre alertas debe estar entre {MIN_COOLDOWN_SECS} y {MAX_COOLDOWN_SECS} segundos."
            )));
        }
        Ok(self)
    }

    pub fn is_muted(&self, now: i64) -> bool {
        self.muted_until_ms.is_some_and(|t| t > now)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    pub device_id: String,
    /// Último nombre anunciado por el propio dispositivo (visto en un HELLO autenticado
    /// por TLS). mDNS nunca lo modifica.
    pub name: String,
    /// Nombre personalizado local. Tiene prioridad sobre `name` y nadie lo pisa.
    pub alias: Option<String>,
    /// `false` = el usuario lo desconectó a propósito; no se reconecta solo.
    pub auto_connect: bool,
    pub os: String,
    /// SHA-256 del certificado, 64 hex en minúscula.
    pub fingerprint: String,
    pub last_address: Option<String>,
    pub last_port: Option<u16>,
    pub paired_at: i64,
    pub last_seen_at: Option<i64>,
}

impl PairedDevice {
    /// Nombre que ve el usuario: alias local o, si no hay, el anunciado.
    pub fn display_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }

    fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            device_id: r.get("device_id")?,
            name: r.get("name")?,
            alias: r.get("alias")?,
            auto_connect: r.get::<_, i64>("auto_connect")? != 0,
            os: r.get("os")?,
            fingerprint: r.get("fingerprint")?,
            last_address: r.get("last_address")?,
            last_port: r.get::<_, Option<i64>>("last_port")?.and_then(|p| u16::try_from(p).ok()),
            paired_at: r.get("paired_at")?,
            last_seen_at: r.get("last_seen_at")?,
        })
    }
}

#[derive(Clone)]
pub struct NetStore {
    db: Arc<Database>,
}

impl NetStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    // ------------------------------------------------------------ identidad

    pub fn identity_cert(&self) -> AppResult<Option<Vec<u8>>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT cert_der FROM device_identity WHERE id = 1", [], |r| r.get(0))
            .optional()?)
    }

    /// Alta de la identidad. **Nunca sobrescribe**: falla si ya hay un certificado.
    pub fn create_identity_cert(&self, cert_der: &[u8]) -> AppResult<()> {
        let n = self.db.lock().execute(
            "INSERT INTO device_identity (id, cert_der, created_at) VALUES (1, ?1, ?2) ON CONFLICT(id) DO NOTHING",
            params![cert_der, now_ms()],
        )?;
        if n == 0 {
            return Err(AppError::Validation("Ya existe una identidad: no se sobrescribe.".into()));
        }
        Ok(())
    }

    /// Reemplazo explícito (recuperación): archiva el certificado público anterior y guarda el nuevo
    /// en una sola transacción. Solo debe llamarse tras una confirmación del usuario.
    pub fn replace_identity_cert_archiving(&self, old_cert: &[u8], old_device_id: &str, old_fp_hex: &str, new_cert: &[u8], reason: &str) -> AppResult<()> {
        let mut conn = self.db.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO identity_archive (device_id, fingerprint, cert_der, archived_at, reason) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![old_device_id, old_fp_hex, old_cert, now_ms(), reason],
        )?;
        tx.execute(
            "INSERT INTO device_identity (id, cert_der, created_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET cert_der = excluded.cert_der, created_at = excluded.created_at",
            params![new_cert, now_ms()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn archived_identities(&self) -> AppResult<i64> {
        Ok(self.db.lock().query_row("SELECT count(*) FROM identity_archive", [], |r| r.get(0))?)
    }

    pub fn has_paired_devices(&self) -> AppResult<bool> {
        Ok(self.db.lock().query_row("SELECT EXISTS(SELECT 1 FROM paired_devices)", [], |r| r.get::<_, i64>(0))? != 0)
    }

    // ------------------------------------------------------------- ajustes

    pub fn settings(&self) -> AppResult<NetworkSettings> {
        let json: Option<String> = self
            .db
            .lock()
            .query_row("SELECT value FROM settings WHERE key = ?1", params![KEY_NETWORK], |r| r.get(0))
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
    }

    pub fn save_settings(&self, settings: NetworkSettings) -> AppResult<NetworkSettings> {
        let settings = settings.validated()?;
        let json = serde_json::to_string(&settings).map_err(|e| AppError::System(e.to_string()))?;
        self.db.lock().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![KEY_NETWORK, json],
        )?;
        Ok(settings)
    }

    // ------------------------------------------------------- emparejados

    pub fn list_paired(&self) -> AppResult<Vec<PairedDevice>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT * FROM paired_devices ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], PairedDevice::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_paired(&self, device_id: &str) -> AppResult<Option<PairedDevice>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT * FROM paired_devices WHERE device_id = ?1", params![device_id], PairedDevice::from_row)
            .optional()?)
    }

    /// Autorización: ¿está esta identidad criptográfica emparejada?
    pub fn paired_by_fingerprint(&self, fingerprint_hex: &str) -> AppResult<Option<PairedDevice>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT * FROM paired_devices WHERE fingerprint = ?1", params![fingerprint_hex], PairedDevice::from_row)
            .optional()?)
    }

    /// Alta o reemplazo (si el mismo `device_id` se vuelve a emparejar).
    pub fn upsert_paired(&self, d: &PairedDevice) -> AppResult<()> {
        self.db.lock().execute(
            "INSERT INTO paired_devices (device_id, name, os, fingerprint, last_address, last_port, paired_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(device_id) DO UPDATE SET name = excluded.name, os = excluded.os, fingerprint = excluded.fingerprint,
               last_address = excluded.last_address, last_port = excluded.last_port, paired_at = excluded.paired_at,
               last_seen_at = excluded.last_seen_at, auto_connect = 1",
            params![d.device_id, d.name, d.os, d.fingerprint, d.last_address, d.last_port.map(i64::from), d.paired_at, d.last_seen_at],
        )?;
        Ok(())
    }

    pub fn touch_paired(&self, device_id: &str, name: &str, os: &str, address: Option<(String, u16)>) -> AppResult<()> {
        let (addr, port) = match address {
            Some((a, p)) => (Some(a), Some(i64::from(p))),
            None => (None, None),
        };
        self.db.lock().execute(
            "UPDATE paired_devices SET name = ?2, os = ?3, last_seen_at = ?4,
               last_address = COALESCE(?5, last_address), last_port = COALESCE(?6, last_port)
             WHERE device_id = ?1",
            params![device_id, name, os, now_ms(), addr, port],
        )?;
        Ok(())
    }

    /// Actualiza solo la dirección conocida (p. ej. por un anuncio mDNS ya validado). No toca nombre ni alias.
    pub fn touch_address(&self, device_id: &str, address: &str, port: u16) -> AppResult<()> {
        self.db.lock().execute(
            "UPDATE paired_devices SET last_address = ?2, last_port = ?3, last_seen_at = ?4 WHERE device_id = ?1",
            params![device_id, address, i64::from(port), now_ms()],
        )?;
        Ok(())
    }

    /// Alias local. `None` (o vacío) lo quita. Devuelve `false` si el dispositivo no existe.
    pub fn set_alias(&self, device_id: &str, alias: Option<&str>) -> AppResult<bool> {
        let alias = alias.map(str::trim).filter(|a| !a.is_empty());
        Ok(self.db.lock().execute("UPDATE paired_devices SET alias = ?2 WHERE device_id = ?1", params![device_id, alias])? > 0)
    }

    /// Persiste la intención del usuario: `false` = desconectado a propósito.
    pub fn set_auto_connect(&self, device_id: &str, auto: bool) -> AppResult<bool> {
        Ok(self.db.lock().execute("UPDATE paired_devices SET auto_connect = ?2 WHERE device_id = ?1", params![device_id, auto as i64])? > 0)
    }

    /// Revoca un dispositivo. Devuelve `true` si existía.
    pub fn remove_paired(&self, device_id: &str) -> AppResult<bool> {
        Ok(self.db.lock().execute("DELETE FROM paired_devices WHERE device_id = ?1", params![device_id])? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> NetStore {
        NetStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn dev(id: char, fp: char) -> PairedDevice {
        PairedDevice {
            device_id: id.to_string().repeat(32),
            name: "Ana".into(),
            alias: None,
            auto_connect: true,
            os: "windows".into(),
            fingerprint: fp.to_string().repeat(64),
            last_address: Some("192.0.2.10".into()),
            last_port: Some(38473),
            paired_at: 1,
            last_seen_at: None,
        }
    }

    #[test]
    fn settings_default_persist_and_validate() {
        let s = store();
        assert_eq!(s.settings().unwrap(), NetworkSettings::default());
        let mut n = NetworkSettings::default();
        n.port = 40000;
        n.muted_until_ms = Some(123);
        s.save_settings(n.clone()).unwrap();
        assert_eq!(s.settings().unwrap(), n);
        assert!(s.save_settings(NetworkSettings { port: 80, ..n.clone() }).is_err());
        assert!(s.save_settings(NetworkSettings { alert_cooldown_secs: 1, ..n }).is_err());
    }

    #[test]
    fn mute_expires() {
        let n = NetworkSettings { muted_until_ms: Some(100), ..Default::default() };
        assert!(n.is_muted(99));
        assert!(!n.is_muted(100));
        assert!(!NetworkSettings::default().is_muted(0));
    }

    #[test]
    fn pairing_lookup_by_fingerprint_and_revocation() {
        let s = store();
        let d = dev('a', 'b');
        s.upsert_paired(&d).unwrap();
        assert_eq!(s.paired_by_fingerprint(&d.fingerprint).unwrap().unwrap().device_id, d.device_id);
        assert!(s.paired_by_fingerprint(&"c".repeat(64)).unwrap().is_none());
        assert!(s.remove_paired(&d.device_id).unwrap());
        assert!(s.paired_by_fingerprint(&d.fingerprint).unwrap().is_none());
        assert!(!s.remove_paired(&d.device_id).unwrap());
    }

    #[test]
    fn touch_updates_last_seen_and_keeps_address_when_unknown() {
        let s = store();
        let d = dev('a', 'b');
        s.upsert_paired(&d).unwrap();
        s.touch_paired(&d.device_id, "Ana R.", "windows", None).unwrap();
        let back = s.get_paired(&d.device_id).unwrap().unwrap();
        assert_eq!(back.name, "Ana R.");
        assert!(back.last_seen_at.is_some());
        assert_eq!(back.last_address.as_deref(), Some("192.0.2.10"));
    }

    #[test]
    fn alias_and_auto_connect_survive_touch_and_repair() {
        let s = store();
        let d = dev('a', 'b');
        s.upsert_paired(&d).unwrap();
        assert!(s.set_alias(&d.device_id, Some("  Mate  ")).unwrap());
        assert!(s.set_auto_connect(&d.device_id, false).unwrap());
        // Un HELLO autenticado actualiza el nombre anunciado, no el alias ni la intención.
        s.touch_paired(&d.device_id, "Ana R.", "windows", None).unwrap();
        let back = s.get_paired(&d.device_id).unwrap().unwrap();
        assert_eq!((back.name.as_str(), back.alias.as_deref(), back.auto_connect, back.display_name()), ("Ana R.", Some("Mate"), false, "Mate"));
        // Quitar el alias vuelve al nombre anunciado.
        s.set_alias(&d.device_id, Some("   ")).unwrap();
        assert_eq!(s.get_paired(&d.device_id).unwrap().unwrap().display_name(), "Ana R.");
        // Volver a emparejar conserva el alias y reactiva la conexión automática.
        s.set_alias(&d.device_id, Some("Mate")).unwrap();
        s.upsert_paired(&d).unwrap();
        let again = s.get_paired(&d.device_id).unwrap().unwrap();
        assert_eq!((again.alias.as_deref(), again.auto_connect), (Some("Mate"), true));
        assert!(!s.set_alias(&"z".repeat(32), Some("x")).unwrap());
    }

    #[test]
    fn identity_is_never_overwritten_by_create_and_replacement_archives_the_old_one() {
        let s = store();
        s.create_identity_cert(&[1, 2, 3]).unwrap();
        assert!(s.create_identity_cert(&[9, 9, 9]).is_err());
        assert_eq!(s.identity_cert().unwrap().unwrap(), vec![1, 2, 3]);
        assert_eq!(s.archived_identities().unwrap(), 0);
        s.replace_identity_cert_archiving(&[1, 2, 3], &"a".repeat(32), &"b".repeat(64), &[7, 7], "prueba").unwrap();
        assert_eq!(s.identity_cert().unwrap().unwrap(), vec![7, 7]);
        assert_eq!(s.archived_identities().unwrap(), 1);
    }
}
