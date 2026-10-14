use serde::{Serialize, Serializer};

/// Error unificado del backend. Se serializa como un mensaje en español
/// listo para mostrar en la interfaz.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Error de base de datos: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Error de archivos: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotFound(String),
    #[error("Error de audio: {0}")]
    Audio(String),
    #[error("{0}")]
    Alert(String),
    #[error("{0}")]
    System(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
