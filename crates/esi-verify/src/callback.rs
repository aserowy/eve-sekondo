//! The listener for the login's callback, on the address the application is registered with.

use std::{
    collections::HashMap,
    net::{SocketAddr, TcpListener},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use axum_server::Handle;
use tokio::sync::oneshot;

type Sender = Arc<Mutex<Option<oneshot::Sender<HashMap<String, String>>>>>;

pub struct CallbackServer {
    receiver: oneshot::Receiver<HashMap<String, String>>,
    handle: Handle<SocketAddr>,
}

/// Binds `addr` (failing with a message that names it) and serves `/auth/callback` over HTTPS.
pub async fn listen(addr: SocketAddr, cert: &Path, key: &Path) -> Result<CallbackServer, String> {
    let listener = TcpListener::bind(addr).map_err(|e| {
        format!(
            "cannot listen on {addr} for the login callback ({e}): stop whatever uses that address, \
             for example the app container"
        )
    })?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    sekondo::tls::install_crypto_provider();
    let tls = sekondo::tls::server_config(cert, key).map_err(|e| e.to_string())?;
    let (sender, receiver) = oneshot::channel();
    let sender: Sender = Arc::new(Mutex::new(Some(sender)));
    let router = Router::new()
        .route("/auth/callback", get(callback))
        .with_state(sender);
    let handle = Handle::new();
    let server = axum_server::from_tcp_rustls(listener, tls)
        .map_err(|e| e.to_string())?
        .handle(handle.clone())
        .serve(router.into_make_service());
    tokio::spawn(async move {
        let _ = server.await;
    });
    handle.listening().await;
    Ok(CallbackServer { receiver, handle })
}

async fn callback(
    State(sender): State<Sender>,
    Query(query): Query<HashMap<String, String>>,
) -> Html<&'static str> {
    if let Some(sender) = sender.lock().unwrap().take() {
        let _ = sender.send(query);
    }
    Html("<!DOCTYPE html><title>esi-verify</title><p>Login received. You can close this tab.</p>")
}

impl CallbackServer {
    /// The query of the first callback, or an error after `timeout`.
    pub async fn wait(self, timeout: Duration) -> Result<HashMap<String, String>, String> {
        let result = tokio::time::timeout(timeout, self.receiver).await;
        self.handle.graceful_shutdown(Some(Duration::from_secs(1)));
        match result {
            Ok(Ok(query)) => Ok(query),
            Ok(Err(_)) => Err("the callback listener stopped before a login arrived".to_owned()),
            Err(_) => Err(format!(
                "no login arrived within {} seconds",
                timeout.as_secs()
            )),
        }
    }
}
