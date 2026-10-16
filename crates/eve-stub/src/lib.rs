//! A stand-in for EVE SSO (and, in later changes, ESI) for tests and local development.
//!
//! It speaks HTTPS only, like the app. Control endpoints under `/_stub/` change its behaviour
//! and require the bearer token it was started with.

pub mod pki;
mod routes;
mod state;

use std::{net::SocketAddr, net::TcpListener, sync::Arc};

use axum_server::{Handle, tls_rustls::RustlsConfig};

pub use state::{Character, StubConfig, TokenOptions, default_characters};
use state::{Inner, Shared};

#[derive(Debug, thiserror::Error)]
pub enum StubError {
    #[error("tls: {0}")]
    Tls(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("key generation failed")]
    KeyGeneration,
}

/// A running stub server.
pub struct Stub {
    pub addr: SocketAddr,
    pub base_url: String,
    pub control_token: String,
    shared: Shared,
    handle: Handle<SocketAddr>,
}

impl Stub {
    /// Starts serving HTTPS on `listener` with the given PEM certificate and key.
    pub async fn start(
        config: StubConfig,
        listener: TcpListener,
        cert_pem: Vec<u8>,
        key_pem: Vec<u8>,
    ) -> Result<Self, StubError> {
        install_crypto_provider();
        let addr = listener.local_addr()?;
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| format!("https://localhost:{}", addr.port()));
        let control_token = config.control_token.clone();
        let shared: Shared = Arc::new(std::sync::Mutex::new(Inner::new(config, &base_url)?));

        let rustls = RustlsConfig::from_pem(cert_pem, key_pem)
            .await
            .map_err(|e| StubError::Tls(e.to_string()))?;
        listener.set_nonblocking(true)?;
        let handle = Handle::new();
        let server = axum_server::from_tcp_rustls(listener, rustls)?
            .handle(handle.clone())
            .serve(routes::router(shared.clone()).into_make_service());
        tokio::spawn(async move {
            if let Err(e) = server.await {
                eprintln!("eve-stub server stopped: {e}");
            }
        });
        handle.listening().await;

        Ok(Self {
            addr,
            base_url,
            control_token,
            shared,
            handle,
        })
    }

    /// Issues a signed access token for a fixture character (for tests that skip the flow).
    pub fn issue_access_token(&self, character_id: i64) -> Option<String> {
        self.shared
            .lock()
            .unwrap()
            .issue_access_token(character_id, None)
    }

    /// Issues a token with deliberate oddities (expired, wrong audience, other issuer, ...).
    pub fn issue_token(&self, character_id: i64, options: &TokenOptions) -> Option<String> {
        self.shared
            .lock()
            .unwrap()
            .issue_token(character_id, options)
    }

    /// Makes every access token issued from now on odd (expired, wrong audience, ...).
    pub fn set_token_options(&self, options: TokenOptions) {
        self.shared.lock().unwrap().token_options = options;
    }

    /// Rotates the signing key but keeps its `kid`.
    pub fn rotate_key_keeping_kid(&self) {
        self.shared
            .lock()
            .unwrap()
            .rotate_key_keeping_kid()
            .expect("key generation");
    }

    /// How many times `/oauth/jwks` was fetched.
    pub fn jwks_calls(&self) -> u64 {
        self.shared.lock().unwrap().jwks_calls
    }

    /// Rotates the signing key, like EVE does from time to time.
    pub fn rotate_key(&self) {
        self.shared
            .lock()
            .unwrap()
            .rotate_key()
            .expect("key generation");
    }

    /// How many times `/v2/oauth/token` was called (including failed calls).
    pub fn token_calls(&self) -> u64 {
        self.shared.lock().unwrap().token_calls
    }

    pub fn shutdown(&self) {
        self.handle.shutdown();
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

/// Selects aws-lc-rs as the process-wide rustls provider (dependencies enable both it and ring).
pub fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
