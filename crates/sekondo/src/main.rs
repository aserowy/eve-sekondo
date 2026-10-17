use std::process::ExitCode;

use axum_server::Handle;
use sekondo::{
    app::{self, App},
    config::Config,
};

#[tokio::main]
async fn main() -> ExitCode {
    sekondo::tls::install_crypto_provider();
    tracing_subscriber::fmt().init();

    let result = match std::env::args().nth(1).as_deref() {
        None => serve().await,
        Some("healthcheck") => healthcheck().await,
        Some(other) => {
            Err(format!("unknown command {other:?} (expected none or `healthcheck`)").into())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let listener = std::net::TcpListener::bind(config.bind_addr)?;
    let app = App::connect(config).await?;
    tracing::info!("listening on https://{}", listener.local_addr()?);
    // Started here and not in `App::new`, so tests opt in to background polling.
    let scheduler = app.state.poller.clone().spawn_scheduler();
    let market_scheduler = app.state.market.clone().spawn_scheduler();
    let result = app::serve(app, listener, Handle::new()).await;
    scheduler.abort();
    market_scheduler.abort();
    result?;
    Ok(())
}

/// Used by the container healthcheck: asks the running server over HTTPS, verifying its certificate.
async fn healthcheck() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let base_url = format!("https://localhost:{}", config.bind_addr.port());
    app::healthcheck(&base_url, config.tls_extra_ca_path.as_deref()).await?;
    Ok(())
}
