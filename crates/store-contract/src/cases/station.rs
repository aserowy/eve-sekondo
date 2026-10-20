//! The station's order book, its snapshots and rollups, its market history and the item rows.

use sekondo::{
    esi::EsiHistoryDay,
    store::{
        BookFields, ExpansionColumns, IndicatorColumns, SeedHistoryRow, SeedSnapshotRow,
        StationOrder, StoreError, Stores,
    },
};
use time::{
    Date, Duration, OffsetDateTime,
    macros::{date, datetime},
};

use super::{
    Case,
    support::{REGION, STATION, assert_close, boxed, case, now},
};

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "station/a-refresh-replaces-the-book-and-snapshots-the-tradable-items",
            |s| boxed(book_replaced(s)),
        ),
        case(
            "station/depth-near-the-best-price-is-within-one-percent",
            |s| boxed(near_depth(s)),
        ),
        case(
            "station/units-taken-count-orders-present-in-both-books",
            |s| boxed(units_taken_from_both_books(s)),
        ),
        case(
            "station/another-stations-book-is-not-ours-to-replace",
            |s| boxed(foreign_book(s)),
        ),
        case("station/the-book-refresh-is-claimed-once-per-lease", |s| {
            boxed(claim_book(s))
        }),
        case(
            "station/a-book-failure-backs-off-and-a-deferral-only-moves-the-poll",
            |s| boxed(book_failure_and_deferral(s)),
        ),
        case("station/history-is-tracked-claimed-and-stored", |s| {
            boxed(history_flow(s))
        }),
        case(
            "station/history-keeps-only-the-days-from-the-cut-and-never-doubles-a-day",
            |s| boxed(history_days(s)),
        ),
        case(
            "station/a-history-failure-backs-off-and-a-schedule-is-no-failure",
            |s| boxed(history_failure(s)),
        ),
        case("station/the-history-pass-records-when-it-finished", |s| {
            boxed(history_pass(s))
        }),
        case("station/rollups-appear-only-after-a-refresh", |s| {
            boxed(rollups_after_refresh(s))
        }),
        case(
            "station/hourly-buckets-split-at-the-hour-and-aggregate",
            |s| boxed(hourly_buckets(s)),
        ),
        case(
            "station/a-refresh-window-only-materializes-whole-buckets",
            |s| boxed(refresh_windows(s)),
        ),
        case("station/a-day-with-twenty-hours-has-its-own-figures", |s| {
            boxed(daily_figures(s))
        }),
        case(
            "station/units-taken-come-from-days-and-the-hours-after-them",
            |s| boxed(units_taken_window(s)),
        ),
        case("station/item-rows-follow-the-tradable-items", |s| {
            boxed(item_rows(s))
        }),
        case(
            "station/indicators-and-expansions-update-existing-rows",
            |s| boxed(indicators_and_expansions(s)),
        ),
        case(
            "station/the-latest-snapshot-per-item-within-the-window",
            |s| boxed(latest_snapshots(s)),
        ),
        case(
            "station/seeded-snapshots-and-history-are-rounded-to-cents",
            |s| boxed(seeds(s)),
        ),
        case(
            "station/the-history-table-and-the-item-row-show-what-is-stored",
            |s| boxed(stored_rows(s)),
        ),
        case("station/the-materialized-buckets-are-listed", |s| {
            boxed(listed_buckets(s))
        }),
        case("station/a-seeded-item-row-replaces-the-whole-row", |s| {
            boxed(seeded_item_row(s))
        }),
    ]
}

/// A row of the station's book.
fn order(id: i64, type_id: i64, buy: bool, price_cents: i64, volume: i64) -> StationOrder {
    StationOrder {
        order_id: id,
        type_id,
        is_buy_order: buy,
        price_cents,
        volume_remain: volume,
    }
}

/// A seeded snapshot of item 34 at `at` with these best prices.
fn snapshot(at: OffsetDateTime, buy: f64, sell: f64) -> SeedSnapshotRow {
    SeedSnapshotRow {
        recorded_at: at,
        type_id: 34,
        best_buy: buy,
        best_sell: sell,
        buy_near_units: 100,
        buy_near_orders: 2,
        sell_near_units: 100,
        sell_near_orders: 2,
        buy_units: 1_000,
        sell_units: 1_000,
        buy_taken: 3,
        sell_taken: 4,
    }
}

async fn replace(stores: &Stores, at: OffsetDateTime, rows: &[StationOrder]) {
    stores
        .station
        .replace_station_book(
            REGION,
            STATION,
            at,
            rows,
            at + Duration::minutes(6),
            Some(at),
        )
        .await
        .unwrap();
}

async fn refresh(stores: &Stores, from: OffsetDateTime, to: OffsetDateTime) {
    stores.station.refresh_rollups(from, to).await.unwrap();
}

async fn book_replaced(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    replace(
        stores,
        t,
        &[
            order(1, 34, true, 1_000, 10),
            order(2, 34, true, 995, 5),
            order(3, 34, true, 980, 7),
            order(4, 34, false, 1_100, 20),
            order(5, 34, false, 1_105, 3),
            order(6, 34, false, 1_200, 9),
            // 35 is only sold, 36 has a buy order at nothing: both are item rows to see.
            order(7, 35, false, 500, 1),
            order(8, 36, true, 0, 1),
            order(9, 36, false, 100, 1),
        ],
    )
    .await;

    let mut tradable = stores
        .station
        .tradable_items(REGION, STATION)
        .await
        .unwrap();
    tradable.sort_unstable();
    assert_eq!(tradable, [34, 36], "both sides present");
    let snapshots = stores
        .station
        .latest_snapshots(REGION, STATION, t - Duration::days(1))
        .await
        .unwrap();
    assert_eq!(
        snapshots,
        [(34, "10.00".to_owned(), "11.00".to_owned(), 2, 2)],
        "a best buy price of nothing gets no snapshot"
    );
    let book = stores.station.book(REGION, STATION).await.unwrap();
    assert_eq!(
        book.iter()
            .map(|o| (o.order_id, o.is_buy_order, o.price_cents, o.volume_remain))
            .collect::<Vec<_>>(),
        [
            (1, true, 1_000, 10),
            (2, true, 995, 5),
            (3, true, 980, 7),
            (4, false, 1_100, 20),
            (5, false, 1_105, 3),
            (6, false, 1_200, 9),
            (7, false, 500, 1),
            (8, true, 0, 1),
            (9, false, 100, 1),
        ]
    );
    assert!(stores.station.book(REGION, 99).await.unwrap().is_empty());
    let recorded = stores.station.snapshots(REGION, STATION).await.unwrap();
    assert_eq!(recorded.len(), 1);
    let snapshot = &recorded[0];
    assert_eq!(snapshot.recorded_at, t);
    assert_eq!(
        (
            snapshot.type_id,
            snapshot.best_buy.as_str(),
            snapshot.best_sell.as_str()
        ),
        (34, "10.00", "11.00")
    );
    assert_eq!(snapshot.spread, "0.10000000000000000000");
    assert_eq!((snapshot.buy_near_units, snapshot.buy_near_orders), (15, 2));
    assert_eq!(
        (snapshot.sell_near_units, snapshot.sell_near_orders),
        (23, 2)
    );
    assert_eq!((snapshot.buy_units, snapshot.sell_units), (22, 32));
    assert_eq!((snapshot.buy_taken, snapshot.sell_taken), (0, 0));
    let state = stores
        .station
        .station_sync_state(REGION, STATION, "book")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.next_poll_at, t + Duration::minutes(6));
    assert_eq!(state.last_success_at, Some(t));
    assert_eq!(state.last_attempt_at, Some(t));
    assert_eq!(state.esi_expires_at, Some(t));
    assert_eq!((state.failures, state.last_error), (0, None));

    // The next refresh replaces the book: 35 is gone, so is its side of nothing.
    replace(
        stores,
        t + Duration::minutes(5),
        &[
            order(1, 34, true, 1_000, 10),
            order(4, 34, false, 1_100, 20),
        ],
    )
    .await;
    assert_eq!(
        stores
            .station
            .tradable_items(REGION, STATION)
            .await
            .unwrap(),
        [34]
    );
    let both = stores
        .station
        .latest_snapshots(REGION, STATION, t - Duration::days(1))
        .await
        .unwrap();
    assert_eq!(both.len(), 1, "one row per item, the newest snapshot");
    assert_eq!((both[0].3, both[0].4), (1, 1));
    // An empty book has no tradable items, and no new snapshots.
    replace(stores, t + Duration::minutes(10), &[]).await;
    assert!(
        stores
            .station
            .tradable_items(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );
    let kept = stores
        .station
        .latest_snapshots(REGION, STATION, t - Duration::days(1))
        .await
        .unwrap();
    assert!(
        kept.is_empty(),
        "snapshots of items that are no longer tradable are not listed"
    );
}

async fn near_depth(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    // Best buy 10.00: 9.90 is exactly 1% below and counts, 9.89 does not. Best sell 11.00:
    // 11.11 is exactly 1% above and counts, 11.12 does not.
    replace(
        stores,
        t,
        &[
            order(1, 34, true, 1_000, 1),
            order(2, 34, true, 990, 1),
            order(3, 34, true, 989, 1),
            order(4, 34, false, 1_100, 1),
            order(5, 34, false, 1_111, 1),
            order(6, 34, false, 1_112, 1),
        ],
    )
    .await;

    let snapshots = stores
        .station
        .latest_snapshots(REGION, STATION, t - Duration::days(1))
        .await
        .unwrap();

    assert_eq!(snapshots[0].3, 2, "buy orders near the best price");
    assert_eq!(snapshots[0].4, 2, "sell orders near the best price");
}

async fn units_taken_from_both_books(stores: &Stores) {
    let first = datetime!(2026-10-20 10:05 UTC);
    let second = datetime!(2026-10-20 10:25 UTC);
    replace(
        stores,
        first,
        &[
            order(1, 34, true, 1_000, 10),
            order(2, 34, true, 995, 5),
            order(4, 34, false, 1_100, 20),
            order(5, 34, false, 1_105, 3),
        ],
    )
    .await;
    // Order 1 lost 6 units, 4 lost 5, 2 is gone, 5 grew, 9 is new.
    replace(
        stores,
        second,
        &[
            order(1, 34, true, 1_000, 4),
            order(4, 34, false, 1_100, 15),
            order(5, 34, false, 1_105, 9),
            order(9, 34, true, 990, 100),
        ],
    )
    .await;

    refresh(
        stores,
        datetime!(2026-10-20 00:00 UTC),
        datetime!(2026-10-21 00:00 UTC),
    )
    .await;

    let taken = stores
        .station
        .units_taken(REGION, STATION, datetime!(2026-10-19 00:00 UTC))
        .await
        .unwrap();
    assert_eq!(taken, [(34, 6, 5)]);
}

async fn foreign_book(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    stores
        .station
        .replace_station_book(1, 100, t, &[order(1, 34, true, 1_000, 1)], t, None)
        .await
        .unwrap();

    let taken = stores
        .station
        .replace_station_book(REGION, STATION, t, &[order(1, 34, true, 1_000, 1)], t, None)
        .await;
    assert!(taken.is_err(), "an order ID is one order");
    let twice = stores
        .station
        .replace_station_book(
            REGION,
            STATION,
            t,
            &[order(2, 34, true, 1_000, 1), order(2, 34, false, 1_100, 1)],
            t,
            None,
        )
        .await;
    assert!(twice.is_err());
    assert!(
        stores
            .station
            .tradable_items(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        stores
            .station
            .station_sync_state(REGION, STATION, "book")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        stores
            .station
            .station_sync_state(1, 100, "book")
            .await
            .unwrap()
            .is_some()
    );
}

async fn claim_book(stores: &Stores) {
    let t = now();
    let lease = Duration::minutes(5);

    assert!(
        stores
            .station
            .claim_book_refresh(REGION, STATION, t, lease)
            .await
            .unwrap()
    );
    assert!(
        !stores
            .station
            .claim_book_refresh(REGION, STATION, t, lease)
            .await
            .unwrap()
    );
    assert!(
        stores
            .station
            .claim_book_refresh(REGION, STATION, t + lease, lease)
            .await
            .unwrap()
    );

    let state = stores
        .station
        .station_sync_state(REGION, STATION, "book")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.next_poll_at, t + lease + lease);
    assert!(
        stores
            .station
            .claim_book_refresh(1, 2, t, lease)
            .await
            .unwrap(),
        "another station has its own"
    );
}

async fn book_failure_and_deferral(stores: &Stores) {
    let t = now();
    let (start, interval) = (Duration::minutes(1), Duration::minutes(5));

    let mut waits = Vec::new();
    for round in 0..5 {
        let at = t + Duration::hours(round);
        stores
            .station
            .record_book_failure(REGION, STATION, at, "boom", start, interval)
            .await
            .unwrap();
        let s = stores
            .station
            .station_sync_state(REGION, STATION, "book")
            .await
            .unwrap()
            .unwrap();
        waits.push((s.failures, (s.next_poll_at - at).whole_seconds()));
    }
    assert_eq!(waits, [(1, 60), (2, 120), (3, 240), (4, 300), (5, 300)]);

    let until = t + Duration::days(3);
    stores
        .station
        .record_book_deferral(REGION, STATION, t + Duration::days(2), until)
        .await
        .unwrap();
    let s = stores
        .station
        .station_sync_state(REGION, STATION, "book")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((s.next_poll_at, s.failures), (until, 5));
    assert_eq!(s.last_attempt_at, Some(t + Duration::days(2)));
    assert_eq!(s.last_error.as_deref(), Some("boom"));
    // A deferral also creates the row.
    stores
        .station
        .record_book_deferral(1, 2, t, until)
        .await
        .unwrap();
    let fresh = stores
        .station
        .station_sync_state(1, 2, "book")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (fresh.next_poll_at, fresh.failures, fresh.last_success_at),
        (until, 0, None)
    );
}

fn day(date: &str, average: i64, highest: i64, lowest: i64) -> EsiHistoryDay {
    EsiHistoryDay {
        date: date.to_owned(),
        average_cents: average,
        highest_cents: highest,
        lowest_cents: lowest,
        order_count: 12,
        volume: 3_400,
    }
}

async fn history_flow(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    replace(
        stores,
        t,
        &[
            order(1, 34, true, 1_000, 1),
            order(2, 34, false, 1_100, 1),
            order(3, 35, true, 1_000, 1),
            order(4, 35, false, 1_100, 1),
            order(5, 36, false, 1_100, 1),
        ],
    )
    .await;
    let lease = Duration::minutes(5);

    stores
        .station
        .track_history(REGION, STATION, t)
        .await
        .unwrap();
    // Tracking again keeps what is there; a later tick finds the same items due.
    stores
        .station
        .track_history(REGION, STATION, t + Duration::hours(1))
        .await
        .unwrap();

    assert!(
        stores
            .station
            .history_sync(REGION, 36)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        stores
            .station
            .history_sync(REGION, 34)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t
    );
    let first = stores
        .station
        .claim_due_history(REGION, STATION, t, lease, 1)
        .await
        .unwrap();
    assert_eq!(first, [34], "oldest first, then by item, at most one");
    assert_eq!(
        stores
            .station
            .history_sync(REGION, 34)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t + lease
    );
    let rest = stores
        .station
        .claim_due_history(REGION, STATION, t, lease, 10)
        .await
        .unwrap();
    assert_eq!(rest, [35]);
    assert!(
        stores
            .station
            .claim_due_history(REGION, STATION, t, lease, 10)
            .await
            .unwrap()
            .is_empty()
    );
    // An item that is no longer tradable is never claimed.
    replace(
        stores,
        t + Duration::minutes(1),
        &[order(5, 36, false, 1_100, 1)],
    )
    .await;
    assert!(
        stores
            .station
            .claim_due_history(REGION, STATION, t + Duration::hours(1), lease, 10)
            .await
            .unwrap()
            .is_empty()
    );

    let next = t + Duration::hours(24);
    stores
        .station
        .replace_history_days(
            REGION,
            34,
            t,
            next,
            date!(2026 - 09 - 20),
            &[day("2026-10-19", 12_345, 13_000, 12_000)],
        )
        .await
        .unwrap();
    let state = stores
        .station
        .history_sync(REGION, 34)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.next_poll_at, next);
    assert_eq!(state.esi_expires_at, Some(next));
    assert_eq!(
        (state.last_success_at, state.last_attempt_at),
        (Some(t), Some(t))
    );
    assert_eq!((state.failures, state.last_error), (0, None));
}

async fn history_days(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    let from = date!(2026 - 10 - 01);
    stores
        .station
        .replace_history_days(
            REGION,
            34,
            t,
            t,
            from,
            &[
                day("2026-10-01", 1_000, 1_100, 900),
                day("2026-10-02", 2_000, 2_100, 1_900),
            ],
        )
        .await
        .unwrap();
    stores
        .station
        .replace_history_days(
            REGION,
            34,
            t,
            t,
            from,
            &[
                day("2026-10-02", 2_222, 2_300, 2_100),
                day("2026-10-03", 3_000, 3_100, 2_900),
            ],
        )
        .await
        .unwrap();
    // Another item of the region, and a cut that drops the older days of the whole region.
    stores
        .station
        .replace_history_days(
            REGION,
            35,
            t,
            t,
            date!(2026 - 10 - 03),
            &[day("2026-10-04", 400, 410, 390)],
        )
        .await
        .unwrap();

    let days = stores
        .opportunities
        .history_days(REGION, 34, date!(2026 - 09 - 01))
        .await
        .unwrap();

    assert_eq!(
        days.iter().map(|d| (d.0, d.1, d.2)).collect::<Vec<_>>(),
        [(date!(2026 - 10 - 03), 30.0, 3_400.0),],
        "the cut of the last call dropped the days before 3 October of the whole region"
    );
    let other = stores
        .opportunities
        .history_days(REGION, 35, date!(2026 - 09 - 01))
        .await
        .unwrap();
    assert_eq!(other.len(), 1);
    let nothing = stores
        .station
        .replace_history_days(REGION, 34, t, t, from, &[day("not a date", 1, 1, 1)])
        .await;
    assert!(nothing.is_err());
    let twice = stores
        .station
        .replace_history_days(
            REGION,
            34,
            t,
            t,
            from,
            &[day("2026-10-05", 1, 1, 1), day("2026-10-05", 2, 2, 2)],
        )
        .await;
    assert!(twice.is_err());
}

async fn history_failure(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    replace(
        stores,
        t,
        &[order(1, 34, true, 1_000, 1), order(2, 34, false, 1_100, 1)],
    )
    .await;
    stores
        .station
        .track_history(REGION, STATION, t)
        .await
        .unwrap();
    let (start, cap) = (Duration::minutes(1), Duration::hours(1));

    let mut waits = Vec::new();
    for round in 0..8 {
        let at = t + Duration::days(round);
        stores
            .station
            .record_history_failure(REGION, 34, at, "boom", start, cap)
            .await
            .unwrap();
        let s = stores
            .station
            .history_sync(REGION, 34)
            .await
            .unwrap()
            .unwrap();
        waits.push((s.failures, (s.next_poll_at - at).whole_seconds()));
    }
    assert_eq!(
        waits,
        [
            (1, 60),
            (2, 120),
            (3, 240),
            (4, 480),
            (5, 960),
            (6, 1_920),
            (7, 3_600),
            (8, 3_600)
        ]
    );

    let later = t + Duration::days(20);
    stores
        .station
        .schedule_history(REGION, 34, later, later + Duration::days(1))
        .await
        .unwrap();
    let s = stores
        .station
        .history_sync(REGION, 34)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (s.failures, s.last_error),
        (0, None),
        "a schedule is not a failure"
    );
    assert_eq!(s.next_poll_at, later + Duration::days(1));
    assert_eq!(s.last_attempt_at, Some(later));
    // Items without a row are left alone.
    stores
        .station
        .record_history_failure(REGION, 77, t, "x", start, cap)
        .await
        .unwrap();
    stores
        .station
        .schedule_history(REGION, 77, t, t)
        .await
        .unwrap();
    assert!(
        stores
            .station
            .history_sync(REGION, 77)
            .await
            .unwrap()
            .is_none()
    );
}

async fn history_pass(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);

    stores
        .station
        .finish_history_pass(REGION, STATION, t, false, Some("all failed"))
        .await
        .unwrap();
    let failed = stores
        .station
        .station_sync_state(REGION, STATION, "history")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.last_success_at, None);
    assert_eq!(failed.last_error.as_deref(), Some("all failed"));
    assert_eq!(failed.last_attempt_at, Some(t));

    stores
        .station
        .finish_history_pass(REGION, STATION, t + Duration::hours(1), true, None)
        .await
        .unwrap();
    let ok = stores
        .station
        .station_sync_state(REGION, STATION, "history")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ok.last_success_at, Some(t + Duration::hours(1)));
    assert_eq!(ok.last_error, None);

    stores
        .station
        .finish_history_pass(
            REGION,
            STATION,
            t + Duration::hours(2),
            false,
            Some("later"),
        )
        .await
        .unwrap();
    let kept = stores
        .station
        .station_sync_state(REGION, STATION, "history")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        kept.last_success_at,
        Some(t + Duration::hours(1)),
        "a failed pass keeps the last success"
    );
    assert_eq!(kept.last_error.as_deref(), Some("later"));
    assert!(
        stores
            .station
            .station_sync_state(REGION, STATION, "book")
            .await
            .unwrap()
            .is_none()
    );
}

async fn rollups_after_refresh(stores: &Stores) {
    let t = datetime!(2026-10-20 10:15 UTC);
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[
                snapshot(t, 10.0, 11.0),
                snapshot(t + Duration::minutes(30), 10.5, 11.5),
            ],
        )
        .await
        .unwrap();

    let from = datetime!(2026-10-20 00:00 UTC);
    assert!(
        stores
            .station
            .hourly_points(REGION, STATION, from)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        stores
            .station
            .latest_hour(REGION, STATION)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        stores
            .station
            .units_taken(REGION, STATION, from)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        stores
            .station
            .own_hourly_days(REGION, STATION, from)
            .await
            .unwrap(),
        0
    );

    refresh(stores, from, datetime!(2026-10-21 00:00 UTC)).await;

    let points = stores
        .station
        .hourly_points(REGION, STATION, from)
        .await
        .unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(
        stores.station.latest_hour(REGION, STATION).await.unwrap(),
        Some(datetime!(2026-10-20 10:00 UTC))
    );
    assert_eq!(
        stores
            .station
            .units_taken(REGION, STATION, from)
            .await
            .unwrap(),
        [(34, 6, 8)]
    );
    assert_eq!(
        stores
            .station
            .own_hourly_days(REGION, STATION, from)
            .await
            .unwrap(),
        1
    );

    // Snapshots written after the refresh stay out until the next one covers them.
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[snapshot(t + Duration::minutes(40), 9.0, 12.0)],
        )
        .await
        .unwrap();
    assert_eq!(
        stores
            .station
            .units_taken(REGION, STATION, from)
            .await
            .unwrap(),
        [(34, 6, 8)]
    );
    refresh(stores, from, datetime!(2026-10-21 00:00 UTC)).await;
    assert_eq!(
        stores
            .station
            .units_taken(REGION, STATION, from)
            .await
            .unwrap(),
        [(34, 9, 12)]
    );
}

async fn hourly_buckets(stores: &Stores) {
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[
                snapshot(datetime!(2026-10-20 10:15 UTC), 10.0, 11.0),
                snapshot(datetime!(2026-10-20 10:45 UTC), 10.5, 11.5),
                // The last microsecond of the hour, and the first one of the next.
                snapshot(datetime!(2026-10-20 10:59:59.999999 UTC), 10.25, 11.25),
                snapshot(datetime!(2026-10-20 11:00 UTC), 9.0, 12.0),
            ],
        )
        .await
        .unwrap();
    let from = datetime!(2026-10-20 00:00 UTC);

    refresh(stores, from, datetime!(2026-10-21 00:00 UTC)).await;

    let points = stores
        .station
        .hourly_points(REGION, STATION, from)
        .await
        .unwrap();
    assert_eq!(points.len(), 2);
    let ten = &points[0];
    assert_eq!((ten.0, ten.1), (34, datetime!(2026-10-20 10:00 UTC)));
    // Spreads: 1/10, 1/10.5 and 1/10.25 over three snapshots.
    let (a, b, c) = (0.1, 1.0 / 10.5, 1.0 / 10.25);
    assert_close(ten.2, (a + b + c) / 3.0);
    assert_close(ten.3, a);
    assert_eq!(ten.4, 1_150, "the highest sell price in cents");
    let eleven = &points[1];
    assert_eq!(eleven.1, datetime!(2026-10-20 11:00 UTC));
    assert_close(eleven.2, 3.0 / 9.0);
    assert_eq!(eleven.4, 1_200);

    // The same buckets seen as the detail charts read them.
    let prices = stores
        .opportunities
        .price_points(REGION, STATION, 34, from)
        .await
        .unwrap();
    assert_eq!(prices.len(), 2);
    assert_close(prices[0].1, (10.0 + 10.5 + 10.25) / 3.0);
    assert_close(prices[0].2, (11.0 + 11.5 + 11.25) / 3.0);
    assert_close(prices[1].1, 9.0);
    let hours = stores
        .opportunities
        .item_hour_points(REGION, STATION, 34, from)
        .await
        .unwrap();
    assert_eq!(hours.len(), 2);
    assert_eq!(hours[0].0, datetime!(2026-10-20 10:00 UTC));
    assert_eq!(hours[0].3, 1_150);
    let later = stores
        .opportunities
        .price_points(REGION, STATION, 34, datetime!(2026-10-20 11:00 UTC))
        .await
        .unwrap();
    assert_eq!(later.len(), 1, "only the buckets from the cut");
    assert!(
        stores
            .opportunities
            .price_points(REGION, STATION, 35, from)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn refresh_windows(stores: &Stores) {
    // One snapshot at half past each hour from 10:30 on the 19th to 17:30 on the 20th, item 34.
    let first = datetime!(2026-10-19 10:30 UTC);
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &(0..32)
                .map(|h| snapshot(first + Duration::hours(h), 10.0, 11.0))
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    let from = datetime!(2026-10-19 00:00 UTC);
    let hours = |points: Vec<(i64, OffsetDateTime, f64, f64, i64)>| {
        points
            .into_iter()
            .map(|p| (p.1.day(), p.1.hour()))
            .collect::<Vec<_>>()
    };

    // 06:30 on the 19th to 09:20 on the 21st: the whole hours from 07:00 on the 19th up to 09:00
    // on the 21st, and only the whole day, the 20th.
    refresh(
        stores,
        datetime!(2026-10-19 06:30 UTC),
        datetime!(2026-10-21 09:20 UTC),
    )
    .await;
    let materialized = hours(
        stores
            .station
            .hourly_points(REGION, STATION, from)
            .await
            .unwrap(),
    );
    assert_eq!(materialized.len(), 32, "every hour that has a snapshot");
    assert_eq!(materialized[0], (19, 10));
    assert_eq!(materialized[31], (20, 17));

    // A refresh that starts later leaves the hours before it as they are.
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[
                snapshot(datetime!(2026-10-19 11:45 UTC), 10.0, 12.0),
                snapshot(datetime!(2026-10-20 15:45 UTC), 10.0, 12.0),
            ],
        )
        .await
        .unwrap();
    refresh(
        stores,
        datetime!(2026-10-20 00:30 UTC),
        datetime!(2026-10-22 12:00 UTC),
    )
    .await;
    let points = stores
        .station
        .hourly_points(REGION, STATION, from)
        .await
        .unwrap();
    let at = |day: u8, hour: u8| {
        points
            .iter()
            .find(|p| p.1.day() == day && p.1.hour() == hour)
            .map(|p| p.4)
            .unwrap()
    };
    assert_eq!(at(19, 11), 1_100, "the 19th was not in the second window");
    assert_eq!(at(20, 15), 1_200, "the 20th at 15:00 was");

    // A window without a whole day is refused, and so is an empty or reversed one.
    for (start, end) in [
        (
            datetime!(2026-10-20 06:00 UTC),
            datetime!(2026-10-20 18:00 UTC),
        ),
        (
            datetime!(2026-10-20 00:10 UTC),
            datetime!(2026-10-20 23:50 UTC),
        ),
        (
            datetime!(2026-10-20 00:00 UTC),
            datetime!(2026-10-20 00:00 UTC),
        ),
        (
            datetime!(2026-10-21 00:00 UTC),
            datetime!(2026-10-20 00:00 UTC),
        ),
    ] {
        let refused = stores.station.refresh_rollups(start, end).await;
        assert!(
            matches!(refused, Err(StoreError::Backend(_))),
            "{start} to {end}: {refused:?}"
        );
    }
}

/// One snapshot an hour of item 34 over `hours`, the best sell price rising with the hour.
fn hourly_snapshots(day: OffsetDateTime, hours: std::ops::Range<i64>) -> Vec<SeedSnapshotRow> {
    hours
        .map(|h| {
            snapshot(
                day + Duration::hours(h) + Duration::minutes(10),
                10.0,
                11.0 + h as f64 / 100.0,
            )
        })
        .collect()
}

async fn daily_figures(stores: &Stores) {
    // 20 October has 20 hours of own snapshots, 19 October only 19.
    let own = datetime!(2026-10-20 00:00 UTC);
    let short = datetime!(2026-10-19 00:00 UTC);
    stores
        .station
        .seed_snapshots(REGION, STATION, &hourly_snapshots(own, 0..20))
        .await
        .unwrap();
    stores
        .station
        .seed_snapshots(REGION, STATION, &hourly_snapshots(short, 0..19))
        .await
        .unwrap();
    stores
        .station
        .seed_history(
            REGION,
            &[
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 20),
                    average: 10.5,
                    highest: 12.0,
                    lowest: 10.0,
                    volume: 100,
                    order_count: 5,
                },
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 19),
                    average: 10.5,
                    highest: 13.0,
                    lowest: 10.0,
                    volume: 100,
                    order_count: 5,
                },
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 18),
                    average: 9.0,
                    highest: 9.5,
                    lowest: 8.0,
                    volume: 100,
                    order_count: 5,
                },
            ],
        )
        .await
        .unwrap();

    // Nothing is rolled up yet: every day has the history's own range.
    let before = stores
        .station
        .daily_points(REGION, date!(2026 - 10 - 01))
        .await
        .unwrap();
    assert_eq!(before.len(), 3);
    assert_close(before[2].2, 0.2);
    assert_eq!(before[2].3, 1_200);

    refresh(
        stores,
        datetime!(2026-10-19 00:00 UTC),
        datetime!(2026-10-21 00:00 UTC),
    )
    .await;

    let after = stores
        .station
        .daily_points(REGION, date!(2026 - 10 - 01))
        .await
        .unwrap();
    assert_eq!(after.len(), 3);
    let [eighteenth, nineteenth, twentieth] = &after[..] else {
        panic!("three days")
    };
    assert_eq!(
        (eighteenth.1, nineteenth.1, twentieth.1),
        (
            date!(2026 - 10 - 18),
            date!(2026 - 10 - 19),
            date!(2026 - 10 - 20)
        )
    );
    assert_close(eighteenth.2, 1.5 / 8.0);
    assert_eq!(eighteenth.3, 950, "no rollup: the history's high");
    assert_close(nineteenth.2, 0.3);
    assert_eq!(nineteenth.3, 1_300, "19 hours are not enough");
    // 20 hours: the largest spread and the highest sell price of the hours.
    assert_close(twentieth.2, 1.19 / 10.0);
    assert_eq!(
        twentieth.3, 1_119,
        "the best sell price of the 19th hour, 11.19"
    );
    let item = stores
        .opportunities
        .item_day_points(REGION, 34, date!(2026 - 10 - 19))
        .await
        .unwrap();
    assert_eq!(item.len(), 2);
    assert_eq!(
        (item[0].0, item[1].0),
        (date!(2026 - 10 - 19), date!(2026 - 10 - 20))
    );
    assert!(
        stores
            .opportunities
            .item_day_points(REGION, 35, date!(2026 - 10 - 01))
            .await
            .unwrap()
            .is_empty()
    );
}

async fn units_taken_window(stores: &Stores) {
    let at = |day: u8, hour: u8| {
        OffsetDateTime::new_utc(
            Date::from_calendar_date(2026, time::Month::October, day).unwrap(),
            time::Time::from_hms(hour, 10, 0).unwrap(),
        )
    };
    let mut rows = Vec::new();
    for (day, hour, taken) in [
        (17, 12, 1),
        (18, 3, 10),
        (18, 20, 100),
        (19, 5, 1_000),
        (20, 6, 10_000),
    ] {
        let mut s = snapshot(at(day, hour), 10.0, 11.0);
        s.buy_taken = taken;
        s.sell_taken = taken * 2;
        rows.push(s);
    }
    stores
        .station
        .seed_snapshots(REGION, STATION, &rows)
        .await
        .unwrap();
    // Days 17 to 19 are rolled up, and the hours up to the 20th at 17:00.
    refresh(
        stores,
        datetime!(2026-10-17 00:00 UTC),
        datetime!(2026-10-20 18:00 UTC),
    )
    .await;

    // From the 18th: its day and the 19th come from the daily rollup (110 and 1_000), what comes
    // after the last day from the hourly one (the 20th, 10_000).
    let taken = stores
        .station
        .units_taken(REGION, STATION, datetime!(2026-10-18 00:00 UTC))
        .await
        .unwrap();
    assert_eq!(taken, [(34, 11_110, 22_220)]);
    // From the middle of the 18th the day is not whole inside the window, and the hours of that
    // day are not read either.
    let later = stores
        .station
        .units_taken(REGION, STATION, datetime!(2026-10-18 12:00 UTC))
        .await
        .unwrap();
    assert_eq!(later, [(34, 11_000, 22_000)]);
}

async fn item_rows(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    replace(
        stores,
        t,
        &[
            order(1, 34, true, 1_000, 1),
            order(2, 34, false, 1_100, 1),
            order(3, 35, true, 2_000, 1),
            order(4, 35, false, 2_100, 1),
        ],
    )
    .await;
    let book = |type_id: i64, buy: &str, sell: &str, rho: f64| BookFields {
        type_id,
        best_buy: buy.to_owned(),
        best_sell: sell.to_owned(),
        buy_near_orders: 2,
        sell_near_orders: 3,
        rho,
    };

    stores
        .station
        .upsert_book_fields(
            REGION,
            STATION,
            t,
            &[
                book(34, "10.00", "11.00", 0.25),
                book(35, "20.50", "21.00", 0.75),
            ],
        )
        .await
        .unwrap();

    assert_eq!(
        stores.station.item_stat_ids(REGION, STATION).await.unwrap(),
        [34, 35]
    );
    let stats = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!(stats.len(), 2);
    assert_eq!(
        (
            stats[0].type_id,
            stats[0].best_buy_cents,
            stats[0].best_sell_cents
        ),
        (34, 1_000, 1_100)
    );
    assert_eq!(
        (
            stats[0].buy_near_orders,
            stats[0].sell_near_orders,
            stats[0].rho
        ),
        (2, 3, Some(0.25))
    );
    assert_eq!(stats[1].best_buy_cents, 2_050);
    assert!(stats[0].volume_7d.is_none() && stats[0].expansion_kind.is_none());
    // The upsert replaces the book fields of a row that is there.
    stores
        .station
        .upsert_book_fields(REGION, STATION, t, &[book(34, "10.10", "11.00", 0.5)])
        .await
        .unwrap();
    let again = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!((again[0].best_buy_cents, again[0].rho), (1_010, Some(0.5)));
    let twice = stores
        .station
        .upsert_book_fields(
            REGION,
            STATION,
            t,
            &[book(34, "1", "2", 0.1), book(34, "3", "4", 0.2)],
        )
        .await;
    assert!(twice.is_err());

    // Item 35 stops being tradable: its row goes.
    replace(
        stores,
        t + Duration::minutes(5),
        &[order(1, 34, true, 1_000, 1), order(2, 34, false, 1_100, 1)],
    )
    .await;
    stores
        .station
        .prune_item_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!(
        stores.station.item_stat_ids(REGION, STATION).await.unwrap(),
        [34]
    );
    assert_eq!(
        stores
            .opportunities
            .load_stats(REGION, STATION)
            .await
            .unwrap()
            .len(),
        1
    );
    // The stats carry the item's name.
    stores
        .market
        .store_names(
            &[sekondo::store::NameRow {
                id: 34,
                category: "t".to_owned(),
                name: Some("Tritanium".to_owned()),
            }],
            t,
            None,
        )
        .await
        .unwrap();
    let named = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!(named[0].name.as_deref(), Some("Tritanium"));
}

async fn indicators_and_expansions(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    replace(
        stores,
        t,
        &[order(1, 34, true, 1_000, 1), order(2, 34, false, 1_100, 1)],
    )
    .await;
    stores
        .station
        .upsert_book_fields(
            REGION,
            STATION,
            t,
            &[BookFields {
                type_id: 34,
                best_buy: "10.00".to_owned(),
                best_sell: "11.00".to_owned(),
                buy_near_orders: 1,
                sell_near_orders: 1,
                rho: 0.5,
            }],
        )
        .await
        .unwrap();
    stores
        .station
        .seed_history(
            REGION,
            &[
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 19),
                    average: 10.0,
                    highest: 11.0,
                    lowest: 9.0,
                    volume: 100,
                    order_count: 1,
                },
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 18),
                    average: 12.0,
                    highest: 13.0,
                    lowest: 11.0,
                    volume: 200,
                    order_count: 1,
                },
                SeedHistoryRow {
                    type_id: 99,
                    day: date!(2026 - 10 - 19),
                    average: 1.0,
                    highest: 1.0,
                    lowest: 1.0,
                    volume: 1,
                    order_count: 1,
                },
            ],
        )
        .await
        .unwrap();

    let rows = stores
        .station
        .history_rows(REGION, STATION, date!(2026 - 10 - 01))
        .await
        .unwrap();
    assert_eq!(
        rows,
        [
            (34, date!(2026 - 10 - 18), 12.0, 200),
            (34, date!(2026 - 10 - 19), 10.0, 100)
        ],
        "only the items that have a row, oldest day first"
    );
    assert_eq!(
        stores
            .station
            .history_rows(REGION, STATION, date!(2026 - 10 - 19))
            .await
            .unwrap()
            .len(),
        1
    );

    // Update the indicator fields of the item and of one that has no row.
    let columns = IndicatorColumns {
        type_id: vec![34, 77],
        volume_7d: vec![Some(10.0), Some(1.0)],
        volume_30d: vec![Some(20.0), None],
        sigma: vec![Some(0.5), None],
        returns: vec![Some(3), None],
        sma5: vec![Some(1.0), None],
        sma20: vec![None, None],
        cross_day: vec![Some(date!(2026 - 10 - 15)), None],
        volume_rising: vec![Some(true), None],
        percent_b: vec![Some(0.25), None],
        rsi14: vec![Some(55.5), None],
    };
    stores
        .station
        .update_indicators(REGION, STATION, t, &columns)
        .await
        .unwrap();
    let stats = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    let s = &stats[0];
    assert_eq!(
        (s.volume_7d, s.volume_30d, s.sigma),
        (Some(10.0), Some(20.0), Some(0.5))
    );
    assert_eq!(
        (s.sma5, s.sma20, s.cross_day),
        (Some(1.0), None, Some(date!(2026 - 10 - 15)))
    );
    assert_eq!(
        (s.volume_rising, s.percent_b, s.rsi14),
        (Some(true), Some(0.25), Some(55.5))
    );
    assert_eq!(stats.len(), 1, "no row was made for item 77");

    // Expansions follow the newest hour of the hourly rollup.
    assert!(
        stores
            .station
            .latest_hour(REGION, STATION)
            .await
            .unwrap()
            .is_none()
    );
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[snapshot(datetime!(2026-10-20 09:10 UTC), 10.0, 11.0)],
        )
        .await
        .unwrap();
    refresh(
        stores,
        datetime!(2026-10-20 00:00 UTC),
        datetime!(2026-10-21 00:00 UTC),
    )
    .await;
    let latest = stores
        .station
        .latest_hour(REGION, STATION)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        latest,
        datetime!(2026-10-20 10:00 UTC),
        "the book refresh at 10:05 snapshotted too"
    );
    assert!(
        stores
            .station
            .expansions_stale(REGION, STATION, latest)
            .await
            .unwrap()
    );
    stores
        .station
        .update_expansions(
            REGION,
            STATION,
            latest,
            &ExpansionColumns {
                type_id: vec![34],
                kind: vec![Some("weekday_hour".to_owned())],
                weekday: vec![Some(1)],
                hour: vec![Some(14)],
                sell: vec![Some(1_234)],
            },
        )
        .await
        .unwrap();
    assert!(
        !stores
            .station
            .expansions_stale(REGION, STATION, latest)
            .await
            .unwrap()
    );
    assert!(
        stores
            .station
            .expansions_stale(REGION, STATION, latest + Duration::hours(1))
            .await
            .unwrap()
    );
    let stats = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!(stats[0].expansion_kind.as_deref(), Some("weekday_hour"));
    assert_eq!(
        (stats[0].expansion_weekday, stats[0].expansion_hour),
        (Some(1), Some(14))
    );
    assert_eq!(stats[0].expansion_sell_cents, Some(1_234));
    // A row without an expansion window is stale as long as the hour differs.
    stores
        .station
        .update_expansions(
            REGION,
            STATION,
            latest,
            &ExpansionColumns {
                type_id: vec![34],
                kind: vec![None],
                weekday: vec![None],
                hour: vec![None],
                sell: vec![None],
            },
        )
        .await
        .unwrap();
    let cleared = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!(cleared[0].expansion_kind, None);
    assert_eq!(cleared[0].expansion_sell_cents, None);
}

async fn latest_snapshots(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    replace(
        stores,
        t,
        &[
            order(1, 34, true, 1_000, 1),
            order(2, 34, false, 1_100, 1),
            order(3, 35, true, 2_000, 1),
            order(4, 35, false, 2_100, 1),
        ],
    )
    .await;
    // Seeded snapshots: 34 has an older and a newer one, 35 only an old one, 36 is not tradable.
    let mut older = snapshot(t - Duration::hours(2), 9.0, 10.0);
    older.buy_near_orders = 7;
    let mut newer = snapshot(t - Duration::hours(1), 9.5, 10.5);
    newer.buy_near_orders = 8;
    let mut old35 = snapshot(t - Duration::days(3), 1.0, 2.0);
    old35.type_id = 35;
    let mut untradable = snapshot(t - Duration::hours(1), 1.0, 2.0);
    untradable.type_id = 36;
    stores
        .station
        .seed_snapshots(REGION, STATION, &[older, newer, old35, untradable])
        .await
        .unwrap();

    let rows = stores
        .station
        .latest_snapshots(REGION, STATION, t - Duration::days(1))
        .await
        .unwrap();

    let by_type: Vec<(i64, &str, i32)> = rows.iter().map(|r| (r.0, r.1.as_str(), r.3)).collect();
    // The book refresh at `t` is the newest snapshot of 34 and 35 now.
    assert_eq!(by_type, [(34, "10.00", 1), (35, "20.00", 1)]);
    let from_earlier = stores
        .station
        .latest_snapshots(REGION, STATION, t + Duration::minutes(1))
        .await
        .unwrap();
    assert!(from_earlier.is_empty());
    let depth = stores
        .opportunities
        .latest_depth(REGION, STATION, 34)
        .await
        .unwrap();
    assert_eq!(depth, Some((1, 1, 1, 1)));
    assert_eq!(
        stores
            .opportunities
            .latest_depth(REGION, STATION, 36)
            .await
            .unwrap(),
        Some((100, 2, 100, 2))
    );
    assert_eq!(
        stores
            .opportunities
            .latest_depth(REGION, STATION, 99)
            .await
            .unwrap(),
        None
    );
    // The days with a snapshot of the item with the oldest snapshot, 35's: three days ago, today.
    assert_eq!(
        stores
            .opportunities
            .own_snapshot_days(REGION, STATION)
            .await
            .unwrap(),
        2
    );
}

async fn seeds(stores: &Stores) {
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[snapshot(datetime!(2026-10-20 10:15 UTC), 10.123, 11.987)],
        )
        .await
        .unwrap();
    stores
        .station
        .seed_history(
            REGION,
            &[SeedHistoryRow {
                type_id: 34,
                day: date!(2026 - 10 - 19),
                average: 10.005,
                highest: 11.999,
                lowest: 9.994,
                volume: 7,
                order_count: 3,
            }],
        )
        .await
        .unwrap();
    // The same day again replaces it.
    stores
        .station
        .seed_history(
            REGION,
            &[SeedHistoryRow {
                type_id: 34,
                day: date!(2026 - 10 - 19),
                average: 20.0,
                highest: 21.0,
                lowest: 19.0,
                volume: 8,
                order_count: 4,
            }],
        )
        .await
        .unwrap();

    let depth = stores
        .opportunities
        .latest_depth(REGION, STATION, 34)
        .await
        .unwrap();
    assert_eq!(depth, Some((100, 2, 100, 2)));
    let days = stores
        .opportunities
        .history_days(REGION, 34, date!(2026 - 10 - 01))
        .await
        .unwrap();
    assert_eq!(days, [(date!(2026 - 10 - 19), 20.0, 8.0)]);
    // A snapshot's prices are kept to the cent and its spread is not.
    refresh(
        stores,
        datetime!(2026-10-20 00:00 UTC),
        datetime!(2026-10-21 00:00 UTC),
    )
    .await;
    let prices = stores
        .opportunities
        .price_points(REGION, STATION, 34, datetime!(2026-10-20 00:00 UTC))
        .await
        .unwrap();
    assert_close(prices[0].1, 10.12);
    assert_close(prices[0].2, 11.99);
    let spread = (11.987f64 - 10.123) / 10.123;
    assert_close(prices[0].3, spread);
    let zero = stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[snapshot(datetime!(2026-10-20 11:15 UTC), 0.0, 1.0)],
        )
        .await;
    assert!(zero.is_err(), "a best buy price of nothing has no spread");
}

async fn stored_rows(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    stores
        .station
        .seed_history(
            REGION,
            &[
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 19),
                    average: 10.5,
                    highest: 12.0,
                    lowest: 9.0,
                    volume: 7,
                    order_count: 3,
                },
                SeedHistoryRow {
                    type_id: 34,
                    day: date!(2026 - 10 - 18),
                    average: 9.25,
                    highest: 10.0,
                    lowest: 8.0,
                    volume: 8,
                    order_count: 4,
                },
                SeedHistoryRow {
                    type_id: 35,
                    day: date!(2026 - 10 - 19),
                    average: 1.0,
                    highest: 1.0,
                    lowest: 1.0,
                    volume: 1,
                    order_count: 1,
                },
            ],
        )
        .await
        .unwrap();

    let table = stores.station.history_table(REGION, 34).await.unwrap();
    assert_eq!(
        table
            .iter()
            .map(|h| (
                h.day,
                h.average.as_str(),
                h.highest.as_str(),
                h.lowest.as_str(),
                h.volume,
                h.order_count
            ))
            .collect::<Vec<_>>(),
        [
            (date!(2026 - 10 - 18), "9.25", "10.00", "8.00", 8, 4),
            (date!(2026 - 10 - 19), "10.50", "12.00", "9.00", 7, 3),
        ]
    );
    assert!(
        stores
            .station
            .history_table(REGION, 99)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        stores
            .station
            .history_table(1, 34)
            .await
            .unwrap()
            .is_empty()
    );

    assert!(
        stores
            .station
            .item_stats_row(REGION, STATION, 34)
            .await
            .unwrap()
            .is_none()
    );
    stores
        .station
        .upsert_book_fields(
            REGION,
            STATION,
            t,
            &[BookFields {
                type_id: 34,
                best_buy: "10.00".to_owned(),
                best_sell: "11.50".to_owned(),
                buy_near_orders: 2,
                sell_near_orders: 3,
                rho: 0.75,
            }],
        )
        .await
        .unwrap();
    let row = stores
        .station
        .item_stats_row(REGION, STATION, 34)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.book_at, Some(t));
    assert_eq!(
        (row.best_buy.as_deref(), row.best_sell.as_deref()),
        (Some("10.00"), Some("11.50"))
    );
    assert_eq!(
        (row.buy_near_orders, row.sell_near_orders, row.rho),
        (Some(2), Some(3), Some(0.75))
    );
    assert_eq!(
        (row.indicators_at, row.volume_7d, row.expansion_at),
        (None, None, None)
    );
    assert!(
        stores
            .station
            .item_stats_row(REGION, 99, 34)
            .await
            .unwrap()
            .is_none()
    );
}

async fn listed_buckets(stores: &Stores) {
    stores
        .station
        .seed_snapshots(
            REGION,
            STATION,
            &[
                snapshot(datetime!(2026-10-20 10:15 UTC), 10.0, 11.0),
                snapshot(datetime!(2026-10-20 10:45 UTC), 10.0, 11.0),
                snapshot(datetime!(2026-10-20 13:15 UTC), 10.0, 11.0),
                snapshot(datetime!(2026-10-21 02:15 UTC), 10.0, 11.0),
            ],
        )
        .await
        .unwrap();
    assert!(
        stores
            .station
            .hourly_buckets(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        stores
            .station
            .daily_buckets(REGION, STATION)
            .await
            .unwrap()
            .is_empty()
    );

    refresh(
        stores,
        datetime!(2026-10-20 00:00 UTC),
        datetime!(2026-10-22 00:00 UTC),
    )
    .await;

    assert_eq!(
        stores
            .station
            .hourly_buckets(REGION, STATION)
            .await
            .unwrap(),
        [
            datetime!(2026-10-20 10:00 UTC),
            datetime!(2026-10-20 13:00 UTC),
            datetime!(2026-10-21 02:00 UTC),
        ]
    );
    assert_eq!(
        stores.station.daily_buckets(REGION, STATION).await.unwrap(),
        [
            datetime!(2026-10-20 00:00 UTC),
            datetime!(2026-10-21 00:00 UTC)
        ]
    );
    assert!(
        stores
            .station
            .hourly_buckets(REGION, 99)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        stores
            .station
            .daily_buckets(1, STATION)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn seeded_item_row(stores: &Stores) {
    let t = datetime!(2026-10-20 10:05 UTC);
    let full = sekondo::store::ItemStatsRecord {
        book_at: Some(t),
        best_buy: Some("100.00".to_owned()),
        best_sell: Some("130.505".to_owned()),
        buy_near_orders: Some(4),
        sell_near_orders: Some(5),
        rho: Some(0.5),
        indicators_at: Some(t),
        volume_7d: Some(1.5),
        volume_30d: Some(2.5),
        sigma: Some(0.1),
        returns: Some(20),
        sma5: Some(3.0),
        sma20: Some(4.0),
        cross_day: Some(date!(2026 - 10 - 15)),
        volume_rising: Some(true),
        percent_b: Some(0.3),
        rsi14: Some(55.0),
        expansion_at: Some(t),
        expansion_kind: Some("hour".to_owned()),
        expansion_weekday: None,
        expansion_hour: Some(18),
        expansion_sell: Some("125.20".to_owned()),
    };

    stores
        .station
        .seed_item_stats(REGION, STATION, 34, &full)
        .await
        .unwrap();

    let stored = stores
        .station
        .item_stats_row(REGION, STATION, 34)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.best_sell.as_deref(),
        Some("130.51"),
        "kept to the cent"
    );
    assert_eq!(
        sekondo::store::ItemStatsRecord {
            best_sell: full.best_sell.clone(),
            ..stored
        },
        full
    );
    // A second seed replaces the row as a whole.
    let sparse = sekondo::store::ItemStatsRecord {
        book_at: Some(t),
        best_buy: Some("1.00".to_owned()),
        best_sell: Some("2.00".to_owned()),
        buy_near_orders: Some(1),
        sell_near_orders: Some(1),
        ..Default::default()
    };
    stores
        .station
        .seed_item_stats(REGION, STATION, 34, &sparse)
        .await
        .unwrap();
    let replaced = stores
        .station
        .item_stats_row(REGION, STATION, 34)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replaced, sparse);
    let listed = stores
        .opportunities
        .load_stats(REGION, STATION)
        .await
        .unwrap();
    assert_eq!(
        (listed[0].best_buy_cents, listed[0].expansion_kind.clone()),
        (100, None)
    );
    let bad = stores
        .station
        .seed_item_stats(
            REGION,
            STATION,
            35,
            &sekondo::store::ItemStatsRecord {
                best_buy: Some("lots".to_owned()),
                ..Default::default()
            },
        )
        .await;
    assert!(bad.is_err());
    assert!(
        stores
            .station
            .item_stats_row(REGION, STATION, 35)
            .await
            .unwrap()
            .is_none()
    );
}
