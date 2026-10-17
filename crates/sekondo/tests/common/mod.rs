#![allow(dead_code)]

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::Router;
use axum_server::Handle;
use eve_stub::{Stub, StubConfig};
use secrecy::SecretString;
use sekondo::{
    app::{self, App, AppState},
    clock::{Clock, SystemClock},
    config::{Config, SecretKey},
};
use sqlx::PgPool;
use tempfile::TempDir;
use url::Url;

pub use eve_stub::pki::TestPki;

pub const CLIENT_ID: &str = "test-client";
pub const CLIENT_SECRET: &str = "test-secret";
pub const CONTROL_TOKEN: &str = "test-control-token";
pub const ENCRYPTION_KEY: [u8; 32] = [9u8; 32];

pub struct TestApp {
    pub addr: SocketAddr,
    pub base_url: String,
    pub client: reqwest::Client,
    pub pki: TestPki,
    pub ca_path: PathBuf,
    pub stub: Option<Stub>,
    pub state: AppState,
    handle: Handle<SocketAddr>,
    _dir: TempDir,
}

impl TestApp {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub fn stub(&self) -> &Stub {
        self.stub
            .as_ref()
            .expect("the app was started without a stub")
    }

    /// Calls a `/_stub/` control endpoint.
    pub async fn stub_control(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.client
            .post(format!("{}/_stub/{path}", self.stub().base_url))
            .bearer_auth(CONTROL_TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub fn browser(&self) -> Browser {
        Browser::new(self.client.clone())
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

pub fn test_config(pki: &TestPki, dir: &TempDir, port: u16, sso_base_url: &str) -> Config {
    let (ca, cert, key) = pki.write(dir.path()).unwrap();
    Config {
        database_url: SecretString::from(
            "postgres://unused@localhost/unused?sslmode=verify-full&sslrootcert=/unused",
        ),
        bind_addr: format!("127.0.0.1:{port}").parse().unwrap(),
        tls_cert_path: cert,
        tls_key_path: key,
        tls_extra_ca_path: Some(ca),
        public_base_url: Url::parse(&format!("https://localhost:{port}")).unwrap(),
        eve_client_id: CLIENT_ID.to_owned(),
        eve_client_secret: SecretString::from(CLIENT_SECRET),
        eve_sso_base_url: Url::parse(sso_base_url).unwrap(),
        esi_base_url: Url::parse(sso_base_url).unwrap(),
        token_encryption_key: SecretKey(ENCRYPTION_KEY),
        static_dir: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/static")),
        order_poll_interval: time::Duration::minutes(20),
        esi_contact: Some("tests@example.com".to_owned()),
    }
}

/// Builds and starts the full app over HTTPS on a random loopback port.
pub struct AppBuilder {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    routes: Router<AppState>,
    with_stub: bool,
    sse_tick: Option<std::time::Duration>,
}

impl AppBuilder {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            clock: Arc::new(SystemClock),
            routes: Router::new(),
            with_stub: false,
            sse_tick: None,
        }
    }

    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn routes(mut self, routes: Router<AppState>) -> Self {
        self.routes = routes;
        self
    }

    /// Shortens the event stream's keep-alive and session re-check interval.
    pub fn sse_tick(mut self, tick: std::time::Duration) -> Self {
        self.sse_tick = Some(tick);
        self
    }

    /// Also starts `eve-stub` (same certificates) and points the app's SSO and ESI URLs at it.
    pub fn with_stub(mut self) -> Self {
        self.with_stub = true;
        self
    }

    pub async fn spawn(self) -> TestApp {
        let pki = TestPki::generate();
        let dir = TempDir::new().unwrap();

        let stub = if self.with_stub {
            esi_contract::gate::require_verified_stub();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let config = StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN);
            Some(
                Stub::start(
                    config,
                    listener,
                    pki.cert_pem.clone().into_bytes(),
                    pki.key_pem.clone().into_bytes(),
                )
                .await
                .unwrap(),
            )
        } else {
            None
        };
        let sso = stub
            .as_ref()
            .map_or("https://localhost:1".to_owned(), |s| s.base_url.clone());

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let config = test_config(&pki, &dir, addr.port(), &sso);
        let ca_path = config.tls_extra_ca_path.clone().unwrap();
        let mut options = app::Options::default();
        if let Some(tick) = self.sse_tick {
            options.sse_tick = tick;
        }
        let app = App::new_with_options(config, self.pool, self.clock, self.routes, options)
            .await
            .unwrap();
        let state = app.state.clone();
        let handle = Handle::new();
        tokio::spawn(app::serve(app, listener, handle.clone()));
        handle.listening().await;

        TestApp {
            addr,
            base_url: format!("https://localhost:{}", addr.port()),
            client: pki.client(),
            pki,
            ca_path,
            stub,
            state,
            handle,
            _dir: dir,
        }
    }
}

pub async fn spawn(pool: PgPool) -> TestApp {
    AppBuilder::new(pool).spawn().await
}

/// Like [`spawn`], with an injected clock and extra routes.
pub async fn spawn_with(
    pool: PgPool,
    clock: Arc<dyn Clock>,
    extra_routes: Router<AppState>,
) -> TestApp {
    AppBuilder::new(pool)
        .clock(clock)
        .routes(extra_routes)
        .spawn()
        .await
}

/// A clock tests can move forward.
#[derive(Debug)]
pub struct ManualClock(std::sync::Mutex<time::OffsetDateTime>);

impl ManualClock {
    pub fn new() -> Arc<Self> {
        // Whole seconds: Postgres keeps microseconds, and tests compare stored times with the clock.
        let now = time::OffsetDateTime::now_utc();
        Arc::new(Self(std::sync::Mutex::new(
            now.replace_nanosecond(0).unwrap(),
        )))
    }

    pub fn advance(&self, by: time::Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> time::OffsetDateTime {
        *self.0.lock().unwrap()
    }
}

/// The `name=value` pair of the first `Set-Cookie` header, ready for a `Cookie` header.
pub fn cookie_pair(res: &reqwest::Response) -> Option<String> {
    let set_cookie = res
        .headers()
        .get(reqwest::header::SET_COOKIE)?
        .to_str()
        .ok()?;
    Some(set_cookie.split(';').next()?.to_owned())
}

/// A minimal browser: keeps the one session cookie and never follows redirects on its own.
pub struct Browser {
    client: reqwest::Client,
    cookie: std::sync::Mutex<Option<String>>,
}

impl Browser {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            cookie: std::sync::Mutex::new(None),
        }
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.lock().unwrap().clone()
    }

    pub fn set_cookie(&self, cookie: Option<String>) {
        *self.cookie.lock().unwrap() = cookie;
    }

    pub async fn get(&self, url: &str) -> reqwest::Response {
        self.send(self.client.get(url)).await
    }

    pub fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    /// Sends with the stored cookie and remembers (or drops) the cookie the response sets.
    pub async fn send(&self, request: reqwest::RequestBuilder) -> reqwest::Response {
        let request = match self.cookie() {
            Some(cookie) => request.header("cookie", cookie),
            None => request,
        };
        let res = request.send().await.unwrap();
        for value in res.headers().get_all(reqwest::header::SET_COOKIE) {
            let value = value.to_str().unwrap();
            if value.starts_with("__Host-sekondo=") {
                let removed = value.to_lowercase().contains("max-age=0");
                self.set_cookie((!removed).then(|| value.split(';').next().unwrap().to_owned()));
            }
        }
        res
    }

    /// The `Location` header of a redirect response.
    pub fn location(res: &reqwest::Response) -> String {
        res.headers()["location"].to_str().unwrap().to_owned()
    }

    /// Starts an SSO flow at `start_path`, picks `character_id` on the stub's login page, and
    /// returns the app's response to the callback.
    pub async fn sso_round_trip(
        &self,
        app: &TestApp,
        start_path: &str,
        character_id: i64,
    ) -> reqwest::Response {
        let res = self.get(&app.url(start_path)).await;
        assert!(
            res.status().is_redirection(),
            "starting {start_path}: {}",
            res.status()
        );
        let mut select = Url::parse(&Self::location(&res)).unwrap();
        select.set_path("/v2/oauth/authorize/select");
        select
            .query_pairs_mut()
            .append_pair("character_id", &character_id.to_string());
        let picked = self.client.get(select).send().await.unwrap();
        assert_eq!(picked.status(), 302, "the stub redirects back to the app");
        self.get(&Self::location(&picked)).await
    }

    /// Like [`Browser::sso_round_trip`], but lets the test tamper with the callback URL.
    pub async fn sso_round_trip_with(
        &self,
        app: &TestApp,
        start_path: &str,
        character_id: i64,
        tamper: impl FnOnce(&mut Url),
    ) -> reqwest::Response {
        let res = self.get(&app.url(start_path)).await;
        let mut select = Url::parse(&Self::location(&res)).unwrap();
        select.set_path("/v2/oauth/authorize/select");
        select
            .query_pairs_mut()
            .append_pair("character_id", &character_id.to_string());
        let picked = self.client.get(select).send().await.unwrap();
        let mut callback = Url::parse(&Self::location(&picked)).unwrap();
        tamper(&mut callback);
        self.get(callback.as_str()).await
    }
}

/// An `EsiClient` wired to a running stub, without the rest of the app.
pub struct EsiHarness {
    pub stub: eve_stub::Stub,
    pub client: Arc<sekondo::esi::EsiClient>,
    pub clock: Arc<ManualClock>,
    pub config: Config,
    http: reqwest::Client,
    _dir: TempDir,
}

pub async fn esi_harness_with(contact: Option<&str>) -> EsiHarness {
    let pki = TestPki::generate();
    let dir = TempDir::new().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stub = eve_stub::Stub::start(
        StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN),
        listener,
        pki.cert_pem.clone().into_bytes(),
        pki.key_pem.clone().into_bytes(),
    )
    .await
    .unwrap();
    let mut config = test_config(&pki, &dir, 8443, &stub.base_url);
    config.esi_contact = contact.map(str::to_owned);
    let http = sekondo::tls::http_client(config.tls_extra_ca_path.as_deref()).unwrap();
    let clock = ManualClock::new();
    let client = Arc::new(sekondo::esi::EsiClient::new(
        http.clone(),
        &config,
        clock.clone(),
    ));
    EsiHarness {
        stub,
        client,
        clock,
        config,
        http,
        _dir: dir,
    }
}

pub async fn esi_harness() -> EsiHarness {
    esi_harness_with(Some("tests@example.com")).await
}

impl EsiHarness {
    pub fn token(&self, character_id: i64) -> SecretString {
        SecretString::from(self.stub.issue_access_token(character_id).unwrap())
    }

    pub async fn control(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.http
            .post(format!("{}/_stub/{path}", self.stub.base_url))
            .bearer_auth(CONTROL_TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap()
    }
}

/// The app's services over a stub and a database, without the HTTP server: for tests of the
/// background poller.
pub struct PollEnv {
    pub stub: eve_stub::Stub,
    pub pool: PgPool,
    pub repo: sekondo::repo::Repo,
    pub clock: Arc<ManualClock>,
    pub tokens: Arc<sekondo::tokens::TokenService>,
    pub esi: Arc<sekondo::esi::EsiClient>,
    pub poller: Arc<sekondo::poller::Poller>,
    pub market: Arc<sekondo::market::MarketPoller>,
    pub wealth: Arc<sekondo::wealth_poller::WealthPoller>,
    pub prices: Arc<sekondo::prices::PricePoller>,
    pub events: Arc<sekondo::events::InProcessBus>,
    sso: Arc<sekondo::sso::SsoClient>,
    pki: TestPki,
    http: reqwest::Client,
    pub interval: time::Duration,
    _dir: TempDir,
}

impl PollEnv {
    pub async fn new(pool: PgPool) -> Self {
        Self::with_interval(pool, time::Duration::minutes(20)).await
    }

    pub async fn with_interval(pool: PgPool, interval: time::Duration) -> Self {
        sekondo::tls::install_crypto_provider();
        let pki = TestPki::generate();
        let dir = TempDir::new().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stub = eve_stub::Stub::start(
            StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN),
            listener,
            pki.cert_pem.clone().into_bytes(),
            pki.key_pem.clone().into_bytes(),
        )
        .await
        .unwrap();
        let config = test_config(&pki, &dir, 8443, &stub.base_url);
        let http = sekondo::tls::http_client(config.tls_extra_ca_path.as_deref()).unwrap();
        let repo = sekondo::repo::Repo::new(
            pool.clone(),
            sekondo::crypto::TokenCipher::new(&SecretKey(ENCRYPTION_KEY)),
        );
        let clock = ManualClock::new();
        let sso = Arc::new(sekondo::sso::SsoClient::new(
            http.clone(),
            Url::parse(&stub.base_url).unwrap(),
            CLIENT_ID.to_owned(),
            SecretString::from(CLIENT_SECRET),
            std::time::Duration::ZERO,
        ));
        let tokens = Arc::new(sekondo::tokens::TokenService::new(
            repo.clone(),
            sso.clone(),
            clock.clone(),
        ));
        let esi = Arc::new(sekondo::esi::EsiClient::new(
            http.clone(),
            &config,
            clock.clone(),
        ));
        let events = Arc::new(sekondo::events::InProcessBus::new());
        let poller = Arc::new(sekondo::poller::Poller::new(
            pool.clone(),
            tokens.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
            interval,
        ));
        let market = Arc::new(sekondo::market::MarketPoller::new(
            pool.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
            time::Duration::minutes(5),
        ));
        let wealth = Arc::new(sekondo::wealth_poller::WealthPoller::new(
            pool.clone(),
            tokens.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
            interval,
        ));
        let prices = Arc::new(sekondo::prices::PricePoller::new(
            pool.clone(),
            esi.clone(),
            events.clone(),
            clock.clone(),
        ));
        Self {
            stub,
            pool,
            repo,
            clock,
            tokens,
            esi,
            poller,
            market,
            wealth,
            prices,
            events,
            sso,
            pki,
            http,
            interval,
            _dir: dir,
        }
    }

    pub async fn control(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        self.http
            .post(format!("{}/_stub/{path}", self.stub.base_url))
            .bearer_auth(CONTROL_TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    /// Links a character through the stub's real authorization code flow.
    pub async fn link(&self, account: uuid::Uuid, character_id: i64) {
        use secrecy::ExposeSecret;
        let flow = sekondo::oauth::PendingFlow::new(sekondo::oauth::Intent::Login);
        let redirect = "https://localhost:9/auth/callback";
        let sso_url = Url::parse(&self.stub.base_url).unwrap();
        let mut select = flow.authorize_url(&sso_url, CLIENT_ID, redirect);
        select.set_path("/v2/oauth/authorize/select");
        select
            .query_pairs_mut()
            .append_pair("character_id", &character_id.to_string());
        let res = self.pki.client().get(select).send().await.unwrap();
        let location = Url::parse(res.headers()["location"].to_str().unwrap()).unwrap();
        let code = location
            .query_pairs()
            .find(|(k, _)| k == "code")
            .unwrap()
            .1
            .into_owned();
        let tokens = self
            .sso
            .exchange_code(&code, &flow.code_verifier, redirect)
            .await
            .unwrap();
        let verified = self
            .sso
            .validate(tokens.access_token.expose_secret())
            .await
            .unwrap();
        self.repo
            .upsert_character(
                account,
                &sekondo::repo::CharacterUpsert {
                    character_id,
                    name: verified.name,
                    owner_hash: verified.owner_hash,
                    scopes: verified.scopes,
                    refresh_token: tokens.refresh_token.unwrap(),
                    access_token: tokens.access_token,
                    access_expires_at: verified.expires_at,
                },
            )
            .await
            .unwrap();
    }

    pub async fn new_account(&self) -> uuid::Uuid {
        self.repo.create_account().await.unwrap()
    }
}
