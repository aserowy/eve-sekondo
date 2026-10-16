//! Inbound (server) and outbound (client) TLS setup. Verification is never disabled.

use std::{path::Path, sync::Arc};

use axum_server::tls_rustls::RustlsConfig;
use rustls::{
    ServerConfig,
    crypto::aws_lc_rs,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("cannot load TLS material from {path}: {reason}")]
    Load { path: String, reason: String },
    #[error("invalid TLS configuration: {0}")]
    Config(String),
}

fn load_error(path: &Path, reason: impl ToString) -> TlsError {
    TlsError::Load {
        path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

/// TLS 1.2 and 1.3 only, rustls' default cipher suites, h2 and http/1.1 via ALPN.
pub fn server_config(cert_path: &Path, key_path: &Path) -> Result<RustlsConfig, TlsError> {
    let certs = CertificateDer::pem_file_iter(cert_path)
        .map_err(|e| load_error(cert_path, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| load_error(cert_path, e))?;
    let key = PrivateKeyDer::from_pem_file(key_path).map_err(|e| load_error(key_path, e))?;

    let mut config = ServerConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| TlsError::Config(e.to_string()))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::Config(e.to_string()))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(RustlsConfig::from_config(Arc::new(config)))
}

/// Outbound client for SSO and ESI: HTTPS only, TLS 1.2+, system roots plus an optional extra CA.
pub fn http_client(extra_ca: Option<&Path>) -> Result<reqwest::Client, TlsError> {
    install_crypto_provider();
    let mut builder = reqwest::Client::builder()
        .https_only(true)
        .min_tls_version(reqwest::tls::Version::TLS_1_2);
    if let Some(path) = extra_ca {
        let pem = std::fs::read(path).map_err(|e| load_error(path, e))?;
        let cert = reqwest::Certificate::from_pem(&pem).map_err(|e| load_error(path, e))?;
        builder = builder.add_root_certificate(cert);
    }
    builder.build().map_err(|e| TlsError::Config(e.to_string()))
}

/// Selects aws-lc-rs as the process-wide rustls provider. Needed because dependencies enable
/// both aws-lc-rs and ring, which makes rustls refuse to pick one. Safe to call repeatedly.
pub fn install_crypto_provider() {
    let _ = aws_lc_rs::default_provider().install_default();
}
