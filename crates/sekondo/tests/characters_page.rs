mod common;

use common::{AppBuilder, Browser, TestApp};
use secrecy::SecretString;
use sekondo::{
    config::SecretKey,
    crypto::TokenCipher,
    repo::{CharacterUpsert, Repo},
};
use time::OffsetDateTime;
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

struct Env {
    app: TestApp,
    repo: Repo,
}

async fn env() -> Env {
    let app = AppBuilder::new().with_stub().spawn().await;
    let repo = Repo::new(
        app.stores.accounts.clone(),
        TokenCipher::new(&SecretKey(common::ENCRYPTION_KEY)),
    );
    Env { app, repo }
}

impl Env {
    async fn seed(&self, account: Uuid, id: i64, name: &str) {
        self.repo
            .upsert_character(
                account,
                &CharacterUpsert {
                    character_id: id,
                    name: name.to_owned(),
                    owner_hash: format!("owner-hash-{id}"),
                    scopes: vec![],
                    refresh_token: SecretString::from("r".to_owned()),
                    access_token: SecretString::from("a".to_owned()),
                    access_expires_at: OffsetDateTime::now_utc(),
                },
            )
            .await
            .unwrap();
    }

    async fn characters_page(&self, browser: &Browser) -> reqwest::Response {
        browser.get(&self.app.url("/characters")).await
    }
}

#[tokio::test]
async fn unauthenticated_access_redirects_to_the_landing_page() {
    let e = env().await;
    let res = e.characters_page(&e.app.browser()).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn lists_own_characters_with_portrait_name_and_actions_but_nobody_elses() {
    let e = env().await;
    let mine = e.app.browser();
    e.login_as(&mine, ALICE).await;
    let other = e.repo.create_account().await.unwrap();
    e.seed(other, CAROL, "Carol Charlie").await;
    let account = e.account_of(ALICE).await;
    e.seed(account, BOB, "Bob Bravo").await;

    let res = e.characters_page(&mine).await;
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();

    assert!(html.contains("Alice Alpha") && html.contains("Bob Bravo"));
    assert!(
        !html.contains("Carol Charlie"),
        "no character of another account"
    );
    assert!(html.contains("https://images.evetech.net/characters/90000001/portrait?size=64"));
    assert!(html.contains("https://images.evetech.net/characters/90000002/portrait?size=64"));
    assert!(html.contains(r#"href="/auth/add""#) && html.contains("Add character"));
    assert!(html.contains("Sign out"));
    assert!(
        !html.contains("Needs re-login"),
        "no badge for healthy characters"
    );
}

#[tokio::test]
async fn a_character_that_needs_relogin_shows_a_badge_and_a_log_in_again_action() {
    let e = env().await;
    let browser = e.app.browser();
    e.login_as(&browser, ALICE).await;
    let account = e.account_of(ALICE).await;
    e.seed(account, BOB, "Bob Bravo").await;
    e.app.stores.accounts.mark_needs_relogin(BOB).await.unwrap();

    let html = e.characters_page(&browser).await.text().await.unwrap();
    assert_eq!(
        html.matches("Needs re-login").count(),
        1,
        "only the marked character"
    );
    assert!(html.contains("Log in again"));
}

#[tokio::test]
async fn the_landing_page_redirects_signed_in_users_to_the_dashboard() {
    let e = env().await;
    let browser = e.app.browser();
    e.login_as(&browser, ALICE).await;
    let res = browser.get(&e.app.url("/")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/dashboard");
}

#[tokio::test]
async fn a_session_whose_account_no_longer_exists_is_signed_out() {
    let e = env().await;
    let browser = e.app.browser();
    e.login_as(&browser, ALICE).await;
    e.repo.delete_character(ALICE).await.unwrap(); // deletes the (now empty) account too

    let res = e.characters_page(&browser).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
    let again = e.characters_page(&browser).await;
    assert_eq!(
        Browser::location(&again),
        "/",
        "the session is gone, not just empty"
    );
}

#[tokio::test]
async fn a_flash_message_is_shown_once() {
    let e = env().await;
    let browser = e.app.browser();
    e.login_as(&browser, ALICE).await;
    // Adding a character that belongs to another account sets the flash message.
    let other = e.repo.create_account().await.unwrap();
    e.seed(other, BOB, "Bob Bravo").await;
    browser.sso_round_trip(&e.app, "/auth/add", BOB).await;

    let first = e.characters_page(&browser).await.text().await.unwrap();
    assert!(
        first.contains("belongs to another Sekondo account"),
        "{first}"
    );
    let second = e.characters_page(&browser).await.text().await.unwrap();
    assert!(!second.contains("belongs to another Sekondo account"));
}

impl Env {
    async fn login_as(&self, browser: &Browser, character: i64) {
        let res = browser
            .sso_round_trip(&self.app, "/auth/login", character)
            .await;
        assert!(res.status().is_redirection());
    }

    async fn account_of(&self, character: i64) -> Uuid {
        self.repo
            .find_character(character)
            .await
            .unwrap()
            .unwrap()
            .account_id
    }
}
