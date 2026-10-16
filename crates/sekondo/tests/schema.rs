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
