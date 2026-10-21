use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::sync::Arc;
use suppaftp::tokio::AsyncRustlsConnector;
use suppaftp::FtpError;

use crate::{describe, FtpClientError};

/// Accepts any certificate. Only used when the user enabled "accept invalid
/// certificate" for a connection. Signatures are still checked, so the TLS
/// session itself stays sound; only the identity check is skipped.
#[derive(Debug)]
struct AcceptAnyCert(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCert {
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
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub fn connector(allow_invalid: bool) -> Result<AsyncRustlsConnector, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS setup failed: {e}"))?;
    let config = if allow_invalid {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
            .with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Ok(AsyncRustlsConnector::from(
        tokio_rustls::TlsConnector::from(Arc::new(config)),
    ))
}

/// Turn a TLS failure into a message that says what to do about it.
pub fn explain(e: FtpError, addr: &str) -> FtpClientError {
    let msg = describe(&e);
    let cert_problem = [
        "certificate",
        "Certificate",
        "UnknownIssuer",
        "NotValidForName",
        "CaUsedAsEndEntity",
        "Expired",
    ]
    .iter()
    .any(|k| msg.contains(k));
    if cert_problem {
        FtpClientError::Tls(format!(
            "The TLS certificate of {addr} could not be verified ({msg}). On shared hosting the \
             certificate is often issued for the hosting company, not for this address: use the \
             host name the provider gives you, or enable \"accept invalid certificate\" for this \
             connection."
        ))
    } else if let FtpError::UnexpectedResponse(r) = &e {
        // The server refused AUTH TLS (typically 500/502/504/534).
        FtpClientError::Tls(format!(
            "{addr} did not accept FTPS ({msg}). This server may only offer plain FTP, or FTPS \
             in the other mode: try another encryption option for this connection (status {}).",
            r.status.code()
        ))
    } else if matches!(e, FtpError::ConnectionError(_)) {
        FtpClientError::Connect(format!("could not connect to {addr}: {msg}"))
    } else {
        FtpClientError::Tls(format!("TLS negotiation with {addr} failed: {msg}"))
    }
}
