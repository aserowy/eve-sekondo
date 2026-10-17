mod common;

use sekondo::{clock::Clock, esi::EsiError};
use serde_json::json;
use time::Duration;

#[tokio::test]
async fn orders_are_parsed_with_prices_in_cents() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);

    let response = h.client.orders(90000001, &token).await.unwrap();

    assert_eq!(response.body.len(), 3);
    let sell = response.body.iter().find(|o| o.order_id == 5001).unwrap();
    assert_eq!(sell.type_id, 34);
    assert_eq!(sell.price_cents, 150_000_000);
    assert_eq!((sell.volume_remain, sell.volume_total), (10, 20));
    assert!(!sell.is_buy_order && !sell.is_corporation);
    assert_eq!(sell.escrow_cents, None);
    let buy = response.body.iter().find(|o| o.order_id == 5003).unwrap();
    assert!(buy.is_buy_order);
    assert_eq!(buy.escrow_cents, Some(250_000_000));
    assert_eq!(buy.location_id, 1000000000001);
}

#[tokio::test]
async fn expires_is_reported_on_the_local_clock() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.control("esi-expires", json!({"seconds": 300})).await;

    let response = h.client.orders(90000001, &token).await.unwrap();

    let ahead = response.expires_at.unwrap() - h.clock.now();
    assert!(
        ahead >= Duration::seconds(299) && ahead <= Duration::seconds(300),
        "{ahead}"
    );
}

#[tokio::test]
async fn names_are_parsed() {
    let h = common::esi_harness().await;
    let names = h.client.names(&[34, 10000002]).await.unwrap();
    assert_eq!(names.len(), 2);
    let tritanium = names.iter().find(|n| n.id == 34).unwrap();
    assert_eq!(tritanium.name, "Tritanium");
    assert_eq!(tritanium.category, "inventory_type");
}

#[tokio::test]
async fn structures_are_parsed() {
    let h = common::esi_harness().await;
    let structure = h
        .client
        .structure(1000000000001, &h.token(90000001))
        .await
        .unwrap();
    assert_eq!(structure.name, "Alpha Keepstar");
}

#[tokio::test]
async fn every_request_names_sekondo_its_version_url_and_contact() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.client.orders(90000001, &token).await.unwrap();
    h.client.names(&[34]).await.unwrap();
    h.client.structure(1000000000001, &token).await.unwrap();

    let requests = h.stub.esi_requests();
    assert_eq!(requests.len(), 3);
    let expected = format!(
        "Sekondo/{} (+{}; tests@example.com)",
        env!("CARGO_PKG_VERSION"),
        h.config.public_base_url.as_str().trim_end_matches('/')
    );
    for request in &requests {
        assert_eq!(request.user_agent.as_deref(), Some(expected.as_str()));
        assert_eq!(
            request.compatibility_date.as_deref(),
            Some(sekondo::esi::COMPATIBILITY_DATE)
        );
    }
}

#[tokio::test]
async fn the_contact_is_left_out_when_not_configured() {
    let h = common::esi_harness_with(None).await;
    h.client.names(&[34]).await.unwrap();
    let agent = h.stub.esi_requests()[0].user_agent.clone().unwrap();
    assert!(!agent.contains(';'), "{agent}");
    assert!(agent.starts_with("Sekondo/"), "{agent}");
}

#[tokio::test]
async fn forbidden_not_found_and_server_errors_are_told_apart() {
    let h = common::esi_harness().await;

    let foreign = h.client.orders(90000001, &h.token(90000002)).await;
    assert!(matches!(foreign, Err(EsiError::Forbidden)), "{foreign:?}");

    let unknown = h.client.names(&[34, 999999999]).await;
    assert!(matches!(unknown, Err(EsiError::NotFound)), "{unknown:?}");

    h.control("esi-fail-next", json!({"status": 503})).await;
    let broken = h.client.names(&[34]).await;
    assert!(matches!(broken, Err(EsiError::Temporary(_))), "{broken:?}");
}

#[tokio::test]
async fn an_error_limit_response_pauses_all_requests_until_its_reset() {
    let h = common::esi_harness().await;
    h.control(
        "esi-fail-next",
        json!({"status": 420, "headers": {"x-esi-error-limit-remain": "0", "x-esi-error-limit-reset": "30"}}),
    )
    .await;

    let first = h.client.names(&[34]).await;
    let Err(EsiError::Deferred { until }) = first else {
        panic!("{first:?}");
    };
    assert_eq!(until - h.clock.now(), Duration::seconds(30));
    assert_eq!(h.stub.esi_calls("names"), 1);

    // Nothing is sent while blocked, on any route.
    assert!(matches!(
        h.client.names(&[34]).await,
        Err(EsiError::Deferred { .. })
    ));
    assert!(matches!(
        h.client.orders(90000001, &h.token(90000001)).await,
        Err(EsiError::Deferred { .. })
    ));
    assert_eq!(h.stub.esi_calls("names"), 1);
    assert_eq!(h.stub.esi_calls("orders"), 0);

    h.clock.advance(Duration::seconds(31));
    assert!(h.client.names(&[34]).await.is_ok());
    assert_eq!(h.stub.esi_calls("names"), 2);
}

#[tokio::test]
async fn a_nearly_exhausted_error_limit_pauses_requests_even_on_a_normal_failure() {
    let h = common::esi_harness().await;
    h.control(
        "esi-fail-next",
        json!({"status": 500, "headers": {"x-esi-error-limit-remain": "5", "x-esi-error-limit-reset": "30"}}),
    )
    .await;

    assert!(matches!(
        h.client.names(&[34]).await,
        Err(EsiError::Temporary(_))
    ));
    assert!(matches!(
        h.client.names(&[34]).await,
        Err(EsiError::Deferred { .. })
    ));
    assert_eq!(h.stub.esi_calls("names"), 1);

    h.clock.advance(Duration::seconds(30));
    assert!(h.client.names(&[34]).await.is_ok());
}

#[tokio::test]
async fn a_429_pauses_only_the_rate_limited_group_until_retry_after() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.control(
        "esi-fail-next",
        json!({"status": 429, "headers": {"retry-after": "60", "x-ratelimit-group": "char-market"}}),
    )
    .await;

    let limited = h.client.orders(90000001, &token).await;
    let Err(EsiError::Deferred { until }) = limited else {
        panic!("{limited:?}");
    };
    assert_eq!(until - h.clock.now(), Duration::seconds(60));

    assert!(matches!(
        h.client.orders(90000001, &token).await,
        Err(EsiError::Deferred { .. })
    ));
    assert_eq!(h.stub.esi_calls("orders"), 1, "no request during the block");
    assert!(h.client.names(&[34]).await.is_ok(), "other routes go on");

    h.clock.advance(Duration::seconds(61));
    assert!(h.client.orders(90000001, &token).await.is_ok());
    assert_eq!(h.stub.esi_calls("orders"), 2);
}

#[tokio::test]
async fn a_sell_order_without_the_buy_flag_is_accepted_as_real_esi_sends_it() {
    let order: sekondo::esi::EsiOrder = serde_json::from_value(json!({
        "order_id": 9, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "price": 1.5, "volume_remain": 1, "volume_total": 2,
        "range": "region", "issued": "2026-10-01T12:00:00Z", "duration": 90
    }))
    .unwrap();
    assert!(!order.is_buy_order);
}

#[tokio::test]
async fn a_sell_order_without_min_volume_is_accepted_as_esi_sends_it() {
    let h = common::esi_harness().await;
    let order = json!({
        "order_id": 9, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "is_buy_order": false, "price": 1.5, "volume_remain": 1, "volume_total": 2,
        "range": "region", "issued": "2026-10-01T12:00:00Z", "duration": 90
    });
    h.control(
        "orders",
        json!({"character_id": 90000001, "orders": [order]}),
    )
    .await;

    let response = h.client.orders(90000001, &h.token(90000001)).await.unwrap();

    assert_eq!(response.body[0].min_volume, 1);
    assert!(!response.body[0].is_corporation);
}

fn market_order(id: i64, type_id: i64, buy: bool, price: f64) -> serde_json::Value {
    json!({
        "order_id": id, "type_id": type_id, "location_id": 60003760, "system_id": 30000142,
        "is_buy_order": buy, "price": price, "range": "station",
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": 3, "volume_total": 5,
    })
}

#[tokio::test]
async fn a_one_page_market_answer_is_returned_whole_without_a_token() {
    let h = common::esi_harness().await;
    h.control(
        "market-orders",
        json!({"region_id": 10000002, "orders": [
            market_order(1, 34, false, 10.25), market_order(2, 34, true, 9.5),
        ]}),
    )
    .await;

    let response = h.client.market_orders(10000002, 34).await.unwrap();

    assert_eq!(response.body.len(), 2);
    let sell = response.body.iter().find(|o| o.order_id == 1).unwrap();
    assert_eq!(sell.price_cents, 1025);
    assert_eq!((sell.location_id, sell.system_id), (60003760, 30000142));
    assert!(!sell.is_buy_order);
    assert!(
        response
            .body
            .iter()
            .find(|o| o.order_id == 2)
            .unwrap()
            .is_buy_order
    );
    assert_eq!(h.stub.esi_calls("market"), 1);
}

#[tokio::test]
async fn a_three_page_market_answer_is_returned_whole() {
    let h = common::esi_harness().await;
    let orders: Vec<_> = (1..=5)
        .map(|id| market_order(id, 34, false, 10.0))
        .collect();
    h.control(
        "market-orders",
        json!({"region_id": 10000002, "orders": orders, "page_size": 2}),
    )
    .await;

    let response = h.client.market_orders(10000002, 34).await.unwrap();

    let mut ids: Vec<i64> = response.body.iter().map(|o| o.order_id).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
    assert_eq!(h.stub.esi_calls("market"), 3);
}

#[tokio::test]
async fn only_the_requested_item_is_returned() {
    let h = common::esi_harness().await;
    h.control(
        "market-orders",
        json!({"region_id": 10000002, "orders": [
            market_order(1, 34, false, 10.0), market_order(2, 35, false, 10.0),
        ]}),
    )
    .await;

    let response = h.client.market_orders(10000002, 35).await.unwrap();

    assert_eq!(
        response.body.iter().map(|o| o.order_id).collect::<Vec<_>>(),
        [2]
    );
}

#[tokio::test]
async fn the_expiry_of_the_first_page_is_reported() {
    let h = common::esi_harness().await;
    h.control("esi-expires", json!({"seconds": 480})).await;

    let response = h.client.market_orders(10000002, 34).await.unwrap();

    let ahead = response.expires_at.unwrap() - h.clock.now();
    assert!(
        ahead >= Duration::seconds(479) && ahead <= Duration::seconds(480),
        "{ahead}"
    );
}

#[tokio::test]
async fn a_failing_market_page_is_a_temporary_failure() {
    let h = common::esi_harness().await;
    h.control("esi-fail-next", json!({"route": "market", "status": 503}))
        .await;
    let failed = h.client.market_orders(10000002, 34).await;
    assert!(matches!(failed, Err(EsiError::Temporary(_))), "{failed:?}");
}

#[tokio::test]
async fn the_gate_pauses_the_market_route_without_pausing_the_character_orders() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.control(
        "esi-fail-next",
        json!({"route": "market", "status": 429, "headers": {"retry-after": "60", "x-ratelimit-group": "market-order"}}),
    )
    .await;

    let limited = h.client.market_orders(10000002, 34).await;
    assert!(
        matches!(limited, Err(EsiError::Deferred { .. })),
        "{limited:?}"
    );
    assert!(matches!(
        h.client.market_orders(10000002, 34).await,
        Err(EsiError::Deferred { .. })
    ));
    assert_eq!(h.stub.esi_calls("market"), 1, "no request during the block");
    assert!(h.client.orders(90000001, &token).await.is_ok());

    h.clock.advance(Duration::seconds(61));
    assert!(h.client.market_orders(10000002, 34).await.is_ok());
}

#[tokio::test]
async fn a_market_error_limit_does_not_exist_but_the_global_one_still_applies() {
    // The market route sends no error-limit headers; an exhausted error limit seen on another
    // route still pauses it, as everything else.
    let h = common::esi_harness().await;
    h.control(
        "esi-fail-next",
        json!({"route": "names", "status": 420, "headers": {"x-esi-error-limit-reset": "30"}}),
    )
    .await;
    let _ = h.client.names(&[34]).await;

    assert!(matches!(
        h.client.market_orders(10000002, 34).await,
        Err(EsiError::Deferred { .. })
    ));
    assert_eq!(h.stub.esi_calls("market"), 0);
}

fn asset(id: i64, copy: bool) -> serde_json::Value {
    let mut asset = json!({
        "item_id": id, "type_id": 34, "location_id": 60003760, "location_type": "station",
        "location_flag": "Hangar", "quantity": 7, "is_singleton": false,
    });
    if copy {
        asset["is_blueprint_copy"] = json!(true);
    }
    asset
}

#[tokio::test]
async fn the_wallet_balance_is_read_in_cents_with_its_expiry() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.control("esi-expires", json!({"seconds": 300})).await;

    let response = h.client.wallet(90000001, &token).await.unwrap();
    assert_eq!(response.body, 250_000_000);
    let ahead = response.expires_at.unwrap() - h.clock.now();
    assert!(ahead >= Duration::seconds(299), "{ahead}");

    h.control(
        "wallet",
        json!({"character_id": 90000001, "balance": 12.34}),
    )
    .await;
    assert_eq!(h.client.wallet(90000001, &token).await.unwrap().body, 1234);
}

#[tokio::test]
async fn assets_are_read_from_every_page() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.control(
        "assets",
        json!({"character_id": 90000001, "page_size": 2,
               "assets": [asset(1, false), asset(2, false), asset(3, true), asset(4, false), asset(5, false)]}),
    )
    .await;

    let response = h.client.assets(90000001, &token).await.unwrap();

    let ids: Vec<i64> = response.body.iter().map(|a| a.item_id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
    assert_eq!(h.stub.esi_calls("assets"), 3);
    assert!(response.expires_at.is_some());
    let copies: Vec<bool> = response.body.iter().map(|a| a.is_blueprint_copy).collect();
    assert_eq!(
        copies,
        [false, false, true, false, false],
        "missing means false"
    );
    let first = &response.body[0];
    assert_eq!((first.type_id, first.quantity), (34, 7));
    assert_eq!(
        (first.location_id, first.location_type.as_str()),
        (60003760, "station")
    );
    assert!(!first.is_singleton);
}

#[tokio::test]
async fn prices_are_read_in_cents_without_a_token_and_may_lack_an_average() {
    let h = common::esi_harness().await;

    let response = h.client.prices().await.unwrap();

    let price = |type_id| {
        response
            .body
            .iter()
            .find(|p| p.type_id == type_id)
            .unwrap()
            .average_price_cents
    };
    assert_eq!(price(34), Some(550));
    assert_eq!(price(35), Some(1025));
    assert_eq!(price(36), None);
    assert!(response.expires_at.is_some());
}

#[tokio::test]
async fn the_new_calls_map_errors_like_the_orders_call() {
    let h = common::esi_harness().await;
    let foreign = h.token(90000002);
    assert!(matches!(
        h.client.wallet(90000001, &foreign).await,
        Err(EsiError::Forbidden)
    ));
    assert!(matches!(
        h.client.assets(90000001, &foreign).await,
        Err(EsiError::Forbidden)
    ));

    let token = h.token(90000001);
    for route in ["wallet", "assets", "prices"] {
        h.control("esi-fail-next", json!({"route": route, "status": 503}))
            .await;
        let broken = match route {
            "wallet" => h.client.wallet(90000001, &token).await.map(|_| ()),
            "assets" => h.client.assets(90000001, &token).await.map(|_| ()),
            _ => h.client.prices().await.map(|_| ()),
        };
        assert!(
            matches!(broken, Err(EsiError::Temporary(_))),
            "{route}: {broken:?}"
        );
    }
}

#[tokio::test]
async fn a_429_on_the_wallet_pauses_the_wallet_group_and_a_420_pauses_everything() {
    let h = common::esi_harness().await;
    let token = h.token(90000001);
    h.control(
        "esi-fail-next",
        json!({"route": "wallet", "status": 429,
               "headers": {"retry-after": "30", "x-ratelimit-group": "char-wallet"}}),
    )
    .await;
    let Err(EsiError::Deferred { until }) = h.client.wallet(90000001, &token).await else {
        panic!("expected a deferral");
    };
    assert_eq!(until - h.clock.now(), Duration::seconds(30));
    assert!(matches!(
        h.client.wallet(90000001, &token).await,
        Err(EsiError::Deferred { .. })
    ));
    assert_eq!(h.stub.esi_calls("wallet"), 1, "nothing sent while paused");
    // Another group keeps working.
    assert!(h.client.assets(90000001, &token).await.is_ok());

    h.control(
        "esi-fail-next",
        json!({"route": "prices", "status": 420,
               "headers": {"x-esi-error-limit-remain": "0", "x-esi-error-limit-reset": "30"}}),
    )
    .await;
    assert!(matches!(
        h.client.prices().await,
        Err(EsiError::Deferred { .. })
    ));
    assert!(matches!(
        h.client.assets(90000001, &token).await,
        Err(EsiError::Deferred { .. })
    ));
}
