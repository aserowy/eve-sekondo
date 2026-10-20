//! The trade fees of the characters (sales tax, broker fee, minimum margin) and the polls behind
//! them: skills, standings, the location, and the owners of the stations.

use sekondo::{
    fee_poller::Kind,
    oauth::LOCATION_SCOPE,
    store::{StandingInput, StoreError, Stores},
};
use time::Duration;

use super::{
    Case,
    support::{STATION, account_with, boxed, case, now, sell_order},
};

const ACCOUNTING: i64 = 16622;
const BROKER_RELATIONS: i64 = 3446;
/// An NPC corporation of the Caldari State, and the faction.
const CALDARI_NAVY: i64 = 1_000_035;
const CALDARI_STATE: i64 = 500_001;
/// An NPC corporation that is in no faction.
const LONER: i64 = 1_999_999;
const AMARR: i64 = 60_008_494;

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "fees/seeded-skills-standings-and-successes-leave-the-rest-alone",
            |s| boxed(seeded_rows(s)),
        ),
        case("fees/nothing-is-known-until-the-polls-ran", |s| {
            boxed(unknown(s))
        }),
        case("fees/skills-and-standings-lower-the-fee", |s| {
            boxed(skills_and_standings(s))
        }),
        case("fees/the-sales-tax-follows-the-accounting-level", |s| {
            boxed(sales_tax_by_level(s))
        }),
        case(
            "fees/a-structure-has-no-broker-fee-and-an-owner-in-no-faction-no-faction-standing",
            |s| boxed(structures_and_loners(s)),
        ),
        case(
            "fees/fees-at-a-location-follow-the-polls-and-the-owner",
            |s| boxed(at_location(s)),
        ),
        case(
            "fees/a-selection-never-reaches-another-accounts-characters",
            |s| boxed(account_boundary(s)),
        ),
        case(
            "fees/the-fee-location-is-where-a-character-with-the-scope-is-docked",
            |s| boxed(fee_location(s)),
        ),
        case("fees/storing-a-poll-says-whether-the-fees-changed", |s| {
            boxed(change_detection(s))
        }),
        case("fees/a-poll-replaces-what-was-stored", |s| {
            boxed(replaced(s))
        }),
        case(
            "fees/a-standing-pair-is-kept-once-and-a-bad-number-is-refused",
            |s| boxed(standing_input(s)),
        ),
        case(
            "fees/stations-without-an-owner-are-those-with-orders-sales-or-a-docked-character",
            |s| boxed(stations_without_owner(s)),
        ),
        case("fees/an-owner-is-learned-once", |s| boxed(owner_once(s))),
        case(
            "fees/learning-an-owner-names-the-accounts-whose-fees-changed",
            |s| boxed(owner_changes_accounts(s)),
        ),
        case("fees/a-failure-backs-off-and-a-success-resets", |s| {
            boxed(failure_backoff(s))
        }),
        case(
            "fees/a-deferral-moves-the-next-poll-without-a-failure",
            |s| boxed(deferral(s)),
        ),
        case("fees/each-kind-is-scheduled-and-claimed-on-its-own", |s| {
            boxed(schedule_and_claim(s))
        }),
        case("fees/the-location-is-only-polled-with-its-scope", |s| {
            boxed(location_scope(s))
        }),
        case("fees/making-a-poll-due-never-delays-it", |s| {
            boxed(make_due(s))
        }),
        case("fees/unlinking-a-character-removes-its-fee-data", |s| {
            boxed(unlinking(s))
        }),
    ]
}

async fn polls(
    stores: &Stores,
    character_id: i64,
    skills: &[(i64, i16)],
    standings: &[StandingInput],
) {
    let t = now();
    let (ids, levels): (Vec<i64>, Vec<i16>) = skills.iter().copied().unzip();
    stores
        .fees
        .store_skills(character_id, &ids, &levels, t, t + Duration::hours(1), None)
        .await
        .unwrap();
    stores
        .fees
        .store_standings(character_id, standings, t, t + Duration::hours(1), None)
        .await
        .unwrap();
}

fn standing(from_id: i64, from_type: &str, value: &str) -> StandingInput {
    StandingInput {
        from_id,
        from_type: from_type.to_owned(),
        standing: value.to_owned(),
    }
}

async fn with_order(stores: &Stores, character_id: i64, location_id: i64) {
    let t = now();
    let mut order = sell_order(character_id * 100 + location_id % 97, 34);
    order.location_id = location_id;
    stores
        .orders
        .replace_orders(character_id, t, &[order], t, None)
        .await
        .unwrap();
}

/// Alice with an order in Jita, both polls done, Broker Relations 5, Accounting 5, faction
/// standing 2.0 and corporation standing 1.5 towards the owner of Jita 4-4.
async fn alice_at_jita(stores: &Stores) -> uuid::Uuid {
    let account = account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;
    polls(
        stores,
        1,
        &[(ACCOUNTING, 5), (BROKER_RELATIONS, 5)],
        &[
            standing(CALDARI_STATE, "faction", "2.0"),
            standing(CALDARI_NAVY, "npc_corp", "1.5"),
        ],
    )
    .await;
    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, now())
        .await
        .unwrap();
    account
}

async fn unknown(stores: &Stores) {
    account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;

    let rows = stores.fees.for_characters(&[1]).await.unwrap();

    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        (row.character_id, row.location_id, row.is_structure),
        (1, STATION, false)
    );
    assert_eq!((row.accounting, row.broker_relations), (None, None));
    assert_eq!((&row.faction_standing, &row.corp_standing), (&None, &None));
    assert_eq!(
        (&row.sales_tax, &row.broker_fee, &row.min_margin),
        (&None, &None, &None)
    );
    assert!(stores.fees.for_characters(&[2]).await.unwrap().is_empty());
    assert!(stores.fees.for_characters(&[]).await.unwrap().is_empty());
}

async fn skills_and_standings(stores: &Stores) {
    alice_at_jita(stores).await;

    let rows = stores.fees.for_characters(&[1]).await.unwrap();

    let row = &rows[0];
    assert_eq!((row.accounting, row.broker_relations), (Some(5), Some(5)));
    assert_eq!(row.faction_standing.as_deref(), Some("2.0"));
    assert_eq!(row.corp_standing.as_deref(), Some("1.5"));
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    // 3 - 0.3 × 5 - 0.03 × 2.0 - 0.02 × 1.5
    assert_eq!(row.broker_fee.as_deref(), Some("1.410"));
    // ((1 + b) / (1 - b - t) - 1) × 100 with b = 1.41% and t = 3.375%, to PostgreSQL's digits.
    assert_eq!(row.min_margin.as_deref(), Some("6.50632778448773827700"));
}

async fn sales_tax_by_level(stores: &Stores) {
    account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;
    let mut texts = Vec::new();
    for level in [0, 1, 4, 5] {
        polls(stores, 1, &[(ACCOUNTING, level)], &[]).await;
        let rows = stores.fees.for_characters(&[1]).await.unwrap();
        texts.push((rows[0].accounting, rows[0].sales_tax.clone()));
    }
    assert_eq!(
        texts,
        [
            (Some(0), Some("7.500".to_owned())),
            (Some(1), Some("6.675".to_owned())),
            (Some(4), Some("4.200".to_owned())),
            (Some(5), Some("3.375".to_owned())),
        ]
    );
    // A skill that was not trained counts as level 0 once the skills were polled; the other one
    // too.
    polls(stores, 1, &[], &[]).await;
    let rows = stores.fees.for_characters(&[1]).await.unwrap();
    assert_eq!(
        (rows[0].accounting, rows[0].broker_relations),
        (Some(0), Some(0))
    );
    assert_eq!(rows[0].sales_tax.as_deref(), Some("7.500"));
}

async fn structures_and_loners(stores: &Stores) {
    account_with(stores, &[1]).await;
    with_order(stores, 1, 1_000_000_000_001).await;
    with_order(stores, 1, STATION).await;
    // The first order was replaced by the second: one location now. Put both in a single poll.
    let t = now();
    let mut structure = sell_order(1, 34);
    structure.location_id = 1_000_000_000_001;
    let mut station = sell_order(2, 34);
    station.location_id = STATION;
    let mut loner = sell_order(3, 34);
    loner.location_id = AMARR;
    stores
        .orders
        .replace_orders(1, t, &[structure, station, loner], t, None)
        .await
        .unwrap();
    polls(
        stores,
        1,
        &[(ACCOUNTING, 5), (BROKER_RELATIONS, 5)],
        &[standing(LONER, "npc_corp", "1.0")],
    )
    .await;
    stores
        .fees
        .insert_station_owner(AMARR, LONER, t)
        .await
        .unwrap();
    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();

    let rows = stores.fees.for_characters(&[1]).await.unwrap();

    let by = |location: i64| rows.iter().find(|r| r.location_id == location).unwrap();
    let structure = by(1_000_000_000_001);
    assert!(structure.is_structure);
    assert_eq!(structure.sales_tax.as_deref(), Some("3.375"));
    assert_eq!(
        (&structure.faction_standing, &structure.corp_standing),
        (&None, &None)
    );
    assert_eq!(
        (&structure.broker_fee, &structure.min_margin),
        (&None, &None)
    );
    let amarr = by(AMARR);
    assert_eq!(
        amarr.faction_standing.as_deref(),
        Some("0"),
        "an owner in no faction"
    );
    assert_eq!(amarr.corp_standing.as_deref(), Some("1.0"));
    assert_eq!(
        amarr.broker_fee.as_deref(),
        Some("1.480"),
        "3 - 1.5 - 0 - 0.02"
    );
    // Without a standing row towards the faction or the corporation: zero.
    let jita = by(STATION);
    assert_eq!(jita.faction_standing.as_deref(), Some("0"));
    assert_eq!(jita.corp_standing.as_deref(), Some("0"));
    assert_eq!(jita.broker_fee.as_deref(), Some("1.50"));
    // Ordered by character, then location.
    let locations: Vec<i64> = rows.iter().map(|r| r.location_id).collect();
    let mut sorted = locations.clone();
    sorted.sort_unstable();
    assert_eq!(locations, sorted);
}

async fn at_location(stores: &Stores) {
    let account = account_with(stores, &[1]).await;

    let nothing = stores
        .fees
        .at_location(account, &[1], STATION)
        .await
        .unwrap();
    assert_eq!(nothing.len(), 1);
    assert_eq!(
        (
            &nothing[0].sales_tax,
            &nothing[0].broker_fee,
            &nothing[0].min_margin
        ),
        (&None, &None, &None)
    );

    let t = now();
    stores
        .fees
        .store_skills(1, &[ACCOUNTING, BROKER_RELATIONS], &[5, 5], t, t, None)
        .await
        .unwrap();
    let taxed = stores
        .fees
        .at_location(account, &[1], STATION)
        .await
        .unwrap();
    assert_eq!(taxed[0].sales_tax.as_deref(), Some("3.375"));
    assert_eq!((&taxed[0].broker_fee, &taxed[0].min_margin), (&None, &None));

    stores
        .fees
        .store_standings(1, &[], t, t, None)
        .await
        .unwrap();
    let no_owner = stores
        .fees
        .at_location(account, &[1], STATION)
        .await
        .unwrap();
    assert_eq!(no_owner[0].broker_fee, None, "the owner is not known yet");

    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();
    let known = stores
        .fees
        .at_location(account, &[1], STATION)
        .await
        .unwrap();
    assert_eq!(known[0].broker_fee.as_deref(), Some("1.50"), "3 - 0.3 × 5");
    // No order is needed; a structure never has a broker fee.
    let structure = stores
        .fees
        .at_location(account, &[1], 1_000_000_000_001)
        .await
        .unwrap();
    assert!(structure[0].is_structure);
    assert_eq!(structure[0].broker_fee, None);
    assert_eq!(structure[0].sales_tax.as_deref(), Some("3.375"));
}

async fn account_boundary(stores: &Stores) {
    let mine = account_with(stores, &[1]).await;
    let theirs = account_with(stores, &[2]).await;
    for id in [1, 2] {
        with_order(stores, id, STATION).await;
        polls(stores, id, &[(ACCOUNTING, 5)], &[]).await;
    }

    let rows = stores.fees.for_account(mine, &[1, 2]).await.unwrap();
    assert_eq!(rows.iter().map(|r| r.character_id).collect::<Vec<_>>(), [1]);
    let at = stores
        .fees
        .at_location(mine, &[1, 2], STATION)
        .await
        .unwrap();
    assert_eq!(at.iter().map(|r| r.character_id).collect::<Vec<_>>(), [1]);
    let fee_location = stores.fees.at_fee_location(mine, &[1, 2]).await.unwrap();
    assert_eq!(
        fee_location
            .iter()
            .map(|f| f.row.character_id)
            .collect::<Vec<_>>(),
        [1]
    );
    // The pollers read whoever's they are.
    assert_eq!(stores.fees.for_characters(&[1, 2]).await.unwrap().len(), 2);
    assert_eq!(
        stores
            .fees
            .for_account(theirs, &[1, 2])
            .await
            .unwrap()
            .len(),
        1
    );
}

async fn fee_location(stores: &Stores) {
    let account = account_with(stores, &[1, 2, 3]).await;
    let t = now();
    stores
        .accounts
        .set_scopes(2, &[LOCATION_SCOPE.to_owned()])
        .await
        .unwrap();
    stores
        .accounts
        .set_scopes(3, &[LOCATION_SCOPE.to_owned()])
        .await
        .unwrap();
    for id in [1, 2, 3] {
        polls(stores, id, &[(ACCOUNTING, 5), (BROKER_RELATIONS, 5)], &[]).await;
    }
    stores
        .fees
        .insert_station_owner(AMARR, LONER, t)
        .await
        .unwrap();
    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();
    // 1 has no scope but a location; 2 has the scope and is docked in Amarr; 3 has the scope and
    // has never been seen.
    stores
        .fees
        .store_location(1, Some((AMARR, "station")), t, t, None)
        .await
        .unwrap();
    stores
        .fees
        .store_location(2, Some((AMARR, "station")), t, t, None)
        .await
        .unwrap();

    let fees = stores
        .fees
        .at_fee_location(account, &[3, 2, 1])
        .await
        .unwrap();

    let summary: Vec<(i64, i64, bool, Option<String>)> = fees
        .iter()
        .map(|f| {
            (
                f.row.character_id,
                f.row.location_id,
                f.needs_location_scope,
                f.row.broker_fee.clone(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (1, STATION, true, Some("1.50".to_owned())),
            (2, AMARR, false, Some("1.50".to_owned())),
            (3, STATION, false, Some("1.50".to_owned())),
        ],
        "ordered by character"
    );
}

async fn change_detection(stores: &Stores) {
    account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;
    let t = now();
    let next = t + Duration::hours(1);

    // The first skills poll makes a sales tax known; the same one again changes nothing.
    let store_skills = |ids: Vec<i64>, levels: Vec<i16>| {
        let fees = stores.fees.clone();
        async move {
            fees.store_skills(1, &ids, &levels, t, next, None)
                .await
                .unwrap()
        }
    };
    assert!(store_skills(vec![ACCOUNTING], vec![5]).await);
    assert!(!store_skills(vec![ACCOUNTING], vec![5]).await);
    // A skill the fees do not use is not shown.
    assert!(!store_skills(vec![ACCOUNTING, 99], vec![5, 3]).await);
    assert!(store_skills(vec![ACCOUNTING], vec![4]).await);

    // Standings change nothing a page shows until the owner of the station is known.
    let store_standing = |value: &'static str| {
        let fees = stores.fees.clone();
        async move {
            fees.store_standings(
                1,
                &[standing(CALDARI_NAVY, "npc_corp", value)],
                t,
                next,
                None,
            )
            .await
            .unwrap()
        }
    };
    assert!(!store_standing("1.5").await);
    assert!(!store_standing("1.5").await);
    assert!(!store_standing("2.5").await);
    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();
    assert!(store_skills(vec![ACCOUNTING, BROKER_RELATIONS], vec![4, 5]).await);
    assert!(
        store_standing("3.5").await,
        "a standing towards the owner changes the broker fee"
    );
    assert!(!store_standing("3.5").await);

    // The location only matters for the fee location, and only with the scope.
    let store_location = |docked: Option<(i64, &'static str)>| {
        let fees = stores.fees.clone();
        async move { fees.store_location(1, docked, t, next, None).await.unwrap() }
    };
    assert!(
        !store_location(Some((AMARR, "station"))).await,
        "without the scope the fee location is Jita"
    );
    stores
        .accounts
        .set_scopes(1, &[LOCATION_SCOPE.to_owned()])
        .await
        .unwrap();
    assert!(
        !store_location(None).await,
        "in space the last location stays"
    );
    assert!(
        store_location(Some((STATION, "station"))).await,
        "the fee location moved to Jita"
    );
    assert!(
        store_location(Some((AMARR, "station"))).await,
        "and on to Amarr"
    );
    assert!(!store_location(Some((AMARR, "station"))).await);

    let missing = stores.fees.store_location(404, None, t, next, None).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
    let missing = stores.fees.store_skills(404, &[], &[], t, next, None).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
    let missing = stores.fees.store_standings(404, &[], t, next, None).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
}

async fn replaced(stores: &Stores) {
    account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;
    polls(stores, 1, &[(ACCOUNTING, 5), (BROKER_RELATIONS, 5)], &[]).await;
    polls(stores, 1, &[(ACCOUNTING, 3)], &[]).await;

    let rows = stores.fees.for_characters(&[1]).await.unwrap();

    assert_eq!(rows[0].accounting, Some(3));
    assert_eq!(
        rows[0].broker_relations,
        Some(0),
        "Broker Relations is gone: untrained"
    );
    let t = now();
    let state = stores
        .fees
        .fee_sync(1, Kind::Skills)
        .await
        .unwrap()
        .unwrap();
    assert!(state.last_success_at.is_some_and(|s| s <= t));
    assert_eq!((state.failures, state.last_error), (0, None));
    assert!(
        stores
            .fees
            .fee_sync(1, Kind::Location)
            .await
            .unwrap()
            .is_none()
    );
    // A duplicate skill in one answer cannot be stored.
    let twice = stores
        .fees
        .store_skills(1, &[ACCOUNTING, ACCOUNTING], &[1, 2], t, t, None)
        .await;
    assert!(twice.is_err());
    let kept = stores.fees.for_characters(&[1]).await.unwrap();
    assert_eq!(kept[0].accounting, Some(3));
}

async fn standing_input(stores: &Stores) {
    account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;
    let t = now();
    stores
        .fees
        .store_skills(1, &[BROKER_RELATIONS], &[5], t, t, None)
        .await
        .unwrap();
    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();

    // The same pair twice: the first stays.
    stores
        .fees
        .store_standings(
            1,
            &[
                standing(CALDARI_NAVY, "npc_corp", "1.5"),
                standing(CALDARI_NAVY, "npc_corp", "9.0"),
            ],
            t,
            t,
            None,
        )
        .await
        .unwrap();
    let rows = stores.fees.for_characters(&[1]).await.unwrap();
    assert_eq!(rows[0].corp_standing.as_deref(), Some("1.5"));

    let bad = stores
        .fees
        .store_standings(1, &[standing(CALDARI_NAVY, "npc_corp", "lots")], t, t, None)
        .await;
    assert!(bad.is_err());
    let kept = stores.fees.for_characters(&[1]).await.unwrap();
    assert_eq!(kept[0].corp_standing.as_deref(), Some("1.5"));
    // Negative and exact decimals keep their text.
    stores
        .fees
        .store_standings(
            1,
            &[standing(CALDARI_NAVY, "npc_corp", "-2.50")],
            t,
            t,
            None,
        )
        .await
        .unwrap();
    let negative = stores.fees.for_characters(&[1]).await.unwrap();
    assert_eq!(negative[0].corp_standing.as_deref(), Some("-2.50"));
    assert_eq!(
        negative[0].broker_fee.as_deref(),
        Some("1.5500"),
        "3 - 1.5 - 0.00 + 0.0500"
    );
}

async fn stations_without_owner(stores: &Stores) {
    account_with(stores, &[1, 2]).await;
    let t = now();
    let mut here = sell_order(1, 34);
    here.location_id = 60_000_001;
    let mut structure = sell_order(2, 34);
    structure.location_id = 1_000_000_000_001;
    stores
        .orders
        .replace_orders(1, t, &[here, structure], t, None)
        .await
        .unwrap();
    stores
        .wealth
        .store_transactions(
            1,
            t,
            &[sekondo::esi::EsiTransaction {
                transaction_id: 1,
                bought_at: t,
                type_id: 34,
                location_id: 60_000_002,
                quantity: 1,
                unit_price_cents: 100,
                is_buy: false,
                is_personal: true,
            }],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .fees
        .store_location(2, Some((60_000_003, "station")), t, t, None)
        .await
        .unwrap();
    stores
        .fees
        .store_location(1, Some((1_000_000_000_002, "structure")), t, t, None)
        .await
        .unwrap();
    stores
        .fees
        .insert_station_owner(60_000_001, CALDARI_NAVY, t)
        .await
        .unwrap();

    let unknown = stores
        .fees
        .stations_without_owner(100_000_000)
        .await
        .unwrap();

    assert_eq!(unknown, [60_000_002, 60_000_003]);
    let all = stores
        .fees
        .stations_without_owner(2_000_000_000_000)
        .await
        .unwrap();
    assert_eq!(
        all,
        [60_000_002, 60_000_003, 1_000_000_000_001],
        "a docked structure is not a station that lacks an owner"
    );
}

async fn owner_once(stores: &Stores) {
    let t = now();
    assert!(!stores.fees.station_owner_known(STATION).await.unwrap());

    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();
    stores
        .fees
        .insert_station_owner(STATION, LONER, t + Duration::hours(1))
        .await
        .unwrap();

    assert!(stores.fees.station_owner_known(STATION).await.unwrap());
    assert!(!stores.fees.station_owner_known(AMARR).await.unwrap());
    // The first owner stays: the faction of the first one is the one that counts.
    let account = account_with(stores, &[1]).await;
    with_order(stores, 1, STATION).await;
    polls(
        stores,
        1,
        &[(ACCOUNTING, 5), (BROKER_RELATIONS, 5)],
        &[standing(CALDARI_STATE, "faction", "2.0")],
    )
    .await;
    let rows = stores.fees.for_account(account, &[1]).await.unwrap();
    assert_eq!(rows[0].faction_standing.as_deref(), Some("2.0"));
}

async fn owner_changes_accounts(stores: &Stores) {
    let a = account_with(stores, &[1]).await;
    let b = account_with(stores, &[2]).await;
    let c = account_with(stores, &[3]).await;
    let t = now();
    for id in [1, 2, 3] {
        polls(stores, id, &[(ACCOUNTING, 5), (BROKER_RELATIONS, 5)], &[]).await;
    }
    // 1 has an order there, 2 sold there, 3 is elsewhere.
    with_order(stores, 1, STATION).await;
    stores
        .wealth
        .store_transactions(
            2,
            t,
            &[sekondo::esi::EsiTransaction {
                transaction_id: 1,
                bought_at: t,
                type_id: 34,
                location_id: STATION,
                quantity: 1,
                unit_price_cents: 100,
                is_buy: false,
                is_personal: true,
            }],
            t,
            None,
        )
        .await
        .unwrap();
    with_order(stores, 3, AMARR).await;

    let mut accounts = stores
        .fees
        .store_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();
    accounts.sort_unstable();
    let mut expected = vec![a, b];
    expected.sort_unstable();
    assert_eq!(
        accounts, expected,
        "the broker fee of 1 and of 2's sale became known"
    );
    assert!(!expected.contains(&c));

    // Again: the owner is known, so nothing changes.
    let none = stores
        .fees
        .store_station_owner(STATION, LONER, t)
        .await
        .unwrap();
    assert!(none.is_empty());
    let nobody = stores
        .fees
        .store_station_owner(60_000_099, LONER, t)
        .await
        .unwrap();
    assert!(nobody.is_empty());
    assert!(stores.fees.station_owner_known(60_000_099).await.unwrap());
}

async fn failure_backoff(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    let (start, interval) = (Duration::minutes(1), Duration::hours(1));

    stores
        .fees
        .record_failure(1, Kind::Skills, t, "boom", start, interval)
        .await
        .unwrap();
    let first = stores
        .fees
        .fee_sync(1, Kind::Skills)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first.failures, first.next_poll_at), (1, t + start));
    let mut waits = Vec::new();
    for round in 1..=8 {
        let at = t + Duration::days(round);
        stores
            .fees
            .record_failure(1, Kind::Skills, at, "again", start, interval)
            .await
            .unwrap();
        let s = stores
            .fees
            .fee_sync(1, Kind::Skills)
            .await
            .unwrap()
            .unwrap();
        waits.push((s.failures, (s.next_poll_at - at).whole_seconds()));
    }
    assert_eq!(
        waits,
        [
            (2, 120),
            (3, 240),
            (4, 480),
            (5, 960),
            (6, 1_920),
            (7, 3_600),
            (8, 3_600),
            (9, 3_600)
        ]
    );
    assert!(
        stores
            .fees
            .fee_sync(1, Kind::Standings)
            .await
            .unwrap()
            .is_none()
    );
    polls(stores, 1, &[], &[]).await;
    let ok = stores
        .fees
        .fee_sync(1, Kind::Skills)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((ok.failures, ok.last_error), (0, None));
    let gone = stores
        .fees
        .record_failure(404, Kind::Skills, t, "x", start, interval)
        .await;
    assert!(matches!(gone, Err(StoreError::MissingParent)));
}

async fn deferral(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    stores
        .fees
        .record_failure(
            1,
            Kind::Standings,
            t,
            "boom",
            Duration::minutes(1),
            Duration::hours(1),
        )
        .await
        .unwrap();
    let until = t + Duration::hours(2);

    stores
        .fees
        .record_deferral(1, Kind::Standings, t + Duration::seconds(3), until)
        .await
        .unwrap();
    stores
        .fees
        .record_deferral(1, Kind::Skills, t, until)
        .await
        .unwrap();

    let standings = stores
        .fees
        .fee_sync(1, Kind::Standings)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((standings.next_poll_at, standings.failures), (until, 1));
    assert_eq!(standings.last_error.as_deref(), Some("boom"));
    assert_eq!(standings.last_attempt_at, Some(t + Duration::seconds(3)));
    let skills = stores
        .fees
        .fee_sync(1, Kind::Skills)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (skills.next_poll_at, skills.failures, skills.last_error),
        (until, 0, None)
    );
    let gone = stores
        .fees
        .record_deferral(404, Kind::Skills, t, until)
        .await;
    assert!(matches!(gone, Err(StoreError::MissingParent)));
}

async fn schedule_and_claim(stores: &Stores) {
    account_with(stores, &[1, 2, 3]).await;
    stores.accounts.mark_needs_relogin(3).await.unwrap();
    let t = now();
    for kind in [Kind::Skills, Kind::Standings] {
        let mut left = stores.fees.list_unscheduled(kind, None).await.unwrap();
        left.sort_unstable();
        assert_eq!(left, [1, 2]);
    }

    stores
        .fees
        .insert_schedule(
            Kind::Skills,
            &[1, 2],
            &[t - Duration::minutes(10), t + Duration::hours(1)],
        )
        .await
        .unwrap();
    stores
        .fees
        .insert_schedule(Kind::Standings, &[2], &[t - Duration::minutes(20)])
        .await
        .unwrap();
    stores
        .fees
        .insert_schedule(Kind::Skills, &[1], &[t + Duration::days(5)])
        .await
        .unwrap();
    assert_eq!(
        stores
            .fees
            .fee_sync(1, Kind::Skills)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t - Duration::minutes(10),
        "a schedule is kept"
    );
    let missing = stores
        .fees
        .insert_schedule(Kind::Skills, &[404], &[t])
        .await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));

    let lease = Duration::minutes(5);
    let mut claimed = stores.fees.claim_due(t, lease, 10).await.unwrap();
    claimed.sort_by_key(|(id, kind)| (*id, kind.as_str()));
    assert_eq!(claimed, [(1, Kind::Skills), (2, Kind::Standings)]);
    assert!(
        stores
            .fees
            .claim_due(t, lease, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        stores
            .fees
            .fee_sync(1, Kind::Skills)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t + lease
    );
    // A character that needs a login is scheduled by hand but never claimed, and a limit counts.
    stores
        .fees
        .insert_schedule(Kind::Skills, &[3], &[t - Duration::days(1)])
        .await
        .unwrap();
    let limited = stores
        .fees
        .claim_due(t + Duration::days(1), lease, 1)
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_ne!(limited[0].0, 3);
}

async fn location_scope(stores: &Stores) {
    account_with(stores, &[1, 2]).await;
    stores
        .accounts
        .set_scopes(2, &[LOCATION_SCOPE.to_owned()])
        .await
        .unwrap();
    let t = now();

    let with_scope = stores
        .fees
        .list_unscheduled(Kind::Location, Some(LOCATION_SCOPE))
        .await
        .unwrap();
    assert_eq!(with_scope, [2], "only a character that granted the scope");
    assert_eq!(
        stores.fees.has_scope(1, LOCATION_SCOPE).await.unwrap(),
        Some(false)
    );
    assert_eq!(
        stores.fees.has_scope(2, LOCATION_SCOPE).await.unwrap(),
        Some(true)
    );
    assert_eq!(
        stores.fees.has_scope(404, LOCATION_SCOPE).await.unwrap(),
        None
    );

    stores
        .fees
        .insert_schedule(
            Kind::Location,
            &[1, 2],
            &[t - Duration::hours(1), t - Duration::hours(1)],
        )
        .await
        .unwrap();
    let claimed = stores
        .fees
        .claim_due(t, Duration::minutes(5), 10)
        .await
        .unwrap();
    assert_eq!(
        claimed,
        [(2, Kind::Location)],
        "character 1 never granted the scope"
    );
}

async fn make_due(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    stores
        .fees
        .insert_schedule(Kind::Skills, &[1], &[t + Duration::hours(1)])
        .await
        .unwrap();
    stores
        .fees
        .insert_schedule(Kind::Standings, &[1], &[t - Duration::hours(1)])
        .await
        .unwrap();

    stores.fees.make_due(1, Kind::Skills, t).await.unwrap();
    stores.fees.make_due(1, Kind::Standings, t).await.unwrap();
    stores.fees.make_due(1, Kind::Location, t).await.unwrap();
    stores.fees.make_due(404, Kind::Skills, t).await.unwrap();

    assert_eq!(
        stores
            .fees
            .fee_sync(1, Kind::Skills)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t
    );
    assert_eq!(
        stores
            .fees
            .fee_sync(1, Kind::Standings)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t - Duration::hours(1)
    );
    assert!(
        stores
            .fees
            .fee_sync(1, Kind::Location)
            .await
            .unwrap()
            .is_none()
    );
}

async fn unlinking(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    for id in [1, 2] {
        with_order(stores, id, STATION).await;
        polls(
            stores,
            id,
            &[(ACCOUNTING, 5)],
            &[standing(CALDARI_NAVY, "npc_corp", "1.0")],
        )
        .await;
        stores
            .fees
            .store_location(id, Some((AMARR, "station")), now(), now(), None)
            .await
            .unwrap();
    }

    stores.accounts.unlink_character(account, 1).await.unwrap();

    assert!(
        stores
            .fees
            .fee_sync(1, Kind::Skills)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        stores
            .fees
            .fee_sync(1, Kind::Location)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        stores
            .fees
            .fee_sync(2, Kind::Skills)
            .await
            .unwrap()
            .is_some()
    );
    assert!(stores.fees.for_characters(&[1]).await.unwrap().is_empty());
    // A new character of that ID starts from nothing.
    stores
        .accounts
        .upsert_character(
            account,
            &super::support::character(1),
            vec![1],
            vec![2],
            now(),
        )
        .await
        .unwrap();
    with_order(stores, 1, STATION).await;
    let rows = stores.fees.for_characters(&[1]).await.unwrap();
    assert_eq!(
        (rows[0].accounting, &rows[0].faction_standing),
        (None, &None)
    );
}

async fn seeded_rows(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    let standing = |from_id: i64, from_type: &str, value: &str| StandingInput {
        from_id,
        from_type: from_type.to_owned(),
        standing: value.to_owned(),
    };

    stores
        .fees
        .seed_skill(1, BROKER_RELATIONS, 3)
        .await
        .unwrap();
    stores.fees.seed_skill(1, ACCOUNTING, 4).await.unwrap();
    stores.fees.seed_skill(1, ACCOUNTING, 5).await.unwrap();
    assert_eq!(
        stores.fees.skills_of(1).await.unwrap(),
        [(BROKER_RELATIONS, 3), (ACCOUNTING, 5)]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );

    stores
        .fees
        .seed_standing(1, &standing(CALDARI_NAVY, "npc_corp", "1.5"))
        .await
        .unwrap();
    stores
        .fees
        .seed_standing(1, &standing(CALDARI_STATE, "faction", "2.0"))
        .await
        .unwrap();
    stores
        .fees
        .seed_standing(
            1,
            &standing(CALDARI_STATE, "faction", "0.07999999821186066"),
        )
        .await
        .unwrap();
    assert_eq!(
        stores.fees.standings_of(1).await.unwrap(),
        [
            (
                CALDARI_STATE,
                "faction".to_owned(),
                "0.07999999821186066".to_owned()
            ),
            (CALDARI_NAVY, "npc_corp".to_owned(), "1.5".to_owned()),
        ]
    );
    let bad = stores
        .fees
        .seed_standing(1, &standing(1, "faction", "lots"))
        .await;
    assert!(bad.is_err());

    // No sync state came with the rows; a success sets it, keeps the rows and clears a failure.
    assert!(
        stores
            .fees
            .fee_sync(1, Kind::Skills)
            .await
            .unwrap()
            .is_none()
    );
    stores
        .fees
        .record_failure(
            1,
            Kind::Skills,
            t,
            "boom",
            Duration::minutes(1),
            Duration::hours(1),
        )
        .await
        .unwrap();
    stores
        .fees
        .seed_fee_success(1, Kind::Skills, t)
        .await
        .unwrap();
    let state = stores
        .fees
        .fee_sync(1, Kind::Skills)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (state.last_success_at, state.last_error, state.failures),
        (Some(t), None, 0)
    );
    assert_eq!(stores.fees.skills_of(1).await.unwrap().len(), 2);
    for missing in [
        stores.fees.seed_skill(404, ACCOUNTING, 1).await,
        stores
            .fees
            .seed_standing(404, &standing(1, "faction", "1"))
            .await,
        stores.fees.seed_fee_success(404, Kind::Skills, t).await,
    ] {
        assert!(
            matches!(missing, Err(StoreError::MissingParent)),
            "{missing:?}"
        );
    }

    // The location and the owners are read as stored.
    assert_eq!(stores.fees.location_of(1).await.unwrap(), None);
    stores
        .fees
        .store_location(1, Some((STATION, "station")), t, t, None)
        .await
        .unwrap();
    assert_eq!(
        stores.fees.location_of(1).await.unwrap(),
        Some((STATION, "station".to_owned()))
    );
    assert_eq!(stores.fees.station_owner(STATION).await.unwrap(), None);
    assert_eq!(stores.fees.station_owner_count().await.unwrap(), 0);
    stores
        .fees
        .insert_station_owner(STATION, CALDARI_NAVY, t)
        .await
        .unwrap();
    assert_eq!(
        stores.fees.station_owner(STATION).await.unwrap(),
        Some(CALDARI_NAVY)
    );
    assert_eq!(stores.fees.station_owner_count().await.unwrap(), 1);
}
