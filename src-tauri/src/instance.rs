//! Aislamiento de instancias: carpeta de datos, bloqueo exclusivo y variables de desarrollo.
//!
//! Incidente que motivó esto: una instancia de desarrollo con `DAMEBOLA_KEY_STORAGE=file` compartía la
//! carpeta de datos de la app real y le regeneró la identidad. Ahora:
//! - Las variables de desarrollo (`DAMEBOLA_KEY_STORAGE`, `DAMEBOLA_ALLOW_LOOPBACK`) **solo se aplican**
//!   si además se indicó una carpeta de datos propia con `DAMEBOLA_DATA_DIR`. Sin ella se ignoran.
//! - Dos procesos no pueden usar la misma carpeta de datos a la vez (bloqueo exclusivo).

use std::fs::File;
use std::path::{Path, PathBuf};

pub const ENV_DATA_DIR: &str = "DAMEBOLA_DATA_DIR";
pub const ENV_KEY_STORAGE: &str = "DAMEBOLA_KEY_STORAGE";
pub const ENV_ALLOW_LOOPBACK: &str = "DAMEBOLA_ALLOW_LOOPBACK";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEnv {
    pub data_dir: PathBuf,
    /// Instancia aislada (carpeta propia indicada por el desarrollador).
    pub isolated: bool,
    pub force_file_keys: bool,
    pub allow_loopback: bool,
    /// Variables de desarrollo que se ignoraron por no haber carpeta aislada.
    pub ignored: Vec<&'static str>,
}

impl RuntimeEnv {
    /// `get` abstrae `std::env::var` para poder probarlo sin tocar el entorno real.
    pub fn resolve(default_dir: PathBuf, get: impl Fn(&str) -> Option<String>) -> Self {
        let custom = get(ENV_DATA_DIR).filter(|v| !v.trim().is_empty()).map(PathBuf::from);
        let isolated = custom.is_some();
        let key_file = get(ENV_KEY_STORAGE).is_some_and(|v| v == "file");
        let loopback = get(ENV_ALLOW_LOOPBACK).is_some_and(|v| v == "1");
        let mut ignored = vec![];
        if !isolated {
            if get(ENV_KEY_STORAGE).is_some() {
                ignored.push(ENV_KEY_STORAGE);
            }
            if get(ENV_ALLOW_LOOPBACK).is_some() {
                ignored.push(ENV_ALLOW_LOOPBACK);
            }
        }
        Self { data_dir: custom.unwrap_or(default_dir), isolated, force_file_keys: isolated && key_file, allow_loopback: isolated && loopback, ignored }
    }
}

/// Bloqueo exclusivo de la carpeta de datos mientras viva este valor.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Otra instancia de DameBola ya está usando esta carpeta de datos. Cerrala antes de abrir otra (o usá una carpeta distinta con DAMEBOLA_DATA_DIR para desarrollo).")]
    InUse,
    #[error("No se pudo crear el bloqueo de la carpeta de datos: {0}")]
    Io(#[from] std::io::Error),
}

impl InstanceLock {
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        std::fs::create_dir_all(dir)?;
        let file = File::options().create(true).truncate(false).write(true).open(dir.join("instance.lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(LockError::InUse),
            Err(std::fs::TryLockError::Error(e)) => Err(LockError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn dev_variables_are_ignored_without_an_isolated_data_dir() {
        let real = PathBuf::from("/datos/reales");
        let r = RuntimeEnv::resolve(real.clone(), env(&[(ENV_KEY_STORAGE, "file"), (ENV_ALLOW_LOOPBACK, "1")]));
        assert_eq!(r.data_dir, real);
        assert!(!r.isolated && !r.force_file_keys && !r.allow_loopback);
        assert_eq!(r.ignored, vec![ENV_KEY_STORAGE, ENV_ALLOW_LOOPBACK]);
    }

    #[test]
    fn dev_variables_apply_only_with_an_isolated_data_dir() {
        let r = RuntimeEnv::resolve(PathBuf::from("/real"), env(&[(ENV_DATA_DIR, "/tmp/aislada"), (ENV_KEY_STORAGE, "file"), (ENV_ALLOW_LOOPBACK, "1")]));
        assert_eq!(r.data_dir, PathBuf::from("/tmp/aislada"));
        assert!(r.isolated && r.force_file_keys && r.allow_loopback && r.ignored.is_empty());
        // Carpeta aislada sin variables: comportamiento normal (llavero).
        let r = RuntimeEnv::resolve(PathBuf::from("/real"), env(&[(ENV_DATA_DIR, "/tmp/aislada")]));
        assert!(r.isolated && !r.force_file_keys && !r.allow_loopback);
        // Valor vacío = no hay carpeta aislada.
        let r = RuntimeEnv::resolve(PathBuf::from("/real"), env(&[(ENV_DATA_DIR, "  "), (ENV_KEY_STORAGE, "file")]));
        assert!(!r.isolated && !r.force_file_keys);
    }

    #[test]
    fn a_second_instance_cannot_use_the_same_data_dir() {
        let dir = TempDir::new().unwrap();
        let first = InstanceLock::acquire(dir.path()).unwrap();
        assert!(matches!(InstanceLock::acquire(dir.path()), Err(LockError::InUse)));
        drop(first);
        assert!(InstanceLock::acquire(dir.path()).is_ok(), "al cerrar la primera se libera");
        // Otra carpeta no se ve afectada.
        let other = TempDir::new().unwrap();
        let _a = InstanceLock::acquire(dir.path()).unwrap();
        assert!(InstanceLock::acquire(other.path()).is_ok());
    }
}
