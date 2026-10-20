mod common;

use common::{AppBuilder, Browser, TestApp};

const ALICE: i64 = 90000001;

async fn app() -> TestApp {
    AppBuilder::new().with_stub().spawn().await
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

/// The sidebar's links marked as the current page.
fn current_links(html: &str) -> Vec<String> {
    let sidebar = html
        .split_once("<aside")
        .and_then(|(_, rest)| rest.split_once("</aside>"))
        .map_or("", |(aside, _)| aside);
    sidebar
        .split("<a ")
        .skip(1)
        .filter_map(|tag| tag.split_once('>'))
        .filter(|(attrs, _)| attrs.contains(r#"aria-current="page""#))
        .map(|(attrs, rest)| match attrs.split_once(r#"aria-label=""#) {
            Some((_, label)) => label.split('"').next().unwrap().to_owned(),
            None => rest.split('<').next().unwrap().trim().to_owned(),
        })
        .collect()
}

#[tokio::test]
async fn a_signed_in_user_requesting_the_root_is_sent_to_the_dashboard() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let res = browser.get(&app.url("/")).await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/dashboard");
}

#[tokio::test]
async fn completing_the_sign_in_lands_on_the_dashboard_showing_the_totals_of_the_orders() {
    let app = app().await;
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
        html.contains(r#"href="/orders/sell""#),
        "the navigation leads to the orders"
    );
}

#[tokio::test]
async fn an_unauthenticated_request_for_the_dashboard_goes_to_the_landing_page() {
    let app = app().await;

    let res = app.browser().get(&app.url("/dashboard")).await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

const NAV_LINKS: [&str; 6] = [
    "/dashboard",
    "/characters",
    "/wealth",
    "/trades",
    "/orders/sell",
    "/opportunities/expansion",
];

#[tokio::test]
async fn every_signed_in_page_has_the_navigation_with_its_own_entry_marked() {
    let app = app().await;
    let browser = signed_in(&app).await;

    for (path, current) in [
        ("/dashboard", "Dashboard"),
        ("/wealth", "Wealth"),
        ("/trades", "Trades"),
        ("/orders/sell", "Orders"),
        ("/orders/buy", "Orders"),
        ("/opportunities/expansion", "Opportunities"),
        ("/characters", "Characters"),
    ] {
        let html = page(&app, &browser, path).await;
        for link in NAV_LINKS {
            assert!(
                html.contains(&format!(r#"href="{link}""#)),
                "{path} lacks {link}"
            );
        }
        assert!(html.contains("Sign out"), "{path} lacks sign out");
        assert_eq!(current_links(&html), vec![current.to_owned()], "{path}");
    }
}

#[tokio::test]
async fn the_landing_page_has_no_signed_in_navigation() {
    let app = app().await;

    let html = page(&app, &app.browser(), "/").await;

    assert!(!html.contains("Sign out"));
    assert!(!html.contains(r#"href="/orders/sell""#));
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

#[tokio::test]
async fn navigation_links_carry_a_narrowed_selection() {
    let app = app().await;
    let browser = two_characters(&app).await;

    for path in [
        format!("/dashboard?chars={BOB}"),
        format!("/characters?chars={BOB}"),
        format!("/orders/sell?chars={BOB}"),
        format!("/trades?chars={BOB}"),
        format!("/opportunities/expansion?chars={BOB}"),
    ] {
        let html = unescaped(&page(&app, &browser, &path).await);
        for link in NAV_LINKS {
            assert!(
                html.contains(&format!(r#"href="{link}?chars={BOB}""#)),
                "{path}: {link} lacks the selection"
            );
        }
    }
}

#[tokio::test]
async fn navigation_links_stay_plain_without_a_filter_or_with_every_character() {
    let app = app().await;
    let browser = two_characters(&app).await;

    for path in [
        "/dashboard".to_owned(),
        format!("/dashboard?chars={ALICE},{BOB}"),
        format!("/dashboard?chars={CAROL}"),
    ] {
        let html = page(&app, &browser, &path).await;
        assert!(html.contains(r#"href="/orders/sell""#), "{path}");
        assert!(!html.contains("chars="), "{path}");
    }
}

/// The markup of the main navigation, inside the sidebar.
fn sidebar(html: &str) -> &str {
    let aside = html
        .split_once(r#"<aside class="sidebar""#)
        .expect("no sidebar")
        .1;
    aside.split_once("</aside>").expect("unclosed sidebar").0
}

#[tokio::test]
async fn every_entry_has_an_icon_a_label_and_keeps_its_name_when_the_label_is_hidden() {
    let app = app().await;
    let browser = signed_in(&app).await;

    for path in [
        "/dashboard",
        "/orders/sell",
        "/wealth",
        "/trades",
        "/characters",
    ] {
        let html = page(&app, &browser, path).await;
        let side = sidebar(&html);
        for name in [
            "Dashboard",
            "Characters",
            "Wealth",
            "Trades",
            "Orders",
            "Opportunities",
            "Sign out",
        ] {
            let control = side
                .split('<')
                .find(|tag| {
                    (tag.starts_with("a ") || tag.starts_with("button "))
                        && tag.contains(&format!(r#"aria-label="{name}""#))
                })
                .unwrap_or_else(|| panic!("{path}: no control labelled {name}"));
            assert!(
                control.contains(&format!(r#"title="{name}""#)),
                "{path}: {control}"
            );
            assert!(
                side.contains(&format!(r#"<span class="label">{name}</span>"#)),
                "{path}: no visible label for {name}"
            );
        }
        let entries = side.split_once(r#"id="main-nav""#).unwrap().1;
        assert_eq!(
            entries.matches("<svg").count(),
            7,
            "{path}: one icon per entry"
        );
        assert_eq!(html.matches("<h1").count(), 1, "{path} has one page title");
    }
}

#[tokio::test]
async fn the_entries_are_ordered_and_sign_out_comes_last() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = page(&app, &browser, "/dashboard?range=30d&page=2&per_page=100").await;

    let nav = sidebar(&html)
        .split_once(r#"id="main-nav""#)
        .expect("no main nav")
        .1;
    let entries: Vec<&str> = nav
        .split(r#"aria-label=""#)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect();
    assert_eq!(
        entries,
        [
            "Dashboard",
            "Characters",
            "Wealth",
            "Trades",
            "Orders",
            "Opportunities",
            "Sign out"
        ]
    );
    let links: Vec<&str> = nav
        .split(r#"href=""#)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect();
    assert_eq!(links, NAV_LINKS, "no range, page or per-page in the links");
}
