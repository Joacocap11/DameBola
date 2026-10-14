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
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
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
