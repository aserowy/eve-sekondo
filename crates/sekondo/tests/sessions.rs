mod common;

use axum::{Router, extract::State, routing::get};
use common::{ManualClock, TestApp, cookie_pair};
use sekondo::{app::AppState, session};
use sqlx::PgPool;
use time::Duration;
use tower_sessions::Session;

/// `/test/login` signs a session in (rotating the ID) and stores a value; `/test/value` reads it.
fn session_routes() -> Router<AppState> {
    async fn login(State(state): State<AppState>, session: Session) -> &'static str {
        session.cycle_id().await.unwrap();
        session::start(&session, state.clock.now()).await.unwrap();
        session.insert("value", "hello").await.unwrap();
        "ok"
    }
    async fn value(session: Session) -> String {
        session
            .get::<String>("value")
            .await
            .unwrap()
            .unwrap_or_else(|| "none".to_owned())
    }
    Router::new()
        .route("/test/login", get(login))
        .route("/test/value", get(value))
}

async fn login(app: &TestApp) -> String {
    let res = app.client.get(app.url("/test/login")).send().await.unwrap();
    cookie_pair(&res).expect("login sets the session cookie")
}

async fn value(app: &TestApp, cookie: &str) -> String {
    app.client
        .get(app.url("/test/value"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

#[sqlx::test]
async fn cookie_has_host_prefix_and_hardened_attributes(pool: PgPool) {
    let app = common::spawn_with(pool, ManualClock::new(), session_routes()).await;
    let res = app.client.get(app.url("/test/login")).send().await.unwrap();
    let set_cookie = res
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    assert!(set_cookie.starts_with("__Host-sekondo="), "{set_cookie}");
    for attr in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/"] {
        assert!(set_cookie.contains(attr), "missing {attr}: {set_cookie}");
    }
    assert!(
        !set_cookie.to_lowercase().contains("domain"),
        "{set_cookie}"
    );
    assert!(
        set_cookie.contains("Max-Age=604800"),
        "7 day idle window: {set_cookie}"
    );
}

#[sqlx::test]
async fn session_value_survives_a_router_rebuild(pool: PgPool) {
    let clock = ManualClock::new();
    let first = common::spawn_with(pool.clone(), clock.clone(), session_routes()).await;
    let cookie = login(&first).await;
    assert_eq!(value(&first, &cookie).await, "hello");
    drop(first);

    let second = common::spawn_with(pool, clock, session_routes()).await;
    assert_eq!(value(&second, &cookie).await, "hello");
}

#[sqlx::test]
async fn session_expires_after_seven_idle_days(pool: PgPool) {
    let clock = ManualClock::new();
    let app = common::spawn_with(pool, clock.clone(), session_routes()).await;
    let cookie = login(&app).await;

    clock.advance(Duration::days(7) - Duration::seconds(1));
    assert_eq!(value(&app, &cookie).await, "hello");

    clock.advance(Duration::days(7) + Duration::seconds(1));
    assert_eq!(value(&app, &cookie).await, "none");
}

#[sqlx::test]
async fn activity_keeps_the_session_alive_within_the_absolute_limit(pool: PgPool) {
    let clock = ManualClock::new();
    let app = common::spawn_with(pool, clock.clone(), session_routes()).await;
    let cookie = login(&app).await;

    for _ in 0..2 {
        clock.advance(Duration::days(6));
        assert_eq!(value(&app, &cookie).await, "hello");
    }
}

#[sqlx::test]
async fn session_expires_after_thirty_days_even_when_active(pool: PgPool) {
    let clock = ManualClock::new();
    let app = common::spawn_with(pool, clock.clone(), session_routes()).await;
    let cookie = login(&app).await;

    for _ in 0..5 {
        clock.advance(Duration::days(6));
        assert_eq!(value(&app, &cookie).await, "hello");
    }
    clock.advance(Duration::days(1));
    assert_eq!(value(&app, &cookie).await, "none");
}

#[sqlx::test]
async fn cycling_the_id_invalidates_the_old_cookie(pool: PgPool) {
    let app = common::spawn_with(pool, ManualClock::new(), session_routes()).await;
    let first = login(&app).await;

    let res = app
        .client
        .get(app.url("/test/login"))
        .header("cookie", &first)
        .send()
        .await
        .unwrap();
    let second = cookie_pair(&res).expect("a rotated cookie is issued");

    assert_ne!(first, second);
    assert_eq!(value(&app, &second).await, "hello");
    assert_eq!(value(&app, &first).await, "none");
}
