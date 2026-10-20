//! `fees::FEES_SQL`: sales tax, broker fee and minimum margin per character and location, from
//! stored skills, standings and station owners.

mod common;

use sekondo::{
    esi::EsiOrder,
    fee_poller::Kind,
    fees::FeeRow,
    store::{NewCharacter, StandingInput, Stores},
};
use time::OffsetDateTime;
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;
const AMARR: i64 = 60008494;
const STRUCTURE: i64 = 1000000000001;
const ACCOUNTING: i64 = 16622;
const BROKER_RELATIONS: i64 = 3446;
const CALDARI_NAVY: i64 = 1000035;
const CALDARI_STATE: i64 = 500001;

async fn account(stores: &Stores) -> Uuid {
    stores.accounts.create_account().await.unwrap()
}

async fn character(stores: &Stores, account: Uuid, id: i64) {
    let character = NewCharacter {
        character_id: id,
        name: "name".to_owned(),
        owner_hash: "owner".to_owned(),
        scopes: Vec::new(),
    };
    stores
        .accounts
        .upsert_character(
            account,
            &character,
            vec![1],
            vec![2],
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
}

async fn order(stores: &Stores, id: i64, character: i64, location: i64) {
    let now = OffsetDateTime::now_utc();
    stores
        .orders
        .seed_order(
            character,
            &EsiOrder {
                order_id: id,
                type_id: 34,
                region_id: 10000002,
                location_id: location,
                is_buy_order: false,
                is_corporation: false,
                price_cents: 1000,
                volume_remain: 1,
                volume_total: 1,
                min_volume: 1,
                range: "station".to_owned(),
                escrow_cents: None,
                issued: now,
                duration: 90,
            },
        )
        .await
        .unwrap();
}

/// A successful poll of `kind`.
async fn polled(stores: &Stores, character: i64, kind: &str) {
    let kind = match kind {
        "skills" => Kind::Skills,
        "standings" => Kind::Standings,
        _ => Kind::Location,
    };
    stores
        .fees
        .seed_fee_success(character, kind, OffsetDateTime::now_utc())
        .await
        .unwrap();
}

async fn skill(stores: &Stores, character: i64, skill: i64, level: i16) {
    stores
        .fees
        .seed_skill(character, skill, level)
        .await
        .unwrap();
}

async fn standing(stores: &Stores, character: i64, from_id: i64, from_type: &str, value: &str) {
    stores
        .fees
        .seed_standing(
            character,
            &StandingInput {
                from_id,
                from_type: from_type.to_owned(),
                standing: value.to_owned(),
            },
        )
        .await
        .unwrap();
}

async fn owner(stores: &Stores, station: i64, owner: i64) {
    stores
        .fees
        .insert_station_owner(station, owner, OffsetDateTime::now_utc())
        .await
        .unwrap();
}

async fn fees_of(stores: &Stores, characters: &[i64]) -> Vec<FeeRow> {
    fee_store(stores).for_characters(characters).await.unwrap()
}

/// Alice with orders in Jita, both polls done, Broker Relations 5, Accounting 5, faction standing
/// 2.0 and corporation standing 1.5 towards the owner of Jita 4-4.
async fn alice_at_jita(stores: &Stores) -> Uuid {
    let acc = account(stores).await;
    character(stores, acc, ALICE).await;
    order(stores, 1, ALICE, JITA).await;
    polled(stores, ALICE, "skills").await;
    polled(stores, ALICE, "standings").await;
    skill(stores, ALICE, ACCOUNTING, 5).await;
    skill(stores, ALICE, BROKER_RELATIONS, 5).await;
    standing(stores, ALICE, CALDARI_STATE, "faction", "2.0").await;
    standing(stores, ALICE, CALDARI_NAVY, "npc_corp", "1.5").await;
    owner(stores, JITA, CALDARI_NAVY).await;
    acc
}

#[tokio::test]
async fn skills_and_standings_lower_the_fee() {
    let stores = common::stores();
    alice_at_jita(&stores).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!((row.character_id, row.location_id), (ALICE, JITA));
    assert!(!row.is_structure);
    assert_eq!(row.accounting, Some(5));
    assert_eq!(row.broker_relations, Some(5));
    assert_eq!(row.faction_standing.as_deref(), Some("2.0"));
    assert_eq!(row.corp_standing.as_deref(), Some("1.5"));
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    // 3 - 0.3*5 - 0.03*2.0 - 0.02*1.5
    assert_eq!(row.broker_fee.as_deref(), Some("1.410"));
    let margin = row.min_margin.as_deref().unwrap();
    assert!(margin.starts_with("6.5063"), "{margin}");
}

#[tokio::test]
async fn a_negative_standing_raises_the_fee() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    order(&stores, 1, ALICE, JITA).await;
    polled(&stores, ALICE, "skills").await;
    polled(&stores, ALICE, "standings").await;
    standing(&stores, ALICE, CALDARI_STATE, "faction", "-5.0").await;
    owner(&stores, JITA, CALDARI_NAVY).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(rows[0].broker_relations, Some(0), "polled but not trained");
    assert_eq!(
        rows[0].corp_standing.as_deref(),
        Some("0"),
        "no standing is 0"
    );
    assert_eq!(rows[0].broker_fee.as_deref(), Some("3.150"));
}

#[tokio::test]
async fn a_standing_keeps_every_digit() {
    let stores = common::stores();
    alice_at_jita(&stores).await;
    standing(
        &stores,
        ALICE,
        CALDARI_STATE,
        "faction",
        "0.07999999821186066",
    )
    .await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(
        rows[0].faction_standing.as_deref(),
        Some("0.07999999821186066")
    );
}

#[tokio::test]
async fn untrained_accounting_is_a_tax_of_seven_and_a_half_percent() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    order(&stores, 1, ALICE, JITA).await;
    polled(&stores, ALICE, "skills").await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(rows[0].accounting, Some(0));
    assert_eq!(rows[0].sales_tax.as_deref(), Some("7.500"));
}

#[tokio::test]
async fn the_active_level_counts_not_the_trained_one() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    order(&stores, 1, ALICE, JITA).await;
    polled(&stores, ALICE, "skills").await;
    skill(&stores, ALICE, ACCOUNTING, 4).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    // 7.5 * (1 - 0.11 * 4)
    assert_eq!(rows[0].sales_tax.as_deref(), Some("4.200"));
}

#[tokio::test]
async fn a_structure_has_a_sales_tax_but_no_broker_fee_or_margin() {
    let stores = common::stores();
    alice_at_jita(&stores).await;
    order(&stores, 2, ALICE, STRUCTURE).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(rows.len(), 2);
    let structure = rows.iter().find(|r| r.location_id == STRUCTURE).unwrap();
    assert!(structure.is_structure);
    assert_eq!(structure.sales_tax.as_deref(), Some("3.375"));
    assert_eq!(structure.broker_fee, None);
    assert_eq!(structure.min_margin, None);
    assert!(
        rows.iter()
            .find(|r| r.location_id == JITA)
            .unwrap()
            .broker_fee
            .is_some()
    );
}

#[tokio::test]
async fn a_character_whose_skills_were_never_polled_has_nothing() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    order(&stores, 1, ALICE, JITA).await;
    order(&stores, 2, ALICE, STRUCTURE).await;
    // The sync rows exist but nothing succeeded yet.
    let now = OffsetDateTime::now_utc();
    for kind in [Kind::Skills, Kind::Standings] {
        stores
            .fees
            .insert_schedule(kind, &[ALICE], &[now])
            .await
            .unwrap();
    }
    owner(&stores, JITA, CALDARI_NAVY).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.accounting, None);
        assert_eq!(row.broker_relations, None);
        assert_eq!(row.sales_tax, None);
        assert_eq!(row.broker_fee, None);
        assert_eq!(row.min_margin, None);
    }
}

#[tokio::test]
async fn without_standings_polled_there_is_no_npc_fee_but_a_sales_tax() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    order(&stores, 1, ALICE, JITA).await;
    polled(&stores, ALICE, "skills").await;
    owner(&stores, JITA, CALDARI_NAVY).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    assert_eq!(rows[0].faction_standing, None);
    assert_eq!(rows[0].sales_tax.as_deref(), Some("7.500"));
    assert_eq!(rows[0].broker_fee, None);
    assert_eq!(rows[0].min_margin, None);
}

#[tokio::test]
async fn an_unknown_station_owner_leaves_the_station_without_a_fee() {
    let stores = common::stores();
    alice_at_jita(&stores).await;
    order(&stores, 2, ALICE, AMARR).await;

    let rows = fees_of(&stores, &[ALICE]).await;

    let amarr = rows.iter().find(|r| r.location_id == AMARR).unwrap();
    assert_eq!(amarr.sales_tax.as_deref(), Some("3.375"));
    assert_eq!(amarr.broker_fee, None);
    assert_eq!(amarr.min_margin, None);
    assert!(
        rows.iter()
            .find(|r| r.location_id == JITA)
            .unwrap()
            .broker_fee
            .is_some(),
        "the other station is unaffected"
    );
}

#[tokio::test]
async fn an_owner_without_a_faction_counts_the_faction_standing_as_zero() {
    let stores = common::stores();
    alice_at_jita(&stores).await;
    order(&stores, 2, ALICE, AMARR).await;
    // 1999999 is in no faction table; the corporation standing still counts.
    owner(&stores, AMARR, 1999999).await;
    standing(&stores, ALICE, 1999999, "npc_corp", "1.0").await;

    let rows = fees_of(&stores, &[ALICE]).await;

    let amarr = rows.iter().find(|r| r.location_id == AMARR).unwrap();
    assert_eq!(amarr.faction_standing.as_deref(), Some("0"));
    // 3 - 1.5 - 0 - 0.02*1.0
    assert_eq!(amarr.broker_fee.as_deref(), Some("1.480"));
}

#[tokio::test]
async fn a_station_is_listed_once_however_many_orders_are_there() {
    let stores = common::stores();
    alice_at_jita(&stores).await;
    order(&stores, 2, ALICE, JITA).await;
    order(&stores, 3, ALICE, JITA).await;

    assert_eq!(fees_of(&stores, &[ALICE]).await.len(), 1);
}

#[tokio::test]
async fn a_character_without_orders_lists_nothing() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    polled(&stores, ALICE, "skills").await;

    assert!(fees_of(&stores, &[ALICE]).await.is_empty());
}

#[tokio::test]
async fn only_the_selected_characters_of_the_account_are_listed() {
    let stores = common::stores();
    let acc = alice_at_jita(&stores).await;
    character(&stores, acc, BOB).await;
    order(&stores, 5, BOB, JITA).await;
    let other = account(&stores).await;
    character(&stores, other, 90000003).await;
    order(&stores, 6, 90000003, JITA).await;

    let only_bob: Vec<i64> = fees_of(&stores, &[BOB])
        .await
        .iter()
        .map(|r| r.character_id)
        .collect();
    assert_eq!(only_bob, [BOB]);

    let both = fee_store(&stores)
        .for_account(acc, &[ALICE, BOB, 90000003])
        .await
        .unwrap();
    let ids: Vec<i64> = both.iter().map(|r| r.character_id).collect();
    assert_eq!(
        ids,
        [ALICE, BOB],
        "another account's character is never reached"
    );
}

#[tokio::test]
async fn the_same_inputs_give_the_same_rows_and_a_change_gives_different_ones() {
    let stores = common::stores();
    alice_at_jita(&stores).await;
    let before = fees_of(&stores, &[ALICE]).await;
    assert_eq!(before, fees_of(&stores, &[ALICE]).await);

    skill(&stores, ALICE, ACCOUNTING, 4).await;

    assert_ne!(before, fees_of(&stores, &[ALICE]).await);
}

// ---- the built-in station for a character without orders there ----

#[tokio::test]
async fn a_character_without_orders_in_jita_still_has_fees_there() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;
    order(&stores, 1, ALICE, AMARR).await;
    polled(&stores, ALICE, "skills").await;
    polled(&stores, ALICE, "standings").await;
    skill(&stores, ALICE, ACCOUNTING, 5).await;
    skill(&stores, ALICE, BROKER_RELATIONS, 5).await;
    standing(&stores, ALICE, CALDARI_STATE, "faction", "2.0").await;
    standing(&stores, ALICE, CALDARI_NAVY, "npc_corp", "1.5").await;
    owner(&stores, JITA, CALDARI_NAVY).await;

    let at_jita = fee_store(&stores)
        .at_location(acc, &[ALICE], JITA)
        .await
        .unwrap();

    assert_eq!(at_jita.len(), 1);
    let row = &at_jita[0];
    assert_eq!((row.character_id, row.location_id), (ALICE, JITA));
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    assert_eq!(row.broker_fee.as_deref(), Some("1.410"));
    assert!(row.min_margin.as_deref().unwrap().starts_with("6.5063"));
    // What the orders page lists is still driven by the orders: only Amarr.
    let listed = fees_of(&stores, &[ALICE]).await;
    assert_eq!(
        listed.iter().map(|r| r.location_id).collect::<Vec<_>>(),
        [AMARR]
    );
}

#[tokio::test]
async fn fees_at_a_location_follow_the_polls_and_the_owner_like_the_order_fees() {
    let stores = common::stores();
    let acc = account(&stores).await;
    character(&stores, acc, ALICE).await;

    // Nothing polled: nothing known.
    let row = &fee_store(&stores)
        .at_location(acc, &[ALICE], JITA)
        .await
        .unwrap()[0];
    assert_eq!(
        (&row.sales_tax, &row.broker_fee, &row.min_margin),
        (&None, &None, &None)
    );

    // Skills polled: a sales tax, but no broker fee without standings and an owner.
    polled(&stores, ALICE, "skills").await;
    skill(&stores, ALICE, ACCOUNTING, 5).await;
    skill(&stores, ALICE, BROKER_RELATIONS, 5).await;
    let row = &fee_store(&stores)
        .at_location(acc, &[ALICE], JITA)
        .await
        .unwrap()[0];
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    assert_eq!((&row.broker_fee, &row.min_margin), (&None, &None));

    // Standings polled too, but the owner is not known yet.
    polled(&stores, ALICE, "standings").await;
    let row = &fee_store(&stores)
        .at_location(acc, &[ALICE], JITA)
        .await
        .unwrap()[0];
    assert_eq!(row.broker_fee, None);

    // The owner is known: the fee follows (no standings towards it count as 0).
    owner(&stores, JITA, CALDARI_NAVY).await;
    let row = &fee_store(&stores)
        .at_location(acc, &[ALICE], JITA)
        .await
        .unwrap()[0];
    assert_eq!(row.broker_fee.as_deref(), Some("1.50"), "3 - 0.3 * 5");
}

#[tokio::test]
async fn fees_at_a_location_never_reach_another_accounts_characters() {
    let stores = common::stores();
    let acc = account(&stores).await;
    let other = account(&stores).await;
    character(&stores, acc, ALICE).await;
    character(&stores, other, BOB).await;

    let rows = fee_store(&stores)
        .at_location(acc, &[ALICE, BOB], JITA)
        .await
        .unwrap();

    assert_eq!(
        rows.iter().map(|r| r.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
}

// ---- the fee location ----

const LOCATION_SCOPE: &str = "esi-location.read_location.v1";

async fn grant_location_scope(stores: &Stores, character: i64) {
    stores
        .accounts
        .set_scopes(character, &[LOCATION_SCOPE.to_owned()])
        .await
        .unwrap();
}

async fn docked(stores: &Stores, character: i64, location: i64, kind: &str) {
    stores
        .fees
        .store_location(
            character,
            Some((location, kind)),
            OffsetDateTime::now_utc(),
            OffsetDateTime::now_utc(),
            None,
        )
        .await
        .unwrap();
}

async fn fee_location_of(stores: &Stores, acc: Uuid, character: i64) -> (i64, bool) {
    let rows = fee_store(stores)
        .at_fee_location(acc, &[character])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    (rows[0].row.location_id, rows[0].needs_location_scope)
}

#[tokio::test]
async fn without_the_location_scope_the_fee_location_is_jita_with_the_relogin_hint() {
    let stores = common::stores();
    let acc = alice_at_jita(&stores).await;
    // A docked row left over from before the scope went missing is not used.
    docked(&stores, ALICE, AMARR, "station").await;

    assert_eq!(fee_location_of(&stores, acc, ALICE).await, (JITA, true));
}

#[tokio::test]
async fn a_docked_character_has_its_fees_at_the_station_it_is_docked_in() {
    let stores = common::stores();
    let acc = alice_at_jita(&stores).await;
    grant_location_scope(&stores, ALICE).await;
    owner(&stores, AMARR, 1000086).await;
    docked(&stores, ALICE, AMARR, "station").await;

    assert_eq!(fee_location_of(&stores, acc, ALICE).await, (AMARR, false));
    let row = &fee_store(&stores)
        .at_fee_location(acc, &[ALICE])
        .await
        .unwrap()[0]
        .row;
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    assert!(
        row.broker_fee.is_some(),
        "the owner of the docked station is known"
    );
}

#[tokio::test]
async fn a_character_docked_in_a_structure_has_its_fees_there() {
    let stores = common::stores();
    let acc = alice_at_jita(&stores).await;
    grant_location_scope(&stores, ALICE).await;
    docked(&stores, ALICE, STRUCTURE, "structure").await;

    let row = &fee_store(&stores)
        .at_fee_location(acc, &[ALICE])
        .await
        .unwrap()[0]
        .row;

    assert_eq!((row.location_id, row.is_structure), (STRUCTURE, true));
    assert_eq!(row.broker_fee, None);
}

#[tokio::test]
async fn a_character_never_reported_docked_uses_jita() {
    let stores = common::stores();
    let acc = alice_at_jita(&stores).await;
    grant_location_scope(&stores, ALICE).await;

    assert_eq!(fee_location_of(&stores, acc, ALICE).await, (JITA, false));
}

#[tokio::test]
async fn the_fee_location_never_reaches_another_accounts_characters() {
    let stores = common::stores();
    let acc = account(&stores).await;
    let other = account(&stores).await;
    character(&stores, acc, ALICE).await;
    character(&stores, other, BOB).await;

    let rows = fee_store(&stores)
        .at_fee_location(acc, &[ALICE, BOB])
        .await
        .unwrap();

    assert_eq!(
        rows.iter().map(|r| r.row.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
}

fn fee_store(stores: &Stores) -> std::sync::Arc<dyn sekondo::store::FeeStore> {
    stores.fees.clone()
}
