//! Biblioteca de canciones: ABM sobre SQLite + archivos administrados por la app.
//!
//! Invariantes de seguridad:
//! - El frontend solo maneja ids internos (UUID). Nunca rutas.
//! - Los archivos se guardan como `<uuid>.<ext>` dentro de `audio_dir`; el
//!   nombre de archivo se valida antes de construir cualquier ruta.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::db::{now_ms, Database};
use crate::error::{AppError, AppResult};
use crate::selection;

pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_NAME_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Wav,
    Ogg,
}

impl AudioFormat {
    pub fn from_extension(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "ogg" => Some(Self::Ogg),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
        }
    }

    /// Comprueba la firma del contenido (no solo la extensión).
    fn matches_magic(self, head: &[u8]) -> bool {
        match self {
            Self::Wav => head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE",
            Self::Ogg => head.len() >= 4 && &head[0..4] == b"OggS",
            Self::Mp3 => {
                (head.len() >= 3 && &head[0..3] == b"ID3")
                    || (head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub name: String,
    /// Nombre interno en disco. Nunca se expone al frontend.
    #[serde(skip)]
    pub filename: String,
    pub original_filename: String,
    pub format: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// El registro existe pero su archivo ya no está en disco.
    pub file_missing: bool,
}

impl Song {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            filename: row.get("filename")?,
            original_filename: row.get("original_filename")?,
            format: row.get("format")?,
            size_bytes: row.get("size_bytes")?,
            sha256: row.get("sha256")?,
            enabled: row.get::<_, i64>("enabled")? != 0,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            file_missing: false,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportIssue {
    pub file_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: Vec<Song>,
    pub duplicates: Vec<ImportIssue>,
    pub failed: Vec<ImportIssue>,
    /// El usuario cerró el selector sin elegir archivos.
    pub cancelled: bool,
}

enum ImportOutcome {
    Imported(Song),
    Duplicate(String),
}

pub struct SongService {
    db: Arc<Database>,
    audio_dir: PathBuf,
}

impl SongService {
    pub fn new(db: Arc<Database>, audio_dir: PathBuf) -> AppResult<Self> {
        fs::create_dir_all(&audio_dir)?;
        let svc = Self { db, audio_dir };
        svc.cleanup_partial_files();
        Ok(svc)
    }

    /// Borra restos `.part` de importaciones interrumpidas.
    fn cleanup_partial_files(&self) {
        if let Ok(entries) = fs::read_dir(&self.audio_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "part") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    /// Ruta absoluta del archivo de una canción, garantizando que queda
    /// dentro de `audio_dir`.
    fn file_path(&self, filename: &str) -> AppResult<PathBuf> {
        let mut comps = Path::new(filename).components();
        match (comps.next(), comps.next()) {
            (Some(Component::Normal(_)), None) => Ok(self.audio_dir.join(filename)),
            _ => Err(AppError::Validation("Nombre de archivo interno inválido.".into())),
        }
    }

    pub fn list(&self) -> AppResult<Vec<Song>> {
        let mut songs = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT * FROM songs ORDER BY created_at DESC, name COLLATE NOCASE")?;
            let rows = stmt.query_map([], Song::from_row)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for s in &mut songs {
            s.file_missing = !self.file_path(&s.filename).map(|p| p.is_file()).unwrap_or(false);
        }
        Ok(songs)
    }

    pub fn get(&self, id: &str) -> AppResult<Song> {
        let mut song = self
            .db
            .lock()
            .query_row("SELECT * FROM songs WHERE id = ?1", params![id], Song::from_row)
            .optional()?
            .ok_or_else(|| AppError::NotFound("La canción no existe.".into()))?;
        song.file_missing = !self.file_path(&song.filename)?.is_file();
        Ok(song)
    }

    /// Ruta del archivo de audio de una canción existente y presente en disco.
    pub fn audio_path(&self, id: &str) -> AppResult<PathBuf> {
        let song = self.get(id)?;
        if song.file_missing {
            return Err(AppError::NotFound(format!(
                "El archivo de «{}» ya no existe en disco.",
                song.name
            )));
        }
        self.file_path(&song.filename)
    }

    // ---------------------------------------------------------------- ALTA

    pub fn import_paths(&self, paths: &[PathBuf]) -> ImportReport {
        let mut report = ImportReport::default();
        for path in paths {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            match self.import_one(path, &file_name) {
                Ok(ImportOutcome::Imported(song)) => report.imported.push(song),
                Ok(ImportOutcome::Duplicate(existing)) => report.duplicates.push(ImportIssue {
                    file_name,
                    reason: format!("Ya existe en la biblioteca como «{existing}»."),
                }),
                Err(reason) => report.failed.push(ImportIssue { file_name, reason }),
            }
        }
        report
    }

    fn import_one(&self, src: &Path, original_name: &str) -> Result<ImportOutcome, String> {
        let format = AudioFormat::from_extension(src)
            .ok_or("Formato no soportado: solo se aceptan MP3, WAV y OGG.")?;
        let meta = fs::metadata(src).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
        if !meta.is_file() {
            return Err("No es un archivo.".into());
        }
        if meta.len() == 0 {
            return Err("El archivo está vacío.".into());
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(format!("El archivo supera el máximo de {} MB.", MAX_FILE_BYTES / 1024 / 1024));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let tmp = self.audio_dir.join(format!("{id}.part"));
        let result = self.import_via_tmp(src, &tmp, &id, format, original_name);
        // Si el archivo temporal sigue ahí (error o duplicado), se elimina.
        let _ = fs::remove_file(&tmp);
        result
    }

    fn import_via_tmp(
        &self,
        src: &Path,
        tmp: &Path,
        id: &str,
        format: AudioFormat,
        original_name: &str,
    ) -> Result<ImportOutcome, String> {
        // 1. Copia + hash en una sola pasada. Todo lo siguiente opera sobre
        //    la copia propia, no sobre el original.
        let (sha256, size) = copy_with_hash(src, tmp).map_err(|e| format!("No se pudo copiar el archivo: {e}"))?;

        // 2. Contenido real.
        validate_audio_content(tmp, format)?;

        // 3. Duplicados por contenido.
        let existing: Option<String> = self
            .db
            .lock()
            .query_row("SELECT name FROM songs WHERE sha256 = ?1", params![sha256], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(name) = existing {
            return Ok(ImportOutcome::Duplicate(name));
        }

        // 4. Publicar archivo y registro.
        let filename = format!("{id}.{}", format.as_str());
        let final_path = self.file_path(&filename).map_err(|e| e.to_string())?;
        fs::rename(tmp, &final_path).map_err(|e| format!("No se pudo guardar el archivo: {e}"))?;

        let now = now_ms();
        let name = default_name(original_name);
        let insert = self.db.lock().execute(
            "INSERT INTO songs (id, name, filename, original_filename, format, size_bytes, sha256, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
            params![id, name, filename, original_name, format.as_str(), size as i64, sha256, now],
        );
        if let Err(e) = insert {
            let _ = fs::remove_file(&final_path);
            return Err(format!("No se pudo registrar la canción: {e}"));
        }

        Ok(ImportOutcome::Imported(Song {
            id: id.to_string(),
            name,
            filename,
            original_filename: original_name.to_string(),
            format: format.as_str().to_string(),
            size_bytes: size as i64,
            sha256,
            enabled: true,
            created_at: now,
            updated_at: now,
            file_missing: false,
        }))
    }

    // -------------------------------------------------------- MODIFICACIÓN

    pub fn rename(&self, id: &str, name: &str) -> AppResult<Song> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Validation("El nombre no puede estar vacío.".into()));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(AppError::Validation(format!(
                "El nombre no puede superar {MAX_NAME_CHARS} caracteres."
            )));
        }
        if name.chars().any(char::is_control) {
            return Err(AppError::Validation("El nombre contiene caracteres no válidos.".into()));
        }
        self.update(id, "name = ?2", params![id, name])
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<Song> {
        self.update(id, "enabled = ?2", params![id, enabled as i64])
    }

    fn update(&self, id: &str, set_clause: &str, args: &[&dyn rusqlite::ToSql]) -> AppResult<Song> {
        let sql = format!("UPDATE songs SET {set_clause}, updated_at = ?3 WHERE id = ?1");
        let now = now_ms();
        let mut all: Vec<&dyn rusqlite::ToSql> = args.to_vec();
        all.push(&now);
        let changed = self.db.lock().execute(&sql, all.as_slice())?;
        if changed == 0 {
            return Err(AppError::NotFound("La canción no existe.".into()));
        }
        self.get(id)
    }

    // ---------------------------------------------------------------- BAJA

    /// Elimina archivo y registro. Primero el archivo: si falla la BD el
    /// registro queda visible como «archivo faltante» y puede reintentarse.
    pub fn delete(&self, id: &str) -> AppResult<()> {
        let song = self.get(id)?;
        let path = self.file_path(&song.filename)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.db.lock().execute("DELETE FROM songs WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ------------------------------------------------------- ALEATORIEDAD

    /// Canciones activas cuyo archivo existe en disco.
    pub fn playable(&self) -> AppResult<Vec<Song>> {
        Ok(self.list()?.into_iter().filter(|s| s.enabled && !s.file_missing).collect())
    }

    pub fn pick_random(&self, last_id: Option<&str>) -> AppResult<Option<Song>> {
        let candidates = self.playable()?;
        Ok(selection::pick(&candidates, |s| s.id.as_str(), last_id, |n| fastrand::usize(0..n)).cloned())
    }
}

fn default_name(original: &str) -> String {
    let stem = Path::new(original)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_default();
    let stem: String = stem.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
    if stem.trim().is_empty() {
        "Sin nombre".into()
    } else {
        stem
    }
}

fn copy_with_hash(src: &Path, dst: &Path) -> std::io::Result<(String, u64)> {
    let mut input = File::open(src)?;
    let mut output = File::create(dst)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n])?;
        total += n as u64;
    }
    output.sync_all()?;
    Ok((hex(&hasher.finalize()), total))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Firma del archivo + decodificación real de al menos una muestra.
fn validate_audio_content(path: &Path, format: AudioFormat) -> Result<(), String> {
    let mut head = [0u8; 12];
    let n = File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    if !format.matches_magic(&head[..n]) {
        return Err(format!(
            "El contenido no corresponde a un archivo {} válido.",
            format.as_str().to_uppercase()
        ));
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = rodio::Decoder::new(BufReader::new(file))
        .map_err(|_| "No se pudo decodificar el audio: el archivo está dañado o no es compatible.".to_string())?;
    if decoder.next().is_none() {
        return Err("El archivo no contiene audio reproducible.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct Fixture {
        svc: SongService,
        audio_dir: PathBuf,
        src: TempDir,
        _data: TempDir,
    }

    fn fixture() -> Fixture {
        let data = TempDir::new().unwrap();
        let audio_dir = data.path().join("audio");
        let svc = SongService::new(Arc::new(Database::open_in_memory().unwrap()), audio_dir.clone()).unwrap();
        Fixture { svc, audio_dir, src: TempDir::new().unwrap(), _data: data }
    }

    /// WAV PCM 16-bit mono válido; `seed` cambia el contenido (y el hash).
    fn wav_bytes(seed: i16) -> Vec<u8> {
        let samples: Vec<i16> = (0..800).map(|i| ((i % 50) as i16 * 100).wrapping_add(seed)).collect();
        let data_len = (samples.len() * 2) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&8000u32.to_le_bytes());
        v.extend_from_slice(&16000u32.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    fn write_src(f: &Fixture, name: &str, bytes: &[u8]) -> PathBuf {
        let p = f.src.path().join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    fn import_wav(f: &Fixture, name: &str, seed: i16) -> Song {
        let p = write_src(f, name, &wav_bytes(seed));
        let mut report = f.svc.import_paths(&[p]);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        report.imported.remove(0)
    }

    fn audio_files(f: &Fixture) -> Vec<PathBuf> {
        fs::read_dir(&f.audio_dir).unwrap().map(|e| e.unwrap().path()).collect()
    }

    #[test]
    fn import_copies_file_and_registers_it() {
        let f = fixture();
        let song = import_wav(&f, "Mi Canción.WAV", 0);
        assert_eq!(song.name, "Mi Canción");
        assert_eq!(song.format, "wav");
        assert!(song.enabled);
        assert_eq!(song.sha256.len(), 64);
        let files = audio_files(&f);
        assert_eq!(files.len(), 1);
        assert_eq!(fs::read(&files[0]).unwrap(), wav_bytes(0));
        assert_eq!(f.svc.list().unwrap().len(), 1);
    }

    #[test]
    fn import_does_not_depend_on_original_location() {
        let f = fixture();
        let p = write_src(&f, "a.wav", &wav_bytes(1));
        let song = f.svc.import_paths(&[p.clone()]).imported.remove(0);
        fs::remove_file(p).unwrap();
        assert!(f.svc.audio_path(&song.id).unwrap().is_file());
    }

    #[test]
    fn duplicate_content_is_rejected_even_with_other_name() {
        let f = fixture();
        import_wav(&f, "a.wav", 7);
        let p = write_src(&f, "otro-nombre.wav", &wav_bytes(7));
        let report = f.svc.import_paths(&[p]);
        assert!(report.imported.is_empty());
        assert_eq!(report.duplicates.len(), 1);
        assert_eq!(f.svc.list().unwrap().len(), 1);
        assert_eq!(audio_files(&f).len(), 1, "no deben quedar copias ni .part");
    }

    #[test]
    fn rejects_unsupported_extension_empty_and_fake_content() {
        let f = fixture();
        let txt = write_src(&f, "a.txt", b"hola");
        let empty = write_src(&f, "b.mp3", b"");
        let fake_mp3 = write_src(&f, "c.mp3", b"esto no es un mp3, solo texto");
        let wav_as_ogg = write_src(&f, "d.ogg", &wav_bytes(3));
        let missing = f.src.path().join("no-existe.wav");
        let report = f.svc.import_paths(&[txt, empty, fake_mp3, wav_as_ogg, missing]);
        assert!(report.imported.is_empty());
        assert_eq!(report.failed.len(), 5, "{:?}", report.failed);
        assert!(f.svc.list().unwrap().is_empty());
        assert!(audio_files(&f).is_empty(), "no deben quedar archivos huérfanos");
    }

    #[test]
    fn rejects_truncated_wav_header_only() {
        let f = fixture();
        let p = write_src(&f, "x.wav", &wav_bytes(0)[..44]);
        let report = f.svc.import_paths(&[p]);
        assert_eq!(report.failed.len(), 1);
        assert!(audio_files(&f).is_empty());
    }

    #[test]
    fn mixed_batch_reports_each_file() {
        let f = fixture();
        let ok = write_src(&f, "ok.wav", &wav_bytes(11));
        let bad = write_src(&f, "bad.wav", b"nope");
        let report = f.svc.import_paths(&[ok, bad]);
        assert_eq!(report.imported.len(), 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].file_name, "bad.wav");
    }

    #[test]
    fn rename_and_toggle_persist() {
        let f = fixture();
        let song = import_wav(&f, "a.wav", 0);
        let renamed = f.svc.rename(&song.id, "  Pollito  ").unwrap();
        assert_eq!(renamed.name, "Pollito");
        let off = f.svc.set_enabled(&song.id, false).unwrap();
        assert!(!off.enabled);
        let back = f.svc.get(&song.id).unwrap();
        assert_eq!(back.name, "Pollito");
        assert!(!back.enabled);
        assert!(back.updated_at >= song.updated_at);
    }

    #[test]
    fn rename_validates_input_and_unknown_id() {
        let f = fixture();
        let song = import_wav(&f, "a.wav", 0);
        assert!(f.svc.rename(&song.id, "   ").is_err());
        assert!(f.svc.rename(&song.id, &"x".repeat(121)).is_err());
        assert!(f.svc.rename(&song.id, "a\nb").is_err());
        assert!(matches!(f.svc.rename("nope", "x"), Err(AppError::NotFound(_))));
        assert!(matches!(f.svc.set_enabled("nope", true), Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_removes_row_and_file_and_excludes_from_random() {
        let f = fixture();
        let a = import_wav(&f, "a.wav", 1);
        let b = import_wav(&f, "b.wav", 2);
        f.svc.delete(&a.id).unwrap();
        assert!(matches!(f.svc.get(&a.id), Err(AppError::NotFound(_))));
        assert_eq!(audio_files(&f).len(), 1);
        for _ in 0..20 {
            assert_eq!(f.svc.pick_random(None).unwrap().unwrap().id, b.id);
        }
    }

    #[test]
    fn delete_rejects_ids_that_are_not_songs() {
        let f = fixture();
        let victim = f.src.path().join("importante.txt");
        fs::write(&victim, "no me borres").unwrap();
        for bad in ["../importante.txt", victim.to_str().unwrap(), "", "nope"] {
            assert!(f.svc.delete(bad).is_err());
        }
        assert!(victim.exists());
    }

    #[test]
    fn delete_tolerates_already_missing_file() {
        let f = fixture();
        let a = import_wav(&f, "a.wav", 1);
        fs::remove_file(f.svc.audio_path(&a.id).unwrap()).unwrap();
        assert!(f.svc.get(&a.id).unwrap().file_missing);
        f.svc.delete(&a.id).unwrap();
        assert!(f.svc.list().unwrap().is_empty());
    }

    #[test]
    fn random_pick_only_uses_enabled_existing_songs() {
        let f = fixture();
        assert!(f.svc.pick_random(None).unwrap().is_none());
        let a = import_wav(&f, "a.wav", 1);
        let b = import_wav(&f, "b.wav", 2);
        let c = import_wav(&f, "c.wav", 3);
        f.svc.set_enabled(&b.id, false).unwrap();
        fs::remove_file(f.svc.audio_path(&c.id).unwrap()).unwrap();
        for _ in 0..20 {
            assert_eq!(f.svc.pick_random(None).unwrap().unwrap().id, a.id);
        }
    }

    #[test]
    fn random_pick_avoids_immediate_repeat() {
        let f = fixture();
        let a = import_wav(&f, "a.wav", 1);
        let b = import_wav(&f, "b.wav", 2);
        for _ in 0..20 {
            assert_eq!(f.svc.pick_random(Some(&a.id)).unwrap().unwrap().id, b.id);
            assert_eq!(f.svc.pick_random(Some(&b.id)).unwrap().unwrap().id, a.id);
        }
    }

    #[test]
    fn single_active_song_may_repeat() {
        let f = fixture();
        let a = import_wav(&f, "a.wav", 1);
        assert_eq!(f.svc.pick_random(Some(&a.id)).unwrap().unwrap().id, a.id);
    }

    #[test]
    fn data_survives_reopening_the_database_file() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("t.db");
        let audio = dir.path().join("audio");
        let src = dir.path().join("a.wav");
        fs::write(&src, wav_bytes(5)).unwrap();
        let id = {
            let svc = SongService::new(Arc::new(Database::open(&db_path).unwrap()), audio.clone()).unwrap();
            let s = svc.import_paths(&[src]).imported.remove(0);
            svc.rename(&s.id, "Persistente").unwrap();
            svc.set_enabled(&s.id, false).unwrap();
            s.id
        };
        let svc = SongService::new(Arc::new(Database::open(&db_path).unwrap()), audio).unwrap();
        let s = svc.get(&id).unwrap();
        assert_eq!(s.name, "Persistente");
        assert!(!s.enabled);
    }
}
