mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;

async fn app(pool: PgPool) -> TestApp {
    AppBuilder::new(pool).with_stub().spawn().await
}

async fn signed_in(app: &TestApp) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", ALICE).await;
    browser
}

async fn page(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The nav links marked as the current page.
fn current_links(html: &str) -> Vec<String> {
    html.split("<a ")
        .skip(1)
        .filter_map(|tag| tag.split_once('>'))
        .filter(|(attrs, _)| attrs.contains(r#"aria-current="page""#))
        .map(|(attrs, rest)| match attrs.split_once(r#"aria-label=""#) {
            Some((_, label)) => label.split('"').next().unwrap().to_owned(),
            None => rest.split('<').next().unwrap().trim().to_owned(),
        })
        .collect()
}

#[sqlx::test]
async fn a_signed_in_user_requesting_the_root_is_sent_to_the_dashboard(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let res = browser.get(&app.url("/")).await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/dashboard");
}

#[sqlx::test]
async fn completing_the_sign_in_lands_on_the_dashboard_showing_the_totals_of_the_orders(
    pool: PgPool,
) {
    let app = app(pool).await;
    let browser = app.browser();
    let mut res = browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    app.state.poller.poll_character(ALICE).await;

    // Follows the redirects the sign-in ends with, as a browser would.
    for _ in 0..3 {
        if !res.status().is_redirection() {
            break;
        }
        let target = Browser::location(&res);
        res = browser.get(&app.url(&target)).await;
    }

    assert_eq!(res.status(), 200);
    assert_eq!(res.url().path(), "/dashboard");
    let html = res.text().await.unwrap();
    assert!(html.contains(r#"id="tile-sell-value""#), "{html}");
    assert!(html.contains(r#"id="breakdown""#));
    assert!(
        html.contains(r#"href="/orders""#),
        "the navigation leads to the orders"
    );
}

#[sqlx::test]
async fn an_unauthenticated_request_for_the_dashboard_goes_to_the_landing_page(pool: PgPool) {
    let app = app(pool).await;

    let res = app.browser().get(&app.url("/dashboard")).await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn every_signed_in_page_has_the_navigation_with_its_own_entry_marked(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    for (path, current) in [
        ("/dashboard", "Dashboard"),
        ("/wealth", "Wealth"),
        ("/opportunities", "Opportunities"),
        ("/characters", "Characters"),
    ] {
        let html = page(&app, &browser, path).await;
        for link in [
            "/dashboard",
            "/orders",
            "/wealth",
            "/opportunities",
            "/characters",
        ] {
            assert!(
                html.contains(&format!(r#"href="{link}""#)),
                "{path} lacks {link}"
            );
        }
        assert!(html.contains("Sign out"), "{path} lacks sign out");
        assert_eq!(current_links(&html), vec![current.to_owned()], "{path}");
    }
}

#[sqlx::test]
async fn the_landing_page_has_no_signed_in_navigation(pool: PgPool) {
    let app = app(pool).await;

    let html = page(&app, &app.browser(), "/").await;

    assert!(!html.contains("Sign out"));
    assert!(!html.contains(r#"href="/orders""#));
    assert!(!html.contains(r#"href="/dashboard""#));
}

const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

/// Alice's account with Bob added to it.
async fn two_characters(app: &TestApp) -> Browser {
    let browser = signed_in(app).await;
    browser.sso_round_trip(app, "/auth/add", BOB).await;
    browser
}

fn unescaped(html: &str) -> String {
    html.replace("&#38;", "&").replace("&amp;", "&")
}

#[sqlx::test]
async fn navigation_links_carry_a_narrowed_selection(pool: PgPool) {
    let app = app(pool).await;
    let browser = two_characters(&app).await;

    for path in [
        format!("/dashboard?chars={BOB}"),
        format!("/characters?chars={BOB}"),
        format!("/orders?chars={BOB}"),
        format!("/opportunities?chars={BOB}"),
    ] {
        let html = unescaped(&page(&app, &browser, &path).await);
        for link in [
            "/dashboard",
            "/orders",
            "/wealth",
            "/opportunities",
            "/characters",
        ] {
            assert!(
                html.contains(&format!(r#"href="{link}?chars={BOB}""#)),
                "{path}: {link} lacks the selection"
            );
        }
    }
}

#[sqlx::test]
async fn navigation_links_stay_plain_without_a_filter_or_with_every_character(pool: PgPool) {
    let app = app(pool).await;
    let browser = two_characters(&app).await;

    for path in [
        "/dashboard".to_owned(),
        format!("/dashboard?chars={ALICE},{BOB}"),
        format!("/dashboard?chars={CAROL}"),
    ] {
        let html = page(&app, &browser, &path).await;
        assert!(html.contains(r#"href="/orders""#), "{path}");
        assert!(!html.contains("chars="), "{path}");
    }
}

#[sqlx::test]
async fn characters_and_sign_out_are_icon_only_controls_that_keep_their_names(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    for path in [
        "/dashboard",
        "/orders",
        "/wealth",
        "/opportunities",
        "/characters",
    ] {
        let html = page(&app, &browser, path).await;
        let nav = html
            .split_once(r#"id="main-nav""#)
            .and_then(|(_, rest)| rest.split_once("</nav>"))
            .unwrap_or_else(|| panic!("{path} has no navigation"))
            .0;
        for name in ["Characters", "Sign out"] {
            let control = nav
                .split(['<'])
                .find(|tag| {
                    (tag.starts_with("a ") || tag.starts_with("button "))
                        && tag.contains(&format!(r#"aria-label="{name}""#))
                })
                .unwrap_or_else(|| panic!("{path}: no control labelled {name}"));
            assert!(
                control.contains(&format!(r#"title="{name}""#)),
                "{path}: {control}"
            );
        }
        assert!(nav.matches("<svg").count() >= 2, "{path}: icons in the bar");
        assert!(!html.contains("<h1"), "{path} has a page heading");
    }
}
