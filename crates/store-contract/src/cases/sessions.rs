//! Session records, as tower-sessions reads and writes them.

use std::collections::HashMap;

use sekondo::store::Stores;
use time::Duration;
use tower_sessions::session::{Id, Record};

use super::{
    Case,
    support::{boxed, case, now},
};

pub fn cases() -> Vec<Case> {
    vec![
        case("sessions/a-saved-record-loads", |s| boxed(save_and_load(s))),
        case("sessions/a-record-past-its-expiry-is-gone", |s| {
            boxed(expiry(s))
        }),
        case("sessions/saving-again-replaces-the-record", |s| {
            boxed(save_replaces(s))
        }),
        case("sessions/creating-never-takes-a-used-id", |s| {
            boxed(create_takes_a_free_id(s))
        }),
        case("sessions/deleting-removes-one-record", |s| boxed(delete(s))),
        case("sessions/only-expired-records-are-deleted-in-bulk", |s| {
            boxed(delete_expired(s))
        }),
    ]
}

fn record(expires_in: Duration, key: &str) -> Record {
    Record {
        id: Id::default(),
        data: HashMap::from([(
            key.to_owned(),
            serde_json::json!({ "n": 1, "k": [true, null] }),
        )]),
        expiry_date: now() + expires_in,
    }
}

async fn save_and_load(stores: &Stores) {
    let record = record(Duration::days(7), "account");
    stores.sessions.save(&record).await.unwrap();

    let loaded = stores.sessions.load(&record.id).await.unwrap().unwrap();

    assert_eq!(loaded.id, record.id);
    assert_eq!(loaded.data, record.data);
    assert_eq!(loaded.expiry_date, record.expiry_date);
    assert!(
        stores
            .sessions
            .load(&Id::default())
            .await
            .unwrap()
            .is_none()
    );
}

async fn expiry(stores: &Stores) {
    let live = record(Duration::minutes(5), "live");
    let dead = record(-Duration::seconds(1), "dead");
    stores.sessions.save(&live).await.unwrap();
    stores.sessions.save(&dead).await.unwrap();

    assert!(stores.sessions.load(&live.id).await.unwrap().is_some());
    assert!(stores.sessions.load(&dead.id).await.unwrap().is_none());
}

async fn save_replaces(stores: &Stores) {
    let mut record = record(Duration::days(1), "first");
    stores.sessions.save(&record).await.unwrap();
    record.data = HashMap::from([("second".to_owned(), serde_json::json!("two"))]);
    record.expiry_date += Duration::days(1);

    stores.sessions.save(&record).await.unwrap();

    let loaded = stores.sessions.load(&record.id).await.unwrap().unwrap();
    assert_eq!(loaded.data, record.data);
    assert_eq!(loaded.expiry_date, record.expiry_date);
}

async fn create_takes_a_free_id(stores: &Stores) {
    let taken = record(Duration::days(1), "taken");
    stores.sessions.save(&taken).await.unwrap();
    let mut fresh = record(Duration::days(1), "fresh");
    fresh.id = taken.id;

    stores.sessions.create(&mut fresh).await.unwrap();

    assert_ne!(fresh.id, taken.id, "a used id is replaced");
    let first = stores.sessions.load(&taken.id).await.unwrap().unwrap();
    assert!(first.data.contains_key("taken"));
    let second = stores.sessions.load(&fresh.id).await.unwrap().unwrap();
    assert!(second.data.contains_key("fresh"));
}

async fn delete(stores: &Stores) {
    let a = record(Duration::days(1), "a");
    let b = record(Duration::days(1), "b");
    stores.sessions.save(&a).await.unwrap();
    stores.sessions.save(&b).await.unwrap();

    stores.sessions.delete(&a.id).await.unwrap();
    stores.sessions.delete(&Id::default()).await.unwrap();

    assert!(stores.sessions.load(&a.id).await.unwrap().is_none());
    assert!(stores.sessions.load(&b.id).await.unwrap().is_some());
}

async fn delete_expired(stores: &Stores) {
    let live = record(Duration::days(1), "live");
    let dead = record(-Duration::seconds(5), "dead");
    stores.sessions.save(&live).await.unwrap();
    stores.sessions.save(&dead).await.unwrap();

    stores.sessions.delete_expired().await.unwrap();

    assert!(stores.sessions.load(&live.id).await.unwrap().is_some());
    // Saved again with a live expiry, a deleted record would be gone for good; an expired one
    // that was not deleted would come back.
    let mut revived = dead.clone();
    revived.expiry_date = now() + Duration::days(1);
    assert_eq!(
        stores.sessions.load(&dead.id).await.unwrap().map(|r| r.id),
        None
    );
    stores.sessions.save(&revived).await.unwrap();
    assert!(stores.sessions.load(&dead.id).await.unwrap().is_some());
}
