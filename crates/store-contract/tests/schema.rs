//! The schema and the migrations, against a freshly migrated PostgreSQL with TimescaleDB. Run with
//! `cargo test -p store-contract --features postgres`.
#![cfg(feature = "postgres")]

use sqlx::{PgPool, migrate::Migrator};

static MIGRATOR: Migrator = sqlx::migrate!("../sekondo/migrations");

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn insert_character(pool: &PgPool, account: uuid::Uuid, id: i64) {
    sqlx::query("INSERT INTO characters (character_id, account_id, name, owner_hash) VALUES ($1, $2, 'name', 'owner')")
        .bind(id)
        .bind(account)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO character_tokens (character_id, refresh_token) VALUES ($1, '\\x01')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_an_account_removes_its_characters_and_tokens(pool: PgPool) {
    let account: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let other: uuid::Uuid = sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    insert_character(&pool, account, 1).await;
    insert_character(&pool, account, 2).await;
    insert_character(&pool, other, 3).await;

    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(count(&pool, "accounts").await, 1);
    assert_eq!(count(&pool, "characters").await, 1);
    assert_eq!(count(&pool, "character_tokens").await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_a_character_removes_only_its_tokens(pool: PgPool) {
    let account: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    insert_character(&pool, account, 1).await;
    insert_character(&pool, account, 2).await;

    sqlx::query("DELETE FROM characters WHERE character_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(count(&pool, "characters").await, 1);
    assert_eq!(count(&pool, "character_tokens").await, 1);
    assert_eq!(count(&pool, "accounts").await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_character_belongs_to_exactly_one_account(pool: PgPool) {
    let a: uuid::Uuid = sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    let b: uuid::Uuid = sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    insert_character(&pool, a, 1).await;
    let duplicate = sqlx::query("INSERT INTO characters (character_id, account_id, name, owner_hash) VALUES (1, $1, 'x', 'y')")
        .bind(b)
        .execute(&pool)
        .await;
    assert!(duplicate.is_err());
}

async fn insert_account(pool: &PgPool) -> uuid::Uuid {
    sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn order_snapshots_is_a_hypertable(pool: PgPool) {
    let hypertables: Vec<String> =
        sqlx::query_scalar("SELECT hypertable_name::text FROM timescaledb_information.hypertables")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        hypertables.contains(&"order_snapshots".to_owned()),
        "{hypertables:?}"
    );

    let interval_secs: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM d.time_interval)::float8 FROM timescaledb_information.dimensions d \
         WHERE d.hypertable_name = 'order_snapshots'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(interval_secs, 7.0 * 86_400.0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_a_character_removes_its_order_data(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    insert_character(&pool, account, 2).await;

    for character in [1_i64, 2] {
        sqlx::query(
            "INSERT INTO current_orders (order_id, character_id, type_id, region_id, location_id, \
             is_buy_order, is_corporation, price, volume_remain, volume_total, min_volume, range, \
             issued, duration) \
             VALUES ($1, $1, 34, 10000002, 60003760, false, false, 5.50, 1, 2, 1, 'station', now(), 90)",
        )
        .bind(character)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO order_snapshots (polled_at, character_id, order_id, type_id, region_id, \
             location_id, is_buy_order, is_corporation, price, volume_remain, volume_total, \
             min_volume, range, issued, duration) \
             VALUES (now(), $1, $1, 34, 10000002, 60003760, false, false, 5.50, 1, 2, 1, 'station', now(), 90)",
        )
        .bind(character)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO order_sync (character_id, next_poll_at) VALUES ($1, now())")
            .bind(character)
            .execute(&pool)
            .await
            .unwrap();
    }

    sqlx::query("DELETE FROM characters WHERE character_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    for table in ["current_orders", "order_snapshots", "order_sync"] {
        assert_eq!(count(&pool, table).await, 1, "{table}");
        let remaining: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT character_id FROM {table}"
        )))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 2, "{table}");
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn names_can_hold_an_unresolvable_structure(pool: PgPool) {
    sqlx::query(
        "INSERT INTO names (id, category, name, resolved_at, retry_after) \
         VALUES (1000000000001, 'structure', NULL, now(), now() + interval '24 hours')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(count(&pool, "names").await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn order_poll_totals_is_a_hypertable_with_thirty_day_chunks(pool: PgPool) {
    let hypertables: Vec<String> =
        sqlx::query_scalar("SELECT hypertable_name::text FROM timescaledb_information.hypertables")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        hypertables.contains(&"order_poll_totals".to_owned()),
        "{hypertables:?}"
    );

    let interval_secs: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM d.time_interval)::float8 FROM timescaledb_information.dimensions d \
         WHERE d.hypertable_name = 'order_poll_totals'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(interval_secs, 30.0 * 86_400.0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_a_character_removes_its_poll_totals(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    insert_character(&pool, account, 2).await;
    for character in [1_i64, 2] {
        sqlx::query(
            "INSERT INTO order_poll_totals (character_id, polled_at, sell_value, escrow, sell_corp, \
             escrow_corp, sell_count, buy_count) VALUES ($1, now(), 0, 0, 0, 0, 0, 0)",
        )
        .bind(character)
        .execute(&pool)
        .await
        .unwrap();
    }

    sqlx::query("DELETE FROM characters WHERE character_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let remaining: Vec<i64> = sqlx::query_scalar("SELECT character_id FROM order_poll_totals")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, vec![2]);
}

/// Everything in the migration after the `-- backfill` marker.
fn backfill_statement() -> &'static str {
    let migration = include_str!("../../sekondo/migrations/20261016000004_order_poll_totals.sql");
    migration
        .split_once("-- backfill\n")
        .expect("a `-- backfill` marker in the migration")
        .1
}

async fn insert_snapshot(
    pool: &PgPool,
    polled_at: &str,
    (character, order_id): (i64, i64),
    (is_buy, is_corp): (bool, bool),
    price: &str,
    volume_remain: i64,
    escrow: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO order_snapshots (polled_at, character_id, order_id, type_id, region_id, \
         location_id, is_buy_order, is_corporation, price, volume_remain, volume_total, \
         min_volume, range, escrow, issued, duration) \
         VALUES ($1::timestamptz, $2, $3, 34, 10000002, 60003760, $4, $5, $6::numeric, $7, 100, 1, \
         'station', $8::numeric, now(), 90)",
    )
    .bind(polled_at)
    .bind(character)
    .bind(order_id)
    .bind(is_buy)
    .bind(is_corp)
    .bind(price)
    .bind(volume_remain)
    .bind(escrow)
    .execute(pool)
    .await
    .unwrap();
}

type Totals = (String, String, String, String, i32, i32);

async fn totals_at(pool: &PgPool, character: i64, polled_at: &str) -> Totals {
    sqlx::query_as(
        "SELECT sell_value::text, escrow::text, sell_corp::text, escrow_corp::text, sell_count, \
         buy_count FROM order_poll_totals WHERE character_id = $1 AND polled_at = $2::timestamptz",
    )
    .bind(character)
    .bind(polled_at)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_backfill_sums_each_snapshot_poll_into_one_entry(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    insert_character(&pool, account, 2).await;
    let (sell, buy) = ((false, false), (true, false));
    let (corp_sell, corp_buy) = ((false, true), (true, true));

    // Poll one of character 1: two sell orders (one corporation's) and two buy orders (one
    // corporation's, one without an escrow).
    let t1 = "2026-10-01 10:00:00+00";
    insert_snapshot(&pool, t1, (1, 1), sell, "10.00", 3, None).await;
    insert_snapshot(&pool, t1, (1, 2), corp_sell, "5.00", 2, None).await;
    insert_snapshot(&pool, t1, (1, 3), buy, "4.00", 5, Some("2.00")).await;
    insert_snapshot(&pool, t1, (1, 4), corp_buy, "9.00", 1, Some("7.50")).await;
    insert_snapshot(&pool, t1, (1, 5), buy, "1.00", 1, None).await;
    // Poll two of character 1, and a poll of character 2 at the first time.
    let t2 = "2026-10-01 10:20:00+00";
    insert_snapshot(&pool, t2, (1, 1), sell, "1.00", 1, None).await;
    insert_snapshot(&pool, t1, (2, 6), sell, "2.50", 4, None).await;

    sqlx::query(sqlx::AssertSqlSafe(backfill_statement().to_owned()))
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(count(&pool, "order_poll_totals").await, 3);
    assert_eq!(
        totals_at(&pool, 1, t1).await,
        (
            "40.00".into(),
            "9.50".into(),
            "10.00".into(),
            "7.50".into(),
            2,
            3
        )
    );
    assert_eq!(
        totals_at(&pool, 1, t2).await,
        (
            "1.00".into(),
            "0.00".into(),
            "0.00".into(),
            "0.00".into(),
            1,
            0
        )
    );
    assert_eq!(
        totals_at(&pool, 2, t1).await,
        (
            "10.00".into(),
            "0.00".into(),
            "0.00".into(),
            "0.00".into(),
            1,
            0
        )
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn market_orders_hold_one_public_order_per_id_and_no_account_data(pool: PgPool) {
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'market_orders' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        columns,
        [
            "is_buy_order",
            "location_id",
            "order_id",
            "price",
            "range",
            "region_id",
            "system_id",
            "type_id",
            "volume_remain"
        ]
    );
    let insert = "INSERT INTO market_orders \
        (order_id, region_id, type_id, location_id, system_id, is_buy_order, price, range) \
        VALUES (1, 10000002, 34, 60003760, 30000142, false, 10.50, 'station')";
    sqlx::query(insert).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(insert).execute(&pool).await.is_err(),
        "order_id is the key"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn market_orders_are_indexed_by_region_and_item(pool: PgPool) {
    let indexes: Vec<String> =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE tablename = 'market_orders'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        indexes.iter().any(|d| d.contains("(region_id, type_id)")),
        "{indexes:?}"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn market_sync_has_one_row_per_region_and_item(pool: PgPool) {
    let insert = "INSERT INTO market_sync (region_id, type_id, next_poll_at) \
                  VALUES (10000002, 34, now())";
    sqlx::query(insert).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(insert).execute(&pool).await.is_err(),
        "(region_id, type_id) is the key"
    );
    let failures: i32 =
        sqlx::query_scalar("SELECT failures FROM market_sync WHERE region_id = 10000002")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(failures, 0, "no failures yet");
    for column in [
        "last_success_at",
        "last_attempt_at",
        "last_error",
        "esi_expires_at",
    ] {
        let found: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM information_schema.columns \
             WHERE table_name = 'market_sync' AND column_name = $1",
        )
        .bind(column)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(found, 1, "{column}");
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn wallet_balances_and_inventory_values_are_hypertables_with_thirty_day_chunks(pool: PgPool) {
    let hypertables: Vec<String> =
        sqlx::query_scalar("SELECT hypertable_name::text FROM timescaledb_information.hypertables")
            .fetch_all(&pool)
            .await
            .unwrap();
    for table in ["wallet_balances", "inventory_values"] {
        assert!(
            hypertables.contains(&table.to_owned()),
            "{table}: {hypertables:?}"
        );
        let interval_secs: f64 = sqlx::query_scalar(
            "SELECT extract(epoch FROM d.time_interval)::float8 \
             FROM timescaledb_information.dimensions d WHERE d.hypertable_name = $1",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(interval_secs, 30.0 * 86_400.0, "{table}");
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_wealth_tables_exist_and_the_price_tables_have_no_owner(pool: PgPool) {
    for table in [
        "wallet_balances",
        "current_assets",
        "item_prices",
        "price_sync",
        "inventory_values",
        "wealth_sync",
    ] {
        assert_eq!(count(&pool, table).await, 0, "{table}");
    }
    let owned: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.columns \
         WHERE table_name IN ('item_prices', 'price_sync') \
           AND column_name IN ('account_id', 'character_id')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(owned.is_empty(), "{owned:?}");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn wealth_sync_only_knows_wallet_and_assets(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let bad = sqlx::query(
        "INSERT INTO wealth_sync (character_id, kind, next_poll_at) VALUES (1, 'orders', now())",
    )
    .execute(&pool)
    .await;
    assert!(bad.is_err());
    for kind in ["wallet", "assets"] {
        sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at) VALUES (1, $1, now())",
        )
        .bind(kind)
        .execute(&pool)
        .await
        .unwrap();
    }
    let twice = sqlx::query(
        "INSERT INTO wealth_sync (character_id, kind, next_poll_at) VALUES (1, 'wallet', now())",
    )
    .execute(&pool)
    .await;
    assert!(twice.is_err(), "one row per character and kind");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_fee_tables_exist_and_the_public_ones_have_no_owner(pool: PgPool) {
    for table in [
        "character_skills",
        "character_standings",
        "station_owners",
        "character_locations",
        "fee_sync",
    ] {
        assert_eq!(count(&pool, table).await, 0, "{table}");
    }
    let owned: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.columns \
         WHERE table_name IN ('station_owners', 'npc_corporation_factions') \
           AND column_name IN ('account_id', 'character_id')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(owned.is_empty(), "{owned:?}");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_faction_table_is_loaded_from_the_checked_in_csv(pool: PgPool) {
    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sekondo/data/npc_corporation_factions.csv"
    ))
    .unwrap();
    let mut expected: Vec<(i64, i64)> = csv
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("corporation_id"))
        .map(|l| {
            let (corp, faction) = l.split_once(',').unwrap();
            (corp.parse().unwrap(), faction.parse().unwrap())
        })
        .collect();
    expected.sort_unstable();
    let loaded: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT corporation_id, faction_id FROM npc_corporation_factions ORDER BY corporation_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(loaded, expected);
    assert!(
        loaded.contains(&(1000035, 500001)),
        "Caldari Navy belongs to Caldari State"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn fee_sync_only_knows_skills_standings_and_location(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let bad = sqlx::query(
        "INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES (1, 'wallet', now())",
    )
    .execute(&pool)
    .await;
    assert!(bad.is_err());
    for kind in ["skills", "standings", "location"] {
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES (1, $1, now())",
        )
        .bind(kind)
        .execute(&pool)
        .await
        .unwrap();
    }
    let twice = sqlx::query(
        "INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES (1, 'skills', now())",
    )
    .execute(&pool)
    .await;
    assert!(twice.is_err(), "one row per character and kind");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_character_has_one_location_that_is_a_station_or_a_structure(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let insert = |kind: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO character_locations (character_id, location_id, kind, updated_at) \
                 VALUES (1, 60003760, $1, now())",
            )
            .bind(kind)
            .execute(&pool)
            .await
        }
    };
    assert!(
        insert("system").await.is_err(),
        "only stations and structures"
    );
    insert("structure").await.unwrap();
    assert!(insert("station").await.is_err(), "one row per character");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn standings_keep_every_digit_and_only_know_npc_corporations_and_factions(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    sqlx::query(
        "INSERT INTO character_standings (character_id, from_id, from_type, standing) \
         VALUES (1, 1000035, 'npc_corp', '0.07999999821186066'::numeric)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let text: String = sqlx::query_scalar("SELECT standing::text FROM character_standings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(text, "0.07999999821186066");
    let agent = sqlx::query(
        "INSERT INTO character_standings (character_id, from_id, from_type, standing) \
         VALUES (1, 3009841, 'agent', 3.2)",
    )
    .execute(&pool)
    .await;
    assert!(agent.is_err());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn deleting_a_character_removes_its_fee_data_and_keeps_the_public_tables(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    for statement in [
        "INSERT INTO character_skills (character_id, skill_id, active_level) VALUES (1, 16622, 4)",
        "INSERT INTO character_standings (character_id, from_id, from_type, standing) VALUES (1, 500001, 'faction', 2)",
        "INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES (1, 'skills', now())",
        "INSERT INTO character_locations (character_id, location_id, kind, updated_at) VALUES (1, 60003760, 'station', now())",
        "INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES (60003760, 1000035, now())",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    sqlx::query("DELETE FROM characters WHERE character_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    for table in [
        "character_skills",
        "character_standings",
        "character_locations",
        "fee_sync",
    ] {
        assert_eq!(count(&pool, table).await, 0, "{table}");
    }
    assert_eq!(count(&pool, "station_owners").await, 1);
}

// ---- trade margin: purchases, remembered sell orders, costs ----

/// Everything in the migration after the `-- backfill` marker.
fn history_backfill_statement() -> &'static str {
    include_str!("../../sekondo/migrations/20261017000005_trade_margin.sql")
        .split_once("-- backfill\n")
        .expect("a `-- backfill` marker in the migration")
        .1
}

async fn insert_sell_snapshot(
    pool: &PgPool,
    (polled_at, issued): (&str, &str),
    (character, order_id): (i64, i64),
    (is_buy, is_corp): (bool, bool),
) {
    sqlx::query(
        "INSERT INTO order_snapshots (polled_at, character_id, order_id, type_id, region_id, \
         location_id, is_buy_order, is_corporation, price, volume_remain, volume_total, \
         min_volume, range, escrow, issued, duration) \
         VALUES ($1::timestamptz, $2, $3, 34, 10000002, 60003760, $4, $5, 10, 3, 20, 1, \
         'station', NULL, $6::timestamptz, 90)",
    )
    .bind(polled_at)
    .bind(character)
    .bind(order_id)
    .bind(is_buy)
    .bind(is_corp)
    .bind(issued)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_history_backfill_keeps_the_earliest_issued_and_skips_corp_and_buy_orders(
    pool: PgPool,
) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let (sell, buy, corp_sell) = ((false, false), (true, false), (false, true));
    // Order 1 is repriced between the two polls and ESI then reports a later issue time.
    insert_sell_snapshot(
        &pool,
        ("2026-10-01 10:00+00", "2026-10-01 10:00+00"),
        (1, 1),
        sell,
    )
    .await;
    insert_sell_snapshot(
        &pool,
        ("2026-10-01 14:20+00", "2026-10-01 14:00+00"),
        (1, 1),
        sell,
    )
    .await;
    insert_sell_snapshot(
        &pool,
        ("2026-10-01 10:00+00", "2026-10-01 09:00+00"),
        (1, 2),
        buy,
    )
    .await;
    insert_sell_snapshot(
        &pool,
        ("2026-10-01 10:00+00", "2026-10-01 09:00+00"),
        (1, 3),
        corp_sell,
    )
    .await;
    sqlx::query("DELETE FROM sell_order_history")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(history_backfill_statement())
        .execute(&pool)
        .await
        .unwrap();

    let rows: Vec<(i64, i64, i64, String, i64)> = sqlx::query_as(
        "SELECT order_id, character_id, type_id, first_issued::text, volume_total \
         FROM sell_order_history ORDER BY order_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows, [(1, 1, 34, "2026-10-01 10:00:00+00".to_owned(), 20)]);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn purchases_and_remembered_orders_are_keyed_and_indexed_for_the_allocation(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let purchase = "INSERT INTO purchases (transaction_id, character_id, type_id, location_id, \
                    quantity, unit_price, bought_at) \
                    VALUES (7, 1, 34, 60003760, 10, 100.25, now())";
    sqlx::query(purchase).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(purchase).execute(&pool).await.is_err(),
        "transaction_id is the key"
    );
    let history = "INSERT INTO sell_order_history (order_id, character_id, type_id, first_issued, \
                   volume_total) VALUES (9, 1, 34, now(), 20)";
    sqlx::query(history).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(history).execute(&pool).await.is_err(),
        "order_id is the key"
    );
    sqlx::query(
        "INSERT INTO sell_order_costs (order_id, covered_units, cost) VALUES (9, 20, 2005.00)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let indexes: Vec<String> =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE tablename = 'purchases'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        indexes
            .iter()
            .any(|d| d.contains("(character_id, type_id, bought_at, transaction_id)")),
        "{indexes:?}"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_transactions_sync_row_is_allowed_and_an_unknown_kind_is_not(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let insert = |kind: &'static str| {
        sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at) VALUES (1, $1, now())",
        )
        .bind(kind)
    };
    insert("transactions").execute(&pool).await.unwrap();
    assert!(insert("journal").execute(&pool).await.is_err());
}

/// Everything in the trades migration after the `-- full read` marker.
fn full_read_statement() -> &'static str {
    include_str!("../../sekondo/migrations/20261019000001_trades.sql")
        .split_once("-- full read\n")
        .expect("a `-- full read` marker in the migration")
        .1
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_full_read_flag_is_set_for_existing_transactions_rows_only(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    for kind in ["wallet", "assets", "transactions"] {
        sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at) VALUES (1, $1, now())",
        )
        .bind(kind)
        .execute(&pool)
        .await
        .unwrap();
    }
    async fn flags(pool: &PgPool) -> Result<Vec<(String, bool)>, sqlx::Error> {
        sqlx::query_as("SELECT kind, full_read FROM wealth_sync ORDER BY kind")
            .fetch_all(pool)
            .await
    }
    assert_eq!(
        flags(&pool).await.unwrap(),
        [
            ("assets".to_owned(), false),
            ("transactions".to_owned(), false),
            ("wallet".to_owned(), false)
        ],
        "a new row is created without the flag"
    );

    sqlx::query(full_read_statement())
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        flags(&pool).await.unwrap(),
        [
            ("assets".to_owned(), false),
            ("transactions".to_owned(), true),
            ("wallet".to_owned(), false)
        ]
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn sales_and_their_costs_are_keyed_indexed_and_cascade(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let sale = "INSERT INTO sales (transaction_id, character_id, type_id, location_id, quantity, \
                unit_price, sold_at) VALUES (7, 1, 34, 60003760, 10, 150.25, now())";
    sqlx::query(sale).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(sale).execute(&pool).await.is_err(),
        "transaction_id is the key"
    );
    sqlx::query(
        "INSERT INTO sale_costs (transaction_id, covered_units, cost) VALUES (7, 10, 1000.00)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let indexes: Vec<String> =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE tablename = 'sales'")
            .fetch_all(&pool)
            .await
            .unwrap();
    for columns in [
        "(character_id, type_id, sold_at, transaction_id)",
        "(character_id, sold_at)",
    ] {
        assert!(
            indexes.iter().any(|d| d.contains(columns)),
            "{columns}: {indexes:?}"
        );
    }

    sqlx::query("DELETE FROM sales WHERE transaction_id = 7")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        count(&pool, "sale_costs").await,
        0,
        "costs go with the sale"
    );
}

async fn columns_of(pool: &PgPool, table: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_name = $1 ORDER BY column_name",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn station_snapshots_is_a_hypertable_compressed_after_a_day_and_kept_thirty(pool: PgPool) {
    let interval_secs: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM d.time_interval)::float8 FROM timescaledb_information.dimensions d \
         WHERE d.hypertable_name = 'station_snapshots'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(interval_secs, 86_400.0);

    let segmentby: Vec<String> = sqlx::query_scalar(
        "SELECT attname::text FROM timescaledb_information.compression_settings \
         WHERE hypertable_name = 'station_snapshots' AND segmentby_column_index IS NOT NULL",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(segmentby, ["type_id"]);

    let after = |proc: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, f64>(
                "SELECT extract(epoch FROM coalesce( \
                     (config->>'compress_after')::interval, (config->>'drop_after')::interval))::float8 \
                 FROM timescaledb_information.jobs \
                 WHERE hypertable_name = 'station_snapshots' AND proc_name = $1",
            )
            .bind(proc)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(after("policy_compression").await, 86_400.0);
    assert_eq!(after("policy_retention").await, 30.0 * 86_400.0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_station_tables_hold_the_book_the_snapshot_and_the_sync_state(pool: PgPool) {
    assert_eq!(
        columns_of(&pool, "station_orders").await,
        [
            "is_buy_order",
            "order_id",
            "price",
            "region_id",
            "station_id",
            "type_id",
            "volume_remain"
        ]
    );
    assert_eq!(
        columns_of(&pool, "station_snapshots").await,
        [
            "best_buy",
            "best_sell",
            "buy_near_orders",
            "buy_near_units",
            "buy_taken",
            "buy_units",
            "recorded_at",
            "region_id",
            "sell_near_orders",
            "sell_near_units",
            "sell_taken",
            "sell_units",
            "spread",
            "station_id",
            "type_id"
        ]
    );
    let order = "INSERT INTO station_orders (region_id, station_id, order_id, type_id, is_buy_order, price, volume_remain) \
                 VALUES (10000002, 60003760, 1, 34, false, 10.50, 3)";
    sqlx::query(order).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(order).execute(&pool).await.is_err(),
        "order_id is the key"
    );

    let sync = "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at) \
                VALUES (10000002, 60003760, $1, now())";
    sqlx::query(sync).bind("book").execute(&pool).await.unwrap();
    sqlx::query(sync)
        .bind("history")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sqlx::query(sync).bind("book").execute(&pool).await.is_err(),
        "one row per kind"
    );
    assert!(
        sqlx::query(sync)
            .bind("other")
            .execute(&pool)
            .await
            .is_err(),
        "unknown kind"
    );
    let failures: i32 = sqlx::query_scalar("SELECT failures FROM station_sync LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(failures, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn market_history_and_its_sync_state_are_keyed_per_item_and_day(pool: PgPool) {
    assert_eq!(
        columns_of(&pool, "market_history").await,
        [
            "average",
            "day",
            "highest",
            "lowest",
            "order_count",
            "region_id",
            "type_id",
            "volume"
        ]
    );
    let day = "INSERT INTO market_history (region_id, type_id, day, average, highest, lowest, volume, order_count) \
               VALUES (10000002, 34, '2026-10-16', 5.5, 6, 5, 100, 10)";
    sqlx::query(day).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(day).execute(&pool).await.is_err(),
        "one row per item and day"
    );

    let sync =
        "INSERT INTO history_sync (region_id, type_id, next_poll_at) VALUES (10000002, 34, now())";
    sqlx::query(sync).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(sync).execute(&pool).await.is_err(),
        "one row per item"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn item_stats_are_one_row_per_item_and_hold_no_account_data(pool: PgPool) {
    let columns = columns_of(&pool, "station_item_stats").await;
    for column in [
        "region_id",
        "station_id",
        "type_id",
        "book_at",
        "best_buy",
        "best_sell",
        "buy_near_orders",
        "sell_near_orders",
        "rho",
        "volume_7d",
        "volume_30d",
        "sigma",
        "returns",
        "sma5",
        "sma20",
        "cross_day",
        "percent_b",
        "rsi14",
        "indicators_at",
        "expansion_at",
    ] {
        assert!(
            columns.iter().any(|c| c == column),
            "{column} in {columns:?}"
        );
    }
    // The windows live in their own table.
    for column in [
        "expansion_kind",
        "expansion_weekday",
        "expansion_hour",
        "expansion_sell",
    ] {
        assert!(!columns.iter().any(|c| c == column), "{column} is gone");
    }
    assert!(
        !columns
            .iter()
            .any(|c| c.contains("account") || c.contains("character"))
    );
    let insert = "INSERT INTO station_item_stats (region_id, station_id, type_id) VALUES (10000002, 60003760, 34)";
    sqlx::query(insert).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(insert).execute(&pool).await.is_err(),
        "one row per item"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn opportunity_settings_default_and_go_with_the_account(pool: PgPool) {
    let account = insert_account(&pool).await;
    let other = insert_account(&pool).await;
    for id in [account, other] {
        sqlx::query("INSERT INTO opportunity_settings (account_id) VALUES ($1)")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let row: (String, String, String, String, String) = sqlx::query_as(
        "SELECT min_margin::text, max_margin::text, min_isk_per_day::text, search, capital::text \
         FROM opportunity_settings WHERE account_id = $1",
    )
    .bind(account)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "0.05".into(),
            "0.80".into(),
            "5000000".into(),
            String::new(),
            "100000000".into()
        )
    );
    assert!(
        sqlx::query("INSERT INTO opportunity_settings (account_id) VALUES ($1)")
            .bind(account)
            .execute(&pool)
            .await
            .is_err(),
        "one row per account"
    );

    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(count(&pool, "opportunity_settings").await, 1);
}

async fn snapshot_at(
    pool: &PgPool,
    at: &str,
    type_id: i64,
    buy: f64,
    sell: f64,
    taken: (i64, i64),
) {
    sqlx::query(
        "INSERT INTO station_snapshots (recorded_at, region_id, station_id, type_id, best_buy, best_sell, \
             spread, buy_near_units, buy_near_orders, sell_near_units, sell_near_orders, \
             buy_units, sell_units, buy_taken, sell_taken) \
         VALUES ($1::timestamptz, 10000002, 60003760, $2, $3::numeric, $4::numeric, \
             ($4::numeric - $3::numeric) / $3::numeric, 10, 2, 20, 3, 100, 200, $5, $6)",
    )
    .bind(at)
    .bind(type_id)
    .bind(buy)
    .bind(sell)
    .bind(taken.0)
    .bind(taken.1)
    .execute(pool)
    .await
    .unwrap();
}

async fn history_day(
    pool: &PgPool,
    type_id: i64,
    day: &str,
    average: f64,
    highest: f64,
    lowest: f64,
    volume: i64,
) {
    sqlx::query(
        "INSERT INTO market_history (region_id, type_id, day, average, highest, lowest, volume, order_count) \
         VALUES (10000002, $1, $2::date, $3::numeric, $4::numeric, $5::numeric, $6, 10)",
    )
    .bind(type_id)
    .bind(day)
    .bind(average)
    .bind(highest)
    .bind(lowest)
    .bind(volume)
    .execute(pool)
    .await
    .unwrap();
}

async fn roll_up(pool: &PgPool) {
    for view in ["station_hourly", "station_daily"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CALL refresh_continuous_aggregate('{view}', '2026-08-01', '2026-10-01')"
        )))
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn an_hour_keeps_minimum_maximum_and_average_apart(pool: PgPool) {
    for (at, sell, taken) in [
        ("2026-09-01 10:05:00+00", 110.0, 1),
        ("2026-09-01 10:20:00+00", 130.0, 2),
        ("2026-09-01 10:40:00+00", 120.0, 4),
    ] {
        snapshot_at(&pool, at, 34, 100.0, sell, (taken, taken * 10)).await;
    }
    roll_up(&pool).await;

    let row: (String, String, String, i64, i64, i64) = sqlx::query_as(
        "SELECT best_sell_min::text, best_sell_max::text, round(best_sell_avg, 2)::text, \
                snapshots, buy_taken, sell_taken \
         FROM station_hourly WHERE bucket = '2026-09-01 10:00:00+00' AND type_id = 34",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        row,
        ("110.00".into(), "130.00".into(), "120.00".into(), 3, 7, 70)
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_day_averages_over_its_snapshots_not_over_its_hourly_averages(pool: PgPool) {
    // Hour 0 has three snapshots at 100, hour 1 has one at 200: the day's average is 125, not 150.
    for at in [
        "2026-09-01 00:10:00+00",
        "2026-09-01 00:20:00+00",
        "2026-09-01 00:30:00+00",
    ] {
        snapshot_at(&pool, at, 34, 90.0, 100.0, (1, 1)).await;
    }
    snapshot_at(&pool, "2026-09-01 01:10:00+00", 34, 90.0, 200.0, (1, 1)).await;
    roll_up(&pool).await;

    let row: (String, String, String, i64, i64, i64) = sqlx::query_as(
        "SELECT best_sell_min::text, best_sell_max::text, round(best_sell_avg, 2)::text, \
                hours_covered, buy_taken, sell_taken \
         FROM station_daily WHERE bucket = '2026-09-01 00:00:00+00' AND type_id = 34",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        row,
        ("100.00".into(), "200.00".into(), "125.00".into(), 2, 4, 4)
    );
}

async fn fill_hours(pool: &PgPool, day: &str, hours: std::ops::Range<u32>) {
    for hour in hours {
        let at = format!("{day} {hour:02}:30:00+00");
        snapshot_at(
            pool,
            &at,
            34,
            100.0 + f64::from(hour),
            120.0 + f64::from(hour),
            (0, 0),
        )
        .await;
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_covered_day_takes_low_and_high_from_its_own_data(pool: PgPool) {
    fill_hours(&pool, "2026-09-01", 0..24).await;
    history_day(&pool, 34, "2026-09-01", 111.5, 500.0, 50.0, 7777).await;
    roll_up(&pool).await;

    let row: (String, String, String, i64, String, i64) = sqlx::query_as(
        "SELECT low::text, high::text, source, hours_covered, average::text, volume \
         FROM station_daily_figures WHERE type_id = 34 AND day = '2026-09-01'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Lowest best buy of the day is 100 (hour 0), highest best sell is 143 (hour 23).
    assert_eq!(
        row,
        (
            "100.00".into(),
            "143.00".into(),
            "own".into(),
            24,
            "111.50".into(),
            7777
        )
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_gap_in_own_data_falls_back_to_the_history_and_says_so(pool: PgPool) {
    fill_hours(&pool, "2026-09-01", 0..5).await;
    history_day(&pool, 34, "2026-09-01", 111.5, 500.0, 50.0, 7777).await;
    roll_up(&pool).await;

    let row: (String, String, String, String, String, i64) = sqlx::query_as(
        "SELECT low::text, high::text, round(spread, 4)::text, source, average::text, volume \
         FROM station_daily_figures WHERE type_id = 34 AND day = '2026-09-01'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // (500 - 50) / 50 = 9 from the history's highest and lowest.
    assert_eq!(
        row,
        (
            "50.00".into(),
            "500.00".into(),
            "9.0000".into(),
            "history".into(),
            "111.50".into(),
            7777
        )
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_day_of_exactly_twenty_hours_counts_as_covered_and_nineteen_does_not(pool: PgPool) {
    fill_hours(&pool, "2026-09-01", 0..20).await;
    fill_hours(&pool, "2026-09-02", 0..19).await;
    for day in ["2026-09-01", "2026-09-02"] {
        history_day(&pool, 34, day, 100.0, 500.0, 50.0, 1).await;
    }
    roll_up(&pool).await;

    let sources: Vec<(String, String)> = sqlx::query_as(
        "SELECT day::text, source FROM station_daily_figures WHERE type_id = 34 ORDER BY day",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        sources,
        [
            ("2026-09-01".into(), "own".into()),
            ("2026-09-02".into(), "history".into())
        ]
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_rollups_only_read_completed_periods_and_have_policies(pool: PgPool) {
    for view in ["station_hourly", "station_daily"] {
        let materialized_only: bool = sqlx::query_scalar(
            "SELECT materialized_only FROM timescaledb_information.continuous_aggregates \
             WHERE view_name = $1",
        )
        .bind(view)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(materialized_only, "{view}");

        for proc in [
            "policy_refresh_continuous_aggregate",
            "policy_retention",
            "policy_compression",
        ] {
            let jobs: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM timescaledb_information.jobs \
                 WHERE hypertable_name = (SELECT materialization_hypertable_name \
                     FROM timescaledb_information.continuous_aggregates WHERE view_name = $1) \
                 AND proc_name = $2",
            )
            .bind(view)
            .bind(proc)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(jobs, 1, "{view} {proc}");
        }
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_rollups_are_kept_a_year_and_compressed_after_a_week(pool: PgPool) {
    for view in ["station_hourly", "station_daily"] {
        let after = |proc: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, f64>(
                    "SELECT extract(epoch FROM coalesce( \
                         (config->>'compress_after')::interval, (config->>'drop_after')::interval))::float8 \
                     FROM timescaledb_information.jobs \
                     WHERE hypertable_name = (SELECT materialization_hypertable_name \
                         FROM timescaledb_information.continuous_aggregates WHERE view_name = $1) \
                     AND proc_name = $2",
                )
                .bind(view)
                .bind(proc)
                .fetch_one(&pool)
                .await
                .unwrap()
            }
        };
        assert_eq!(after("policy_retention").await, 365.0 * 86_400.0, "{view}");
        assert_eq!(after("policy_compression").await, 7.0 * 86_400.0, "{view}");
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_flag_tables_and_the_windows_have_their_keys(pool: PgPool) {
    assert_eq!(
        columns_of(&pool, "station_flagged_days").await,
        [
            "day",
            "expanded_hours",
            "hours",
            "region_id",
            "station_id",
            "type_id"
        ]
    );
    assert_eq!(
        columns_of(&pool, "station_expanded_hours").await,
        [
            "direction",
            "extreme_cents",
            "ratio",
            "region_id",
            "start",
            "station_id",
            "taken",
            "type_id"
        ]
    );
    assert_eq!(
        columns_of(&pool, "station_item_windows").await,
        [
            "covered",
            "direction",
            "hour",
            "kind",
            "occurrences",
            "price_cents",
            "region_id",
            "station_id",
            "type_id",
            "v_win",
            "weekday"
        ]
    );
    let day = "INSERT INTO station_flagged_days VALUES (1, 2, 34, '2026-10-01', 24, 1)";
    sqlx::query(day).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(day).execute(&pool).await.is_err(),
        "one row per item and day"
    );

    let hour = "INSERT INTO station_expanded_hours VALUES (1, 2, 34, '2026-10-01 18:00+00', 'dip', 0.8, 8000, 5)";
    sqlx::query(hour).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(hour).execute(&pool).await.is_err(),
        "one row per item and hour"
    );
    assert!(
        sqlx::query("INSERT INTO station_expanded_hours VALUES (1, 2, 34, '2026-10-02 18:00+00', 'sideways', 0.8, 8000, 5)")
            .execute(&pool)
            .await
            .is_err(),
        "a direction is dip or spike"
    );

    let window = "INSERT INTO station_item_windows VALUES (1, 2, 34, 'hour', NULL, 18, 'spike', 12520, 14, 20, 100)";
    sqlx::query(window).execute(&pool).await.unwrap();
    assert!(
        sqlx::query(window).execute(&pool).await.is_err(),
        "one window per item, kind, weekday, hour and direction, also without a weekday"
    );
    sqlx::query("INSERT INTO station_item_windows VALUES (1, 2, 34, 'hour', NULL, 18, 'dip', 8001, 14, 20, 100)")
        .execute(&pool)
        .await
        .unwrap();
}
