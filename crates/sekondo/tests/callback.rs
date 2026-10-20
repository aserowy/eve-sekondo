mod common;

use axum::{Router, extract::State, response::IntoResponse, routing::get};
use common::{AppBuilder, Browser, TestApp};
use eve_stub::TokenOptions;
use secrecy::{ExposeSecret, SecretString};
use sekondo::{
    account::{self, CurrentAccount},
    app::AppState,
    auth,
    config::SecretKey,
    crypto::TokenCipher,
    fee_poller::Kind,
    repo::{CharacterUpsert, Repo},
};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

fn routes() -> Router<AppState> {
    async fn whoami(account: CurrentAccount) -> String {
        account.id.to_string()
    }
    async fn flash(session: Session) -> String {
        auth::take_flash(&session).await.unwrap_or_default()
    }
    async fn signin(State(state): State<AppState>, session: Session) -> impl IntoResponse {
        let id = Uuid::new_v4();
        account::sign_in(&session, id, state.clock.now())
            .await
            .unwrap();
        id.to_string()
    }
    Router::new()
        .route("/test/whoami", get(whoami))
        .route("/test/flash", get(flash))
        .route("/test/signin", get(signin))
}

struct Env {
    app: TestApp,
    repo: Repo,
}

async fn env() -> Env {
    let app = AppBuilder::new().routes(routes()).with_stub().spawn().await;
    let repo = Repo::new(
        app.stores.accounts.clone(),
        TokenCipher::new(&SecretKey(common::ENCRYPTION_KEY)),
    );
    Env { app, repo }
}

impl Env {
    /// The account the browser is signed in to, if any.
    async fn whoami(&self, browser: &Browser) -> Option<Uuid> {
        let res = browser.get(&self.app.url("/test/whoami")).await;
        if res.status().is_redirection() {
            assert_eq!(Browser::location(&res), "/");
            return None;
        }
        Some(res.text().await.unwrap().parse().unwrap())
    }

    async fn login(&self, browser: &Browser, character: i64) -> reqwest::Response {
        browser
            .sso_round_trip(&self.app, "/auth/login", character)
            .await
    }

    async fn add(&self, browser: &Browser, character: i64) -> reqwest::Response {
        browser
            .sso_round_trip(&self.app, "/auth/add", character)
            .await
    }

    /// How many accounts exist, or how many characters all accounts have.
    async fn count(&self, table: &str) -> i64 {
        let accounts = &self.app.stores.accounts;
        match table {
            "accounts" => accounts.account_count().await.unwrap(),
            "characters" => {
                let mut total = 0;
                for account in accounts.account_ids().await.unwrap() {
                    total += accounts.list_characters(account).await.unwrap().len() as i64;
                }
                total
            }
            other => panic!("no count of {other}"),
        }
    }

    /// Links a character directly, with placeholder tokens, to set up existing state.
    async fn seed(&self, account: Uuid, character_id: i64, owner_hash: &str) {
        let upsert = CharacterUpsert {
            character_id,
            name: format!("seeded {character_id}"),
            owner_hash: owner_hash.to_owned(),
            scopes: vec![],
            refresh_token: SecretString::from("seeded-refresh".to_owned()),
            access_token: SecretString::from("seeded-access".to_owned()),
            access_expires_at: OffsetDateTime::now_utc() - Duration::hours(1),
        };
        self.repo.upsert_character(account, &upsert).await.unwrap();
    }

    async fn owner_hash_of(&self, character_id: i64) -> String {
        self.repo
            .find_character(character_id)
            .await
            .unwrap()
            .unwrap()
            .owner_hash
    }
}

fn assert_redirects_to_characters(res: &reqwest::Response) {
    assert!(res.status().is_redirection(), "{}", res.status());
    assert_eq!(Browser::location(res), "/characters");
}

fn assert_redirects_to_dashboard(res: &reqwest::Response) {
    assert!(res.status().is_redirection(), "{}", res.status());
    assert_eq!(Browser::location(res), "/dashboard");
}

// ---- Sign in with EVE SSO ----

#[tokio::test]
async fn brand_new_character_creates_an_account_and_signs_in() {
    let e = env().await;
    let browser = e.app.browser();
    let res = e.login(&browser, ALICE).await;
    assert_redirects_to_dashboard(&res);

    let account = e.whoami(&browser).await.expect("signed in");
    let characters = e.repo.list_characters(account).await.unwrap();
    assert_eq!(characters.len(), 1);
    assert_eq!(characters[0].character_id, ALICE);
    assert_eq!(characters[0].name, "Alice Alpha");
    assert_eq!(characters[0].owner_hash, "owner-hash-90000001");
    assert_eq!(characters[0].scopes.len(), 7);
    assert!(
        characters[0]
            .scopes
            .iter()
            .any(|s| s == sekondo::oauth::LOCATION_SCOPE)
    );
    assert_eq!(e.count("accounts").await, 1);

    let tokens = e.repo.load_tokens(ALICE).await.unwrap().unwrap();
    assert!(!tokens.refresh_token.expose_secret().is_empty());
    assert!(tokens.access_expires_at.unwrap() > OffsetDateTime::now_utc());
}

#[tokio::test]
async fn callback_with_a_mismatched_state_is_rejected() {
    let e = env().await;
    let browser = e.app.browser();
    let res = browser
        .sso_round_trip_with(&e.app, "/auth/login", ALICE, |url| {
            let pairs: Vec<_> = url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            url.query_pairs_mut()
                .clear()
                .extend_pairs(pairs.iter().map(|(k, v)| {
                    (
                        k.as_str(),
                        if k == "state" {
                            "forged-state"
                        } else {
                            v.as_str()
                        },
                    )
                }));
        })
        .await;
    assert_eq!(res.status(), 400);
    let body = res.text().await.unwrap();
    assert!(
        body.contains("Try again") && body.contains(r#"href="/auth/login""#),
        "{body}"
    );

    assert_eq!(e.count("characters").await, 0);
    assert_eq!(e.count("accounts").await, 0);
    assert!(e.whoami(&browser).await.is_none());
}

#[tokio::test]
async fn callback_without_a_started_flow_is_rejected() {
    let e = env().await;
    let res = e
        .app
        .browser()
        .get(&e.app.url("/auth/callback?code=whatever&state=whatever"))
        .await;
    assert_eq!(res.status(), 400);
    assert_eq!(e.count("characters").await, 0);
}

#[tokio::test]
async fn state_is_single_use() {
    let e = env().await;
    let browser = e.app.browser();
    let start = browser.get(&e.app.url("/auth/login")).await;
    let mut select = url::Url::parse(&Browser::location(&start)).unwrap();
    select.set_path("/v2/oauth/authorize/select");
    select
        .query_pairs_mut()
        .append_pair("character_id", &ALICE.to_string());
    let picked = e.app.client.get(select).send().await.unwrap();
    let callback = Browser::location(&picked);

    assert_redirects_to_dashboard(&browser.get(&callback).await);
    let replay = browser.get(&callback).await;
    assert_eq!(
        replay.status(),
        400,
        "the same callback cannot be used twice"
    );
}

#[tokio::test]
async fn sso_error_or_cancellation_links_nothing() {
    let e = env().await;
    let browser = e.app.browser();
    let res = browser
        .sso_round_trip_with(&e.app, "/auth/login", ALICE, |url| {
            let state = url
                .query_pairs()
                .find(|(k, _)| k == "state")
                .unwrap()
                .1
                .into_owned();
            url.query_pairs_mut()
                .clear()
                .append_pair("error", "access_denied")
                .append_pair("error_description", "The user cancelled")
                .append_pair("state", &state);
        })
        .await;
    assert_eq!(res.status(), 400);
    let body = res.text().await.unwrap();
    assert!(body.contains("The user cancelled") && body.contains("Try again"));
    assert_eq!(e.count("characters").await, 0);
    assert!(e.whoami(&browser).await.is_none());
}

#[tokio::test]
async fn an_invalid_access_token_is_rejected_and_nothing_is_stored() {
    let e = env().await;
    for options in [
        TokenOptions {
            expires_in_secs: Some(-3600),
            ..Default::default()
        },
        TokenOptions {
            audience: Some(vec!["someone-else".into(), "EVE Online".into()]),
            ..Default::default()
        },
        TokenOptions {
            issuer: Some("https://evil.example".into()),
            ..Default::default()
        },
    ] {
        e.app.stub().set_token_options(options.clone());
        let browser = e.app.browser();
        let res = e.login(&browser, ALICE).await;
        assert_eq!(res.status(), 400, "{options:?}");
        assert_eq!(e.count("characters").await, 0, "{options:?}");
        assert!(e.whoami(&browser).await.is_none());
    }
}

#[tokio::test]
async fn a_login_that_lacks_required_scopes_is_rejected() {
    let e = env().await;
    e.app
        .stub_control(
            "scopes",
            json!({"character_id": ALICE, "scopes": ["esi-universe.read_structures.v1"]}),
        )
        .await;
    let browser = e.app.browser();
    let res = e.login(&browser, ALICE).await;
    assert_eq!(res.status(), 400);
    assert!(
        res.text()
            .await
            .unwrap()
            .contains("esi-markets.read_character_orders.v1")
    );
    assert_eq!(e.count("characters").await, 0);
}

#[tokio::test]
async fn an_sso_outage_during_login_links_nothing() {
    let e = env().await;
    e.app.stub_control("fail-next-token", json!({})).await;
    let browser = e.app.browser();
    let res = e.login(&browser, ALICE).await;
    assert_eq!(res.status(), 502);
    assert_eq!(e.count("characters").await, 0);
    // Trying again works.
    assert_redirects_to_dashboard(&e.login(&browser, ALICE).await);
}

// ---- Login resumes an existing account ----

#[tokio::test]
async fn returning_character_signs_in_to_its_account_with_all_characters() {
    let e = env().await;
    let account = e.repo.create_account().await.unwrap();
    for (id, owner) in [
        (ALICE, "owner-hash-90000001"),
        (BOB, "owner-hash-90000002"),
        (CAROL, "owner-hash-90000003"),
    ] {
        e.seed(account, id, owner).await;
    }
    e.app.stores.accounts.mark_needs_relogin(BOB).await.unwrap();

    let browser = e.app.browser();
    assert_redirects_to_dashboard(&e.login(&browser, BOB).await);

    assert_eq!(e.whoami(&browser).await, Some(account));
    assert_eq!(e.repo.list_characters(account).await.unwrap().len(), 3);
    assert_eq!(e.count("accounts").await, 1);

    let bob = e.repo.find_character(BOB).await.unwrap().unwrap();
    assert!(!bob.needs_relogin, "re-login clears the state");
    let tokens = e.repo.load_tokens(BOB).await.unwrap().unwrap();
    assert_ne!(
        tokens.refresh_token.expose_secret(),
        "seeded-refresh",
        "tokens are replaced"
    );
    assert_eq!(bob.name, "Bob Bravo", "the name is refreshed too");
}

#[tokio::test]
async fn logging_in_with_another_linked_character_returns_to_the_same_account() {
    let e = env().await;
    let first = e.app.browser();
    e.login(&first, ALICE).await;
    let account = e.whoami(&first).await.unwrap();
    assert_redirects_to_characters(&e.add(&first, BOB).await);

    // "Sign out" by starting from a fresh browser, then log in with the other character.
    let second = e.app.browser();
    assert_redirects_to_dashboard(&e.login(&second, BOB).await);
    assert_eq!(e.whoami(&second).await, Some(account));
    assert_eq!(e.count("accounts").await, 1);
}

// ---- Character ownership change ----

#[tokio::test]
async fn a_transferred_character_leaves_the_old_account_and_starts_a_new_one() {
    let e = env().await;
    let old_account = e.repo.create_account().await.unwrap();
    e.seed(old_account, ALICE, "the-previous-owner").await;
    e.seed(old_account, BOB, "owner-hash-90000002").await;

    let browser = e.app.browser();
    assert_redirects_to_dashboard(&e.login(&browser, ALICE).await);

    let new_account = e.whoami(&browser).await.unwrap();
    assert_ne!(new_account, old_account);
    let old: Vec<_> = e
        .repo
        .list_characters(old_account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    assert_eq!(old, [BOB], "Alice is gone from the old account");
    let new: Vec<_> = e
        .repo
        .list_characters(new_account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    assert_eq!(new, [ALICE]);
    assert_eq!(e.owner_hash_of(ALICE).await, "owner-hash-90000001");
}

#[tokio::test]
async fn a_transferred_character_leaves_no_fee_data_with_the_new_owner() {
    let e = env().await;
    let old_account = e.repo.create_account().await.unwrap();
    e.seed(old_account, ALICE, "the-previous-owner").await;
    let now = e.app.state.clock.now();
    let fees = &e.app.stores.fees;
    fees.store_skills(ALICE, &[16622], &[4], now, now, None)
        .await
        .unwrap();
    fees.store_standings(
        ALICE,
        &[sekondo::store::StandingInput {
            from_id: 1000035,
            from_type: "npc_corp".to_owned(),
            standing: "1.5".to_owned(),
        }],
        now,
        now,
        None,
    )
    .await
    .unwrap();
    assert!(fees.fee_sync(ALICE, Kind::Skills).await.unwrap().is_some());

    let browser = e.app.browser();
    e.login(&browser, ALICE).await;

    // What the new owner finds: no sync state, no skills and no standings, even towards a station
    // whose owner is known.
    fees.insert_station_owner(60003760, 1000035, now)
        .await
        .unwrap();
    let account = e.whoami(&browser).await.unwrap();
    for kind in Kind::ALL {
        assert!(
            fees.fee_sync(ALICE, kind).await.unwrap().is_none(),
            "{kind:?}"
        );
    }
    let rows = fees.at_location(account, &[ALICE], 60003760).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].accounting, &rows[0].corp_standing), (None, &None));
}

#[tokio::test]
async fn a_transferred_last_character_deletes_the_old_account() {
    let e = env().await;
    let old_account = e.repo.create_account().await.unwrap();
    e.seed(old_account, ALICE, "the-previous-owner").await;

    let browser = e.app.browser();
    e.login(&browser, ALICE).await;
    assert_ne!(e.whoami(&browser).await, Some(old_account));
    assert_eq!(e.count("accounts").await, 1, "the emptied account is gone");
}

#[tokio::test]
async fn ownership_change_is_detected_through_the_stub_control() {
    let e = env().await;
    let first = e.app.browser();
    e.login(&first, ALICE).await;
    let account = e.whoami(&first).await.unwrap();
    e.add(&first, BOB).await;

    e.app
        .stub_control(
            "owner-hash",
            json!({"character_id": ALICE, "owner_hash": "sold"}),
        )
        .await;
    let second = e.app.browser();
    e.login(&second, ALICE).await;

    let new_account = e.whoami(&second).await.unwrap();
    assert_ne!(new_account, account);
    assert_eq!(e.owner_hash_of(ALICE).await, "sold");
    assert_eq!(e.repo.list_characters(account).await.unwrap().len(), 1);
}

// ---- Add character to the current account ----

#[tokio::test]
async fn adding_a_second_character_keeps_the_session_and_account() {
    let e = env().await;
    let browser = e.app.browser();
    e.login(&browser, ALICE).await;
    let account = e.whoami(&browser).await.unwrap();
    let cookie = browser.cookie();

    assert_redirects_to_characters(&e.add(&browser, BOB).await);

    assert_eq!(e.whoami(&browser).await, Some(account));
    let ids: Vec<_> = e
        .repo
        .list_characters(account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    assert_eq!(ids, [ALICE, BOB]);
    assert_eq!(
        browser.cookie(),
        cookie,
        "adding a character does not end or rotate the session"
    );
}

#[tokio::test]
async fn adding_a_character_already_in_the_account_refreshes_it() {
    let e = env().await;
    let browser = e.app.browser();
    e.login(&browser, ALICE).await;
    let account = e.whoami(&browser).await.unwrap();
    let before = e
        .repo
        .load_tokens(ALICE)
        .await
        .unwrap()
        .unwrap()
        .refresh_token;
    e.app
        .stores
        .accounts
        .mark_needs_relogin(ALICE)
        .await
        .unwrap();

    assert_redirects_to_characters(&e.add(&browser, ALICE).await);

    assert_eq!(
        e.repo.list_characters(account).await.unwrap().len(),
        1,
        "no duplicate"
    );
    assert!(
        !e.repo
            .find_character(ALICE)
            .await
            .unwrap()
            .unwrap()
            .needs_relogin
    );
    let after = e
        .repo
        .load_tokens(ALICE)
        .await
        .unwrap()
        .unwrap()
        .refresh_token;
    assert_ne!(
        before.expose_secret(),
        after.expose_secret(),
        "tokens are replaced"
    );
}

#[tokio::test]
async fn adding_a_character_of_another_account_is_rejected_with_a_message() {
    let e = env().await;
    let mine = e.app.browser();
    e.login(&mine, ALICE).await;
    let my_account = e.whoami(&mine).await.unwrap();
    let theirs = e.app.browser();
    e.login(&theirs, BOB).await;
    let their_account = e.whoami(&theirs).await.unwrap();

    assert_redirects_to_characters(&e.add(&mine, BOB).await);

    assert_eq!(
        e.whoami(&mine).await,
        Some(my_account),
        "still signed in to my account"
    );
    assert_eq!(
        e.repo
            .find_character(BOB)
            .await
            .unwrap()
            .unwrap()
            .account_id,
        their_account,
        "not moved"
    );
    assert_eq!(e.repo.list_characters(my_account).await.unwrap().len(), 1);
    let message = mine
        .get(&e.app.url("/test/flash"))
        .await
        .text()
        .await
        .unwrap();
    assert!(
        message.contains("belongs to another Sekondo account") && message.contains("Unlink"),
        "{message}"
    );
}

// ---- Sessions ----

#[tokio::test]
async fn the_session_cookie_changes_on_login_and_the_old_one_stops_working() {
    let e = env().await;
    let browser = e.app.browser();
    // A browser that already has a session (here: an anonymous one that also holds a CSRF token).
    browser.get(&e.app.url("/")).await;
    let before = browser.cookie().expect("session cookie before login");

    e.login(&browser, ALICE).await;
    let after = browser.cookie().expect("session cookie after login");
    assert_ne!(before, after);
    assert!(e.whoami(&browser).await.is_some());

    // The old value no longer authenticates, and cannot be used to reach the new session.
    let attacker = e.app.browser();
    attacker.set_cookie(Some(before));
    assert!(e.whoami(&attacker).await.is_none());
}

#[tokio::test]
async fn login_while_a_fixated_session_exists_does_not_authenticate_the_old_cookie() {
    let e = env().await;
    let victim = e.app.browser();
    victim.get(&e.app.url("/")).await;
    let planted = victim.cookie().unwrap();

    let attacker = e.app.browser();
    attacker.set_cookie(Some(planted.clone()));
    e.login(&victim, ALICE).await;

    assert!(e.whoami(&victim).await.is_some());
    assert!(
        e.whoami(&attacker).await.is_none(),
        "the planted cookie stays anonymous"
    );
}

#[tokio::test]
async fn the_csrf_token_is_renewed_on_sign_in() {
    let e = env().await;
    let browser = e.app.browser();
    let token = |body: String| {
        body.split(r#"<meta name="csrf-token" content=""#)
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned()
    };
    let before = token(browser.get(&e.app.url("/")).await.text().await.unwrap());
    e.login(&browser, ALICE).await;
    let after = token(
        browser
            .get(&e.app.url("/characters"))
            .await
            .text()
            .await
            .unwrap(),
    );
    assert_ne!(before, after);
}
