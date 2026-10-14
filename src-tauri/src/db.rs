//! Acceso a SQLite: apertura, migraciones y acceso serializado a la conexión.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::error::AppResult;

/// Migraciones ordenadas. El índice + 1 es el valor de `PRAGMA user_version`
/// una vez aplicada. NUNCA editar una migración ya publicada: agregar otra.
const MIGRATIONS: &[&str] = &[
    // 1: esquema inicial.
    //
    // `id` es un UUID (no un autoincremental) y `sha256` es único: ambos
    // identifican la canción de forma estable entre computadoras, lo que
    // permitirá sincronizar en fases futuras sin colisiones de ids.
    r#"
    CREATE TABLE songs (
        id                TEXT PRIMARY KEY NOT NULL,
        name              TEXT NOT NULL CHECK (length(trim(name)) > 0),
        filename          TEXT NOT NULL UNIQUE CHECK (length(filename) > 0),
        original_filename TEXT NOT NULL,
        format            TEXT NOT NULL CHECK (format IN ('mp3', 'wav', 'ogg')),
        size_bytes        INTEGER NOT NULL CHECK (size_bytes > 0),
        sha256            TEXT NOT NULL UNIQUE CHECK (length(sha256) = 64),
        enabled           INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
        created_at        INTEGER NOT NULL,
        updated_at        INTEGER NOT NULL
    ) STRICT;

    CREATE INDEX idx_songs_enabled    ON songs (enabled);
    CREATE INDEX idx_songs_created_at ON songs (created_at);
    CREATE INDEX idx_songs_name       ON songs (name COLLATE NOCASE);

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    ) STRICT;
    "#,
    // 2: Fase 2 (red P2P). Solo agrega tablas; no toca `songs` ni `settings`.
    //
    // `device_id` se deriva del fingerprint del certificado (ver net/identity.rs),
    // por lo que no puede falsificarse sin la clave privada. La clave privada
    // NUNCA se guarda aquí: vive en el llavero del sistema.
    r#"
    CREATE TABLE device_identity (
        id         INTEGER PRIMARY KEY CHECK (id = 1),
        cert_der   BLOB NOT NULL,
        created_at INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE paired_devices (
        device_id        TEXT PRIMARY KEY NOT NULL CHECK (length(device_id) = 32),
        name             TEXT NOT NULL CHECK (length(trim(name)) > 0),
        os               TEXT NOT NULL,
        fingerprint      TEXT NOT NULL UNIQUE CHECK (length(fingerprint) = 64),
        last_address     TEXT,
        last_port        INTEGER CHECK (last_port IS NULL OR last_port BETWEEN 1 AND 65535),
        paired_at        INTEGER NOT NULL,
        last_seen_at     INTEGER
    ) STRICT;
    "#,
    // 3: Fase 2.5 (estabilización). Solo agrega columnas con valor por defecto y una
    // tabla de archivo; los nombres y emparejamientos existentes se conservan intactos.
    //
    // - `name` pasa a significar «último nombre anunciado y autenticado por el propio
    //   dispositivo»; `alias` es el nombre personalizado local (nunca lo pisa la red).
    // - `auto_connect = 0` guarda la desconexión voluntaria: no se reconecta solo.
    // - `identity_archive` guarda certificados públicos reemplazados (nunca claves privadas).
    r#"
    ALTER TABLE paired_devices ADD COLUMN alias TEXT CHECK (alias IS NULL OR length(trim(alias)) > 0);
    ALTER TABLE paired_devices ADD COLUMN auto_connect INTEGER NOT NULL DEFAULT 1 CHECK (auto_connect IN (0, 1));

    CREATE TABLE identity_archive (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        device_id   TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        cert_der    BLOB NOT NULL,
        archived_at INTEGER NOT NULL,
        reason      TEXT NOT NULL
    ) STRICT;
    "#,
];

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> AppResult<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        // Una conexión SQLite sigue siendo usable aunque otro hilo haya
        // entrado en pánico con el candado tomado.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn migrate(conn: &mut Connection) -> AppResult<()> {
    migrate_to(conn, MIGRATIONS.len())
}

/// Aplica las migraciones pendientes hasta la versión `target` (inclusive).
fn migrate_to(conn: &mut Connection, target: usize) -> AppResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (idx, sql) in MIGRATIONS.iter().enumerate().take(target).skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (idx + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

/// Milisegundos desde epoch Unix (UTC).
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap()
    }

    #[test]
    fn upgrading_a_phase1_database_keeps_songs_and_settings() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fase1.db");
        {
            // Base creada exactamente como la Fase 1 (solo migración 1).
            let mut conn = Connection::open(&path).unwrap();
            migrate_to(&mut conn, 1).unwrap();
            assert_eq!(version(&conn), 1);
            conn.execute(
                "INSERT INTO songs (id, name, filename, original_filename, format, size_bytes, sha256, enabled, created_at, updated_at)
                 VALUES ('id1', 'Mi canción', 'id1.mp3', 'x.mp3', 'mp3', 10, ?1, 1, 5, 5)",
                [&"a".repeat(64)],
            )
            .unwrap();
            conn.execute("INSERT INTO settings (key, value) VALUES ('app', '{\"userName\":\"Ana\"}')", []).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let conn = db.lock();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        let name: String = conn.query_row("SELECT name FROM songs WHERE id = 'id1'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Mi canción");
        let s: String = conn.query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0)).unwrap();
        assert!(s.contains("Ana"));
        let n: i64 = conn.query_row("SELECT count(*) FROM paired_devices", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn migrations_are_idempotent_on_reopen() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("x.db");
        drop(Database::open(&path).unwrap());
        let db = Database::open(&path).unwrap();
        assert_eq!(version(&db.lock()), MIGRATIONS.len() as i64);
    }

    #[test]
    fn paired_devices_constraints_reject_bad_rows() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.lock();
        let bad = conn.execute(
            "INSERT INTO paired_devices (device_id, name, os, fingerprint, paired_at) VALUES ('short', 'x', 'macos', ?1, 1)",
            [&"b".repeat(64)],
        );
        assert!(bad.is_err());
    }

    /// Base de la versión 0.1.2 (migración 2) con un dispositivo emparejado: tras migrar conserva
    /// su nombre exacto, queda con reconexión automática activa y sin alias.
    #[test]
    fn upgrading_from_migration_2_preserves_paired_device_names() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("v2.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            migrate_to(&mut conn, 2).unwrap();
            assert_eq!(version(&conn), 2);
            conn.execute(
                "INSERT INTO paired_devices (device_id, name, os, fingerprint, last_address, last_port, paired_at, last_seen_at)
                 VALUES (?1, 'Mate de Oficina', 'windows', ?2, '192.0.2.9', 38473, 10, 20)",
                [&"a".repeat(32), &"b".repeat(64)],
            )
            .unwrap();
            conn.execute("INSERT INTO device_identity (id, cert_der, created_at) VALUES (1, x'0102', 5)", []).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let conn = db.lock();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        let (name, alias, auto): (String, Option<String>, i64) = conn
            .query_row("SELECT name, alias, auto_connect FROM paired_devices", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((name.as_str(), alias, auto), ("Mate de Oficina", None, 1));
        let cert: Vec<u8> = conn.query_row("SELECT cert_der FROM device_identity", [], |r| r.get(0)).unwrap();
        assert_eq!(cert, vec![1, 2]);
        let n: i64 = conn.query_row("SELECT count(*) FROM identity_archive", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
