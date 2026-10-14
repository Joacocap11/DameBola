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

/// Ranura histórica de la clave (versiones 0.1.x). Nunca se sobrescribe.
pub const LEGACY_SLOT: &str = "device-private-key";

/// Las identidades nuevas guardan su clave en una ranura propia derivada del fingerprint: así crear
/// o recuperar una identidad **nunca** pisa la clave de otra (la anterior sigue disponible).
fn slot_for(fp: &Fingerprint) -> String {
    format!("key-{}", hex(&fp[..8]))
}

/// Almacén de claves privadas (PKCS#8 DER), por ranura.
pub trait SecretStore: Send + Sync {
    fn kind(&self) -> KeyStorage;
    fn get(&self, slot: &str) -> AppResult<Option<Vec<u8>>>;
    fn set(&self, slot: &str, secret: &[u8]) -> AppResult<()>;
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Llavero del sistema operativo.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self { service: service.to_string() }
    }

    fn entry(&self, slot: &str) -> AppResult<keyring::Entry> {
        keyring::Entry::new(&self.service, slot).map_err(|e| AppError::System(format!("llavero: {e}")))
    }
}

impl SecretStore for KeyringStore {
    fn kind(&self) -> KeyStorage {
        KeyStorage::Keychain
    }

    fn get(&self, slot: &str) -> AppResult<Option<Vec<u8>>> {
        match self.entry(slot)?.get_password() {
            Ok(s) => Ok(B64.decode(s.trim()).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::System(format!("llavero: {e}"))),
        }
    }

    fn set(&self, slot: &str, secret: &[u8]) -> AppResult<()> {
        self.entry(slot)?.set_password(&B64.encode(secret)).map_err(|e| AppError::System(format!("llavero: {e}")))
    }
}

/// Respaldo en archivo dentro del directorio de datos, con permisos 0600 (Unix) o la ACL del perfil
/// (Windows). Protección más débil que el llavero; la interfaz lo informa.
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, slot: &str) -> PathBuf {
        if slot == LEGACY_SLOT {
            self.dir.join("device.key")
        } else {
            self.dir.join(format!("device-{slot}.key"))
        }
    }
}

impl SecretStore for FileStore {
    fn kind(&self) -> KeyStorage {
        KeyStorage::File
    }

    fn get(&self, slot: &str) -> AppResult<Option<Vec<u8>>> {
        match std::fs::read_to_string(self.path(slot)) {
            Ok(s) => Ok(B64.decode(s.trim()).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, slot: &str, secret: &[u8]) -> AppResult<()> {
        use std::io::Write;
        let path = self.path(slot);
        let mut opts = std::fs::OpenOptions::new();
        // `create_new`: jamás se pisa un archivo de clave existente.
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&path)?;
        f.write_all(B64.encode(secret).as_bytes())?;
        Ok(())
    }
}

/// Conjunto de almacenes: se escribe en `primary` y se **lee** (sin copiar) de todos.
pub struct SecretBackends {
    pub primary: Arc<dyn SecretStore>,
    pub alternates: Vec<Arc<dyn SecretStore>>,
}

impl SecretBackends {
    fn all(&self) -> impl Iterator<Item = &Arc<dyn SecretStore>> {
        std::iter::once(&self.primary).chain(self.alternates.iter())
    }
}

/// Problemas de identidad que NO se resuelven regenerando en silencio.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("No se encontró la clave privada que corresponde a la identidad de este equipo. Se buscó en el llavero y en el archivo de claves.")]
    KeyMissing,
    #[error("Hay una clave privada, pero no corresponde al certificado de identidad guardado.")]
    KeyMismatch,
    #[error("No se pudo acceder al almacén de claves ({0}). Puede ser temporal (llavero bloqueado o sin permiso).")]
    StoreUnavailable(String),
    #[error("Hay dispositivos emparejados (o una identidad anterior) pero falta el certificado de identidad de este equipo.")]
    CertMissing,
    #[error("El certificado de identidad guardado está dañado.")]
    CertCorrupt,
    #[error("Error interno de identidad: {0}")]
    Internal(String),
}

impl IdentityError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::KeyMissing => "key_missing",
            Self::KeyMismatch => "key_mismatch",
            Self::StoreUnavailable(_) => "store_unavailable",
            Self::CertMissing => "cert_missing",
            Self::CertCorrupt => "cert_corrupt",
            Self::Internal(_) => "internal",
        }
    }
}

impl From<AppError> for IdentityError {
    fn from(e: AppError) -> Self {
        IdentityError::Internal(e.to_string())
    }
}

/// Almacén en memoria (pruebas).
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    pub slots: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    /// Simula un llavero bloqueado/inaccesible.
    pub unavailable: std::sync::atomic::AtomicBool,
    pub file_like: bool,
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn kind(&self) -> KeyStorage {
        if self.file_like { KeyStorage::File } else { KeyStorage::Keychain }
    }
    fn get(&self, slot: &str) -> AppResult<Option<Vec<u8>>> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::System("llavero bloqueado".into()));
        }
        Ok(self.slots.lock().unwrap().get(slot).cloned())
    }
    fn set(&self, slot: &str, secret: &[u8]) -> AppResult<()> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::System("llavero bloqueado".into()));
        }
        let mut m = self.slots.lock().unwrap();
        if m.contains_key(slot) {
            return Err(AppError::System("la ranura ya existe: no se sobrescribe".into()));
        }
        m.insert(slot.to_string(), secret.to_vec());
        Ok(())
    }
}

#[cfg(test)]
pub fn memory_backends() -> SecretBackends {
    SecretBackends { primary: Arc::new(MemoryStore::default()), alternates: vec![] }
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
    slot: String,
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
        let slot = slot_for(&fingerprint);
        Ok((Self { cert_der, key_der: key_der.clone(), fingerprint, storage, slot }, key_der))
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

/// Carga la identidad existente **sin modificar nada**. Busca la clave (primero la ranura propia del
/// fingerprint y luego la histórica) en todos los almacenes, de solo lectura y sin copiar entre ellos.
/// Si no aparece una clave que corresponda al certificado, devuelve un error: NO regenera.
fn load_existing(cert: &[u8], backends: &SecretBackends) -> Result<Identity, IdentityError> {
    if cert.len() < 64 || cert.len() > 4096 || cert[0] != 0x30 {
        return Err(IdentityError::CertCorrupt);
    }
    let fingerprint = fingerprint_of(cert);
    let slots = [slot_for(&fingerprint), LEGACY_SLOT.to_string()];
    let (mut mismatch, mut unavailable) = (false, None);
    for store in backends.all() {
        for slot in &slots {
            match store.get(slot) {
                Ok(Some(key)) if key_matches_cert(&key, cert) => {
                    let id = Identity { cert_der: CertificateDer::from(cert.to_vec()), key_der: key, fingerprint, storage: store.kind(), slot: slot.clone() };
                    // Comprobación final: rustls exige que clave y certificado formen un par válido.
                    super::tls::server_config(&id).map_err(|_| IdentityError::KeyMismatch)?;
                    return Ok(id);
                }
                Ok(Some(_)) => mismatch = true,
                Ok(None) => {}
                Err(e) => unavailable = Some(e.to_string()),
            }
        }
    }
    // Si algún almacén no respondió, no se puede afirmar que la clave no exista.
    Err(match (unavailable, mismatch) {
        (Some(u), _) => IdentityError::StoreUnavailable(u),
        (None, true) => IdentityError::KeyMismatch,
        (None, false) => IdentityError::KeyMissing,
    })
}

/// Escribe la clave en el primer almacén que la acepte (primario y, solo para identidades nuevas, respaldos).
fn store_new_key(identity: &mut Identity, key_der: &[u8], backends: &SecretBackends) -> Result<(), IdentityError> {
    let mut last = String::new();
    for store in backends.all() {
        match store.set(&identity.slot, key_der) {
            Ok(()) => {
                identity.storage = store.kind();
                return Ok(());
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(IdentityError::StoreUnavailable(last))
}

/// Arranque normal. Con identidad previa jamás crea otra; sin ninguna evidencia previa (instalación
/// nueva) crea la primera.
pub fn load_or_create(store: &NetStore, backends: &SecretBackends) -> Result<Identity, IdentityError> {
    match store.identity_cert()? {
        Some(cert) => load_existing(&cert, backends),
        None => {
            // Sin certificado pero con emparejamientos o una identidad archivada hubo una identidad:
            // crear otra la invalidaría ante los demás equipos. Se pide una decisión explícita.
            if store.has_paired_devices()? || store.archived_identities()? > 0 {
                return Err(IdentityError::CertMissing);
            }
            let (mut identity, key_der) = Identity::generate(backends.primary.kind())?;
            store_new_key(&mut identity, &key_der, backends)?;
            store.create_identity_cert(&identity.cert_der)?;
            Ok(identity)
        }
    }
}

/// Recuperación **explícita** (confirmada por el usuario): crea una identidad nueva. El certificado
/// anterior se archiva (nunca se borra) y la clave nueva usa una ranura propia: no se toca ninguna
/// clave existente. Los demás equipos deberán emparejarse de nuevo.
pub fn reset_identity(store: &NetStore, backends: &SecretBackends, reason: &str) -> Result<Identity, IdentityError> {
    let old = store.identity_cert()?;
    let (mut identity, key_der) = Identity::generate(backends.primary.kind())?;
    store_new_key(&mut identity, &key_der, backends)?;
    match old {
        Some(old_cert) => {
            let old_fp = fingerprint_of(&old_cert);
            store.replace_identity_cert_archiving(&old_cert, &device_id_from_fingerprint(&old_fp), &hex(&old_fp), &identity.cert_der, reason)?;
        }
        None => store.create_identity_cert(&identity.cert_der)?,
    }
    Ok(identity)
}

/// Elige los almacenes. El llavero es el principal salvo que `force_file` (solo en instancias de
/// desarrollo aisladas, ver `lib.rs`); ambos se consultan siempre al cargar.
pub fn open_secret_backends(app_id: &str, data_dir: &std::path::Path, force_file: bool) -> SecretBackends {
    let keyring: Arc<dyn SecretStore> = Arc::new(KeyringStore::new(app_id));
    let file: Arc<dyn SecretStore> = Arc::new(FileStore::new(data_dir.to_path_buf()));
    if force_file {
        SecretBackends { primary: file, alternates: vec![keyring] }
    } else {
        SecretBackends { primary: keyring, alternates: vec![file] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    fn store() -> NetStore {
        NetStore::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    fn mem(file_like: bool) -> Arc<MemoryStore> {
        Arc::new(MemoryStore { file_like, ..Default::default() })
    }

    fn backends(primary: &Arc<MemoryStore>, alternate: &Arc<MemoryStore>) -> SecretBackends {
        SecretBackends { primary: primary.clone(), alternates: vec![alternate.clone()] }
    }

    fn paired(store: &NetStore) {
        store
            .upsert_paired(&crate::net::store::PairedDevice {
                device_id: "a".repeat(32),
                name: "Mate".into(),
                alias: None,
                auto_connect: true,
                os: "windows".into(),
                fingerprint: "b".repeat(64),
                last_address: None,
                last_port: None,
                paired_at: 1,
                last_seen_at: None,
            })
            .unwrap();
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
    fn fresh_install_creates_one_identity_and_it_persists_across_restarts() {
        let (st, k, f) = (store(), mem(false), mem(true));
        let first = load_or_create(&st, &backends(&k, &f)).unwrap();
        // «Reiniciar»: mismos datos, nuevas estructuras.
        let second = load_or_create(&st, &backends(&k, &f)).unwrap();
        assert_eq!((first.device_id(), *first.fingerprint()), (second.device_id(), *second.fingerprint()));
        assert_eq!(k.slots.lock().unwrap().len(), 1, "una sola clave");
        assert!(f.slots.lock().unwrap().is_empty(), "no se escribe en el respaldo");
    }

    #[test]
    fn existing_identity_is_found_in_the_other_backend_without_regenerating_or_copying() {
        // Identidad creada con llavero; luego la app arranca «en modo archivo» (o al revés).
        let (st, k, f) = (store(), mem(false), mem(true));
        let original = load_or_create(&st, &backends(&k, &f)).unwrap();
        let swapped = SecretBackends { primary: f.clone(), alternates: vec![k.clone()] };
        let again = load_or_create(&st, &swapped).unwrap();
        assert_eq!(original.device_id(), again.device_id());
        assert_eq!(again.storage(), KeyStorage::Keychain, "se usa la clave donde está");
        assert!(f.slots.lock().unwrap().is_empty(), "NO se copia la clave al otro almacén");
        assert_eq!(st.archived_identities().unwrap(), 0);
    }

    #[test]
    fn legacy_slot_keys_from_previous_versions_are_still_found() {
        let (st, k, f) = (store(), mem(false), mem(true));
        // Instalación 0.1.x: certificado en SQLite y clave en la ranura histórica.
        let (id, key) = Identity::generate(KeyStorage::Keychain).unwrap();
        st.create_identity_cert(id.cert_der.as_ref()).unwrap();
        k.slots.lock().unwrap().insert(LEGACY_SLOT.into(), key);
        let loaded = load_or_create(&st, &backends(&k, &f)).unwrap();
        assert_eq!(loaded.device_id(), id.device_id());
        assert_eq!(k.slots.lock().unwrap().len(), 1, "no se añadió ninguna clave");
    }

    #[test]
    fn missing_key_with_existing_certificate_never_regenerates() {
        let (st, k, f) = (store(), mem(false), mem(true));
        let original = load_or_create(&st, &backends(&k, &f)).unwrap();
        let cert_before = st.identity_cert().unwrap().unwrap();
        k.slots.lock().unwrap().clear();
        let err = load_or_create(&st, &backends(&k, &f)).unwrap_err();
        assert_eq!(err, IdentityError::KeyMissing);
        assert_eq!(st.identity_cert().unwrap().unwrap(), cert_before, "el certificado no se tocó");
        assert!(k.slots.lock().unwrap().is_empty() && f.slots.lock().unwrap().is_empty(), "no se escribió ninguna clave");
        assert_eq!(st.archived_identities().unwrap(), 0);
        let _ = original;
    }

    #[test]
    fn incompatible_key_is_reported_and_nothing_is_overwritten() {
        let (st, k, f) = (store(), mem(false), mem(true));
        let orig = load_or_create(&st, &backends(&k, &f)).unwrap();
        let slot = slot_for(orig.fingerprint());
        let (_, other_key) = Identity::generate(KeyStorage::File).unwrap();
        let good = k.slots.lock().unwrap().insert(slot.clone(), other_key.clone()).unwrap();
        let err = load_or_create(&st, &backends(&k, &f)).unwrap_err();
        assert_eq!(err, IdentityError::KeyMismatch);
        assert_eq!(k.slots.lock().unwrap().get(&slot).unwrap(), &other_key, "no se sobrescribió la clave");
        // Restaurar la clave correcta repara la situación sin más.
        k.slots.lock().unwrap().insert(slot, good);
        assert!(load_or_create(&st, &backends(&k, &f)).is_ok());
    }

    #[test]
    fn temporarily_unavailable_keychain_is_not_mistaken_for_a_missing_key() {
        let (st, k, f) = (store(), mem(false), mem(true));
        let orig = load_or_create(&st, &backends(&k, &f)).unwrap();
        k.unavailable.store(true, Ordering::SeqCst);
        let err = load_or_create(&st, &backends(&k, &f)).unwrap_err();
        assert!(matches!(err, IdentityError::StoreUnavailable(_)), "{err:?}");
        assert_eq!(st.archived_identities().unwrap(), 0);
        // Al volver el llavero, la MISMA identidad.
        k.unavailable.store(false, Ordering::SeqCst);
        assert_eq!(load_or_create(&st, &backends(&k, &f)).unwrap().device_id(), orig.device_id());
    }

    #[test]
    fn paired_devices_without_a_certificate_block_automatic_creation() {
        let (st, k, f) = (store(), mem(false), mem(true));
        paired(&st);
        let err = load_or_create(&st, &backends(&k, &f)).unwrap_err();
        assert_eq!(err, IdentityError::CertMissing);
        assert!(st.identity_cert().unwrap().is_none() && k.slots.lock().unwrap().is_empty());
        assert!(st.has_paired_devices().unwrap(), "los emparejamientos se conservan");
    }

    #[test]
    fn corrupt_certificate_is_reported() {
        let (st, k, f) = (store(), mem(false), mem(true));
        st.create_identity_cert(&[1, 2, 3]).unwrap();
        assert_eq!(load_or_create(&st, &backends(&k, &f)).unwrap_err(), IdentityError::CertCorrupt);
    }

    #[test]
    fn explicit_reset_archives_the_old_certificate_and_never_touches_existing_keys() {
        let (st, k, f) = (store(), mem(false), mem(true));
        let orig = load_or_create(&st, &backends(&k, &f)).unwrap();
        paired(&st);
        let old_slot_key = k.slots.lock().unwrap().get(&slot_for(orig.fingerprint())).cloned().unwrap();
        // Estado roto: la clave del certificado se perdió.
        k.slots.lock().unwrap().clear();
        assert!(load_or_create(&st, &backends(&k, &f)).is_err());
        let fresh = reset_identity(&st, &backends(&k, &f), "key_missing").unwrap();
        assert_ne!(fresh.device_id(), orig.device_id());
        assert_eq!(st.archived_identities().unwrap(), 1);
        assert!(st.has_paired_devices().unwrap(), "los emparejados (pines de los demás) se conservan");
        // Y desde entonces arranca normal con la identidad nueva.
        assert_eq!(load_or_create(&st, &backends(&k, &f)).unwrap().device_id(), fresh.device_id());
        // La clave vieja no estaba en el llavero (la perdimos a propósito): no reaparece ni se pisó nada.
        assert_ne!(k.slots.lock().unwrap().values().next().unwrap(), &old_slot_key);
    }

    #[test]
    fn reset_uses_a_new_slot_so_an_existing_legacy_key_survives() {
        let (st, k, f) = (store(), mem(false), mem(true));
        // Certificado "equivocado" en la base + clave legítima antigua en la ranura histórica.
        let (real, real_key) = Identity::generate(KeyStorage::Keychain).unwrap();
        k.slots.lock().unwrap().insert(LEGACY_SLOT.into(), real_key.clone());
        let (wrong, _) = Identity::generate(KeyStorage::Keychain).unwrap();
        st.create_identity_cert(wrong.cert_der.as_ref()).unwrap();
        assert_eq!(load_or_create(&st, &backends(&k, &f)).unwrap_err(), IdentityError::KeyMismatch);
        reset_identity(&st, &backends(&k, &f), "key_mismatch").unwrap();
        assert_eq!(k.slots.lock().unwrap().get(LEGACY_SLOT).unwrap(), &real_key, "la clave original sigue intacta");
        let _ = real;
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
    fn private_key_is_not_in_the_database_nor_in_debug_output() {
        let (st, k, f) = (store(), mem(false), mem(true));
        let id = load_or_create(&st, &backends(&k, &f)).unwrap();
        let blob = st.identity_cert().unwrap().unwrap();
        let n = id.key_der.len().min(32);
        assert!(!blob.windows(n).any(|w| w == &id.key_der[..n]));
        let dbg = format!("{id:?} {:?}", IdentityError::KeyMissing);
        assert!(!dbg.contains(&hex(&id.key_der[..n])));
    }

    #[test]
    fn file_store_never_overwrites_and_uses_owner_only_permissions() {
        let dir = TempDir::new().unwrap();
        let fs = FileStore::new(dir.path().to_path_buf());
        assert_eq!(fs.get("key-x").unwrap(), None);
        fs.set("key-x", b"secreto").unwrap();
        assert_eq!(fs.get("key-x").unwrap().unwrap(), b"secreto");
        assert!(fs.set("key-x", b"otro").is_err(), "no se pisa una clave existente");
        assert_eq!(fs.get("key-x").unwrap().unwrap(), b"secreto");
        fs.set(LEGACY_SLOT, b"legacy").unwrap();
        assert!(dir.path().join("device.key").exists(), "la ranura histórica conserva su nombre de archivo");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.path().join("device-key-x.key")).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn file_backend_persists_the_identity_across_restarts_in_an_isolated_dir() {
        let dir = TempDir::new().unwrap();
        let st = store();
        let mk = || SecretBackends { primary: Arc::new(FileStore::new(dir.path().to_path_buf())), alternates: vec![] };
        let first = load_or_create(&st, &mk()).unwrap();
        let second = load_or_create(&st, &mk()).unwrap();
        assert_eq!(first.device_id(), second.device_id());
    }

    #[test]
    fn fingerprint_formatting_groups_hex() {
        let fp = [0xABu8; 32];
        let s = format_fingerprint(&fp);
        assert_eq!(s.split(' ').count(), 8);
        assert!(s.starts_with("ABABABAB "));
    }
}
