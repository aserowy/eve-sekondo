//! Accounts, characters and their encrypted tokens.

use sekondo::store::{DeleteOutcome, StoreError, Stores};
use time::Duration;

use super::{
    Case,
    support::{account_with, boxed, case, character, now},
};

pub fn cases() -> Vec<Case> {
    vec![
        case("accounts/tokens-round-trip-as-ciphertext", |s| {
            boxed(tokens_round_trip(s))
        }),
        case(
            "accounts/an-account-lists-its-characters-in-link-order",
            |s| boxed(link_order(s)),
        ),
        case(
            "accounts/upserting-an-existing-character-keeps-its-account",
            |s| boxed(upsert_existing(s)),
        ),
        case("accounts/a-new-character-needs-an-existing-account", |s| {
            boxed(upsert_needs_account(s))
        }),
        case(
            "accounts/creating-with-a-taken-character-changes-nothing",
            |s| boxed(create_is_atomic(s)),
        ),
        case("accounts/an-unknown-character-has-nothing", |s| {
            boxed(unknown_character(s))
        }),
        case(
            "accounts/deleting-the-last-character-deletes-the-account",
            |s| boxed(delete_last(s)),
        ),
        case(
            "accounts/deleting-another-character-keeps-the-account",
            |s| boxed(delete_other(s)),
        ),
        case("accounts/unlinking-needs-the-owning-account", |s| {
            boxed(unlink_owner(s))
        }),
        case(
            "accounts/needing-a-relogin-and-the-scopes-are-recorded",
            |s| boxed(relogin_and_scopes(s)),
        ),
        case("accounts/saving-tokens-replaces-them-and-the-scopes", |s| {
            boxed(save_tokens(s))
        }),
        case("accounts/accounts-with-characters-are-listed", |s| {
            boxed(account_ids(s))
        }),
        case("accounts/the-access-token-expiry-can-be-set", |s| {
            boxed(access_expiry(s))
        }),
    ]
}

async fn tokens_round_trip(stores: &Stores) {
    let expires = now() + Duration::minutes(20);
    let account = stores
        .accounts
        .create_account_with_character(&character(1), vec![9, 8, 7], vec![6, 5], expires)
        .await
        .unwrap();

    let tokens = stores.accounts.load_tokens(1).await.unwrap().unwrap();

    assert_eq!(tokens.refresh_token, [9, 8, 7]);
    assert_eq!(tokens.access_token, Some(vec![6, 5]));
    assert_eq!(tokens.access_expires_at, Some(expires));
    assert!(stores.accounts.load_tokens(2).await.unwrap().is_none());
    let found = stores.accounts.find_character(1).await.unwrap().unwrap();
    assert_eq!(found.account_id, account);
    assert_eq!(found.name, "Pilot 1");
    assert_eq!(found.owner_hash, "owner-1");
    assert_eq!(found.scopes, ["scope.a", "scope.b"]);
    assert!(!found.needs_relogin);
}

async fn link_order(stores: &Stores) {
    let account = stores.accounts.create_account().await.unwrap();
    for id in [30, 10, 20] {
        stores
            .accounts
            .upsert_character(account, &character(id), vec![1], vec![2], now())
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let other = account_with(stores, &[40]).await;

    let ids = |list: Vec<sekondo::store::CharacterRecord>| -> Vec<i64> {
        list.into_iter().map(|c| c.character_id).collect()
    };
    assert_eq!(
        ids(stores.accounts.list_characters(account).await.unwrap()),
        [30, 10, 20]
    );
    assert_eq!(
        ids(stores.accounts.list_characters(other).await.unwrap()),
        [40]
    );
}

async fn upsert_existing(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let other = account_with(stores, &[2]).await;
    stores.accounts.mark_needs_relogin(1).await.unwrap();
    let renamed = sekondo::store::NewCharacter {
        character_id: 1,
        name: "Renamed".to_owned(),
        owner_hash: "new-owner".to_owned(),
        scopes: vec!["scope.c".to_owned()],
    };
    let expires = now() + Duration::hours(1);

    // Another account asks for it: the character stays where it is.
    stores
        .accounts
        .upsert_character(other, &renamed, vec![11], vec![12], expires)
        .await
        .unwrap();

    let found = stores.accounts.find_character(1).await.unwrap().unwrap();
    assert_eq!(found.account_id, account);
    assert_eq!(found.name, "Renamed");
    assert_eq!(found.owner_hash, "new-owner");
    assert_eq!(found.scopes, ["scope.c"]);
    assert!(!found.needs_relogin, "a login clears the flag");
    let tokens = stores.accounts.load_tokens(1).await.unwrap().unwrap();
    assert_eq!(tokens.refresh_token, [11]);
    assert_eq!(tokens.access_token, Some(vec![12]));
    assert_eq!(tokens.access_expires_at, Some(expires));
    assert_eq!(
        stores.accounts.list_characters(other).await.unwrap().len(),
        1
    );
}

async fn upsert_needs_account(stores: &Stores) {
    let result = stores
        .accounts
        .upsert_character(uuid::Uuid::new_v4(), &character(1), vec![1], vec![2], now())
        .await;

    assert!(
        matches!(result, Err(StoreError::MissingParent)),
        "{result:?}"
    );
    assert!(stores.accounts.find_character(1).await.unwrap().is_none());
    assert!(stores.accounts.load_tokens(1).await.unwrap().is_none());
}

async fn create_is_atomic(stores: &Stores) {
    account_with(stores, &[1]).await;
    let before = stores.accounts.account_ids().await.unwrap();

    let result = stores
        .accounts
        .create_account_with_character(&character(1), vec![9], vec![9], now())
        .await;

    assert!(result.is_err());
    assert_eq!(stores.accounts.account_ids().await.unwrap(), before);
    let tokens = stores.accounts.load_tokens(1).await.unwrap().unwrap();
    assert_eq!(
        tokens.refresh_token,
        [1],
        "the first character's tokens stay"
    );
}

async fn unknown_character(stores: &Stores) {
    assert!(stores.accounts.find_character(5).await.unwrap().is_none());
    assert!(stores.accounts.load_tokens(5).await.unwrap().is_none());
    let nobody = uuid::Uuid::new_v4();
    assert!(
        stores
            .accounts
            .list_characters(nobody)
            .await
            .unwrap()
            .is_empty()
    );
    // Changing what is not there is not an error.
    stores.accounts.mark_needs_relogin(5).await.unwrap();
    stores
        .accounts
        .set_scopes(5, &["x".to_owned()])
        .await
        .unwrap();
    stores
        .accounts
        .save_tokens(5, vec![1], vec![2], now(), &[])
        .await
        .unwrap();
    assert_eq!(
        stores.accounts.delete_character(5).await.unwrap(),
        DeleteOutcome {
            deleted: false,
            account_deleted: false
        }
    );
}

async fn delete_last(stores: &Stores) {
    let account = account_with(stores, &[1]).await;

    let outcome = stores.accounts.delete_character(1).await.unwrap();

    assert_eq!(
        outcome,
        DeleteOutcome {
            deleted: true,
            account_deleted: true
        }
    );
    assert!(stores.accounts.find_character(1).await.unwrap().is_none());
    assert!(stores.accounts.load_tokens(1).await.unwrap().is_none());
    assert!(
        !stores
            .accounts
            .account_ids()
            .await
            .unwrap()
            .contains(&account)
    );
}

async fn delete_other(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;

    let outcome = stores.accounts.delete_character(1).await.unwrap();

    assert_eq!(
        outcome,
        DeleteOutcome {
            deleted: true,
            account_deleted: false
        }
    );
    assert_eq!(stores.accounts.account_ids().await.unwrap(), [account]);
    assert!(stores.accounts.find_character(2).await.unwrap().is_some());
}

async fn unlink_owner(stores: &Stores) {
    let mine = account_with(stores, &[1, 2]).await;
    let theirs = account_with(stores, &[3]).await;

    let stranger = stores.accounts.unlink_character(theirs, 1).await.unwrap();
    assert_eq!(
        stranger,
        DeleteOutcome {
            deleted: false,
            account_deleted: false
        }
    );
    assert!(stores.accounts.find_character(1).await.unwrap().is_some());

    let owner = stores.accounts.unlink_character(mine, 1).await.unwrap();
    assert_eq!(
        owner,
        DeleteOutcome {
            deleted: true,
            account_deleted: false
        }
    );
    let last = stores.accounts.unlink_character(mine, 2).await.unwrap();
    assert_eq!(
        last,
        DeleteOutcome {
            deleted: true,
            account_deleted: true
        }
    );
}

async fn relogin_and_scopes(stores: &Stores) {
    account_with(stores, &[1, 2]).await;

    stores.accounts.mark_needs_relogin(1).await.unwrap();
    stores
        .accounts
        .set_scopes(1, &["scope.x".to_owned()])
        .await
        .unwrap();

    let first = stores.accounts.find_character(1).await.unwrap().unwrap();
    assert!(first.needs_relogin);
    assert_eq!(first.scopes, ["scope.x"]);
    let second = stores.accounts.find_character(2).await.unwrap().unwrap();
    assert!(!second.needs_relogin);
    assert_eq!(second.scopes, ["scope.a", "scope.b"]);
}

async fn save_tokens(stores: &Stores) {
    account_with(stores, &[1]).await;
    let expires = now() + Duration::minutes(19);

    stores
        .accounts
        .save_tokens(1, vec![7], vec![8], expires, &["scope.z".to_owned()])
        .await
        .unwrap();

    let tokens = stores.accounts.load_tokens(1).await.unwrap().unwrap();
    assert_eq!(tokens.refresh_token, [7]);
    assert_eq!(tokens.access_token, Some(vec![8]));
    assert_eq!(tokens.access_expires_at, Some(expires));
    let found = stores.accounts.find_character(1).await.unwrap().unwrap();
    assert_eq!(found.scopes, ["scope.z"]);
}

async fn account_ids(stores: &Stores) {
    assert!(stores.accounts.account_ids().await.unwrap().is_empty());
    let a = account_with(stores, &[1, 2]).await;
    let b = account_with(stores, &[3]).await;
    stores.accounts.create_account().await.unwrap();

    let mut expected = vec![a, b];
    expected.sort_unstable();
    let mut listed = stores.accounts.account_ids().await.unwrap();
    listed.sort_unstable();
    assert_eq!(listed, expected);
    assert_eq!(
        stores.accounts.account_count().await.unwrap(),
        3,
        "the account without a character counts"
    );
    stores.accounts.delete_character(3).await.unwrap();
    assert_eq!(stores.accounts.account_count().await.unwrap(), 2);
}

async fn access_expiry(stores: &Stores) {
    account_with(stores, &[1]).await;
    let at = now() - Duration::hours(1);

    stores.accounts.seed_access_expiry(1, at).await.unwrap();

    let tokens = stores.accounts.load_tokens(1).await.unwrap().unwrap();
    assert_eq!(tokens.access_expires_at, Some(at));
    assert_eq!(tokens.refresh_token, [1]);
}
