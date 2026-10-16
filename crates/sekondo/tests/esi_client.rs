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
