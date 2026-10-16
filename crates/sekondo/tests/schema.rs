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
