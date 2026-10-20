mod common;

use axum::{Router, routing::post};
use common::{ManualClock, TestApp, cookie_pair};
use sekondo::app::AppState;

fn routes() -> Router<AppState> {
    async fn changed() -> &'static str {
        "changed"
    }
    Router::new().route("/test/change", post(changed))
}

struct Visitor {
    cookie: String,
    token: String,
}

/// Opens the landing page like a browser and reads the CSRF token the page renders.
async fn visit(app: &TestApp) -> Visitor {
    let res = app.client.get(app.url("/")).send().await.unwrap();
    let cookie = cookie_pair(&res).expect("the landing page starts a session");
    let body = res.text().await.unwrap();
    let token = body
        .split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("csrf meta tag")
        .to_owned();
    assert!(
        body.contains(&format!(r#"hx-headers='{{"X-CSRF-Token": "{token}"}}'"#)),
        "body must carry hx-headers"
    );
    Visitor { cookie, token }
}

fn post_change(app: &TestApp, v: &Visitor) -> reqwest::RequestBuilder {
    post_change_as(app, v, &app.base_url, &v.token)
}

/// A POST with explicit `Origin` and token values (headers are appended, never replaced).
fn post_change_as(
    app: &TestApp,
    v: &Visitor,
    origin: &str,
    token: &str,
) -> reqwest::RequestBuilder {
    app.client
        .post(app.url("/test/change"))
        .header("cookie", &v.cookie)
        .header("origin", origin)
        .header("x-csrf-token", token)
}

async fn app() -> TestApp {
    common::spawn_with(ManualClock::new(), routes()).await
}

#[tokio::test]
async fn valid_request_is_accepted() {
    let app = app().await;
    let v = visit(&app).await;
    let res = post_change(&app, &v).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "changed");
}

#[tokio::test]
async fn token_is_256_bit_and_stable_within_a_session() {
    let app = app().await;
    let v = visit(&app).await;
    assert_eq!(v.token.len(), 43, "32 bytes, base64url without padding");

    let again = app
        .client
        .get(app.url("/"))
        .header("cookie", &v.cookie)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(again.contains(&v.token));
}

#[tokio::test]
async fn missing_token_is_rejected() {
    let app = app().await;
    let v = visit(&app).await;
    let res = app
        .client
        .post(app.url("/test/change"))
        .header("cookie", &v.cookie)
        .header("origin", &app.base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn token_from_another_session_is_rejected() {
    let app = app().await;
    let mine = visit(&app).await;
    let other = visit(&app).await;
    assert_ne!(mine.token, other.token);
    let res = post_change_as(&app, &mine, &app.base_url, &other.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn request_without_a_session_is_rejected() {
    let app = app().await;
    let v = visit(&app).await;
    let res = app
        .client
        .post(app.url("/test/change"))
        .header("origin", &app.base_url)
        .header("x-csrf-token", &v.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn foreign_origin_is_rejected_even_with_a_valid_token() {
    let app = app().await;
    let v = visit(&app).await;
    let res = post_change_as(&app, &v, "https://evil.example", &v.token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn missing_origin_falls_back_to_referer() {
    let app = app().await;
    let v = visit(&app).await;
    let build = |referer: Option<String>| {
        let mut req = app
            .client
            .post(app.url("/test/change"))
            .header("cookie", &v.cookie)
            .header("x-csrf-token", &v.token);
        if let Some(r) = referer {
            req = req.header("referer", r);
        }
        req
    };
    let ok = build(Some(format!("{}/characters", app.base_url)))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let foreign = build(Some("https://evil.example/page".into()))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    let neither = build(None).send().await.unwrap();
    assert_eq!(neither.status(), 403);
}

#[tokio::test]
async fn cross_site_fetch_metadata_is_rejected() {
    let app = app().await;
    let v = visit(&app).await;
    let res = post_change(&app, &v)
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    let same = post_change(&app, &v)
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .unwrap();
    assert_eq!(same.status(), 200);
}

#[tokio::test]
async fn token_may_come_from_a_form_field() {
    let app = app().await;
    let v = visit(&app).await;
    let form = |token: &str| {
        app.client
            .post(app.url("/test/change"))
            .header("cookie", &v.cookie)
            .header("origin", &app.base_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("other=x&csrf_token={token}"))
    };
    assert_eq!(form(&v.token).send().await.unwrap().status(), 200);
    assert_eq!(form("wrong").send().await.unwrap().status(), 403);
}

#[tokio::test]
async fn safe_methods_are_not_checked() {
    let app = app().await;
    let res = app.client.get(app.url("/healthz")).send().await.unwrap();
    assert_eq!(res.status(), 200);
}
