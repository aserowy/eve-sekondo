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
            "type_id"
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
