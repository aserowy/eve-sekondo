//! What the Opportunities page keeps and reads: the account's filters, the station sync and the
//! account's own orders at the station.

use sekondo::{
    opportunities::settings::Settings,
    store::{StoreError, Stores},
};
use time::{Duration, macros::datetime};

use super::{
    Case,
    support::{REGION, STATION, account_with, boxed, buy_order, case, now, sell_order},
};

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "opportunities/an-account-without-filters-has-none-stored",
            |s| boxed(no_settings(s)),
        ),
        case(
            "opportunities/saved-filters-come-back-and-are-replaced",
            |s| boxed(settings_round_trip(s)),
        ),
        case("opportunities/filters-belong-to-their-account", |s| {
            boxed(settings_per_account(s))
        }),
        case("opportunities/the-station-sync-lists-what-ran", |s| {
            boxed(station_sync(s))
        }),
        case(
            "opportunities/own-orders-at-the-station-are-the-items-the-characters-trade",
            |s| boxed(own_orders(s)),
        ),
        case(
            "opportunities/stats-without-book-fields-are-not-listed",
            |s| boxed(stats_need_book_fields(s)),
        ),
    ]
}

async fn no_settings(stores: &Stores) {
    let account = account_with(stores, &[1]).await;

    assert!(
        stores
            .opportunities
            .load_settings(account)
            .await
            .unwrap()
            .is_none()
    );
    let loaded = Settings::load(&*stores.opportunities, account)
        .await
        .unwrap();
    assert_eq!(loaded, Settings::default());
}

async fn settings_round_trip(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let settings = Settings {
        min_margin: 0.075,
        min_isk_per_day: 12_500_000.0,
        search: "pyer".to_owned(),
        capital: 2_000_000_000.0,
    };

    settings
        .save(&*stores.opportunities, account)
        .await
        .unwrap();

    assert_eq!(
        stores.opportunities.load_settings(account).await.unwrap(),
        Some(settings)
    );
    let replaced = Settings {
        min_margin: 0.0,
        min_isk_per_day: 0.0,
        search: String::new(),
        capital: 1.0,
    };
    stores
        .opportunities
        .save_settings(account, &replaced)
        .await
        .unwrap();
    assert_eq!(
        stores.opportunities.load_settings(account).await.unwrap(),
        Some(replaced)
    );
}

async fn settings_per_account(stores: &Stores) {
    let mine = account_with(stores, &[1]).await;
    let theirs = account_with(stores, &[2]).await;
    let settings = Settings {
        search: "mine".to_owned(),
        ..Settings::default()
    };
    settings.save(&*stores.opportunities, mine).await.unwrap();

    assert!(
        stores
            .opportunities
            .load_settings(theirs)
            .await
            .unwrap()
            .is_none()
    );
    let unknown = settings
        .save(&*stores.opportunities, uuid::Uuid::new_v4())
        .await;
    assert!(
        matches!(unknown, Err(StoreError::MissingParent)),
        "{unknown:?}"
    );
    // The filters go with the account.
    stores.accounts.delete_character(1).await.unwrap();
    assert!(
        stores
            .opportunities
            .load_settings(mine)
            .await
            .unwrap()
            .is_none()
    );
}

async fn station_sync(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    assert!(
        stores
            .opportunities
            .station_sync(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );

    stores
        .station
        .replace_station_book(REGION, STATION, t, &[], t, None)
        .await
        .unwrap();
    stores
        .station
        .finish_history_pass(REGION, STATION, t + Duration::hours(1), false, Some("down"))
        .await
        .unwrap();
    stores
        .station
        .replace_station_book(1, 2, t, &[], t, None)
        .await
        .unwrap();

    let mut rows: Vec<_> = stores
        .opportunities
        .station_sync(REGION, STATION)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.kind, r.last_success_at, r.last_error))
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            ("book".to_owned(), Some(t), None),
            ("history".to_owned(), None, Some("down".to_owned())),
        ]
    );
}

async fn own_orders(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let other = account_with(stores, &[3]).await;
    let t = now();
    let mut elsewhere = sell_order(3, 36);
    elsewhere.location_id = 60_000_004;
    stores
        .orders
        .replace_orders(
            1,
            t,
            &[
                sell_order(1, 34),
                buy_order(2, 34),
                elsewhere,
                sell_order(4, 35),
            ],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .orders
        .replace_orders(2, t, &[buy_order(5, 37), sell_order(6, 34)], t, None)
        .await
        .unwrap();
    stores
        .orders
        .replace_orders(3, t, &[sell_order(7, 38)], t, None)
        .await
        .unwrap();

    let found = stores
        .opportunities
        .own_orders_at(account, &[1, 2], STATION)
        .await
        .unwrap();
    let mut found = found;
    found.sort_unstable();
    assert_eq!(found, [(34, false), (34, true), (35, false), (37, true)]);
    let only_two = stores
        .opportunities
        .own_orders_at(account, &[2], STATION)
        .await
        .unwrap();
    assert_eq!(only_two.len(), 2);
    let stranger = stores
        .opportunities
        .own_orders_at(account, &[3], STATION)
        .await
        .unwrap();
    assert!(
        stranger.is_empty(),
        "character 3 belongs to another account"
    );
    assert_eq!(
        stores
            .opportunities
            .own_orders_at(other, &[3], STATION)
            .await
            .unwrap(),
        [(38, false)]
    );
}

async fn stats_need_book_fields(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    stores
        .station
        .replace_station_book(
            REGION,
            STATION,
            t,
            &[
                sekondo::store::StationOrder {
                    order_id: 1,
                    type_id: 34,
                    is_buy_order: true,
                    price_cents: 1_000,
                    volume_remain: 1,
                },
                sekondo::store::StationOrder {
                    order_id: 2,
                    type_id: 34,
                    is_buy_order: false,
                    price_cents: 1_100,
                    volume_remain: 1,
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();
    // Indicators for an item that has no row do nothing; a row only exists once the book fields do.
    let columns = sekondo::store::IndicatorColumns {
        type_id: vec![34],
        volume_7d: vec![Some(1.0)],
        volume_30d: vec![None],
        sigma: vec![None],
        returns: vec![None],
        sma5: vec![None],
        sma20: vec![None],
        cross_day: vec![None],
        volume_rising: vec![None],
        percent_b: vec![None],
        rsi14: vec![None],
    };
    stores
        .station
        .update_indicators(REGION, STATION, t, &columns)
        .await
        .unwrap();
    assert!(
        stores
            .opportunities
            .load_stats(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        stores
            .station
            .item_stat_ids(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );
    // Another station's rows are not listed.
    stores
        .station
        .upsert_book_fields(
            REGION,
            99,
            t,
            &[sekondo::store::BookFields {
                type_id: 34,
                best_buy: "1.00".to_owned(),
                best_sell: "2.00".to_owned(),
                buy_near_orders: 1,
                sell_near_orders: 1,
                rho: 0.0,
            }],
        )
        .await
        .unwrap();
    assert!(
        stores
            .opportunities
            .load_stats(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        stores
            .opportunities
            .load_stats(REGION, 99)
            .await
            .unwrap()
            .len(),
        1
    );
}
