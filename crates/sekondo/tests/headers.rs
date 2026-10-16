mod common;

use sqlx::PgPool;

#[sqlx::test]
async fn security_headers_are_present_on_every_kind_of_response(pool: PgPool) {
    let app = common::spawn(pool).await;
    // An endpoint, a page, a static file and an unknown path.
    for path in ["/healthz", "/", "/static/htmx.min.js", "/does-not-exist"] {
        let res = app.client.get(app.url(path)).send().await.unwrap();
        let h = res.headers();
        let get = |name: &str| {
            h.get(name)
                .unwrap_or_else(|| panic!("{path}: {name} missing"))
                .to_str()
                .unwrap()
                .to_owned()
        };

        let csp = get("content-security-policy");
        for directive in [
            "default-src 'self'",
            "img-src 'self' https://images.evetech.net",
            "script-src 'self'",
            "style-src 'self'",
            "frame-ancestors 'none'",
            "base-uri 'none'",
            "form-action 'self' https://localhost:1",
        ] {
            assert!(
                csp.contains(directive),
                "{path}: CSP lacks `{directive}`: {csp}"
            );
        }
        assert_eq!(
            get("strict-transport-security"),
            "max-age=31536000",
            "{path}"
        );
        assert_eq!(get("x-content-type-options"), "nosniff", "{path}");
        assert_eq!(get("referrer-policy"), "same-origin", "{path}");
        assert_eq!(get("x-frame-options"), "DENY", "{path}");
    }
}

#[sqlx::test]
async fn landing_page_has_login_button_and_no_inline_script(pool: PgPool) {
    let app = common::spawn(pool).await;
    let body = app
        .client
        .get(app.url("/"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("Log in with EVE Online"));
    assert!(body.contains(r#"href="/auth/login""#));
    assert!(!body.contains("<script>"), "inline script present");
    assert!(!body.contains(" onclick="), "inline handler present");
}
