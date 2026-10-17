//! `fees::FEES_SQL`: sales tax, broker fee and minimum margin per character and location, from
//! stored skills, standings and station owners.

use sekondo::fees::{self, FeeRow};
use sqlx::PgPool;
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

async fn account(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn character(pool: &PgPool, account: Uuid, id: i64) {
    sqlx::query("INSERT INTO characters (character_id, account_id, name, owner_hash) VALUES ($1, $2, 'name', 'owner')")
        .bind(id)
        .bind(account)
        .execute(pool)
        .await
        .unwrap();
}

async fn order(pool: &PgPool, id: i64, character: i64, location: i64) {
    sqlx::query(
        "INSERT INTO current_orders (order_id, character_id, type_id, region_id, location_id, \
         is_buy_order, is_corporation, price, volume_remain, volume_total, min_volume, range, \
         issued, duration) \
         VALUES ($1, $2, 34, 10000002, $3, false, false, 10, 1, 1, 1, 'station', now(), 90)",
    )
    .bind(id)
    .bind(character)
    .bind(location)
    .execute(pool)
    .await
    .unwrap();
}

/// A successful poll of `kind`.
async fn polled(pool: &PgPool, character: i64, kind: &str) {
    sqlx::query(
        "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_success_at) \
         VALUES ($1, $2, now(), now())",
    )
    .bind(character)
    .bind(kind)
    .execute(pool)
    .await
    .unwrap();
}

async fn skill(pool: &PgPool, character: i64, skill: i64, level: i16) {
    sqlx::query(
        "INSERT INTO character_skills (character_id, skill_id, active_level) VALUES ($1, $2, $3)",
    )
    .bind(character)
    .bind(skill)
    .bind(level)
    .execute(pool)
    .await
    .unwrap();
}

async fn standing(pool: &PgPool, character: i64, from_id: i64, from_type: &str, value: &str) {
    sqlx::query(
        "INSERT INTO character_standings (character_id, from_id, from_type, standing) \
         VALUES ($1, $2, $3, $4::numeric)",
    )
    .bind(character)
    .bind(from_id)
    .bind(from_type)
    .bind(value)
    .execute(pool)
    .await
    .unwrap();
}

async fn owner(pool: &PgPool, station: i64, owner: i64) {
    sqlx::query(
        "INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES ($1, $2, now())",
    )
    .bind(station)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
}

async fn fees_of(pool: &PgPool, characters: &[i64]) -> Vec<FeeRow> {
    fees::for_characters(pool, characters).await.unwrap()
}

/// Alice with orders in Jita, both polls done, Broker Relations 5, Accounting 5, faction standing
/// 2.0 and corporation standing 1.5 towards the owner of Jita 4-4.
async fn alice_at_jita(pool: &PgPool) -> Uuid {
    let acc = account(pool).await;
    character(pool, acc, ALICE).await;
    order(pool, 1, ALICE, JITA).await;
    polled(pool, ALICE, "skills").await;
    polled(pool, ALICE, "standings").await;
    skill(pool, ALICE, ACCOUNTING, 5).await;
    skill(pool, ALICE, BROKER_RELATIONS, 5).await;
    standing(pool, ALICE, CALDARI_STATE, "faction", "2.0").await;
    standing(pool, ALICE, CALDARI_NAVY, "npc_corp", "1.5").await;
    owner(pool, JITA, CALDARI_NAVY).await;
    acc
}

#[sqlx::test]
async fn skills_and_standings_lower_the_fee(pool: PgPool) {
    alice_at_jita(&pool).await;

    let rows = fees_of(&pool, &[ALICE]).await;

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

#[sqlx::test]
async fn a_negative_standing_raises_the_fee(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    order(&pool, 1, ALICE, JITA).await;
    polled(&pool, ALICE, "skills").await;
    polled(&pool, ALICE, "standings").await;
    standing(&pool, ALICE, CALDARI_STATE, "faction", "-5.0").await;
    owner(&pool, JITA, CALDARI_NAVY).await;

    let rows = fees_of(&pool, &[ALICE]).await;

    assert_eq!(rows[0].broker_relations, Some(0), "polled but not trained");
    assert_eq!(
        rows[0].corp_standing.as_deref(),
        Some("0"),
        "no standing is 0"
    );
    assert_eq!(rows[0].broker_fee.as_deref(), Some("3.150"));
}

#[sqlx::test]
async fn a_standing_keeps_every_digit(pool: PgPool) {
    alice_at_jita(&pool).await;
    sqlx::query("UPDATE character_standings SET standing = '0.07999999821186066'::numeric WHERE from_type = 'faction'")
        .execute(&pool)
        .await
        .unwrap();

    let rows = fees_of(&pool, &[ALICE]).await;

    assert_eq!(
        rows[0].faction_standing.as_deref(),
        Some("0.07999999821186066")
    );
}

#[sqlx::test]
async fn untrained_accounting_is_a_tax_of_seven_and_a_half_percent(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    order(&pool, 1, ALICE, JITA).await;
    polled(&pool, ALICE, "skills").await;

    let rows = fees_of(&pool, &[ALICE]).await;

    assert_eq!(rows[0].accounting, Some(0));
    assert_eq!(rows[0].sales_tax.as_deref(), Some("7.500"));
}

#[sqlx::test]
async fn the_active_level_counts_not_the_trained_one(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    order(&pool, 1, ALICE, JITA).await;
    polled(&pool, ALICE, "skills").await;
    skill(&pool, ALICE, ACCOUNTING, 4).await;

    let rows = fees_of(&pool, &[ALICE]).await;

    // 7.5 * (1 - 0.11 * 4)
    assert_eq!(rows[0].sales_tax.as_deref(), Some("4.200"));
}

#[sqlx::test]
async fn a_structure_has_a_sales_tax_but_no_broker_fee_or_margin(pool: PgPool) {
    alice_at_jita(&pool).await;
    order(&pool, 2, ALICE, STRUCTURE).await;

    let rows = fees_of(&pool, &[ALICE]).await;

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

#[sqlx::test]
async fn a_character_whose_skills_were_never_polled_has_nothing(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    order(&pool, 1, ALICE, JITA).await;
    order(&pool, 2, ALICE, STRUCTURE).await;
    // The sync rows exist but nothing succeeded yet.
    sqlx::query("INSERT INTO fee_sync (character_id, kind, next_poll_at) VALUES ($1, 'skills', now()), ($1, 'standings', now())")
        .bind(ALICE)
        .execute(&pool)
        .await
        .unwrap();
    owner(&pool, JITA, CALDARI_NAVY).await;

    let rows = fees_of(&pool, &[ALICE]).await;

    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.accounting, None);
        assert_eq!(row.broker_relations, None);
        assert_eq!(row.sales_tax, None);
        assert_eq!(row.broker_fee, None);
        assert_eq!(row.min_margin, None);
    }
}

#[sqlx::test]
async fn without_standings_polled_there_is_no_npc_fee_but_a_sales_tax(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    order(&pool, 1, ALICE, JITA).await;
    polled(&pool, ALICE, "skills").await;
    owner(&pool, JITA, CALDARI_NAVY).await;

    let rows = fees_of(&pool, &[ALICE]).await;

    assert_eq!(rows[0].faction_standing, None);
    assert_eq!(rows[0].sales_tax.as_deref(), Some("7.500"));
    assert_eq!(rows[0].broker_fee, None);
    assert_eq!(rows[0].min_margin, None);
}

#[sqlx::test]
async fn an_unknown_station_owner_leaves_the_station_without_a_fee(pool: PgPool) {
    alice_at_jita(&pool).await;
    order(&pool, 2, ALICE, AMARR).await;

    let rows = fees_of(&pool, &[ALICE]).await;

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

#[sqlx::test]
async fn an_owner_without_a_faction_counts_the_faction_standing_as_zero(pool: PgPool) {
    alice_at_jita(&pool).await;
    order(&pool, 2, ALICE, AMARR).await;
    // 1999999 is in no faction table; the corporation standing still counts.
    owner(&pool, AMARR, 1999999).await;
    standing(&pool, ALICE, 1999999, "npc_corp", "1.0").await;

    let rows = fees_of(&pool, &[ALICE]).await;

    let amarr = rows.iter().find(|r| r.location_id == AMARR).unwrap();
    assert_eq!(amarr.faction_standing.as_deref(), Some("0"));
    // 3 - 1.5 - 0 - 0.02*1.0
    assert_eq!(amarr.broker_fee.as_deref(), Some("1.480"));
}

#[sqlx::test]
async fn a_station_is_listed_once_however_many_orders_are_there(pool: PgPool) {
    alice_at_jita(&pool).await;
    order(&pool, 2, ALICE, JITA).await;
    order(&pool, 3, ALICE, JITA).await;

    assert_eq!(fees_of(&pool, &[ALICE]).await.len(), 1);
}

#[sqlx::test]
async fn a_character_without_orders_lists_nothing(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    polled(&pool, ALICE, "skills").await;

    assert!(fees_of(&pool, &[ALICE]).await.is_empty());
}

#[sqlx::test]
async fn only_the_selected_characters_of_the_account_are_listed(pool: PgPool) {
    let acc = alice_at_jita(&pool).await;
    character(&pool, acc, BOB).await;
    order(&pool, 5, BOB, JITA).await;
    let other = account(&pool).await;
    character(&pool, other, 90000003).await;
    order(&pool, 6, 90000003, JITA).await;

    let only_bob: Vec<i64> = fees_of(&pool, &[BOB])
        .await
        .iter()
        .map(|r| r.character_id)
        .collect();
    assert_eq!(only_bob, [BOB]);

    let both = fees::for_account(&pool, acc, &[ALICE, BOB, 90000003])
        .await
        .unwrap();
    let ids: Vec<i64> = both.iter().map(|r| r.character_id).collect();
    assert_eq!(
        ids,
        [ALICE, BOB],
        "another account's character is never reached"
    );
}

#[sqlx::test]
async fn the_same_inputs_give_the_same_rows_and_a_change_gives_different_ones(pool: PgPool) {
    alice_at_jita(&pool).await;
    let before = fees_of(&pool, &[ALICE]).await;
    assert_eq!(before, fees_of(&pool, &[ALICE]).await);

    sqlx::query("UPDATE character_skills SET active_level = 4 WHERE skill_id = 16622")
        .execute(&pool)
        .await
        .unwrap();

    assert_ne!(before, fees_of(&pool, &[ALICE]).await);
}

// ---- the built-in station for a character without orders there ----

#[sqlx::test]
async fn a_character_without_orders_in_jita_still_has_fees_there(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;
    order(&pool, 1, ALICE, AMARR).await;
    polled(&pool, ALICE, "skills").await;
    polled(&pool, ALICE, "standings").await;
    skill(&pool, ALICE, ACCOUNTING, 5).await;
    skill(&pool, ALICE, BROKER_RELATIONS, 5).await;
    standing(&pool, ALICE, CALDARI_STATE, "faction", "2.0").await;
    standing(&pool, ALICE, CALDARI_NAVY, "npc_corp", "1.5").await;
    owner(&pool, JITA, CALDARI_NAVY).await;

    let at_jita = fees::at_location(&pool, acc, &[ALICE], JITA).await.unwrap();

    assert_eq!(at_jita.len(), 1);
    let row = &at_jita[0];
    assert_eq!((row.character_id, row.location_id), (ALICE, JITA));
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    assert_eq!(row.broker_fee.as_deref(), Some("1.410"));
    assert!(row.min_margin.as_deref().unwrap().starts_with("6.5063"));
    // What the orders page lists is still driven by the orders: only Amarr.
    let listed = fees_of(&pool, &[ALICE]).await;
    assert_eq!(
        listed.iter().map(|r| r.location_id).collect::<Vec<_>>(),
        [AMARR]
    );
}

#[sqlx::test]
async fn fees_at_a_location_follow_the_polls_and_the_owner_like_the_order_fees(pool: PgPool) {
    let acc = account(&pool).await;
    character(&pool, acc, ALICE).await;

    // Nothing polled: nothing known.
    let row = &fees::at_location(&pool, acc, &[ALICE], JITA).await.unwrap()[0];
    assert_eq!(
        (&row.sales_tax, &row.broker_fee, &row.min_margin),
        (&None, &None, &None)
    );

    // Skills polled: a sales tax, but no broker fee without standings and an owner.
    polled(&pool, ALICE, "skills").await;
    skill(&pool, ALICE, ACCOUNTING, 5).await;
    skill(&pool, ALICE, BROKER_RELATIONS, 5).await;
    let row = &fees::at_location(&pool, acc, &[ALICE], JITA).await.unwrap()[0];
    assert_eq!(row.sales_tax.as_deref(), Some("3.375"));
    assert_eq!((&row.broker_fee, &row.min_margin), (&None, &None));

    // Standings polled too, but the owner is not known yet.
    polled(&pool, ALICE, "standings").await;
    let row = &fees::at_location(&pool, acc, &[ALICE], JITA).await.unwrap()[0];
    assert_eq!(row.broker_fee, None);

    // The owner is known: the fee follows (no standings towards it count as 0).
    owner(&pool, JITA, CALDARI_NAVY).await;
    let row = &fees::at_location(&pool, acc, &[ALICE], JITA).await.unwrap()[0];
    assert_eq!(row.broker_fee.as_deref(), Some("1.50"), "3 - 0.3 * 5");
}

#[sqlx::test]
async fn fees_at_a_location_never_reach_another_accounts_characters(pool: PgPool) {
    let acc = account(&pool).await;
    let other = account(&pool).await;
    character(&pool, acc, ALICE).await;
    character(&pool, other, BOB).await;

    let rows = fees::at_location(&pool, acc, &[ALICE, BOB], JITA)
        .await
        .unwrap();

    assert_eq!(
        rows.iter().map(|r| r.character_id).collect::<Vec<_>>(),
        [ALICE]
    );
}
