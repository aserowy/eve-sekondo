use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use axum::{
    Router, middleware,
    routing::{get, post},
};
use axum_server::Handle;
use tower_http::services::ServeDir;

use crate::{
    auth, characters,
    clock::{Clock, SystemClock},
    config::Config,
    crypto::TokenCipher,
    csrf, dashboard,
    esi::EsiClient,
    events::{EventBus, InProcessBus},
    fee_poller::FeePoller,
    headers,
    market::{MARKET_INTERVAL, MarketPoller},
    opportunities, orders, pages,
    poller::Poller,
    prices::PricePoller,
    repo::Repo,
    session, sse,
    sso::SsoClient,
    station_book::{BOOK_INTERVAL, StationBookPoller},
    station_history::StationHistoryPoller,
    store::{Stores, pg::PgStores},
    tls,
    tokens::TokenService,
    trades_page, wealth_page,
    wealth_poller::WealthPoller,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub stores: Stores,
    pub http: reqwest::Client,
    pub clock: Arc<dyn Clock>,
    pub repo: Repo,
    pub sso: Arc<SsoClient>,
    pub tokens: Arc<TokenService>,
    pub esi: Arc<EsiClient>,
    pub poller: Arc<Poller>,
    pub market: Arc<MarketPoller>,
    pub station_book: Arc<StationBookPoller>,
    pub station_history: Arc<StationHistoryPoller>,
    pub wealth: Arc<WealthPoller>,
    pub fees: Arc<FeePoller>,
    pub prices: Arc<PricePoller>,
    pub events: Arc<dyn EventBus>,
    pub sse_tick: std::time::Duration,
}

/// Knobs only tests turn.
#[derive(Debug, Clone)]
pub struct Options {
    /// Keep-alive interval of the event stream, which is also how often it re-checks its session.
    pub sse_tick: std::time::Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            sse_tick: crate::sse::KEEP_ALIVE,
        }
    }
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
    /// Builds the router over stores whose database is already migrated.
    pub async fn new(config: Config, stores: Stores) -> Result<Self, StartupError> {
        Self::new_with(config, stores, Arc::new(SystemClock), Router::new()).await
    }

    /// Like [`App::new`], with an injected clock and extra routes (used by tests).
    pub async fn new_with(
        config: Config,
        stores: Stores,
        clock: Arc<dyn Clock>,
        extra_routes: Router<AppState>,
    ) -> Result<Self, StartupError> {
        Self::new_with_options(config, stores, clock, extra_routes, Options::default()).await
    }

    pub async fn new_with_options(
        config: Config,
        stores: Stores,
        clock: Arc<dyn Clock>,
        extra_routes: Router<AppState>,
        options: Options,
    ) -> Result<Self, StartupError> {
        tls::install_crypto_provider();
        let http = tls::http_client(config.tls_extra_ca_path.as_deref())?;
        let repo = Repo::new(
            stores.accounts.clone(),
            TokenCipher::new(&config.token_encryption_key),
        );
        let sso = Arc::new(SsoClient::new(
            http.clone(),
            config.eve_sso_base_url.clone(),
            config.eve_client_id.clone(),
            config.eve_client_secret.clone(),
            std::time::Duration::from_secs(30),
        ));
        let tokens = Arc::new(TokenService::new(repo.clone(), sso.clone(), clock.clone()));
        let esi = Arc::new(EsiClient::new(http.clone(), &config, clock.clone()));
        let events: Arc<dyn EventBus> = Arc::new(InProcessBus::new());
        let poller = Arc::new(Poller::new(
            &stores,
            tokens.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
            config.order_poll_interval,
        ));
        let market = Arc::new(MarketPoller::new(
            &stores,
            esi.clone(),
            events.clone(),
            clock.clone(),
            MARKET_INTERVAL,
        ));
        let station_book = Arc::new(StationBookPoller::new(
            &stores,
            esi.clone(),
            events.clone(),
            clock.clone(),
            BOOK_INTERVAL,
        ));
        let station_history = Arc::new(StationHistoryPoller::new(
            &stores,
            esi.clone(),
            events.clone(),
            clock.clone(),
        ));
        let wealth = Arc::new(WealthPoller::new(
            &stores,
            tokens.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
            config.order_poll_interval,
        ));
        let fees = Arc::new(FeePoller::new(
            &stores,
            tokens.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
        ));
        let prices = Arc::new(PricePoller::new(
            &stores,
            esi.clone(),
            events.clone(),
            clock.clone(),
        ));
        let state = AppState {
            config: Arc::new(config),
            stores,
            http,
            clock,
            repo,
            sso,
            tokens,
            esi,
            poller,
            market,
            station_book,
            station_history,
            wealth,
            fees,
            prices,
            events,
            sse_tick: options.sse_tick,
        };
        let router = router(state.clone(), extra_routes);
        Ok(Self { state, router })
    }

    /// Connects to the (verify-full) database named in the config.
    pub async fn connect(config: Config) -> Result<Self, StartupError> {
        let pg = PgStores::connect(&config).await?;
        Self::new(config, pg.stores()).await
    }
}

pub fn router(state: AppState, extra_routes: Router<AppState>) -> Router {
    let routes = Router::new()
        .route("/", get(pages::index))
        .route("/dashboard", get(dashboard::page))
        .route("/dashboard/summary", get(dashboard::summary))
        .route("/orders", get(orders::index))
        .route("/orders/sell", get(orders::sell_page))
        .route("/orders/buy", get(orders::buy_page))
        .route("/orders/sell/content", get(orders::sell_content))
        .route("/orders/buy/content", get(orders::buy_content))
        .route("/orders/freshness", get(orders::freshness))
        .route("/wealth", get(wealth_page::page))
        .route("/wealth/freshness", get(wealth_page::freshness))
        .route("/wealth/content", get(wealth_page::content))
        .route("/trades", get(trades_page::page))
        .route("/trades/table", get(trades_page::table))
        .route("/trades/freshness", get(trades_page::freshness))
        .route("/opportunities", get(opportunities::page::index))
        .route(
            "/opportunities/freshness",
            get(opportunities::page::freshness_fragment),
        )
        .route(
            "/opportunities/detail/{table}/{type_id}",
            get(opportunities::page::detail),
        )
        .route(
            "/opportunities/settings",
            post(opportunities::page::save_settings),
        )
        .route(
            "/opportunities/fee-character",
            post(opportunities::page::save_fee_character),
        )
        .route(
            "/opportunities/{table}/{type_id}",
            get(opportunities::page::detail_page),
        )
        .route(
            "/opportunities/{table}",
            get(opportunities::page::table_page),
        )
        .route(
            "/opportunities/{table}/content",
            get(opportunities::page::content),
        )
        .route("/characters", get(pages::characters))
        .route("/characters/{id}/unlink", post(characters::unlink))
        .route("/events", get(sse::stream))
        .route("/healthz", get(healthz))
        .route("/auth/login", get(auth::login))
        .route("/auth/add", get(auth::add))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .merge(extra_routes);

    #[cfg(feature = "test-hooks")]
    let routes = {
        tracing::warn!(
            "built with the test-hooks feature: POST /_test/refresh/{{id}}, POST /_test/poll/{{id}} and POST /_test/market-poll, POST /_test/wealth-poll/{{id}}/{{kind}} and POST /_test/price-refresh and POST /_test/fee-poll and the /_test/station-* hooks are enabled"
        );
        routes
            .route("/_test/refresh/{id}", post(crate::test_hooks::refresh))
            .route("/_test/poll/{id}", post(crate::test_hooks::poll))
            .route("/_test/market-poll", post(crate::test_hooks::market_poll))
            .route(
                "/_test/wealth-poll/{id}/{kind}",
                post(crate::test_hooks::wealth_poll),
            )
            .route(
                "/_test/price-refresh",
                post(crate::test_hooks::price_refresh),
            )
            .route("/_test/fee-poll", post(crate::test_hooks::fee_poll))
            .route("/_test/station-book", post(crate::test_hooks::station_book))
            .route(
                "/_test/station-history",
                post(crate::test_hooks::station_history),
            )
            .route(
                "/_test/station-rollups",
                post(crate::test_hooks::station_rollups),
            )
            .route("/_test/station-seed", post(crate::test_hooks::station_seed))
    };

    let router = routes
        .nest_service("/static", ServeDir::new(&state.config.static_dir))
        .layer(middleware::from_fn_with_state(state.clone(), csrf::protect))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            session::enforce_lifetime,
        ))
        .layer(session::layer(session::AppSessionStore::new(
            state.stores.sessions.clone(),
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
