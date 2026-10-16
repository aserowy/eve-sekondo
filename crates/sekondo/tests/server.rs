mod common;

use std::io::{Read, Write};

use sqlx::PgPool;

#[sqlx::test]
async fn healthz_returns_200_over_tls(pool: PgPool) {
    let app = common::spawn(pool).await;
    let res = app.client.get(app.url("/healthz")).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[sqlx::test]
async fn certificate_is_verified_by_clients(pool: PgPool) {
    let app = common::spawn(pool).await;
    // A client that does not trust the per-run CA must fail the handshake.
    let untrusting = reqwest::Client::new();
    assert!(untrusting.get(app.url("/healthz")).send().await.is_err());
}

#[sqlx::test]
async fn vendored_static_files_are_served(pool: PgPool) {
    let app = common::spawn(pool).await;
    for path in [
        "/static/htmx.min.js",
        "/static/pico.min.css",
        "/static/app.js",
    ] {
        let res = app.client.get(app.url(path)).send().await.unwrap();
        assert_eq!(res.status(), 200, "{path}");
    }
    let js = app
        .client
        .get(app.url("/static/app.js"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(js.contains("htmx.config.includeIndicatorStyles = false"));
}

#[sqlx::test]
async fn plain_http_request_gets_no_response_body(pool: PgPool) {
    let app = common::spawn(pool).await;
    let addr = app.addr;
    let body = tokio::task::spawn_blocking(move || {
        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
        buf
    })
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains("HTTP/1.1"), "got an HTTP response: {text}");
    assert!(!text.contains("ok"), "got a body: {text}");
}

#[sqlx::test]
async fn tls_1_2_is_accepted(pool: PgPool) {
    let app = common::spawn(pool).await;
    // TLS 1.2 stays supported; 1.0/1.1 are not offered by rustls at all.
    let client = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(app.pki.ca_pem.as_bytes()).unwrap())
        .max_tls_version(reqwest::tls::Version::TLS_1_2)
        .build()
        .unwrap();
    let res = client.get(app.url("/healthz")).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[sqlx::test]
async fn healthcheck_succeeds_against_a_running_app_that_it_trusts(pool: PgPool) {
    let app = common::spawn(pool).await;
    sekondo::tls::install_crypto_provider();
    sekondo::app::healthcheck(&app.base_url, Some(&app.ca_path))
        .await
        .unwrap();
}

#[sqlx::test]
async fn healthcheck_verifies_the_certificate(pool: PgPool) {
    let app = common::spawn(pool).await;
    sekondo::tls::install_crypto_provider();
    // Without the CA the certificate is not trusted, so the check must fail rather than pass.
    let result = sekondo::app::healthcheck(&app.base_url, None).await;
    assert!(
        result.is_err(),
        "an untrusted certificate must fail the health check"
    );
}

#[tokio::test]
async fn healthcheck_fails_when_nothing_is_listening() {
    sekondo::tls::install_crypto_provider();
    let free = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    assert!(
        sekondo::app::healthcheck(&format!("https://localhost:{free}"), None)
            .await
            .is_err()
    );
}
