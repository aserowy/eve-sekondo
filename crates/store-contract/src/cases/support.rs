//! Small helpers the cases share.

use std::{future::Future, pin::Pin};

use sekondo::store::{NewCharacter, Stores};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{Case, CaseFuture};

/// The database's `now()`, at the precision `timestamptz` keeps.
pub fn now() -> OffsetDateTime {
    let now = OffsetDateTime::now_utc();
    now.replace_nanosecond(now.nanosecond() / 1000 * 1000)
        .expect("a valid nanosecond")
}

pub fn case(name: &'static str, run: for<'a> fn(&'a Stores) -> CaseFuture<'a>) -> Case {
    Case { name, run }
}

/// Boxes an async case function, for registration.
pub fn boxed<'a>(
    future: impl Future<Output = ()> + Send + 'a,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(future)
}

pub fn character(id: i64) -> NewCharacter {
    NewCharacter {
        character_id: id,
        name: format!("Pilot {id}"),
        owner_hash: format!("owner-{id}"),
        scopes: vec!["scope.a".to_owned(), "scope.b".to_owned()],
    }
}

/// A new account with these characters, in this order.
pub async fn account_with(stores: &Stores, characters: &[i64]) -> Uuid {
    let (first, rest) = characters.split_first().expect("at least one character");
    let account = stores
        .accounts
        .create_account_with_character(&character(*first), vec![1], vec![2], now())
        .await
        .unwrap();
    for id in rest {
        stores
            .accounts
            .upsert_character(account, &character(*id), vec![1], vec![2], now())
            .await
            .unwrap();
    }
    account
}

/// A personal sell order of 10 units at 100.00 ISK, issued a day ago, with every field a poll
/// has. Tests change what they care about.
pub fn sell_order(order_id: i64, type_id: i64) -> sekondo::esi::EsiOrder {
    sekondo::esi::EsiOrder {
        order_id,
        type_id,
        region_id: REGION,
        location_id: STATION,
        is_buy_order: false,
        is_corporation: false,
        price_cents: 10_000,
        volume_remain: 10,
        volume_total: 10,
        min_volume: 1,
        range: "station".to_owned(),
        escrow_cents: None,
        issued: now() - time::Duration::days(1),
        duration: 90,
    }
}

/// A personal buy order of 10 units at 50.00 ISK with 500.00 ISK in escrow.
pub fn buy_order(order_id: i64, type_id: i64) -> sekondo::esi::EsiOrder {
    sekondo::esi::EsiOrder {
        is_buy_order: true,
        price_cents: 5_000,
        min_volume: 1,
        escrow_cents: Some(50_000),
        range: "region".to_owned(),
        ..sell_order(order_id, type_id)
    }
}

/// The Forge, and Jita 4-4 in it.
pub const REGION: i64 = 10_000_002;
pub const STATION: i64 = 60_003_760;

/// Two numbers that are the same up to rounding noise.
#[track_caller]
pub fn assert_close(actual: f64, expected: f64) {
    let tolerance = 1e-9 * expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not {expected}"
    );
}
