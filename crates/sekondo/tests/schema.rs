use sqlx::PgPool;

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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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
    let migration = include_str!("../migrations/20261016000004_order_poll_totals.sql");
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
async fn the_fee_tables_exist_and_the_public_ones_have_no_owner(pool: PgPool) {
    for table in [
        "character_skills",
        "character_standings",
        "station_owners",
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

#[sqlx::test]
async fn the_faction_table_is_loaded_from_the_checked_in_csv(pool: PgPool) {
    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/npc_corporation_factions.csv"
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

#[sqlx::test]
async fn fee_sync_only_knows_skills_and_standings(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    let bad = sqlx::query(
        "INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES (1, 'wallet', now())",
    )
    .execute(&pool)
    .await;
    assert!(bad.is_err());
    for kind in ["skills", "standings"] {
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

#[sqlx::test]
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

#[sqlx::test]
async fn deleting_a_character_removes_its_fee_data_and_keeps_the_public_tables(pool: PgPool) {
    let account = insert_account(&pool).await;
    insert_character(&pool, account, 1).await;
    for statement in [
        "INSERT INTO character_skills (character_id, skill_id, active_level) VALUES (1, 16622, 4)",
        "INSERT INTO character_standings (character_id, from_id, from_type, standing) VALUES (1, 500001, 'faction', 2)",
        "INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES (1, 'skills', now())",
        "INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES (60003760, 1000035, now())",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    sqlx::query("DELETE FROM characters WHERE character_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    for table in ["character_skills", "character_standings", "fee_sync"] {
        assert_eq!(count(&pool, table).await, 0, "{table}");
    }
    assert_eq!(count(&pool, "station_owners").await, 1);
}

// ---- trade margin: purchases, remembered sell orders, costs ----

/// Everything in the migration after the `-- backfill` marker.
fn history_backfill_statement() -> &'static str {
    include_str!("../migrations/20261017000005_trade_margin.sql")
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

#[sqlx::test]
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

#[sqlx::test]
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

#[sqlx::test]
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
