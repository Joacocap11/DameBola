//! TLS 1.3 con autenticación mutua por certificados autofirmados y *pinning*
//! de identidad. No hay CA: la confianza se establece (a) por emparejamiento
//! verificado con un código y (b) comparando el fingerprint fijado en cada
//! conexión posterior.
//!
//! IMPORTANTE: los verificadores personalizados NO ignoran la verificación:
//! - Siempre validan la firma del handshake (prueba de posesión de la clave
//!   privada del certificado presentado), usando las rutinas de rustls.
//! - Cliente → servidor conocido: el fingerprint debe coincidir exactamente
//!   (`PeerPin::Expect`). Un impostor falla en el handshake.
//! - `PeerPin::AcceptAny` solo se usa para descubrir/emparejar un equipo aún
//!   desconocido; esa sesión no tiene autoridad (ver `service.rs`) y el
//!   emparejamiento exige comparar un código derivado del canal.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls13_signature, CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, ClientConfig, ConnectionCommon, DigitallySignedStruct, DistinguishedName, Error, ServerConfig, SignatureScheme};

use super::identity::{fingerprint_of, Fingerprint, Identity};
use crate::error::{AppError, AppResult};

/// Nombre ficticio: la identidad se verifica por fingerprint, no por nombre DNS.
pub const SERVER_NAME: &str = "damebola.local";
const MAX_CERT_BYTES: usize = 2048;
const SAS_LABEL: &[u8] = b"EXPERIMENTAL damebola pairing sas v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerPin {
    /// Exigir exactamente este certificado.
    Expect(Fingerprint),
    /// Dispositivo todavía desconocido (solo descubrimiento/emparejamiento).
    AcceptAny,
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

#[derive(Debug)]
struct ServerPinVerifier {
    pin: PeerPin,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for ServerPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if end_entity.len() > MAX_CERT_BYTES {
            return Err(Error::InvalidCertificate(CertificateError::BadEncoding));
        }
        match self.pin {
            PeerPin::AcceptAny => Ok(ServerCertVerified::assertion()),
            PeerPin::Expect(fp) if fingerprint_of(end_entity) == fp => Ok(ServerCertVerified::assertion()),
            PeerPin::Expect(_) => Err(Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)),
        }
    }

    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// Acepta cualquier certificado de cliente *bien formado que pruebe poseer su
/// clave*; la autorización se decide después por fingerprint (ver `service.rs`).
#[derive(Debug)]
struct ClientCertAnyVerifier {
    algs: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for ClientCertAnyVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(&self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, Error> {
        if end_entity.len() > MAX_CERT_BYTES || !intermediates.is_empty() {
            return Err(Error::InvalidCertificate(CertificateError::BadEncoding));
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

pub fn server_config(identity: &Identity) -> AppResult<Arc<ServerConfig>> {
    let p = provider();
    let verifier = Arc::new(ClientCertAnyVerifier { algs: p.signature_verification_algorithms });
    let cfg = ServerConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_err)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![identity.cert()], identity.private_key())
        .map_err(tls_err)?;
    Ok(Arc::new(cfg))
}

pub fn client_config(identity: &Identity, pin: PeerPin) -> AppResult<Arc<ClientConfig>> {
    let p = provider();
    let verifier = Arc::new(ServerPinVerifier { pin, algs: p.signature_verification_algorithms });
    let cfg = ClientConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(tls_err)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![identity.cert()], identity.private_key())
        .map_err(tls_err)?;
    Ok(Arc::new(cfg))
}

fn tls_err(e: Error) -> AppError {
    AppError::System(format!("TLS: {e}"))
}

/// Fingerprint del certificado presentado por el otro extremo.
pub fn peer_fingerprint(certs: Option<&[CertificateDer<'static>]>) -> Option<Fingerprint> {
    certs.and_then(|c| c.first()).map(|c| fingerprint_of(c.as_ref()))
}

/// Código de verificación (SAS) de 8 dígitos, derivado del *exporter* de la
/// sesión TLS (RFC 8446 §7.5) con ambos fingerprints como contexto. Ambos
/// extremos de una misma sesión obtienen el mismo código; un intermediario
/// que mantiene dos sesiones TLS distintas produce códigos distintos en cada
/// punta, y el usuario lo detecta al compararlos.
pub fn pairing_code<D>(conn: &ConnectionCommon<D>, initiator: &Fingerprint, responder: &Fingerprint) -> AppResult<String> {
    let mut ctx = [0u8; 64];
    ctx[..32].copy_from_slice(initiator);
    ctx[32..].copy_from_slice(responder);
    let out = conn.export_keying_material([0u8; 8], SAS_LABEL, Some(&ctx)).map_err(tls_err)?;
    let n = u64::from_be_bytes(out) % 100_000_000;
    Ok(format!("{:04} {:04}", n / 10_000, n % 10_000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::net::identity::{load_or_create, MemoryStore};
    use crate::net::store::NetStore;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    fn ident() -> Identity {
        load_or_create(&NetStore::new(Arc::new(Database::open_in_memory().unwrap())), &MemoryStore::default()).unwrap()
    }

    #[derive(Debug)]
    struct Outcome {
        client_code: String,
        server_code: String,
        server_saw: Fingerprint,
        client_saw: Fingerprint,
    }

    /// Handshake completo cliente↔servidor en memoria.
    async fn handshake(client: &Identity, server: &Identity, pin: PeerPin) -> Result<Outcome, String> {
        let (c_io, s_io) = duplex(16 * 1024);
        let acceptor = TlsAcceptor::from(server_config(server).unwrap());
        let connector = TlsConnector::from(client_config(client, pin).unwrap());
        let (cfp, sfp) = (*client.fingerprint(), *server.fingerprint());
        let srv = tokio::spawn(async move {
            let mut tls = acceptor.accept(s_io).await.map_err(|e| e.to_string())?;
            let (_, conn) = tls.get_ref();
            let code = pairing_code(conn, &cfp, &sfp).map_err(|e| e.to_string())?;
            let saw = peer_fingerprint(conn.peer_certificates().map(|c| &c[..]).map(|c| c.to_vec()).as_deref()).ok_or("sin cert")?;
            let mut b = [0u8; 1];
            let _ = tls.read(&mut b).await;
            Ok::<_, String>((code, saw))
        });
        let mut tls = connector
            .connect(ServerName::try_from(SERVER_NAME).unwrap(), c_io)
            .await
            .map_err(|e| e.to_string())?;
        let (_, conn) = tls.get_ref();
        let client_code = pairing_code(conn, &cfp, &sfp).map_err(|e| e.to_string())?;
        let client_saw = peer_fingerprint(conn.peer_certificates().map(|c| c.to_vec()).as_deref()).ok_or("sin cert")?;
        tls.write_all(b"x").await.map_err(|e| e.to_string())?;
        tls.shutdown().await.ok();
        let (server_code, server_saw) = srv.await.map_err(|e| e.to_string())??;
        Ok(Outcome { client_code, server_code, server_saw, client_saw })
    }

    #[tokio::test]
    async fn mutual_auth_exposes_peer_identities_and_equal_codes() {
        let (a, b) = (ident(), ident());
        let o = handshake(&a, &b, PeerPin::AcceptAny).await.unwrap();
        assert_eq!(o.server_saw, *a.fingerprint());
        assert_eq!(o.client_saw, *b.fingerprint());
        assert_eq!(o.client_code, o.server_code);
        assert_eq!(o.client_code.len(), 9);
    }

    #[tokio::test]
    async fn pinned_connection_succeeds_only_with_the_expected_certificate() {
        let (a, b, impostor) = (ident(), ident(), ident());
        assert!(handshake(&a, &b, PeerPin::Expect(*b.fingerprint())).await.is_ok());
        let err = handshake(&a, &impostor, PeerPin::Expect(*b.fingerprint())).await.unwrap_err();
        assert!(err.to_lowercase().contains("cert") || err.contains("Invalid") || err.contains("alert"), "{err}");
    }

    #[tokio::test]
    async fn a_man_in_the_middle_yields_different_codes_on_each_leg() {
        let (alice, bob, mallory) = (ident(), ident(), ident());
        // Alice cree hablar con Bob pero habla con Mallory; Mallory abre otra sesión con Bob.
        let leg_a = handshake(&alice, &mallory, PeerPin::AcceptAny).await.unwrap();
        let leg_b = handshake(&mallory, &bob, PeerPin::AcceptAny).await.unwrap();
        // Lo que ve Alice (su código) ≠ lo que ve Bob.
        assert_ne!(leg_a.client_code, leg_b.server_code, "los códigos de Alice y Bob deben diferir");
        // Además, Alice ve el fingerprint de Mallory, no el de Bob.
        assert_ne!(leg_a.client_saw, *bob.fingerprint());
    }

    #[tokio::test]
    async fn two_sessions_between_the_same_peers_have_different_codes() {
        let (a, b) = (ident(), ident());
        let x = handshake(&a, &b, PeerPin::AcceptAny).await.unwrap();
        let y = handshake(&a, &b, PeerPin::AcceptAny).await.unwrap();
        assert_ne!(x.client_code, y.client_code);
    }
}
