//! Identidad criptográfica persistente del dispositivo.
//!
//! - Certificado X.509 autofirmado con clave ECDSA P-256 (generado con `rcgen`).
//! - El **device id** se deriva del fingerprint (SHA-256) del certificado:
//!   no se puede reclamar el id de otro dispositivo sin su clave privada.
//! - La clave privada vive en el llavero del sistema (macOS Keychain /
//!   Windows Credential Manager) y, si no está disponible, en un archivo
//!   con permisos restringidos. Nunca se guarda en SQLite ni se envía por red.

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine as _;
use rcgen::{date_time_ymd, CertificateParams, DnType, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::store::NetStore;
use crate::error::{AppError, AppResult};

pub type Fingerprint = [u8; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyStorage {
    Keychain,
    File,
}

/// Almacén de la clave privada (PKCS#8 DER).
pub trait SecretStore: Send + Sync {
    fn kind(&self) -> KeyStorage;
    fn get(&self) -> AppResult<Option<Vec<u8>>>;
    fn set(&self, secret: &[u8]) -> AppResult<()>;
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Llavero del sistema operativo.
pub struct KeyringStore {
    entry: keyring::Entry,
}

impl KeyringStore {
    pub fn new(service: &str) -> AppResult<Self> {
        let entry = keyring::Entry::new(service, "device-private-key").map_err(|e| AppError::System(format!("llavero: {e}")))?;
        Ok(Self { entry })
    }
}

impl SecretStore for KeyringStore {
    fn kind(&self) -> KeyStorage {
        KeyStorage::Keychain
    }

    fn get(&self) -> AppResult<Option<Vec<u8>>> {
        match self.entry.get_password() {
            Ok(s) => Ok(B64.decode(s.trim()).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::System(format!("llavero: {e}"))),
        }
    }

    fn set(&self, secret: &[u8]) -> AppResult<()> {
        self.entry.set_password(&B64.encode(secret)).map_err(|e| AppError::System(format!("llavero: {e}")))
    }
}

/// Respaldo cuando no hay llavero: archivo en el directorio de datos con
/// permisos 0600 (Unix) o heredando la ACL del perfil del usuario (Windows).
/// Es una protección más débil que el llavero y la interfaz lo informa.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SecretStore for FileStore {
    fn kind(&self) -> KeyStorage {
        KeyStorage::File
    }

    fn get(&self) -> AppResult<Option<Vec<u8>>> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(B64.decode(s.trim()).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, secret: &[u8]) -> AppResult<()> {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&self.path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(B64.encode(secret).as_bytes())?;
        Ok(())
    }
}

/// Almacén en memoria (pruebas).
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(pub std::sync::Mutex<Option<Vec<u8>>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn kind(&self) -> KeyStorage {
        KeyStorage::Keychain
    }
    fn get(&self) -> AppResult<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, secret: &[u8]) -> AppResult<()> {
        *self.0.lock().unwrap() = Some(secret.to_vec());
        Ok(())
    }
}

pub fn fingerprint_of(cert_der: &[u8]) -> Fingerprint {
    Sha256::digest(cert_der).into()
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Id público: primeros 16 bytes del fingerprint (32 hex).
pub fn device_id_from_fingerprint(fp: &Fingerprint) -> String {
    hex(&fp[..16])
}

/// Fingerprint completo en grupos de 8 hex para mostrarlo al usuario.
pub fn format_fingerprint(fp: &Fingerprint) -> String {
    hex(fp).as_bytes().chunks(8).map(|c| String::from_utf8_lossy(c).to_uppercase()).collect::<Vec<_>>().join(" ")
}

pub struct Identity {
    cert_der: CertificateDer<'static>,
    key_der: Vec<u8>,
    fingerprint: Fingerprint,
    storage: KeyStorage,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // La clave privada jamás se imprime.
        f.debug_struct("Identity").field("device_id", &self.device_id()).finish_non_exhaustive()
    }
}

impl Identity {
    fn generate(storage: KeyStorage) -> AppResult<(Self, Vec<u8>)> {
        let key = KeyPair::generate().map_err(|e| AppError::System(format!("no se pudo generar la clave: {e}")))?;
        let mut params = CertificateParams::new(vec!["damebola.local".to_string()])
            .map_err(|e| AppError::System(format!("certificado: {e}")))?;
        params.distinguished_name.push(DnType::CommonName, "DameBola device");
        params.not_before = date_time_ymd(2024, 1, 1);
        params.not_after = date_time_ymd(2124, 1, 1);
        let cert = params.self_signed(&key).map_err(|e| AppError::System(format!("certificado: {e}")))?;
        let cert_der = cert.der().clone();
        let key_der = key.serialize_der();
        let fingerprint = fingerprint_of(&cert_der);
        Ok((Self { cert_der, key_der: key_der.clone(), fingerprint, storage }, key_der))
    }

    pub fn cert(&self) -> CertificateDer<'static> {
        self.cert_der.clone()
    }

    pub fn private_key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }

    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    pub fn device_id(&self) -> String {
        device_id_from_fingerprint(&self.fingerprint)
    }

    pub fn storage(&self) -> KeyStorage {
        self.storage
    }
}

/// ¿La clave privada corresponde al certificado? (la clave pública cruda
/// aparece literalmente dentro del DER del certificado).
fn key_matches_cert(key_der: &[u8], cert_der: &[u8]) -> bool {
    let Ok(pair) = KeyPair::try_from(key_der) else { return false };
    let raw = pair.public_key_raw();
    !raw.is_empty() && cert_der.windows(raw.len()).any(|w| w == raw)
}

/// Carga la identidad persistida o crea una nueva. Si el certificado o la
/// clave faltan o no coinciden, se genera una identidad nueva (los
/// emparejamientos existentes dejarán de ser válidos en los otros equipos).
pub fn load_or_create(store: &NetStore, secrets: &dyn SecretStore) -> AppResult<Identity> {
    let cert = store.identity_cert()?;
    let key = secrets.get()?;
    if let (Some(cert), Some(key)) = (cert, key) {
        if key_matches_cert(&key, &cert) {
            let fingerprint = fingerprint_of(&cert);
            return Ok(Identity { cert_der: CertificateDer::from(cert), key_der: key, fingerprint, storage: secrets.kind() });
        }
    }
    let (identity, key_der) = Identity::generate(secrets.kind())?;
    secrets.set(&key_der)?;
    store.set_identity_cert(&identity.cert_der)?;
    Ok(identity)
}

/// Elige el almacén de claves: llavero salvo que `DAMEBOLA_KEY_STORAGE=file`
/// o que el llavero no esté disponible.
pub fn open_secret_store(app_id: &str, file_path: PathBuf) -> Arc<dyn SecretStore> {
    let forced_file = std::env::var("DAMEBOLA_KEY_STORAGE").is_ok_and(|v| v == "file");
    if !forced_file {
        if let Ok(k) = KeyringStore::new(app_id) {
            // Verifica que el llavero responda (lectura inocua).
            if k.get().is_ok() {
                return Arc::new(k);
            }
        }
    }
    Arc::new(FileStore::new(file_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::TempDir;

    fn store() -> NetStore {
        NetStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn device_id_is_derived_from_the_certificate_fingerprint() {
        let (a, _) = Identity::generate(KeyStorage::File).unwrap();
        let id = a.device_id();
        assert_eq!(id.len(), 32);
        assert_eq!(id, hex(&fingerprint_of(a.cert_der.as_ref())[..16]));
        let (b, _) = Identity::generate(KeyStorage::File).unwrap();
        assert_ne!(a.device_id(), b.device_id());
        assert!(!format!("{a:?}").contains("key_der"));
    }

    #[test]
    fn identity_persists_across_loads() {
        let (st, secrets) = (store(), MemoryStore::default());
        let first = load_or_create(&st, &secrets).unwrap();
        let second = load_or_create(&st, &secrets).unwrap();
        assert_eq!(first.device_id(), second.device_id());
        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn mismatched_or_missing_key_regenerates_identity() {
        let (st, secrets) = (store(), MemoryStore::default());
        let first = load_or_create(&st, &secrets).unwrap();
        // Clave de otro par: no coincide con el certificado guardado.
        let (_, other_key) = Identity::generate(KeyStorage::File).unwrap();
        secrets.set(&other_key).unwrap();
        let second = load_or_create(&st, &secrets).unwrap();
        assert_ne!(first.device_id(), second.device_id());
        // Clave ausente.
        *secrets.0.lock().unwrap() = None;
        let third = load_or_create(&st, &secrets).unwrap();
        assert_ne!(second.device_id(), third.device_id());
        // Y tras regenerar, es estable.
        assert_eq!(third.device_id(), load_or_create(&st, &secrets).unwrap().device_id());
    }

    #[test]
    fn key_matches_only_its_own_certificate() {
        let (a, ka) = Identity::generate(KeyStorage::File).unwrap();
        let (b, _) = Identity::generate(KeyStorage::File).unwrap();
        assert!(key_matches_cert(&ka, a.cert_der.as_ref()));
        assert!(!key_matches_cert(&ka, b.cert_der.as_ref()));
        assert!(!key_matches_cert(b"basura", a.cert_der.as_ref()));
    }

    #[test]
    fn private_key_is_not_in_the_database() {
        let (st, secrets) = (store(), MemoryStore::default());
        let id = load_or_create(&st, &secrets).unwrap();
        let blob = st.identity_cert().unwrap().unwrap();
        assert!(!blob.windows(id.key_der.len().min(32)).any(|w| w == &id.key_der[..id.key_der.len().min(32)]));
    }

    #[cfg(unix)]
    #[test]
    fn file_store_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let fs = FileStore::new(dir.path().join("k"));
        assert_eq!(fs.get().unwrap(), None);
        fs.set(b"secreto").unwrap();
        assert_eq!(fs.get().unwrap().unwrap(), b"secreto");
        assert_eq!(std::fs::metadata(dir.path().join("k")).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn fingerprint_formatting_groups_hex() {
        let fp = [0xABu8; 32];
        let s = format_fingerprint(&fp);
        assert_eq!(s.split(' ').count(), 8);
        assert!(s.starts_with("ABABABAB "));
    }
}
