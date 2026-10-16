mod common;

use common::{AppBuilder, Browser, TestApp};
use sekondo::{config::SecretKey, crypto::TokenCipher, repo::Repo};
use sqlx::PgPool;
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

struct Env {
    app: TestApp,
    pool: PgPool,
    repo: Repo,
}

async fn env(pool: PgPool) -> Env {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let repo = Repo::new(
        pool.clone(),
        TokenCipher::new(&SecretKey(common::ENCRYPTION_KEY)),
    );
    Env { app, pool, repo }
}

impl Env {
    /// A browser signed in as Alice, who also has Bob linked.
    async fn alice_and_bob(&self) -> (Browser, Uuid) {
        let browser = self.app.browser();
        browser
            .sso_round_trip(&self.app, "/auth/login", ALICE)
            .await;
        browser.sso_round_trip(&self.app, "/auth/add", BOB).await;
        let account = self
            .repo
            .find_character(ALICE)
            .await
            .unwrap()
            .unwrap()
            .account_id;
        (browser, account)
    }

    async fn csrf_token(&self, browser: &Browser) -> String {
        let html = browser
            .get(&self.app.url("/characters"))
            .await
            .text()
            .await
            .unwrap();
        html.split(r#"<meta name="csrf-token" content=""#)
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned()
    }

    /// POSTs the way the page's htmx does: token header, own origin, `HX-Request`.
    async fn unlink(&self, browser: &Browser, token: &str, character: i64) -> reqwest::Response {
        browser
            .send(
                browser
                    .post(&self.app.url(&format!("/characters/{character}/unlink")))
                    .header("origin", &self.app.base_url)
                    .header("x-csrf-token", token)
                    .header("hx-request", "true"),
            )
            .await
    }

    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

#[sqlx::test]
async fn unlinking_one_of_two_characters_deletes_it_and_all_its_data(pool: PgPool) {
    let e = env(pool).await;
    let (browser, account) = e.alice_and_bob().await;
    let token = e.csrf_token(&browser).await;

    let res = e.unlink(&browser, &token, BOB).await;
    assert_eq!(res.status(), 200);
    assert_eq!(
        res.text().await.unwrap(),
        "",
        "an empty body removes the row in the page"
    );

    assert!(e.repo.find_character(BOB).await.unwrap().is_none());
    assert!(
        e.repo.load_tokens(BOB).await.unwrap().is_none(),
        "tokens are gone"
    );
    assert_eq!(e.count("character_tokens").await, 1);
    let remaining: Vec<_> = e
        .repo
        .list_characters(account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    assert_eq!(remaining, [ALICE]);

    let page = browser
        .get(&e.app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    assert!(page.contains("Alice Alpha") && !page.contains("Bob Bravo"));
}

#[sqlx::test]
async fn unlinking_the_last_character_deletes_the_account_and_signs_out(pool: PgPool) {
    let e = env(pool).await;
    let browser = e.app.browser();
    browser.sso_round_trip(&e.app, "/auth/login", ALICE).await;
    let token = e.csrf_token(&browser).await;

    let res = e.unlink(&browser, &token, ALICE).await;
    assert_eq!(res.status(), 200);
    assert_eq!(
        res.headers()["hx-redirect"],
        "/",
        "htmx navigates to the landing page"
    );

    assert_eq!(e.count("characters").await, 0);
    assert_eq!(e.count("accounts").await, 0);
    let after = browser.get(&e.app.url("/characters")).await;
    assert!(after.status().is_redirection());
    assert_eq!(Browser::location(&after), "/", "signed out");
}

#[sqlx::test]
async fn without_htmx_the_last_unlink_redirects_to_the_landing_page(pool: PgPool) {
    let e = env(pool).await;
    let browser = e.app.browser();
    browser.sso_round_trip(&e.app, "/auth/login", ALICE).await;
    let token = e.csrf_token(&browser).await;

    let res = browser
        .send(
            browser
                .post(&e.app.url(&format!("/characters/{ALICE}/unlink")))
                .header("origin", &e.app.base_url)
                .header("x-csrf-token", &token),
        )
        .await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn another_accounts_character_cannot_be_unlinked(pool: PgPool) {
    let e = env(pool).await;
    let mine = e.app.browser();
    mine.sso_round_trip(&e.app, "/auth/login", ALICE).await;
    let theirs = e.app.browser();
    theirs.sso_round_trip(&e.app, "/auth/login", BOB).await;
    let token = e.csrf_token(&mine).await;

    let res = e.unlink(&mine, &token, BOB).await;
    assert_eq!(res.status(), 404);
    assert!(e.repo.find_character(BOB).await.unwrap().is_some());
    assert!(e.repo.load_tokens(BOB).await.unwrap().is_some());
    assert_eq!(e.count("accounts").await, 2);
    // Still signed in.
    assert_eq!(mine.get(&e.app.url("/characters")).await.status(), 200);
}

#[sqlx::test]
async fn an_unknown_character_is_not_found(pool: PgPool) {
    let e = env(pool).await;
    let (browser, _) = e.alice_and_bob().await;
    let token = e.csrf_token(&browser).await;
    assert_eq!(e.unlink(&browser, &token, 12345).await.status(), 404);
    assert_eq!(e.count("characters").await, 2);
}

#[sqlx::test]
async fn unlink_without_a_csrf_token_is_forbidden_and_deletes_nothing(pool: PgPool) {
    let e = env(pool).await;
    let (browser, _) = e.alice_and_bob().await;
    let res = browser
        .send(
            browser
                .post(&e.app.url(&format!("/characters/{BOB}/unlink")))
                .header("origin", &e.app.base_url)
                .header("hx-request", "true"),
        )
        .await;
    assert_eq!(res.status(), 403);
    assert_eq!(e.count("characters").await, 2);
}

#[sqlx::test]
async fn unlink_with_a_foreign_origin_or_another_sessions_token_is_forbidden(pool: PgPool) {
    let e = env(pool).await;
    let (browser, _) = e.alice_and_bob().await;
    let token = e.csrf_token(&browser).await;

    let foreign = browser
        .send(
            browser
                .post(&e.app.url(&format!("/characters/{BOB}/unlink")))
                .header("origin", "https://evil.example")
                .header("x-csrf-token", &token),
        )
        .await;
    assert_eq!(foreign.status(), 403);

    let stranger = e.app.browser();
    stranger
        .sso_round_trip(&e.app, "/auth/login", 90000003)
        .await;
    let their_token = e.csrf_token(&stranger).await;
    let wrong = e.unlink(&browser, &their_token, BOB).await;
    assert_eq!(wrong.status(), 403);
    assert_eq!(e.count("characters").await, 3, "nothing was deleted");
}
