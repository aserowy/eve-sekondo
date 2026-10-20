mod common;

use common::{AppBuilder, Browser, TestApp};
use sekondo::{config::SecretKey, crypto::TokenCipher, repo::Repo};
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

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
            "character_tokens" => {
                let mut total = 0;
                for id in [ALICE, BOB, 90000003] {
                    total += i64::from(accounts.load_tokens(id).await.unwrap().is_some());
                }
                total
            }
            other => panic!("no count of {other}"),
        }
    }
}

#[tokio::test]
async fn unlinking_one_of_two_characters_deletes_it_and_all_its_data() {
    let e = env().await;
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

#[tokio::test]
async fn unlinking_the_last_character_deletes_the_account_and_signs_out() {
    let e = env().await;
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

#[tokio::test]
async fn without_htmx_the_last_unlink_redirects_to_the_landing_page() {
    let e = env().await;
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

#[tokio::test]
async fn another_accounts_character_cannot_be_unlinked() {
    let e = env().await;
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

#[tokio::test]
async fn an_unknown_character_is_not_found() {
    let e = env().await;
    let (browser, _) = e.alice_and_bob().await;
    let token = e.csrf_token(&browser).await;
    assert_eq!(e.unlink(&browser, &token, 12345).await.status(), 404);
    assert_eq!(e.count("characters").await, 2);
}

#[tokio::test]
async fn unlink_without_a_csrf_token_is_forbidden_and_deletes_nothing() {
    let e = env().await;
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

#[tokio::test]
async fn unlink_with_a_foreign_origin_or_another_sessions_token_is_forbidden() {
    let e = env().await;
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

#[tokio::test]
async fn unlinking_a_character_removes_its_wealth_and_fee_data_and_keeps_the_others() {
    use sekondo::{fee_poller::Kind as FeeKind, store::StandingInput, wealth_poller::Kind};

    let e = env().await;
    let (browser, account) = e.alice_and_bob().await;
    let stores = &e.app.stores;
    let now = e.app.state.clock.now();
    let soon = now + time::Duration::hours(1);
    let station = 60003760;
    for character in [ALICE, BOB] {
        stores
            .wealth
            .store_wallet(character, now, 500, soon, None)
            .await
            .unwrap();
        let asset = sekondo::esi::EsiAsset {
            item_id: character,
            type_id: 34,
            location_id: station,
            location_type: "station".to_owned(),
            location_flag: "Hangar".to_owned(),
            quantity: 1,
            is_singleton: false,
            is_blueprint_copy: false,
        };
        stores
            .wealth
            .store_assets(character, now, &[asset], &[Some(station)], soon, None)
            .await
            .unwrap();
        let transaction = |id: i64, is_buy: bool, price_cents: i64| sekondo::esi::EsiTransaction {
            transaction_id: id,
            bought_at: now - time::Duration::days(1),
            type_id: 34,
            location_id: station,
            quantity: 10,
            unit_price_cents: price_cents,
            is_buy,
            is_personal: true,
        };
        stores
            .wealth
            .store_transactions(
                character,
                now,
                &[
                    transaction(character, true, 10_000),
                    transaction(character + 1000, false, 15_000),
                ],
                soon,
                None,
            )
            .await
            .unwrap();
        // A remembered sell order, with the cost the purchase gives it.
        let mut order = sekondo::esi::EsiOrder {
            order_id: character,
            type_id: 34,
            region_id: 10000002,
            location_id: station,
            is_buy_order: false,
            is_corporation: false,
            price_cents: 20_000,
            volume_remain: 10,
            volume_total: 10,
            min_volume: 1,
            range: "station".to_owned(),
            escrow_cents: None,
            issued: now,
            duration: 90,
        };
        order.issued = now;
        stores
            .orders
            .replace_orders(character, now, &[order], soon, None)
            .await
            .unwrap();
        stores
            .fees
            .store_skills(character, &[16622], &[4], now, soon, None)
            .await
            .unwrap();
        stores
            .fees
            .store_standings(
                character,
                &[StandingInput {
                    from_id: 1000035,
                    from_type: "npc_corp".to_owned(),
                    standing: "1.5".to_owned(),
                }],
                now,
                soon,
                None,
            )
            .await
            .unwrap();
        stores
            .fees
            .store_location(character, Some((station, "station")), now, soon, None)
            .await
            .unwrap();
    }
    let token = e.csrf_token(&browser).await;

    assert_eq!(e.unlink(&browser, &token, BOB).await.status(), 200);

    // Bob's data is gone ...
    for kind in Kind::ALL {
        assert!(
            stores
                .wealth
                .wealth_sync(BOB, kind)
                .await
                .unwrap()
                .is_none(),
            "{kind:?}"
        );
        assert!(
            stores
                .wealth
                .wealth_sync(ALICE, kind)
                .await
                .unwrap()
                .is_some(),
            "{kind:?}"
        );
    }
    for kind in [FeeKind::Skills, FeeKind::Standings] {
        assert!(
            stores.fees.fee_sync(BOB, kind).await.unwrap().is_none(),
            "{kind:?}"
        );
        assert!(
            stores.fees.fee_sync(ALICE, kind).await.unwrap().is_some(),
            "{kind:?}"
        );
    }
    assert!(stores.orders.order_sync(BOB).await.unwrap().is_none());
    assert!(
        !stores
            .wealth
            .any_transaction_stored(&[BOB, BOB + 1000])
            .await
            .unwrap()
    );
    assert!(
        stores
            .wealth
            .any_transaction_stored(&[ALICE, ALICE + 1000])
            .await
            .unwrap()
    );
    let range = (now - time::Duration::days(2), now);
    let trades = stores
        .trades
        .list_trades(account, &[ALICE, BOB], range.0, range.1)
        .await
        .unwrap();
    assert_eq!(
        trades.iter().map(|t| t.character_id).collect::<Vec<_>>(),
        [ALICE, ALICE],
        "Alice's purchase and sale are all that is left"
    );
    // ... and what hangs off it: the remembered order with its cost, the fees' inputs, the location.
    let sale = trades.iter().find(|t| t.is_sale).unwrap();
    assert_eq!(sale.covered_units, Some(10), "Alice's own cost is intact");
    let orders = stores
        .orders
        .list_orders(
            account,
            &[ALICE, BOB],
            sekondo::orders::Side::Sell,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        orders.iter().map(|o| o.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
    assert_eq!(orders[0].covered_units, Some(10));
    let fees = stores.fees.for_characters(&[ALICE, BOB]).await.unwrap();
    assert_eq!(
        fees.iter().map(|f| f.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
    let current = stores.wealth.current(account, &[ALICE, BOB]).await.unwrap();
    assert_eq!(
        (current.wallet_cents, current.inventory_cents),
        (500, 0),
        "one wallet, and no prices to value the asset"
    );
    let rows = stores
        .wealth
        .inventory_rows(account, &[ALICE, BOB], Default::default())
        .await
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
    let at = stores
        .fees
        .at_fee_location(account, &[ALICE, BOB])
        .await
        .unwrap();
    assert_eq!(
        at.iter().map(|f| f.row.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
}
