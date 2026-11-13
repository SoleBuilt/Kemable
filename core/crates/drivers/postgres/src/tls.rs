//! TLS for a connection, built from its `TlsMode` the way libpq reads `sslmode`.
//!
//! | Mode | sslmode | Certificate |
//! |---|---|---|
//! | Disable | disable | no TLS |
//! | Prefer | prefer | TLS when the server offers it, not checked |
//! | Require | require | TLS, not checked |
//! | VerifyFull | verify-full | chains to Windows' trusted roots, or `sslrootcert`, and names the host |
//!
//! "Not checked" still verifies the handshake's signatures; it skips only the
//! chain and the name, as libpq does. Neon reads the endpoint from the server
//! name (SNI), which rustls always sends for a host name.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use kemable_core::config::TlsMode;
use kemable_core::{DriverError, ErrorCode};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_postgres::config::SslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// The largest CA file read, so a device or a huge file named by mistake
/// cannot fill memory.
const MAX_CA_FILE: u64 = 1024 * 1024;

#[derive(Clone)]
pub(crate) enum Tls {
    Plain,
    Rustls(MakeRustlsConnect),
}

pub(crate) fn build(mode: TlsMode, options: &BTreeMap<String, String>) -> Result<(Tls, SslMode), DriverError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let ca_file = options.get("sslrootcert").filter(|path| !path.is_empty());
    if ca_file.is_some() && mode != TlsMode::VerifyFull {
        // A CA file with a mode that checks nothing would leave a user who
        // expects a check with none.
        return Err(DriverError::config("sslrootcert needs sslmode verify-full"));
    }
    Ok(match mode {
        TlsMode::Disable => (Tls::Plain, SslMode::Disable),
        TlsMode::Prefer => (Tls::Rustls(unchecked(provider)?), SslMode::Prefer),
        TlsMode::Require => (Tls::Rustls(unchecked(provider)?), SslMode::Require),
        TlsMode::VerifyFull => (
            Tls::Rustls(verified(provider, ca_file.map(Path::new))?),
            SslMode::Require,
        ),
    })
}

fn tls_error(error: rustls::Error) -> DriverError {
    DriverError::new(ErrorCode::Tls, error.to_string())
}

fn unchecked(provider: Arc<CryptoProvider>) -> Result<MakeRustlsConnect, DriverError> {
    let verifier = Arc::new(Unchecked {
        algorithms: provider.signature_verification_algorithms,
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

fn verified(provider: Arc<CryptoProvider>, ca_file: Option<&Path>) -> Result<MakeRustlsConnect, DriverError> {
    let verifier = match ca_file {
        Some(path) => rustls_platform_verifier::Verifier::new_with_extra_roots(read_ca_file(path)?, provider.clone()),
        None => rustls_platform_verifier::Verifier::new(provider.clone()),
    }
    .map_err(tls_error)?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

fn read_ca_file(path: &Path) -> Result<Vec<CertificateDer<'static>>, DriverError> {
    let unreadable = |what: &str| DriverError::config(format!("the CA file {} {what}", path.display()));
    let metadata = std::fs::metadata(path).map_err(|_| unreadable("cannot be read"))?;
    if !metadata.is_file() || metadata.len() > MAX_CA_FILE {
        return Err(unreadable("is not a certificate file of at most 1 MiB"));
    }
    let certificates: Vec<_> = CertificateDer::pem_file_iter(path)
        .map_err(|_| unreadable("cannot be read"))?
        .collect::<Result<_, _>>()
        .map_err(|_| unreadable("is not PEM"))?;
    if certificates.is_empty() {
        return Err(unreadable("holds no certificate"));
    }
    Ok(certificates)
}

/// Accepts any certificate chain and name, as libpq's `require` does, but
/// still checks that the server signed the handshake with the key it sent.
#[derive(Debug)]
struct Unchecked {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Unchecked {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}
