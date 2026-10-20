mod common;

use common::PollEnv;
use sekondo::store::{RememberedSellOrder, StandingInput, TradeSeed};
use serde_json::json;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

/// ISK as written in the figures, in cents.
fn cents(isk: &str) -> i64 {
    (isk.parse::<f64>().unwrap() * 100.0).round() as i64
}

/// Cents as the figures write ISK.
fn isk(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

/// Noon of day `day` of October 2026.
fn day(day: u32) -> time::OffsetDateTime {
    time::OffsetDateTime::new_utc(
        time::Date::from_calendar_date(2026, time::Month::October, day as u8).unwrap(),
        time::Time::from_hms(12, 0, 0).unwrap(),
    )
}

async fn trade(
    env: &PollEnv,
    (id, character): (i64, i64),
    (is_sale, on_day): (bool, u32),
    quantity: i64,
    price: &str,
) {
    env.stores
        .trades
        .seed_trade(
            character,
            &TradeSeed {
                transaction_id: id,
                is_sale,
                type_id: 34,
                location_id: 60003760,
                quantity,
                unit_price_cents: cents(price),
                at: day(on_day),
            },
        )
        .await
        .unwrap();
}

/// A purchase of 34, bought on day `day` of October 2026.
async fn buy(env: &PollEnv, who: (i64, i64), day: u32, quantity: i64, price: &str) {
    trade(env, who, (false, day), quantity, price).await;
}

/// A remembered sell order of 34, first issued on day `day`.
async fn remember(env: &PollEnv, (id, character): (i64, i64), on_day: u32, volume: i64) {
    env.stores
        .trades
        .seed_remembered_order(
            character,
            &RememberedSellOrder {
                order_id: id,
                type_id: 34,
                first_issued: day(on_day),
                volume_total: volume,
            },
        )
        .await
        .unwrap();
}

async fn recompute(env: &PollEnv, character: i64) {
    env.stores.trades.settle(character, &[34]).await.unwrap();
}

/// `(order, covered units, cost)` of every stored cost.
async fn costs(env: &PollEnv) -> Vec<(i64, i64, String)> {
    let rows = env.stores.trades.order_costs().await.unwrap();
    rows.into_iter()
        .map(|(id, units, cost)| (id, units, isk(cost)))
        .collect()
}

/// A sale of 34, sold on day `day` of October 2026.
async fn sold(env: &PollEnv, who: (i64, i64), day: u32, quantity: i64, price: &str) {
    trade(env, who, (true, day), quantity, price).await;
}

/// `(sale, covered units, cost)` of every stored sale cost.
async fn sale_costs(env: &PollEnv) -> Vec<(i64, i64, String)> {
    let rows = env.stores.trades.sale_costs().await.unwrap();
    rows.into_iter()
        .map(|(id, units, cost)| (id, units, isk(cost)))
        .collect()
}

async fn two_characters() -> PollEnv {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env.link(account, BOB).await;
    env
}

#[tokio::test]
async fn the_stored_costs_follow_first_in_first_out() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    buy(&env, (2, ALICE), 2, 10, "120.00").await;
    remember(&env, (11, ALICE), 3, 15).await;
    remember(&env, (12, ALICE), 4, 10).await;

    recompute(&env, ALICE).await;

    assert_eq!(
        costs(&env).await,
        [(11, 15, "1600.00".to_owned()), (12, 5, "600.00".to_owned())]
    );
}

#[tokio::test]
async fn other_characters_purchases_do_not_count() {
    let env = two_characters().await;
    buy(&env, (1, BOB), 1, 100, "100.00").await;
    remember(&env, (11, ALICE), 3, 10).await;

    recompute(&env, ALICE).await;

    assert_eq!(costs(&env).await, [(11, 0, "0.00".to_owned())]);
}

#[tokio::test]
async fn a_purchase_stored_late_feeds_the_orders_issued_after_it() {
    let env = two_characters().await;
    remember(&env, (11, ALICE), 3, 10).await;
    recompute(&env, ALICE).await;
    assert_eq!(costs(&env).await, [(11, 0, "0.00".to_owned())]);

    // The purchase made just before the order only arrives with a later transactions poll.
    buy(&env, (1, ALICE), 2, 10, "100.00").await;
    recompute(&env, ALICE).await;

    assert_eq!(costs(&env).await, [(11, 10, "1000.00".to_owned())]);
}

#[tokio::test]
async fn the_stored_sale_costs_follow_first_in_first_out() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    buy(&env, (2, ALICE), 2, 10, "120.00").await;
    sold(&env, (31, ALICE), 3, 15, "150.00").await;
    sold(&env, (32, ALICE), 4, 10, "150.00").await;

    recompute(&env, ALICE).await;

    assert_eq!(
        sale_costs(&env).await,
        [(31, 15, "1600.00".to_owned()), (32, 5, "600.00".to_owned())]
    );
}

#[tokio::test]
async fn other_characters_purchases_are_not_taken_by_a_sale() {
    let env = two_characters().await;
    buy(&env, (1, BOB), 1, 100, "100.00").await;
    sold(&env, (31, ALICE), 3, 10, "150.00").await;

    recompute(&env, ALICE).await;

    assert_eq!(sale_costs(&env).await, [(31, 0, "0.00".to_owned())]);
}

#[tokio::test]
async fn a_sale_and_a_sell_order_both_take_the_same_lot() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    remember(&env, (11, ALICE), 3, 10).await;
    sold(&env, (31, ALICE), 3, 10, "150.00").await;

    recompute(&env, ALICE).await;

    assert_eq!(costs(&env).await, [(11, 10, "1000.00".to_owned())]);
    assert_eq!(sale_costs(&env).await, [(31, 10, "1000.00".to_owned())]);
}

#[tokio::test]
async fn a_purchase_stored_late_feeds_a_sale_stored_before_it() {
    let env = two_characters().await;
    sold(&env, (31, ALICE), 3, 10, "150.00").await;
    recompute(&env, ALICE).await;
    assert_eq!(sale_costs(&env).await, [(31, 0, "0.00".to_owned())]);

    buy(&env, (1, ALICE), 2, 10, "100.00").await;
    recompute(&env, ALICE).await;

    assert_eq!(sale_costs(&env).await, [(31, 10, "1000.00".to_owned())]);
}

#[tokio::test]
async fn a_recompute_touches_only_the_given_character_and_items() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    remember(&env, (11, ALICE), 3, 10).await;
    remember(&env, (21, BOB), 3, 10).await;
    env.stores.trades.seed_order_cost(21, 7, 700).await.unwrap();

    recompute(&env, ALICE).await;

    assert_eq!(
        costs(&env).await,
        [(11, 10, "1000.00".to_owned()), (21, 7, "7.00".to_owned())]
    );
}

#[tokio::test]
async fn an_order_poll_stores_the_cost_of_a_newly_remembered_sell_order() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 30, "100.00").await;
    let order = json!({
        "order_id": 11, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "is_buy_order": false, "is_corporation": false, "price": 150.0,
        "volume_remain": 20, "volume_total": 20, "min_volume": 1, "range": "station",
        "issued": "2026-10-03T12:00:00Z", "duration": 90
    });
    let res = env
        .control("orders", json!({"character_id": ALICE, "orders": [order]}))
        .await;
    assert_eq!(res.status(), 204);

    env.poller.poll_character(ALICE).await;

    assert_eq!(costs(&env).await, [(11, 20, "2000.00".to_owned())]);
}

// ---- margin of a sell order ----

const JITA: i64 = 60003760;
const STRUCTURE: i64 = 1000000000001;

/// Alice has polled skills and standings: Accounting 5 and Broker Relations 5, a faction standing
/// of 2.0 and a corporation standing of 1.5 towards the owner of Jita 4-4. That is a sales tax of
/// 3.375% and a broker fee of 1.41% there.
async fn alice_with_fees(env: &PollEnv) {
    let now = time::OffsetDateTime::now_utc();
    let fees = &env.stores.fees;
    for kind in [
        sekondo::fee_poller::Kind::Skills,
        sekondo::fee_poller::Kind::Standings,
    ] {
        fees.seed_fee_success(ALICE, kind, now).await.unwrap();
    }
    for skill in [16622, 3446] {
        fees.seed_skill(ALICE, skill, 5).await.unwrap();
    }
    for (from_id, from_type, standing) in [(500001, "faction", "2.0"), (1000035, "npc_corp", "1.5")]
    {
        fees.seed_standing(
            ALICE,
            &StandingInput {
                from_id,
                from_type: from_type.to_owned(),
                standing: standing.to_owned(),
            },
        )
        .await
        .unwrap();
    }
    fees.insert_station_owner(60003760, 1000035, now)
        .await
        .unwrap();
}

/// Alice's two purchases, 10 at 100.00 and 10 at 120.00.
async fn alice_with_two_lots(env: &PollEnv) {
    buy(env, (1, ALICE), 1, 10, "100.00").await;
    buy(env, (2, ALICE), 2, 10, "120.00").await;
}

/// A current sell order of 34 that is also remembered, first issued on day 3, with its cost.
async fn sell(
    env: &PollEnv,
    id: i64,
    (location, corporation): (i64, bool),
    (price, remain, total): (&str, i64, i64),
) {
    let issued = day(3);
    env.stores
        .orders
        .replace_orders(
            ALICE,
            issued,
            &[sekondo::esi::EsiOrder {
                order_id: id,
                type_id: 34,
                region_id: 10000002,
                location_id: location,
                is_buy_order: false,
                is_corporation: corporation,
                price_cents: cents(price),
                volume_remain: remain,
                volume_total: total,
                min_volume: 1,
                range: "station".to_owned(),
                escrow_cents: None,
                issued,
                duration: 90,
            }],
            issued + time::Duration::hours(1),
            None,
        )
        .await
        .unwrap();
}

struct Margin {
    margin_cents: Option<i64>,
    covered_units: Option<i64>,
    volume_total: i64,
    cost_per_unit_cents: Option<i64>,
}

async fn margin_of(env: &PollEnv, order_id: i64) -> Margin {
    use sekondo::{
        orders::{Side, Sort},
        selection::CharacterSelection,
    };
    let account = env.account_of(ALICE).await;
    let selection = CharacterSelection::resolve(&[ALICE, BOB], None);
    let row = env
        .stores
        .orders
        .list_orders(account, selection.ids(), Side::Sell, Sort::default())
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.order_id == order_id)
        .unwrap();
    Margin {
        margin_cents: row.margin_cents,
        covered_units: row.covered_units,
        volume_total: row.volume_total,
        cost_per_unit_cents: row.cost_per_unit_cents,
    }
}

#[tokio::test]
async fn a_fully_covered_order_earns_what_it_sells_for_after_fees_minus_its_cost() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    alice_with_two_lots(&env).await;
    sell(&env, 11, (JITA, false), ("150.00", 15, 15)).await;

    let margin = margin_of(&env, 11).await;

    // 15 * 150 * (1 - 0.03375 - 0.0141) - 1600 = 542.3375
    assert_eq!(margin.margin_cents, Some(54_234));
    assert_eq!(margin.covered_units, Some(15));
    // 1,600.00 over 15 units.
    assert_eq!(margin.cost_per_unit_cents, Some(10_667));
}

#[tokio::test]
async fn a_partly_sold_order_counts_its_remaining_units_at_the_average_cost() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    alice_with_two_lots(&env).await;
    sell(&env, 11, (JITA, false), ("150.00", 5, 15)).await;

    // 5 * 150 * 0.95215 - 5 * 1600 / 15 = 180.7792
    assert_eq!(margin_of(&env, 11).await.margin_cents, Some(18_078));
}

#[tokio::test]
async fn an_order_whose_cost_is_only_partly_known_has_no_margin_and_says_how_much_is() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    buy(&env, (1, ALICE), 1, 5, "100.00").await;
    sell(&env, 11, (JITA, false), ("150.00", 10, 10)).await;

    let margin = margin_of(&env, 11).await;

    assert_eq!(margin.margin_cents, None);
    assert_eq!(margin.cost_per_unit_cents, None);
    assert_eq!((margin.covered_units, margin.volume_total), (Some(5), 10));
}

#[tokio::test]
async fn an_order_in_a_structure_has_no_margin() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    alice_with_two_lots(&env).await;
    sell(&env, 11, (STRUCTURE, false), ("150.00", 15, 15)).await;

    assert_eq!(margin_of(&env, 11).await.margin_cents, None);
}

#[tokio::test]
async fn an_order_of_a_character_whose_skills_were_not_polled_has_no_margin() {
    let env = two_characters().await;
    alice_with_two_lots(&env).await;
    sell(&env, 11, (JITA, false), ("150.00", 15, 15)).await;

    assert_eq!(margin_of(&env, 11).await.margin_cents, None);
}

#[tokio::test]
async fn a_corporation_order_has_no_margin() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    alice_with_two_lots(&env).await;
    sell(&env, 11, (JITA, true), ("150.00", 15, 15)).await;

    let margin = margin_of(&env, 11).await;

    assert_eq!((margin.margin_cents, margin.covered_units), (None, None));
}

#[tokio::test]
async fn a_margin_can_be_negative() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    alice_with_two_lots(&env).await;
    sell(&env, 11, (JITA, false), ("100.00", 15, 15)).await;

    // 15 * 100 * 0.95215 - 1600 = -171.775 (half away from zero)
    assert_eq!(margin_of(&env, 11).await.margin_cents, Some(-17_178));
}

// ---- trades listing and the margin of a sale ----

use time::{OffsetDateTime, macros::datetime};

struct Listed {
    transaction_id: i64,
    is_sale: bool,
    margin_cents: Option<i64>,
    covered_units: Option<i64>,
    cost_per_unit_cents: Option<i64>,
}

/// What the account lists for Alice and Bob within `[from, to]`.
async fn listed_between(
    env: &PollEnv,
    characters: &[i64],
    (from, to): (OffsetDateTime, OffsetDateTime),
) -> Vec<Listed> {
    use sekondo::selection::CharacterSelection;
    let account = env.account_of(ALICE).await;
    let all = [ALICE, BOB];
    let raw = characters
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let selection = CharacterSelection::resolve(&all, Some(&raw));
    env.stores
        .trades
        .list_trades(account, selection.ids(), from, to)
        .await
        .unwrap()
        .into_iter()
        .map(|row| Listed {
            transaction_id: row.transaction_id,
            is_sale: row.is_sale,
            margin_cents: row.margin_cents,
            covered_units: row.covered_units,
            cost_per_unit_cents: row.cost_per_unit_cents,
        })
        .collect()
}

async fn listed(env: &PollEnv) -> Vec<Listed> {
    listed_between(
        env,
        &[ALICE, BOB],
        (
            datetime!(2026-09-01 0:00 UTC),
            datetime!(2026-11-01 0:00 UTC),
        ),
    )
    .await
}

async fn margin_of_sale(env: &PollEnv, id: i64) -> Listed {
    listed(env)
        .await
        .into_iter()
        .find(|row| row.is_sale && row.transaction_id == id)
        .unwrap()
}

#[tokio::test]
async fn a_sale_earns_its_price_after_fees_minus_its_cost() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    sold(&env, (31, ALICE), 2, 10, "150.00").await;
    recompute(&env, ALICE).await;

    let sale = margin_of_sale(&env, 31).await;

    // 10 * 150 * (1 - 0.03375 - 0.0141) - 1000 = 428.225 (half away from zero)
    assert_eq!(sale.margin_cents, Some(42_823));
    assert_eq!(sale.covered_units, Some(10));
    assert_eq!(sale.cost_per_unit_cents, Some(10_000));
}

#[tokio::test]
async fn a_sale_below_its_cost_is_a_loss() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    sold(&env, (31, ALICE), 2, 10, "90.00").await;
    recompute(&env, ALICE).await;

    // 10 * 90 * 0.95215 - 1000 = -143.065
    assert_eq!(margin_of_sale(&env, 31).await.margin_cents, Some(-14_307));
}

#[tokio::test]
async fn a_sale_with_a_partly_known_cost_has_no_margin_and_says_how_much_is_known() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    buy(&env, (1, ALICE), 1, 4, "100.00").await;
    sold(&env, (31, ALICE), 2, 10, "150.00").await;
    recompute(&env, ALICE).await;

    let sale = margin_of_sale(&env, 31).await;

    assert_eq!(sale.margin_cents, None);
    assert_eq!(sale.cost_per_unit_cents, None);
    assert_eq!(sale.covered_units, Some(4));
}

#[tokio::test]
async fn a_sale_in_a_structure_or_without_polled_skills_has_no_margin() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 20, "100.00").await;
    sold(&env, (31, ALICE), 2, 10, "150.00").await;
    recompute(&env, ALICE).await;
    assert_eq!(
        margin_of_sale(&env, 31).await.margin_cents,
        None,
        "skills not polled"
    );

    alice_with_fees(&env).await;
    env.stores
        .trades
        .seed_trade(
            ALICE,
            &TradeSeed {
                transaction_id: 31,
                is_sale: true,
                type_id: 34,
                location_id: STRUCTURE,
                quantity: 10,
                unit_price_cents: 15_000,
                at: day(2),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        margin_of_sale(&env, 31).await.margin_cents,
        None,
        "structure"
    );
}

#[tokio::test]
async fn a_higher_accounting_level_changes_the_margin_of_an_earlier_sale() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    sold(&env, (31, ALICE), 2, 10, "150.00").await;
    recompute(&env, ALICE).await;
    assert_eq!(margin_of_sale(&env, 31).await.margin_cents, Some(42_823));

    // Accounting 4 is 7.5 * (1 - 0.44) = 4.2% instead of 3.375%.
    env.stores.fees.seed_skill(ALICE, 16622, 4).await.unwrap();

    // 1500 * (1 - 0.042 - 0.0141) - 1000 = 415.85
    assert_eq!(margin_of_sale(&env, 31).await.margin_cents, Some(41_585));
}

#[tokio::test]
async fn only_the_selected_personal_trades_in_the_range_are_listed_newest_first() {
    let env = two_characters().await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;
    sold(&env, (31, ALICE), 2, 4, "150.00").await;
    sold(&env, (32, ALICE), 2, 3, "150.00").await;
    buy(&env, (2, ALICE), 20, 5, "100.00").await;
    buy(&env, (3, BOB), 3, 5, "100.00").await;
    let day = |d: u8| datetime!(2026-10-01 0:00 UTC).replace_day(d).unwrap();

    let all = listed_between(&env, &[ALICE, BOB], (day(1), day(25))).await;
    let order: Vec<_> = all.iter().map(|r| (r.is_sale, r.transaction_id)).collect();
    // Same moment: the higher transaction ID first. Buys and sells share the list.
    assert_eq!(
        order,
        [(false, 2), (false, 3), (true, 32), (true, 31), (false, 1)]
    );

    let alice = listed_between(&env, &[ALICE], (day(1), day(25))).await;
    assert_eq!(alice.len(), 4, "Bob's purchase is left out");

    let ranged = listed_between(&env, &[ALICE, BOB], (day(2), day(10))).await;
    let order: Vec<_> = ranged
        .iter()
        .map(|r| (r.is_sale, r.transaction_id))
        .collect();
    assert_eq!(order, [(false, 3), (true, 32), (true, 31)]);
}

#[tokio::test]
async fn a_purchase_has_no_margin() {
    let env = two_characters().await;
    alice_with_fees(&env).await;
    buy(&env, (1, ALICE), 1, 10, "100.00").await;

    let rows = listed(&env).await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].margin_cents, None);
    assert_eq!(rows[0].covered_units, None);
}
