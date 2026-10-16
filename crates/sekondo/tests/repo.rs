use secrecy::{ExposeSecret, SecretString};
use sekondo::{
    config::SecretKey,
    crypto::TokenCipher,
    repo::{CharacterUpsert, Repo},
};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

fn repo(pool: PgPool) -> Repo {
    Repo::new(pool, TokenCipher::new(&SecretKey([3u8; 32])))
}

fn upsert(id: i64, name: &str) -> CharacterUpsert {
    CharacterUpsert {
        character_id: id,
        name: name.to_owned(),
        owner_hash: format!("owner-{id}"),
        scopes: vec!["scope.a".to_owned(), "scope.b".to_owned()],
        refresh_token: SecretString::from(format!("refresh-token-{id}")),
        access_token: SecretString::from(format!("access-token-{id}")),
        access_expires_at: OffsetDateTime::now_utc() + Duration::minutes(20),
    }
}

#[sqlx::test]
async fn creates_accounts(pool: PgPool) {
    let repo = repo(pool);
    let a = repo.create_account().await.unwrap();
    let b = repo.create_account().await.unwrap();
    assert_ne!(a, b);
}

#[sqlx::test]
async fn upserts_and_finds_a_character(pool: PgPool) {
    let repo = repo(pool);
    let account = repo.create_account().await.unwrap();
    repo.upsert_character(account, &upsert(1, "Alice"))
        .await
        .unwrap();

    let found = repo
        .find_character(1)
        .await
        .unwrap()
        .expect("character exists");
    assert_eq!(found.account_id, account);
    assert_eq!(found.name, "Alice");
    assert_eq!(found.owner_hash, "owner-1");
    assert_eq!(found.scopes, ["scope.a", "scope.b"]);
    assert!(!found.needs_relogin);
    assert!(repo.find_character(2).await.unwrap().is_none());
}

#[sqlx::test]
async fn upsert_updates_in_place_keeps_the_account_and_clears_the_flag(pool: PgPool) {
    let repo = repo(pool.clone());
    let account = repo.create_account().await.unwrap();
    let other = repo.create_account().await.unwrap();
    repo.upsert_character(account, &upsert(1, "Alice"))
        .await
        .unwrap();
    sqlx::query("UPDATE characters SET needs_relogin = true WHERE character_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let mut again = upsert(1, "Alice Renamed");
    again.refresh_token = SecretString::from("new-refresh".to_owned());
    // Even when called with another account, an existing character is not moved.
    repo.upsert_character(other, &again).await.unwrap();

    let found = repo.find_character(1).await.unwrap().unwrap();
    assert_eq!(found.name, "Alice Renamed");
    assert_eq!(found.account_id, account);
    assert!(!found.needs_relogin);
    let tokens = repo.load_tokens(1).await.unwrap().unwrap();
    assert_eq!(tokens.refresh_token.expose_secret(), "new-refresh");
}

#[sqlx::test]
async fn lists_characters_of_one_account_only(pool: PgPool) {
    let repo = repo(pool);
    let a = repo.create_account().await.unwrap();
    let b = repo.create_account().await.unwrap();
    repo.upsert_character(a, &upsert(1, "Alice")).await.unwrap();
    repo.upsert_character(a, &upsert(2, "Bob")).await.unwrap();
    repo.upsert_character(b, &upsert(3, "Carol")).await.unwrap();

    let names: Vec<_> = repo
        .list_characters(a)
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["Alice", "Bob"]);
    assert_eq!(repo.list_characters(b).await.unwrap().len(), 1);
}

#[sqlx::test]
async fn stored_tokens_are_encrypted_and_round_trip(pool: PgPool) {
    let repo = repo(pool.clone());
    let account = repo.create_account().await.unwrap();
    repo.upsert_character(account, &upsert(1, "Alice"))
        .await
        .unwrap();

    let (refresh, access): (Vec<u8>, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT refresh_token, access_token FROM character_tokens WHERE character_id = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let access = access.unwrap();
    let contains =
        |haystack: &[u8], needle: &[u8]| haystack.windows(needle.len()).any(|w| w == needle);
    assert!(!contains(&refresh, b"refresh-token-1"));
    assert!(!contains(&access, b"access-token-1"));

    let tokens = repo.load_tokens(1).await.unwrap().unwrap();
    assert_eq!(tokens.refresh_token.expose_secret(), "refresh-token-1");
    assert_eq!(
        tokens.access_token.unwrap().expose_secret(),
        "access-token-1"
    );
    assert!(tokens.access_expires_at.is_some());
}

#[sqlx::test]
async fn deleting_a_character_keeps_the_account_while_others_remain(pool: PgPool) {
    let repo = repo(pool.clone());
    let account = repo.create_account().await.unwrap();
    repo.upsert_character(account, &upsert(1, "Alice"))
        .await
        .unwrap();
    repo.upsert_character(account, &upsert(2, "Bob"))
        .await
        .unwrap();

    let outcome = repo.delete_character(1).await.unwrap();
    assert!(outcome.deleted);
    assert!(!outcome.account_deleted);
    assert!(repo.find_character(1).await.unwrap().is_none());
    assert!(repo.load_tokens(1).await.unwrap().is_none());
    assert_eq!(repo.list_characters(account).await.unwrap().len(), 1);
}

#[sqlx::test]
async fn deleting_the_last_character_deletes_the_account(pool: PgPool) {
    let repo = repo(pool.clone());
    let account = repo.create_account().await.unwrap();
    repo.upsert_character(account, &upsert(1, "Alice"))
        .await
        .unwrap();

    let outcome = repo.delete_character(1).await.unwrap();
    assert!(outcome.deleted && outcome.account_deleted);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 0);
}

#[sqlx::test]
async fn deleting_an_unknown_character_is_a_no_op(pool: PgPool) {
    let repo = repo(pool);
    let outcome = repo.delete_character(99).await.unwrap();
    assert!(!outcome.deleted && !outcome.account_deleted);
}

#[sqlx::test]
async fn unlink_is_scoped_to_the_owning_account(pool: PgPool) {
    let repo = repo(pool);
    let mine = repo.create_account().await.unwrap();
    let theirs = repo.create_account().await.unwrap();
    repo.upsert_character(theirs, &upsert(1, "Alice"))
        .await
        .unwrap();

    let outcome = repo.unlink_character(mine, 1).await.unwrap();
    assert!(
        !outcome.deleted,
        "another account's character must not be deleted"
    );
    assert!(repo.find_character(1).await.unwrap().is_some());

    let outcome = repo.unlink_character(theirs, 1).await.unwrap();
    assert!(outcome.deleted && outcome.account_deleted);
}
