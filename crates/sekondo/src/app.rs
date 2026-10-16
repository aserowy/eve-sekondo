use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use axum::{
    Router, middleware,
    routing::{get, post},
};
use axum_server::Handle;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower_http::services::ServeDir;

use crate::{
    auth, characters,
    clock::{Clock, SystemClock},
    config::Config,
    crypto::TokenCipher,
    csrf, headers, pages,
    repo::Repo,
    session,
    sso::SsoClient,
    tls,
    tokens::TokenService,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub http: reqwest::Client,
    pub clock: Arc<dyn Clock>,
    pub repo: Repo,
    pub sso: Arc<SsoClient>,
    pub tokens: Arc<TokenService>,
}

pub struct App {
    pub state: AppState,
    pub router: Router,
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Tls(#[from] tls::TlsError),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("cannot bind listener: {0}")]
    Io(#[from] std::io::Error),
}

impl App {
    /// Runs migrations over the given pool and builds the router.
    pub async fn new(config: Config, pool: PgPool) -> Result<Self, StartupError> {
        Self::new_with(config, pool, Arc::new(SystemClock), Router::new()).await
    }

    /// Like [`App::new`], with an injected clock and extra routes (used by tests).
    pub async fn new_with(
        config: Config,
        pool: PgPool,
        clock: Arc<dyn Clock>,
        extra_routes: Router<AppState>,
    ) -> Result<Self, StartupError> {
        tls::install_crypto_provider();
        sqlx::migrate!("./migrations").run(&pool).await?;
        let http = tls::http_client(config.tls_extra_ca_path.as_deref())?;
        let repo = Repo::new(pool.clone(), TokenCipher::new(&config.token_encryption_key));
        let sso = Arc::new(SsoClient::new(
            http.clone(),
            config.eve_sso_base_url.clone(),
            config.eve_client_id.clone(),
            config.eve_client_secret.clone(),
            std::time::Duration::from_secs(30),
        ));
        let tokens = Arc::new(TokenService::new(repo.clone(), sso.clone(), clock.clone()));
        let state = AppState {
            config: Arc::new(config),
            pool,
            http,
            clock,
            repo,
            sso,
            tokens,
        };
        let router = router(state.clone(), extra_routes);
        Ok(Self { state, router })
    }

    /// Connects to the (verify-full) database named in the config.
    pub async fn connect(config: Config) -> Result<Self, StartupError> {
        use secrecy::ExposeSecret;
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(config.database_url.expose_secret())
            .await?;
        Self::new(config, pool).await
    }
}

pub fn router(state: AppState, extra_routes: Router<AppState>) -> Router {
    let routes = Router::new()
        .route("/", get(pages::index))
        .route("/characters", get(pages::characters))
        .route("/characters/{id}/unlink", post(characters::unlink))
        .route("/healthz", get(healthz))
        .route("/auth/login", get(auth::login))
        .route("/auth/add", get(auth::add))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .merge(extra_routes);

    #[cfg(feature = "test-hooks")]
    let routes = {
        tracing::warn!("built with the test-hooks feature: POST /_test/refresh/{{id}} is enabled");
        routes.route("/_test/refresh/{id}", post(crate::test_hooks::refresh))
    };

    let router = routes
        .nest_service("/static", ServeDir::new(&state.config.static_dir))
        .layer(middleware::from_fn_with_state(state.clone(), csrf::protect))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            session::enforce_lifetime,
        ))
        .layer(session::layer(session::PgSessionStore::new(
            state.pool.clone(),
        )))
        .with_state(state.clone());
    headers::apply(router, &state.config.eve_sso_base_url)
}

async fn healthz() -> &'static str {
    "ok"
}

/// Serves HTTPS only on an already bound listener. There is no plain-HTTP listener.
pub async fn serve(
    app: App,
    listener: TcpListener,
    handle: Handle<SocketAddr>,
) -> Result<(), StartupError> {
    let rustls = tls::server_config(
        &app.state.config.tls_cert_path,
        &app.state.config.tls_key_path,
    )?;
    listener.set_nonblocking(true)?;
    axum_server::from_tcp_rustls(listener, rustls)?
        .handle(handle)
        .serve(app.router.into_make_service())
        .await?;
    Ok(())
}

/// Calls `/healthz` over HTTPS with full certificate verification (the container healthcheck).
/// `extra_ca` is trusted in addition to the system roots, like for outbound SSO traffic.
pub async fn healthcheck(base_url: &str, extra_ca: Option<&std::path::Path>) -> Result<(), String> {
    let client = tls::http_client(extra_ca).map_err(|e| e.to_string())?;
    let url = format!("{}/healthz", base_url.trim_end_matches('/'));
    let res = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    if res.status() != reqwest::StatusCode::OK {
        return Err(format!("{url}: HTTP {}", res.status()));
    }
    Ok(())
}
