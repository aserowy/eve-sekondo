//! Every answer the stub can give for a contracted call satisfies its contract.
//!
//! This test does not need a verified stub: `esi-verify` runs it before it verifies anything.

mod common;

use common::Fixture;
use esi_contract::{ViolationKind, contract, coverage, jwt_payload};
use serde_json::{Value, json};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const STATION: i64 = 60003760;
const STRUCTURE: i64 = 1000000000001;

/// What an answer looked like, ready to be checked.
struct Answer {
    status: u16,
    headers: http::HeaderMap,
    body: Option<Value>,
}

impl Answer {
    async fn of(response: reqwest::Response) -> Self {
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let text = response.text().await.unwrap();
        Self {
            status,
            headers,
            body: serde_json::from_str(&text).ok(),
        }
    }
}

/// Fails with every violation (and nothing else) when the answer breaks the variant.
fn conforms(call: &str, variant: &str, answer: &Answer) {
    let violations = esi_contract::check(
        &contract(call),
        variant,
        Some(answer.status),
        &answer.headers,
        answer.body.as_ref(),
    );
    assert!(
        violations.is_empty(),
        "{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Like `conforms`, for an answer that may have no element for the variant.
fn conforms_where_present(call: &str, variant: &str, answer: &Answer) -> bool {
    let violations = esi_contract::check(
        &contract(call),
        variant,
        Some(answer.status),
        &answer.headers,
        answer.body.as_ref(),
    );
    if violations
        .iter()
        .all(|v| v.kind == ViolationKind::NothingToCheck)
    {
        return violations.is_empty();
    }
    panic!(
        "{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn conforms_body(call: &str, variant: &str, body: &Value) {
    let violations = esi_contract::check(
        &contract(call),
        variant,
        None,
        &http::HeaderMap::new(),
        Some(body),
    );
    assert!(
        violations.is_empty(),
        "{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

async fn control(f: &Fixture, path: &str, body: Value) {
    let res = f
        .client
        .post(f.url(&format!("/_stub/{path}")))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204, "control {path}");
}

async fn get(f: &Fixture, path: &str, token: Option<&str>) -> Answer {
    let mut request = f.client.get(f.url(path));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    Answer::of(request.send().await.unwrap()).await
}

async fn orders_of(f: &Fixture, character: i64) -> Answer {
    let token = f.stub.issue_access_token(character).unwrap();
    get(f, &format!("/characters/{character}/orders"), Some(&token)).await
}

async fn names(f: &Fixture, ids: &[i64]) -> Answer {
    Answer::of(
        f.client
            .post(f.url("/universe/names"))
            .json(&ids)
            .send()
            .await
            .unwrap(),
    )
    .await
}

fn query_of(location: &str) -> Value {
    let url = url::Url::parse(location).unwrap();
    Value::Object(
        url.query_pairs()
            .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
            .collect(),
    )
}

fn order(id: i64, buy: bool, location: i64) -> Value {
    let mut order = json!({
        "order_id": id, "type_id": 34, "region_id": 10000002, "location_id": location,
        "is_corporation": false, "price": 10.5,
        "volume_remain": 1, "volume_total": 2, "range": "station",
        "issued": "2026-10-11T12:00:00Z", "duration": 90,
    });
    if buy {
        order["is_buy_order"] = json!(true);
        order["min_volume"] = json!(1);
        order["escrow"] = json!(10.5);
    }
    order
}

async fn market_of(f: &Fixture, region: i64, query: &str) -> Answer {
    get(f, &format!("/markets/{region}/orders?{query}"), None).await
}

fn market_order(id: i64, buy: bool) -> Value {
    json!({
        "order_id": id, "type_id": 34, "location_id": STATION, "system_id": 30000142,
        "is_buy_order": buy, "price": 10.5, "range": if buy { "region" } else { "station" },
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": 3, "volume_total": 5,
    })
}

#[test]
fn every_stub_route_has_a_contract_and_every_contract_a_route() {
    assert_eq!(coverage(eve_stub::ROUTES), Vec::<String>::new());
}

#[tokio::test]
async fn the_authorization_redirect_carries_what_the_callback_contract_says() {
    let f = common::start_unverified().await;
    conforms_body(
        "authorization",
        "request",
        &Value::Object(
            f.authorize_query("a-state")
                .into_iter()
                .map(|(k, v)| (k.to_owned(), Value::String(v)))
                .collect(),
        ),
    );

    let mut query = f.authorize_query("a-state");
    query.push(("character_id", ALICE.to_string()));
    let res = f
        .client
        .get(f.url("/v2/oauth/authorize/select"))
        .query(&query)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let location = res.headers()["location"].to_str().unwrap().to_owned();
    conforms_body("authorization", "callback", &query_of(&location));
}

#[tokio::test]
async fn the_token_answers_and_the_issued_jwt_conform() {
    let f = common::start_unverified().await;
    let (code, _) = f.pick(ALICE, "s").await;

    let first = Answer::of(
        f.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", common::VERIFIER),
            ("redirect_uri", common::REDIRECT_URI),
        ])
        .await,
    )
    .await;
    conforms("token", "authorization-code", &first);
    let body = first.body.unwrap();
    let claims = jwt_payload(body["access_token"].as_str().unwrap()).unwrap();
    conforms_body("token", "access-token-claims", &claims);

    let refreshed = Answer::of(
        f.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", body["refresh_token"].as_str().unwrap()),
        ])
        .await,
    )
    .await;
    conforms("token", "refresh", &refreshed);
}

#[tokio::test]
async fn a_token_with_several_scopes_and_with_one_conform() {
    let f = common::start_unverified().await;
    for scopes in [
        vec!["esi-markets.read_character_orders.v1"],
        vec![
            "esi-markets.read_character_orders.v1",
            "esi-universe.read_structures.v1",
        ],
    ] {
        control(&f, "scopes", json!({"character_id": BOB, "scopes": scopes})).await;
        let token = f.stub.issue_access_token(BOB).unwrap();
        conforms_body(
            "token",
            "access-token-claims",
            &jwt_payload(&token).unwrap(),
        );
    }
}

#[tokio::test]
async fn the_jwks_document_conforms() {
    let f = common::start_unverified().await;
    conforms("jwks", "rsa-keys", &get(&f, "/oauth/jwks", None).await);
}

#[tokio::test]
async fn the_default_orders_conform_as_sell_and_buy() {
    let f = common::start_unverified().await;
    let answer = orders_of(&f, ALICE).await;
    conforms("orders", "sell-orders", &answer);
    conforms("orders", "buy-orders", &answer);
}

#[tokio::test]
async fn sell_and_buy_orders_in_stations_and_structures_conform() {
    let f = common::start_unverified().await;
    control(
        &f,
        "orders",
        json!({"character_id": BOB, "orders": [
            order(1, false, STATION),
            order(2, false, STRUCTURE),
            order(3, true, STATION),
            order(4, true, STRUCTURE),
        ]}),
    )
    .await;
    let answer = orders_of(&f, BOB).await;
    conforms("orders", "sell-orders", &answer);
    conforms("orders", "buy-orders", &answer);
}

#[tokio::test]
async fn a_character_without_orders_conforms_to_nothing_but_has_no_violation_but_emptiness() {
    let f = common::start_unverified().await;
    let answer = orders_of(&f, 90000003).await;
    assert!(!conforms_where_present("orders", "sell-orders", &answer));
    assert!(!conforms_where_present("orders", "buy-orders", &answer));
}

#[tokio::test]
async fn market_orders_conform_on_every_page_and_when_empty() {
    let f = common::start_unverified().await;
    let empty = market_of(&f, 10000002, "order_type=all&type_id=34").await;
    conforms("market-orders", "orders", &empty);

    control(
        &f,
        "market-orders",
        json!({"region_id": 10000002, "page_size": 1,
               "orders": [market_order(1, false), market_order(2, true)]}),
    )
    .await;
    for page in 1..=2 {
        let answer = market_of(
            &f,
            10000002,
            &format!("order_type=all&type_id=34&page={page}"),
        )
        .await;
        assert_eq!(answer.headers["x-pages"], "2");
        conforms("market-orders", "orders", &answer);
    }
}

#[tokio::test]
async fn a_market_fixture_without_a_system_fails_the_conformance_check() {
    let f = common::start_unverified().await;
    let mut order = market_order(1, false);
    order.as_object_mut().unwrap().remove("system_id");
    control(
        &f,
        "market-orders",
        json!({"region_id": 10000002, "orders": [order]}),
    )
    .await;

    let answer = market_of(&f, 10000002, "order_type=all&type_id=34").await;

    let violations = esi_contract::check(
        &contract("market-orders"),
        "orders",
        Some(answer.status),
        &answer.headers,
        answer.body.as_ref(),
    );
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].to_string().contains("system_id"),
        "{}",
        violations[0]
    );
}

#[tokio::test]
async fn names_conform_when_found_and_when_one_is_unknown() {
    let f = common::start_unverified().await;
    conforms(
        "names",
        "found",
        &names(&f, &[34, 10000002, 60003760]).await,
    );
    let unknown = names(&f, &[34, 999999999]).await;
    assert_eq!(unknown.status, 404);
    conforms("names", "unknown-id", &unknown);
}

#[tokio::test]
async fn structures_conform_when_found_and_when_refused() {
    let f = common::start_unverified().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let path = format!("/universe/structures/{STRUCTURE}");

    conforms("structures", "found", &get(&f, &path, Some(&token)).await);

    control(
        &f,
        "structure-access",
        json!({"structure_id": STRUCTURE, "allowed": false}),
    )
    .await;
    let refused = get(&f, &path, Some(&token)).await;
    assert_eq!(refused.status, 403);
    conforms("structures", "forbidden", &refused);
}

#[tokio::test]
async fn injected_failures_conform_on_every_esi_route() {
    let f = common::start_unverified().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let structure = format!("/universe/structures/{STRUCTURE}");
    let orders = format!("/characters/{ALICE}/orders");
    let market = "/markets/10000002/orders?order_type=all&type_id=34";
    let wallet = format!("/characters/{ALICE}/wallet");
    let assets = format!("/characters/{ALICE}/assets");
    let skills = format!("/characters/{ALICE}/skills");
    let standings = format!("/characters/{ALICE}/standings");

    let failures: [(u16, &str, Value); 3] = [
        (
            420,
            "error-limited",
            json!({"x-esi-error-limit-reset": "30"}),
        ),
        (
            429,
            "rate-limited",
            json!({"retry-after": "60", "x-ratelimit-group": "char-market"}),
        ),
        (503, "server-error", json!({})),
    ];
    for (status, variant, headers) in failures {
        for (route, call) in [
            ("orders", "orders"),
            ("names", "names"),
            ("structures", "structures"),
            ("market", "market-orders"),
            ("wallet", "wallet"),
            ("assets", "assets"),
            ("prices", "prices"),
            ("skills", "skills"),
            ("standings", "standings"),
            ("stations", "stations"),
        ] {
            control(
                &f,
                "esi-fail-next",
                json!({"route": route, "status": status, "headers": headers}),
            )
            .await;
            let answer = match route {
                "orders" => get(&f, &orders, Some(&token)).await,
                "names" => names(&f, &[34]).await,
                "market" => get(&f, market, None).await,
                "wallet" => get(&f, &wallet, Some(&token)).await,
                "assets" => get(&f, &assets, Some(&token)).await,
                "prices" => get(&f, "/markets/prices", None).await,
                "skills" => get(&f, &skills, Some(&token)).await,
                "standings" => get(&f, &standings, Some(&token)).await,
                "stations" => get(&f, &format!("/universe/stations/{STATION}"), None).await,
                _ => get(&f, &structure, Some(&token)).await,
            };
            assert_eq!(answer.status, status, "{route}");
            conforms(call, variant, &answer);
        }
    }
}

async fn wallet_of(f: &Fixture, character: i64, token: Option<&str>) -> Answer {
    get(f, &format!("/characters/{character}/wallet"), token).await
}

async fn assets_page(f: &Fixture, character: i64, token: &str, page: usize) -> Answer {
    get(
        f,
        &format!("/characters/{character}/assets?page={page}"),
        Some(token),
    )
    .await
}

fn asset(id: i64) -> Value {
    json!({
        "item_id": id, "type_id": 34, "location_id": STATION, "location_type": "station",
        "location_flag": "Hangar", "quantity": 5, "is_singleton": false,
    })
}

#[tokio::test]
async fn the_wallet_answers_conform_with_and_without_the_scope() {
    let f = common::start_unverified().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    conforms(
        "wallet",
        "balance",
        &wallet_of(&f, ALICE, Some(&token)).await,
    );

    let narrow = f
        .stub
        .issue_token(
            ALICE,
            &eve_stub::TokenOptions {
                scopes: Some(vec!["esi-universe.read_structures.v1".to_owned()]),
                ..eve_stub::TokenOptions::default()
            },
        )
        .unwrap();
    let forbidden = wallet_of(&f, ALICE, Some(&narrow)).await;
    assert_eq!(forbidden.status, 403);
    conforms("wallet", "forbidden", &forbidden);
}

#[tokio::test]
async fn the_asset_answers_conform_on_every_page_and_without_the_scope() {
    let f = common::start_unverified().await;
    control(
        &f,
        "assets",
        json!({"character_id": ALICE, "page_size": 1, "assets": [asset(1), asset(2)]}),
    )
    .await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    for page in 1..=2 {
        let answer = assets_page(&f, ALICE, &token, page).await;
        assert_eq!(answer.headers["x-pages"], "2");
        conforms("assets", "items", &answer);
    }

    let narrow = f
        .stub
        .issue_token(
            ALICE,
            &eve_stub::TokenOptions {
                scopes: Some(vec!["esi-universe.read_structures.v1".to_owned()]),
                ..eve_stub::TokenOptions::default()
            },
        )
        .unwrap();
    let forbidden = assets_page(&f, ALICE, &narrow, 1).await;
    assert_eq!(forbidden.status, 403);
    conforms("assets", "forbidden", &forbidden);
}

#[tokio::test]
async fn the_market_prices_conform_with_and_without_an_average() {
    let f = common::start_unverified().await;
    let answer = get(&f, "/markets/prices", None).await;
    conforms("prices", "list", &answer);
    let items = answer.body.unwrap();
    assert!(
        items
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.get("average_price").is_none()),
        "the fixture has an item without an average"
    );
}

fn narrow_token(f: &Fixture, character: i64) -> String {
    f.stub
        .issue_token(
            character,
            &eve_stub::TokenOptions {
                scopes: Some(vec!["esi-universe.read_structures.v1".to_owned()]),
                ..eve_stub::TokenOptions::default()
            },
        )
        .unwrap()
}

#[tokio::test]
async fn the_skill_answers_conform_with_and_without_the_scope() {
    let f = common::start_unverified().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let path = format!("/characters/{ALICE}/skills");
    conforms("skills", "levels", &get(&f, &path, Some(&token)).await);

    let forbidden = get(&f, &path, Some(&narrow_token(&f, ALICE))).await;
    assert_eq!(forbidden.status, 403);
    conforms("skills", "forbidden", &forbidden);
}

#[tokio::test]
async fn a_skills_fixture_without_the_active_level_fails_the_conformance_check() {
    let f = common::start_unverified().await;
    control(
        &f,
        "skills",
        json!({"character_id": ALICE, "skills": [{"skill_id": 16622, "trained_skill_level": 5}]}),
    )
    .await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let answer = get(&f, &format!("/characters/{ALICE}/skills"), Some(&token)).await;

    let violations = esi_contract::check(
        &contract("skills"),
        "levels",
        Some(answer.status),
        &answer.headers,
        answer.body.as_ref(),
    );
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].to_string().contains("active_skill_level"),
        "{}",
        violations[0]
    );
}

#[tokio::test]
async fn the_standing_answers_conform_with_and_without_the_scope() {
    let f = common::start_unverified().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let path = format!("/characters/{ALICE}/standings");
    conforms("standings", "list", &get(&f, &path, Some(&token)).await);

    let forbidden = get(&f, &path, Some(&narrow_token(&f, ALICE))).await;
    assert_eq!(forbidden.status, 403);
    conforms("standings", "forbidden", &forbidden);
}

#[tokio::test]
async fn a_station_conforms() {
    let f = common::start_unverified().await;
    conforms(
        "stations",
        "station",
        &get(&f, &format!("/universe/stations/{STATION}"), None).await,
    );
}
