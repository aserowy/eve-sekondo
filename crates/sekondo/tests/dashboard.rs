mod common;

use common::{AppBuilder, Browser, ManualClock, PollEnv, TestApp};
use sekondo::{
    dashboard::{self, CharacterFigures, Totals},
    orders::Side,
    selection::CharacterSelection,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

/// A sell order with `price` ISK per unit and `remain` units left.
fn sell(id: i64, price: f64, remain: i64) -> Value {
    json!({
        "order_id": id, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "is_buy_order": false, "is_corporation": false, "price": price,
        "volume_remain": remain, "volume_total": 100, "min_volume": 1, "range": "station",
        "issued": "2026-10-01T12:00:00Z", "duration": 90
    })
}

/// A buy order holding `escrow` ISK, or none at all.
fn buy(id: i64, escrow: Option<f64>) -> Value {
    let mut order = sell(id, 4.0, 5);
    order["is_buy_order"] = json!(true);
    if let Some(escrow) = escrow {
        order["escrow"] = json!(escrow);
    }
    order
}

async fn set_orders(env: &PollEnv, character: i64, orders: Vec<Value>) {
    let res = env
        .control(
            "orders",
            json!({"character_id": character, "orders": orders}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

/// A new account with these characters linked, in this order, each polled once.
async fn account_with(env: &PollEnv, characters: &[(i64, Vec<Value>)]) -> Uuid {
    let account = env.new_account().await;
    for (id, orders) in characters {
        env.link(account, *id).await;
        set_orders(env, *id, orders.clone()).await;
        env.poller.poll_character(*id).await;
    }
    account
}

async fn figures(env: &PollEnv, account: Uuid) -> Vec<CharacterFigures> {
    let ids: Vec<i64> = env
        .repo
        .list_characters(account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    let selection = CharacterSelection::resolve(&ids, None);
    dashboard::character_figures(&env.pool, account, &selection)
        .await
        .unwrap()
}

#[sqlx::test]
async fn totals_add_up_the_orders_of_all_selected_characters(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[
            (
                ALICE,
                vec![sell(1, 1_000_000.0, 10), buy(2, Some(2_000_000.0))],
            ),
            (BOB, vec![sell(3, 4_000_000.0, 5)]),
        ],
    )
    .await;

    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!(totals.sell_cents, 3_000_000_000);
    assert_eq!(totals.escrow_cents, 200_000_000);
    assert_eq!((totals.sell_count, totals.buy_count), (2, 1));
}

#[sqlx::test]
async fn characters_without_orders_count_as_zero(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(&env, &[(ALICE, vec![])]).await;

    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!(
        (
            totals.sell_cents,
            totals.escrow_cents,
            totals.sell_count,
            totals.buy_count
        ),
        (0, 0, 0, 0)
    );
}

#[sqlx::test]
async fn orders_of_another_account_never_count(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let mine = account_with(&env, &[(ALICE, vec![sell(1, 10.0, 1)])]).await;
    account_with(
        &env,
        &[(BOB, vec![sell(2, 1_000.0, 1), buy(3, Some(500.0))])],
    )
    .await;

    let totals = Totals::of(&figures(&env, mine).await);

    assert_eq!(totals.sell_cents, 1_000);
    assert_eq!((totals.sell_count, totals.buy_count), (1, 0));
    assert_eq!(totals.escrow_cents, 0);
}

#[sqlx::test]
async fn the_breakdown_lists_every_character_in_link_order_with_zeros_for_the_idle_ones(
    pool: PgPool,
) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[
            (BOB, vec![]),
            (ALICE, vec![sell(1, 10.0, 3), buy(2, Some(7.0))]),
        ],
    )
    .await;

    let rows = figures(&env, account).await;

    let ids: Vec<i64> = rows.iter().map(|r| r.freshness.character_id).collect();
    assert_eq!(ids, vec![BOB, ALICE]);
    let bob = &rows[0];
    assert_eq!(
        (
            bob.sell_cents,
            bob.escrow_cents,
            bob.sell_count,
            bob.buy_count
        ),
        (0, 0, 0, 0)
    );
    let alice = &rows[1];
    assert_eq!(
        (
            alice.sell_cents,
            alice.escrow_cents,
            alice.sell_count,
            alice.buy_count
        ),
        (3_000, 700, 1, 1)
    );
}

#[sqlx::test]
async fn a_character_never_polled_still_gets_a_row(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, CAROL).await;

    let rows = figures(&env, account).await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sell_cents, 0);
    assert!(rows[0].freshness.last_success_at.is_none());
}

#[sqlx::test]
async fn the_totals_equal_the_sums_of_the_rows(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[
            (
                ALICE,
                vec![sell(1, 1.11, 3), sell(2, 2.22, 7), buy(3, Some(0.33))],
            ),
            (
                BOB,
                vec![sell(4, 9.99, 1), buy(5, Some(1.01)), buy(6, Some(2.02))],
            ),
        ],
    )
    .await;

    let rows = figures(&env, account).await;
    let totals = Totals::of(&rows);

    assert_eq!(
        totals.sell_cents,
        rows.iter().map(|r| r.sell_cents).sum::<i64>()
    );
    assert_eq!(
        totals.escrow_cents,
        rows.iter().map(|r| r.escrow_cents).sum::<i64>()
    );
    assert_eq!(
        totals.sell_count,
        rows.iter().map(|r| r.sell_count).sum::<i64>()
    );
    assert_eq!(
        totals.buy_count,
        rows.iter().map(|r| r.buy_count).sum::<i64>()
    );
    // 1.11 * 3 + 2.22 * 7 + 9.99 * 1 = 28.86 ISK; 0.33 + 1.01 + 2.02 = 3.36 ISK
    assert_eq!((totals.sell_cents, totals.escrow_cents), (2_886, 336));
}

#[sqlx::test]
async fn a_buy_order_without_escrow_counts_as_zero_escrow_but_still_as_an_order(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(&env, &[(ALICE, vec![buy(1, None), buy(2, Some(5.0))])]).await;

    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!(totals.escrow_cents, 500);
    assert_eq!(totals.buy_count, 2);
}

fn corporation(mut order: Value) -> Value {
    order["is_corporation"] = json!(true);
    order
}

#[sqlx::test]
async fn corporation_sell_orders_count_in_the_total_and_in_their_share(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[(
            ALICE,
            vec![
                sell(1, 70_000_000.0, 10),
                corporation(sell(2, 30_000_000.0, 10)),
            ],
        )],
    )
    .await;

    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!(totals.sell_cents, 100_000_000_000);
    assert_eq!(totals.sell_corp_cents, 30_000_000_000);
    assert_eq!(totals.sell_count, 2);
}

#[sqlx::test]
async fn the_escrow_of_a_corporation_buy_order_is_its_own_share(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[(
            ALICE,
            vec![buy(1, Some(2_000.0)), corporation(buy(2, Some(500.0)))],
        )],
    )
    .await;

    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!(totals.escrow_cents, 250_000);
    assert_eq!(totals.escrow_corp_cents, 50_000);
    assert_eq!(totals.sell_corp_cents, 0);
}

#[sqlx::test]
async fn without_corporation_orders_both_shares_are_zero(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(&env, &[(ALICE, vec![sell(1, 5.0, 2), buy(2, Some(3.0))])]).await;

    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!((totals.sell_corp_cents, totals.escrow_corp_cents), (0, 0));
}

#[sqlx::test]
async fn a_character_needing_relogin_still_contributes_its_last_known_orders(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(&env, &[(ALICE, vec![sell(1, 10.0, 3)])]).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();

    let rows = figures(&env, account).await;

    assert!(rows[0].freshness.needs_relogin);
    assert_eq!(Totals::of(&rows).sell_cents, 3_000);
}

// ---- top items ----

fn of_type(mut order: Value, type_id: i64) -> Value {
    order["type_id"] = json!(type_id);
    order
}

async fn top(env: &PollEnv, account: Uuid, side: Side) -> Vec<(String, i64)> {
    let ids: Vec<i64> = env
        .repo
        .list_characters(account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    let selection = CharacterSelection::resolve(&ids, None);
    dashboard::top_items(&env.pool, account, &selection, side)
        .await
        .unwrap()
        .into_iter()
        .map(|item| (item.label, item.cents))
        .collect()
}

#[sqlx::test]
async fn an_item_listed_by_two_characters_is_summed_into_one_bar(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[
            (ALICE, vec![sell(1, 4_000_000.0, 1)]),
            (BOB, vec![sell(2, 6_000_000.0, 1)]),
        ],
    )
    .await;

    assert_eq!(
        top(&env, account, Side::Sell).await,
        vec![("Tritanium".to_owned(), 1_000_000_000)]
    );
}

#[sqlx::test]
async fn only_the_ten_most_valuable_items_are_listed_highest_first(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    // Twelve items worth 1.00, 2.00 ... 12.00 ISK.
    let orders = (1..=12)
        .map(|n| of_type(sell(n, n as f64, 1), 100 + n))
        .collect();
    let account = account_with(&env, &[(ALICE, orders)]).await;

    let items = top(&env, account, Side::Sell).await;

    assert_eq!(items.len(), 10);
    assert_eq!(items.first(), Some(&("#112".to_owned(), 1_200)));
    assert_eq!(items.last(), Some(&("#103".to_owned(), 300)));
}

#[sqlx::test]
async fn equal_figures_are_ordered_by_item_name(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[(
            ALICE,
            vec![
                of_type(sell(1, 5.0, 1), 34), // Tritanium
                of_type(sell(2, 5.0, 1), 35), // Pyerite
            ],
        )],
    )
    .await;

    let labels: Vec<String> = top(&env, account, Side::Sell)
        .await
        .into_iter()
        .map(|(label, _)| label)
        .collect();

    assert_eq!(labels, vec!["Pyerite", "Tritanium"]);
}

#[sqlx::test]
async fn an_item_without_a_resolved_name_is_labelled_with_its_type_id(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(&env, &[(ALICE, vec![of_type(sell(1, 5.0, 1), 999_999)])]).await;

    assert_eq!(
        top(&env, account, Side::Sell).await,
        vec![("#999999".to_owned(), 500)]
    );
}

#[sqlx::test]
async fn the_escrow_graph_ranks_buy_orders_and_ignores_sell_orders(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[(
            ALICE,
            vec![
                sell(1, 900.0, 1),
                of_type(buy(2, Some(3.0)), 35),
                buy(3, Some(8.0)),
            ],
        )],
    )
    .await;

    assert_eq!(
        top(&env, account, Side::Buy).await,
        vec![("Tritanium".to_owned(), 800), ("Pyerite".to_owned(), 300)]
    );
}

#[sqlx::test]
async fn orders_of_another_account_for_the_same_item_are_not_summed_in(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let mine = account_with(&env, &[(ALICE, vec![sell(1, 10.0, 1)])]).await;
    account_with(&env, &[(BOB, vec![sell(2, 1_000.0, 1)])]).await;

    assert_eq!(
        top(&env, mine, Side::Sell).await,
        vec![("Tritanium".to_owned(), 1_000)]
    );
}

// ---- history ----

use sekondo::{clock::Clock, dashboard::HistoryPoint, range::Range};
use time::{Duration, OffsetDateTime};

async fn history(env: &PollEnv, account: Uuid, range: Range) -> Vec<HistoryPoint> {
    let ids: Vec<i64> = env
        .repo
        .list_characters(account)
        .await
        .unwrap()
        .iter()
        .map(|c| c.character_id)
        .collect();
    let selection = CharacterSelection::resolve(&ids, None);
    dashboard::history(&env.pool, account, &selection, range, env.clock.now())
        .await
        .unwrap()
}

/// The first point at or after `at`: the earliest one that shows a poll made at `at`.
fn shown_from(points: &[HistoryPoint], at: OffsetDateTime) -> &HistoryPoint {
    points
        .iter()
        .find(|p| p.at >= at)
        .unwrap_or_else(|| panic!("no point at or after {at}"))
}

/// The last point before `at`: the latest one that does not yet show a poll made at `at`.
fn shown_before(points: &[HistoryPoint], at: OffsetDateTime) -> &HistoryPoint {
    points
        .iter()
        .rev()
        .find(|p| p.at < at)
        .unwrap_or_else(|| panic!("no point before {at}"))
}

#[sqlx::test]
async fn the_series_has_one_point_per_bucket_ending_at_now(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(&env, &[(ALICE, vec![])]).await;

    let points = history(&env, account, Range::Day).await;

    assert_eq!(points.last().unwrap().at, env.clock.now());
    assert!((48..=49).contains(&points.len()), "{}", points.len());
    assert!(points.windows(2).all(|w| w[0].at < w[1].at));
    assert!(points[0].at > env.clock.now() - Duration::hours(24));
    // Every point but the last ends on a bucket boundary.
    let half_hour = 30 * 60;
    assert!(
        points[..points.len() - 1]
            .iter()
            .all(|p| p.at.unix_timestamp() % half_hour == 0)
    );
}

#[sqlx::test]
async fn the_value_develops_with_the_polls(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    set_orders(&env, ALICE, vec![sell(1, 1_000_000.0, 10)]).await;
    env.clock.advance(Duration::hours(1));
    let first_poll = env.clock.now();
    env.poller.poll_character(ALICE).await;
    set_orders(&env, ALICE, vec![sell(1, 1_500_000.0, 10)]).await;
    env.clock.advance(Duration::hours(1));
    let second_poll = env.clock.now();
    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::minutes(10));

    let points = history(&env, account, Range::Day).await;

    assert_eq!(shown_before(&points, first_poll).sell_cents, 0);
    assert_eq!(shown_from(&points, first_poll).sell_cents, 1_000_000_000);
    assert_eq!(shown_before(&points, second_poll).sell_cents, 1_000_000_000);
    assert_eq!(shown_from(&points, second_poll).sell_cents, 1_500_000_000);
    assert_eq!(points.last().unwrap().sell_cents, 1_500_000_000);
}

#[sqlx::test]
async fn the_figures_drop_to_zero_when_all_orders_close(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    set_orders(&env, ALICE, vec![sell(1, 10.0, 5), buy(2, Some(7.0))]).await;
    env.clock.advance(Duration::hours(1));
    env.poller.poll_character(ALICE).await;
    set_orders(&env, ALICE, vec![]).await;
    env.clock.advance(Duration::hours(1));
    let closed = env.clock.now();
    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::hours(1));

    let points = history(&env, account, Range::Day).await;

    let before = shown_before(&points, closed);
    assert_eq!(
        (
            before.sell_cents,
            before.escrow_cents,
            before.sell_count,
            before.buy_count
        ),
        (5_000, 700, 1, 1)
    );
    let after = points.last().unwrap();
    assert_eq!(
        (
            after.sell_cents,
            after.escrow_cents,
            after.sell_count,
            after.buy_count
        ),
        (0, 0, 0, 0)
    );
    assert!(
        after.contributors > 0,
        "an empty poll is history, not a gap"
    );
}

#[sqlx::test]
async fn characters_polled_at_different_times_both_contribute_their_latest_figures(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env.link(account, BOB).await;
    set_orders(&env, ALICE, vec![sell(1, 10.0, 1)]).await;
    set_orders(&env, BOB, vec![sell(2, 5.0, 1)]).await;
    env.clock.advance(Duration::hours(3));
    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::hours(1));
    let bob_polled = env.clock.now();
    env.poller.poll_character(BOB).await;
    env.clock.advance(Duration::minutes(20));
    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::hours(1));

    let points = history(&env, account, Range::Day).await;

    assert_eq!(shown_before(&points, bob_polled).sell_cents, 1_000);
    let after_both: Vec<i64> = points
        .iter()
        .filter(|p| p.at >= bob_polled)
        .map(|p| p.sell_cents)
        .collect();
    assert!(!after_both.is_empty());
    assert!(
        after_both.iter().all(|cents| *cents == 1_500),
        "{after_both:?}"
    );
}

#[sqlx::test]
async fn a_stale_character_is_carried_forward_to_now(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    set_orders(&env, ALICE, vec![sell(1, 5_000_000.0, 1)]).await;
    env.clock.advance(Duration::hours(14));
    env.poller.poll_character(ALICE).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    env.clock.advance(Duration::hours(10));

    let points = history(&env, account, Range::Day).await;

    assert_eq!(points.last().unwrap().sell_cents, 500_000_000);
    assert_eq!(
        shown_before(&points, env.clock.now() - Duration::hours(1)).sell_cents,
        500_000_000
    );
}

#[sqlx::test]
async fn without_any_poll_there_is_no_history(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;

    let points = history(&env, account, Range::Week).await;

    assert!(!dashboard::has_history(&points));
    assert!(
        points
            .iter()
            .all(|p| p.sell_cents == 0 && p.contributors == 0)
    );
}

#[sqlx::test]
async fn a_poll_before_the_start_of_the_range_is_carried_in(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    set_orders(&env, ALICE, vec![sell(1, 10.0, 3)]).await;
    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::days(3));

    let points = history(&env, account, Range::Day).await;

    assert!(dashboard::has_history(&points));
    assert!(points.iter().all(|p| p.sell_cents == 3_000), "{points:?}");
}

#[sqlx::test]
async fn the_last_point_equals_the_current_totals(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = account_with(
        &env,
        &[
            (
                ALICE,
                vec![sell(1, 1.11, 3), corporation(buy(2, Some(4.0)))],
            ),
            (
                BOB,
                vec![sell(3, 9.99, 1), buy(4, Some(1.01)), buy(5, Some(2.02))],
            ),
        ],
    )
    .await;
    env.clock.advance(Duration::minutes(5));

    let last = history(&env, account, Range::Week).await.pop().unwrap();
    let totals = Totals::of(&figures(&env, account).await);

    assert_eq!(
        (
            last.sell_cents,
            last.escrow_cents,
            last.sell_count,
            last.buy_count
        ),
        (
            totals.sell_cents,
            totals.escrow_cents,
            totals.sell_count,
            totals.buy_count
        )
    );
}

#[sqlx::test]
async fn another_accounts_history_never_counts(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let mine = account_with(&env, &[(ALICE, vec![sell(1, 10.0, 1)])]).await;
    account_with(&env, &[(BOB, vec![sell(2, 1_000.0, 1)])]).await;

    let last = history(&env, mine, Range::Day).await.pop().unwrap();

    assert_eq!(last.sell_cents, 1_000);
}

// ---- the page ----

async fn app_at(pool: PgPool) -> (TestApp, std::sync::Arc<ManualClock>) {
    let clock = ManualClock::new();
    let app = AppBuilder::new(pool)
        .with_stub()
        .clock(clock.clone())
        .spawn()
        .await;
    (app, clock)
}

/// A browser signed in as `first`, with `others` linked after it, in this order.
async fn signed_in_with(app: &TestApp, first: i64, others: &[i64]) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", first).await;
    for id in others {
        browser.sso_round_trip(app, "/auth/add", *id).await;
    }
    browser
}

async fn stub_orders(app: &TestApp, character: i64, orders: Vec<Value>) {
    let res = app
        .stub_control(
            "orders",
            json!({"character_id": character, "orders": orders}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

async fn page(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The part of `html` from the first `from` up to the next `to`.
fn between<'a>(html: &'a str, from: &str, to: &str) -> &'a str {
    let start = html
        .find(from)
        .unwrap_or_else(|| panic!("no {from} in {html}"));
    let rest = &html[start..];
    &rest[..rest.find(to).unwrap_or(rest.len())]
}

fn tile<'a>(html: &'a str, id: &str) -> &'a str {
    between(html, &format!(r#"id="{id}""#), "</article>")
}

fn breakdown_row(html: &str, character: i64) -> &str {
    between(html, &format!(r#"id="character-{character}""#), "</tr>")
}

#[sqlx::test]
async fn the_dashboard_shows_the_totals_of_all_characters_shortened_with_the_exact_value_on_hover(
    pool: PgPool,
) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 1_000_000.0, 10), buy(2, Some(2_000_000.0))],
    )
    .await;
    stub_orders(&app, BOB, vec![sell(3, 4_000_000.0, 5)]).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;

    let html = page(&app, &browser, "/dashboard").await;

    let sell_tile = tile(&html, "tile-sell-value");
    assert!(sell_tile.contains(">30m<"), "{sell_tile}");
    assert!(
        sell_tile.contains(r#"title="30,000,000.00 ISK""#),
        "{sell_tile}"
    );
    let escrow_tile = tile(&html, "tile-escrow");
    assert!(
        escrow_tile.contains(">2m<") && escrow_tile.contains("2,000,000.00 ISK"),
        "{escrow_tile}"
    );
    assert!(tile(&html, "tile-sell-count").contains(">2<"));
    assert!(tile(&html, "tile-buy-count").contains(">1<"));
    assert!(!html.to_lowercase().contains("coming"));
}

#[sqlx::test]
async fn a_large_total_is_shortened_to_four_significant_digits(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(&app, ALICE, vec![sell(1, 1_234_567_890.12, 1)]).await;
    app.state.poller.poll_character(ALICE).await;

    let html = page(&app, &browser, "/dashboard").await;

    let sell_tile = tile(&html, "tile-sell-value");
    assert!(sell_tile.contains(">1.235b<"), "{sell_tile}");
    assert!(
        sell_tile.contains(r#"title="1,234,567,890.12 ISK""#),
        "{sell_tile}"
    );
}

#[sqlx::test]
async fn an_account_without_orders_shows_zeros(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(&app, ALICE, vec![]).await;
    app.state.poller.poll_character(ALICE).await;

    let html = page(&app, &browser, "/dashboard").await;

    assert!(tile(&html, "tile-sell-value").contains(">0.00<"));
    assert!(tile(&html, "tile-escrow").contains(">0.00<"));
    assert!(tile(&html, "tile-sell-count").contains(">0<"));
    assert!(tile(&html, "tile-buy-count").contains(">0<"));
}

#[sqlx::test]
async fn the_corporation_share_is_shown_only_when_there_is_one(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(
        &app,
        ALICE,
        vec![
            sell(1, 70_000_000.0, 10),
            corporation(sell(2, 30_000_000.0, 10)),
        ],
    )
    .await;
    app.state.poller.poll_character(ALICE).await;

    let html = page(&app, &browser, "/dashboard").await;

    let sell_tile = tile(&html, "tile-sell-value");
    assert!(sell_tile.contains(">1b<"), "{sell_tile}");
    assert!(sell_tile.contains("of which"), "{sell_tile}");
    assert!(
        sell_tile.contains("300m") && sell_tile.contains("300,000,000.00 ISK"),
        "{sell_tile}"
    );
    assert!(!tile(&html, "tile-escrow").contains("of which"));

    stub_orders(&app, ALICE, vec![sell(1, 5.0, 1), buy(2, Some(3.0))]).await;
    app.state.poller.poll_character(ALICE).await;
    let html = page(&app, &browser, "/dashboard").await;
    assert!(
        !html.contains("of which"),
        "no corporation orders, no share"
    );
}

#[sqlx::test]
async fn the_breakdown_has_a_row_per_character_in_link_order_with_its_figures(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 1_000_000.0, 10), buy(2, Some(2_000_000.0))],
    )
    .await;
    stub_orders(&app, BOB, vec![]).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;

    let html = page(&app, &browser, "/dashboard").await;

    let alice = breakdown_row(&html, ALICE);
    assert!(
        alice.contains("Alice Alpha") && alice.contains(">10m<") && alice.contains(">2m<"),
        "{alice}"
    );
    let bob = breakdown_row(&html, BOB);
    assert!(
        bob.contains("Bob Bravo")
            && bob.matches(">0.00<").count() == 2
            && bob.matches(">0<").count() == 2,
        "{bob}"
    );
    assert!(html.find(r#"id="character-90000001""#) < html.find(r#"id="character-90000002""#));
}

#[sqlx::test]
async fn a_freshly_polled_character_shows_its_times_in_eve_time_and_no_badge(pool: PgPool) {
    let (app, clock) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    app.state.poller.poll_character(ALICE).await;
    let polled = clock.now();
    clock.advance(Duration::minutes(5));

    let html = page(&app, &browser, "/dashboard").await;

    let row = breakdown_row(&html, ALICE);
    assert!(row.contains(&sekondo::format::eve_time(polled)), "{row}");
    assert!(row.contains("Next update:"), "{row}");
    assert!(
        !row.contains("Stale") && !row.contains("Log in again") && !row.contains("Waiting"),
        "{row}"
    );
}

#[sqlx::test]
async fn a_character_that_needs_relogin_is_stale_links_to_login_and_still_counts(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(&app, ALICE, vec![sell(1, 10.0, 3)]).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();

    let html = page(&app, &browser, "/dashboard").await;

    let row = breakdown_row(&html, ALICE);
    assert!(
        row.contains("Stale")
            && row.contains(r#"href="/auth/add""#)
            && row.contains("Log in again"),
        "{row}"
    );
    assert!(
        row.contains(">30.00<"),
        "its last known orders still count: {row}"
    );
    assert!(tile(&html, "tile-sell-value").contains(">30.00<"));
}

#[sqlx::test]
async fn a_character_whose_last_poll_is_older_than_twice_the_interval_is_stale(pool: PgPool) {
    let (app, clock) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    app.state.poller.poll_character(ALICE).await;

    clock.advance(Duration::minutes(39));
    let html = page(&app, &browser, "/dashboard").await;
    assert!(!breakdown_row(&html, ALICE).contains("Stale"));

    clock.advance(Duration::minutes(6));
    let html = page(&app, &browser, "/dashboard").await;
    let row = breakdown_row(&html, ALICE);
    assert!(
        row.contains("Stale") && !row.contains("Log in again"),
        "{row}"
    );
}

#[sqlx::test]
async fn a_newly_linked_character_is_waiting_for_its_first_update(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;

    let html = page(&app, &browser, "/dashboard").await;

    let row = breakdown_row(&html, ALICE);
    assert!(row.contains("Waiting for first update"), "{row}");
}

#[sqlx::test]
async fn the_summary_fragment_is_the_tiles_and_breakdown_without_the_page_around_them(
    pool: PgPool,
) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    app.state.poller.poll_character(ALICE).await;

    let fragment = page(&app, &browser, "/dashboard/summary").await;
    let full = page(&app, &browser, "/dashboard").await;

    assert!(!fragment.contains("<html") && !fragment.contains("<nav"));
    assert!(fragment.contains(r#"id="tile-sell-value""#) && fragment.contains(r#"id="breakdown""#));
    assert!(
        full.contains(r#"hx-get="/dashboard/summary""#),
        "the page refreshes through the fragment"
    );
    assert!(
        full.contains(fragment.trim()),
        "the page includes the fragment"
    );
}

#[sqlx::test]
async fn unauthenticated_requests_for_the_dashboard_go_to_the_landing_page(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    for path in ["/dashboard", "/dashboard/summary"] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

// ---- graphs ----

/// The raw attribute value and the parsed JSON of the canvas `data-chart="kind"`.
fn chart_series(html: &str, kind: &str) -> (String, Value) {
    let marker = format!(r#"data-chart="{kind}" data-series=""#);
    let start = html
        .find(&marker)
        .unwrap_or_else(|| panic!("no {kind} chart in {html}"))
        + marker.len();
    let raw = html[start..][..html[start..].find('"').unwrap()].to_owned();
    let unescaped = raw
        .replace("&#34;", "\"")
        .replace("&#60;", "<")
        .replace("&#62;", ">")
        .replace("&#39;", "'")
        .replace("&#38;", "&");
    let parsed = serde_json::from_str(&unescaped).unwrap_or_else(|e| panic!("{e}: {unescaped}"));
    (raw, parsed)
}

#[sqlx::test]
async fn the_split_graph_holds_the_breakdown_figures_and_escapes_names(pool: PgPool) {
    let (app, _) = app_at(pool.clone()).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    stub_orders(&app, ALICE, vec![sell(1, 1_000_000.0, 30)]).await;
    stub_orders(&app, BOB, vec![buy(2, Some(2_000_000.0))]).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;
    sqlx::query("UPDATE characters SET name = $1 WHERE character_id = $2")
        .bind(r#"Eve "<b>" & Co"#)
        .bind(BOB)
        .execute(&pool)
        .await
        .unwrap();

    let html = page(&app, &browser, "/dashboard/summary").await;
    let (raw, series) = chart_series(&html, "split");

    assert!(
        !raw.contains(['<', '>']) && !raw.contains("\"") && !raw.contains("& "),
        "{raw}"
    );
    assert_eq!(
        series["labels"],
        json!(["Alice Alpha", r#"Eve "<b>" & Co"#])
    );
    assert_eq!(series["series"][0]["name"], "Sell value");
    assert_eq!(series["series"][0]["cents"], json!([3_000_000_000_i64, 0]));
    assert_eq!(series["series"][0]["exact"][0], "30,000,000.00 ISK");
    assert_eq!(series["series"][1]["name"], "Buy escrow");
    assert_eq!(series["series"][1]["cents"], json!([0, 200_000_000]));
    assert_eq!(series["series"][1]["exact"][1], "2,000,000.00 ISK");
}

#[sqlx::test]
async fn the_top_item_graphs_hold_the_ranked_items_with_exact_values(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    stub_orders(
        &app,
        ALICE,
        vec![
            of_type(sell(1, 4_000_000.0, 1), 34),
            of_type(sell(2, 9_000_000.0, 1), 35),
            of_type(buy(3, Some(300.0)), 36),
        ],
    )
    .await;
    stub_orders(&app, BOB, vec![of_type(sell(4, 6_000_000.0, 1), 34)]).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;

    let html = page(&app, &browser, "/dashboard/summary").await;

    let (_, sell_top) = chart_series(&html, "top-sell");
    assert_eq!(sell_top["labels"], json!(["Tritanium", "Pyerite"]));
    assert_eq!(sell_top["series"][0]["name"], "Sell value");
    assert_eq!(
        sell_top["series"][0]["cents"],
        json!([1_000_000_000_i64, 900_000_000_i64])
    );
    assert_eq!(sell_top["series"][0]["exact"][0], "10,000,000.00 ISK");
    let (_, buy_top) = chart_series(&html, "top-buy");
    assert_eq!(buy_top["labels"], json!(["Mexallon"]));
    assert_eq!(buy_top["series"][0]["name"], "Buy escrow");
    assert_eq!(buy_top["series"][0]["cents"], json!([30_000]));
}

#[sqlx::test]
async fn a_top_item_graph_without_orders_says_so_instead_of_drawing(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(&app, ALICE, vec![sell(1, 5.0, 1)]).await;
    app.state.poller.poll_character(ALICE).await;

    let html = page(&app, &browser, "/dashboard/summary").await;

    assert!(html.contains(r#"data-chart="top-sell""#));
    assert!(!html.contains(r#"data-chart="top-buy""#));
    assert!(html.contains("No buy orders."), "{html}");
    assert!(!html.contains("No sell orders."));

    stub_orders(&app, ALICE, vec![buy(2, Some(5.0))]).await;
    app.state.poller.poll_character(ALICE).await;
    let html = page(&app, &browser, "/dashboard/summary").await;
    assert!(html.contains("No sell orders.") && !html.contains(r#"data-chart="top-sell""#));
}

#[sqlx::test]
async fn the_history_graphs_hold_bucket_end_times_and_amounts_in_cents(pool: PgPool) {
    let (app, clock) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 1_000_000.0, 10), buy(2, Some(2_000_000.0))],
    )
    .await;
    clock.advance(Duration::hours(3));
    app.state.poller.poll_character(ALICE).await;
    clock.advance(Duration::minutes(10));

    let html = page(&app, &browser, "/dashboard/summary").await;

    let (_, value) = chart_series(&html, "value-history");
    let times: Vec<i64> = value["times"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_i64().unwrap())
        .collect();
    // Seven days in two-hour buckets, the last one ending now.
    assert_eq!(*times.last().unwrap(), clock.now().unix_timestamp() * 1000);
    assert!((84..=85).contains(&times.len()), "{}", times.len());
    let two_hours_ms = 2 * 3600 * 1000;
    assert!(
        times[..times.len() - 1]
            .iter()
            .all(|t| t % two_hours_ms == 0)
    );
    assert!(times.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(value["labels"].as_array().unwrap().len(), times.len());
    assert_eq!(value["series"][0]["name"], "Sell value");
    assert_eq!(value["series"][1]["name"], "Buy escrow");
    let sell_values = value["series"][0]["values"].as_array().unwrap();
    assert_eq!(sell_values.len(), times.len());
    assert_eq!(sell_values.first().unwrap(), 0);
    assert_eq!(sell_values.last().unwrap(), 1_000_000_000_i64);
    assert_eq!(
        value["series"][0]["exact"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "10,000,000.00 ISK"
    );
    assert_eq!(
        value["series"][1]["values"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        200_000_000
    );

    let (_, count) = chart_series(&html, "count-history");
    assert_eq!(count["series"][0]["name"], "Sell orders");
    assert_eq!(
        count["series"][0]["values"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        1
    );
    assert_eq!(count["series"][1]["name"], "Buy orders");
    assert_eq!(
        count["series"][1]["exact"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "1"
    );
}

#[sqlx::test]
async fn the_history_graphs_say_when_there_is_no_history_yet(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;

    let html = page(&app, &browser, "/dashboard/summary").await;

    assert!(html.contains("No history yet."), "{html}");
    assert!(!html.contains(r#"data-chart="value-history""#));
    assert!(!html.contains(r#"data-chart="count-history""#));
}

#[sqlx::test]
async fn the_history_graphs_describe_their_latest_values_for_screen_readers(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 1_000_000.0, 10), buy(2, Some(2_000_000.0))],
    )
    .await;
    app.state.poller.poll_character(ALICE).await;

    let html = page(&app, &browser, "/dashboard/summary").await;

    let label = |kind: &str| {
        let from = html.find(&format!(r#"data-chart="{kind}""#)).unwrap();
        let rest = &html[from..];
        let start = rest.find("aria-label=\"").unwrap() + "aria-label=\"".len();
        rest[start..][..rest[start..].find('"').unwrap()].to_owned()
    };
    let value = label("value-history");
    assert!(
        value.contains("10,000,000.00 ISK") && value.contains("2,000,000.00 ISK"),
        "{value}"
    );
    let count = label("count-history");
    assert!(
        count.contains("1 sell order") && count.contains("1 buy order"),
        "{count}"
    );
}

// ---- range ----

/// The range control's links: label, href, whether it is marked as the current one.
fn range_links(html: &str) -> Vec<(String, String, bool)> {
    let control = between(html, r#"id="range-control""#, "</div>");
    control
        .split("<a ")
        .skip(1)
        .map(|tag| {
            let (attrs, rest) = tag.split_once('>').unwrap();
            let href = between(attrs, "href=\"", "\" ")
                .trim_start_matches("href=\"")
                .replace("&#38;", "&");
            let label = rest.split('<').next().unwrap().trim().to_owned();
            (label, href, attrs.contains(r#"aria-current="true""#))
        })
        .collect()
}

fn history_times(html: &str) -> Vec<i64> {
    let (_, series) = chart_series(html, "value-history");
    series["times"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_i64().unwrap())
        .collect()
}

/// An app whose only character was polled an hour ago.
async fn polled_app(pool: PgPool, others: &[i64]) -> (TestApp, Browser) {
    let (app, clock) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, others).await;
    stub_orders(&app, ALICE, vec![sell(1, 10.0, 5), buy(2, Some(7.0))]).await;
    app.state.poller.poll_character(ALICE).await;
    clock.advance(Duration::hours(1));
    (app, browser)
}

#[sqlx::test]
async fn without_a_range_the_control_marks_seven_days_and_the_graphs_cover_them(pool: PgPool) {
    let (app, browser) = polled_app(pool, &[]).await;

    let html = page(&app, &browser, "/dashboard").await;

    let links = range_links(&html);
    let labels: Vec<&str> = links.iter().map(|(label, ..)| label.as_str()).collect();
    assert_eq!(labels, vec!["24 hours", "7 days", "30 days", "90 days"]);
    let current: Vec<&str> = links.iter().filter(|l| l.2).map(|l| l.0.as_str()).collect();
    assert_eq!(current, vec!["7 days"]);
    let times = history_times(&html);
    assert!((84..=85).contains(&times.len()), "{}", times.len());
}

#[sqlx::test]
async fn an_unknown_range_behaves_like_seven_days(pool: PgPool) {
    let (app, browser) = polled_app(pool, &[]).await;

    let html = page(&app, &browser, "/dashboard?range=1y").await;

    let current: Vec<String> = range_links(&html)
        .into_iter()
        .filter(|l| l.2)
        .map(|l| l.0)
        .collect();
    assert_eq!(current, vec!["7 days"]);
    assert!((84..=85).contains(&history_times(&html).len()));
}

#[sqlx::test]
async fn each_range_marks_itself_and_sets_the_bucket_width(pool: PgPool) {
    let (app, browser) = polled_app(pool, &[]).await;

    for (range, label, buckets) in [
        ("24h", "24 hours", 48..=49),
        ("30d", "30 days", 120..=121),
        ("90d", "90 days", 90..=91),
    ] {
        let html = page(&app, &browser, &format!("/dashboard?range={range}")).await;
        let current: Vec<String> = range_links(&html)
            .into_iter()
            .filter(|l| l.2)
            .map(|l| l.0)
            .collect();
        assert_eq!(current, vec![label], "{range}");
        let times = history_times(&html);
        assert!(buckets.contains(&times.len()), "{range}: {}", times.len());
    }
}

#[sqlx::test]
async fn the_range_links_keep_the_selection_and_omit_the_default_range(pool: PgPool) {
    let (app, browser) = polled_app(pool, &[BOB]).await;

    let html = page(
        &app,
        &browser,
        &format!("/dashboard/summary?chars={ALICE}&range=30d"),
    )
    .await;

    let hrefs: Vec<String> = range_links(&html).into_iter().map(|l| l.1).collect();
    assert_eq!(
        hrefs,
        vec![
            format!("/dashboard?chars={ALICE}&range=24h"),
            format!("/dashboard?chars={ALICE}"),
            format!("/dashboard?chars={ALICE}&range=30d"),
            format!("/dashboard?chars={ALICE}&range=90d"),
        ]
    );

    // The page refreshes itself for the same view.
    let full = page(
        &app,
        &browser,
        &format!("/dashboard?chars={ALICE}&range=30d"),
    )
    .await;
    assert!(
        full.contains(&format!(
            r#"hx-get="/dashboard/summary?chars={ALICE}&#38;range=30d""#
        )),
        "{full}"
    );
}

#[sqlx::test]
async fn the_range_changes_only_the_over_time_graphs(pool: PgPool) {
    let (app, browser) = polled_app(pool, &[]).await;

    let week = page(&app, &browser, "/dashboard/summary").await;
    let quarter = page(&app, &browser, "/dashboard/summary?range=90d").await;

    for id in ["tiles", "breakdown"] {
        let part = |html: &str| between(html, &format!(r#"id="{id}""#), "</section>").to_owned();
        assert_eq!(part(&week), part(&quarter), "{id}");
    }
    for kind in ["split", "top-sell", "top-buy"] {
        assert_eq!(
            chart_series(&week, kind),
            chart_series(&quarter, kind),
            "{kind}"
        );
    }
    assert_ne!(history_times(&week), history_times(&quarter));
}

// ---- the character filter ----

async fn two_polled_characters(pool: PgPool) -> (TestApp, Browser) {
    let (app, clock) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 1_000_000.0, 10), buy(2, Some(2_000_000.0))],
    )
    .await;
    stub_orders(&app, BOB, vec![of_type(sell(3, 4_000_000.0, 5), 35)]).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;
    clock.advance(Duration::minutes(5));
    (app, browser)
}

#[sqlx::test]
async fn a_selection_restricts_every_figure_and_graph_to_those_characters(pool: PgPool) {
    let (app, browser) = two_polled_characters(pool).await;

    let html = page(&app, &browser, &format!("/dashboard?chars={ALICE}")).await;

    assert!(
        tile(&html, "tile-sell-value").contains(">10m<"),
        "only Alice's 10m"
    );
    assert!(tile(&html, "tile-sell-count").contains(">1<"));
    assert!(
        html.contains(r#"id="character-90000001""#) && !html.contains(r#"id="character-90000002""#)
    );
    let (_, split) = chart_series(&html, "split");
    assert_eq!(split["labels"], json!(["Alice Alpha"]));
    let (_, top) = chart_series(&html, "top-sell");
    assert_eq!(top["labels"], json!(["Tritanium"]));
    let (_, value) = chart_series(&html, "value-history");
    assert_eq!(
        value["series"][0]["values"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        1_000_000_000_i64
    );
}

#[sqlx::test]
async fn a_foreign_character_id_falls_back_to_all_characters(pool: PgPool) {
    let (app, browser) = two_polled_characters(pool).await;

    let html = page(&app, &browser, &format!("/dashboard?chars={CAROL}")).await;

    assert!(tile(&html, "tile-sell-value").contains(">30m<"));
    assert!(
        html.contains(r#"id="character-90000001""#) && html.contains(r#"id="character-90000002""#)
    );
}

#[sqlx::test]
async fn the_filter_lists_every_character_with_the_selected_ones_checked_and_keeps_the_range(
    pool: PgPool,
) {
    let (app, browser) = two_polled_characters(pool).await;

    let html = page(
        &app,
        &browser,
        &format!("/dashboard?chars={ALICE}&range=30d"),
    )
    .await;

    let form = between(&html, r#"id="character-filter""#, "</form>");
    assert!(
        form.contains(r#"hx-get="/dashboard""#) && form.contains("data-character-filter"),
        "{form}"
    );
    assert!(
        form.contains(r##"hx-target="#dashboard-view""##)
            && form.contains(r##"hx-select-oob="#main-nav""##),
        "{form}"
    );
    let alice = between(form, "Alice Alpha", "</label>");
    let alice_box = between(form, &format!(r#"value="{ALICE}""#), ">");
    assert!(alice_box.contains("checked"), "{alice_box} {alice}");
    let bob_box = between(form, &format!(r#"value="{BOB}""#), ">");
    assert!(!bob_box.contains("checked"), "{bob_box}");
    assert!(
        form.contains(r#"<input type="hidden" name="range" value="30d">"#),
        "{form}"
    );
    assert!(
        !form.contains(r#"name="sort""#),
        "the dashboard has no sorting"
    );
}

#[sqlx::test]
async fn the_navigation_carries_the_selection_to_the_orders_page(pool: PgPool) {
    let (app, browser) = two_polled_characters(pool).await;

    let html = page(
        &app,
        &browser,
        &format!("/dashboard?chars={ALICE}&range=30d"),
    )
    .await;

    assert!(
        html.contains(&format!(r#"href="/orders?chars={ALICE}""#)),
        "{html}"
    );
}

// ---- the wealth tile ----

use sekondo::wealth_poller::Kind;

async fn stub_wallet(app: &TestApp, character: i64, balance: f64) {
    let res = app
        .stub_control(
            "wallet",
            json!({"character_id": character, "balance": balance}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

#[sqlx::test]
async fn the_wealth_tile_adds_up_everything_of_all_selected_characters(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    // Alice: 200m wallet. Bob: 50m wallet, nothing else.
    stub_orders(&app, ALICE, vec![]).await;
    stub_orders(&app, BOB, vec![]).await;
    stub_wallet(&app, ALICE, 200_000_000.0).await;
    stub_wallet(&app, BOB, 50_000_000.0).await;
    for id in [ALICE, BOB] {
        app.state.poller.poll_character(id).await;
        app.state.wealth.poll(id, Kind::Wallet).await;
    }

    let html = page(&app, &browser, "/dashboard").await;

    let wealth = tile(&html, "tile-wealth");
    assert!(wealth.contains(">250m<"), "{wealth}");
    assert!(wealth.contains(r#"title="250,000,000.00 ISK""#), "{wealth}");
    // A selection narrows it.
    let alice = page(&app, &browser, &format!("/dashboard?chars={ALICE}")).await;
    assert!(tile(&alice, "tile-wealth").contains(">200m<"));
}

#[sqlx::test]
async fn the_wealth_tile_counts_wallet_orders_escrow_and_inventory_but_not_corporation_orders(
    pool: PgPool,
) {
    let (app, _) = app_at(pool.clone()).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    let mut corp = sell(3, 40_000_000.0, 1);
    corp["is_corporation"] = json!(true);
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 3_000_000.0, 10), buy(2, Some(20_000_000.0)), corp],
    )
    .await;
    stub_wallet(&app, ALICE, 100_000_000.0).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.wealth.poll(ALICE, Kind::Wallet).await;
    sqlx::query(
        "INSERT INTO inventory_values (character_id, recorded_at, value) VALUES ($1, now() - interval '1 minute', 50000000)",
    )
    .bind(ALICE)
    .execute(&pool)
    .await
    .unwrap();

    let html = page(&app, &browser, "/dashboard").await;

    assert!(
        tile(&html, "tile-wealth").contains(">200m<"),
        "{}",
        tile(&html, "tile-wealth")
    );
}

#[sqlx::test]
async fn the_wealth_tile_links_to_the_wealth_page_with_the_selection_and_never_the_range(
    pool: PgPool,
) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;

    let narrowed = page(
        &app,
        &browser,
        &format!("/dashboard?chars={ALICE}&range=30d"),
    )
    .await;
    let link = tile(&narrowed, "tile-wealth");
    assert!(link.contains(r#"href="/wealth?chars=90000001""#), "{link}");
    assert!(!link.contains("range"));

    let all = page(&app, &browser, "/dashboard").await;
    assert!(tile(&all, "tile-wealth").contains(r#"href="/wealth""#));
}

#[sqlx::test]
async fn a_character_without_any_figure_makes_a_wealth_of_zero(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;

    let html = page(&app, &browser, "/dashboard").await;

    assert!(tile(&html, "tile-wealth").contains(">0.00<"));
}

// ---- the wealth line ----

use sekondo::wealth;

#[sqlx::test]
async fn the_wealth_line_is_the_top_edge_of_the_wealth_stack_and_ends_at_the_tile(pool: PgPool) {
    let (app, clock) = app_at(pool.clone()).await;
    let browser = signed_in_with(&app, ALICE, &[BOB]).await;
    stub_orders(
        &app,
        ALICE,
        vec![sell(1, 1_000_000.0, 3), buy(2, Some(2_000_000.0))],
    )
    .await;
    stub_orders(&app, BOB, vec![]).await;
    stub_wallet(&app, ALICE, 10_000_000.0).await;
    stub_wallet(&app, BOB, 5_000_000.0).await;
    for id in [ALICE, BOB] {
        clock.advance(Duration::hours(1));
        app.state.poller.poll_character(id).await;
        app.state.wealth.poll(id, Kind::Wallet).await;
    }
    sqlx::query(
        "INSERT INTO inventory_values (character_id, recorded_at, value) VALUES ($1, $2, 7000000)",
    )
    .bind(ALICE)
    .bind(clock.now() - Duration::minutes(30))
    .execute(&pool)
    .await
    .unwrap();
    clock.advance(Duration::minutes(10));

    let html = page(&app, &browser, "/dashboard?range=24h").await;

    let (_, line) = chart_series(&html, "wealth-history");
    assert_eq!(line["series"].as_array().unwrap().len(), 1);
    assert_eq!(line["series"][0]["name"], "Wealth");
    let values: Vec<i64> = line["series"][0]["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    // The same query that draws the wealth page's stack.
    let ids = [ALICE, BOB];
    let selection = CharacterSelection::resolve(&ids, None);
    let account = app
        .state
        .repo
        .find_character(ALICE)
        .await
        .unwrap()
        .unwrap()
        .account_id;
    let stack = wealth::history(&pool, account, &selection, Range::Day, clock.now())
        .await
        .unwrap();
    let top: Vec<i64> = stack.iter().map(|p| p.total_cents()).collect();
    assert_eq!(values, top);
    // Ends at the tile: 10m + 5m wallets, 3m sell, 2m escrow, 7m inventory.
    assert_eq!(*values.last().unwrap(), 2_700_000_000);
    assert!(tile(&html, "tile-wealth").contains(">27m<"));
    assert_eq!(
        line["totals"],
        Value::Null,
        "a single line has no stack total"
    );
}

#[sqlx::test]
async fn the_wealth_line_steps_with_the_wallet_polls(pool: PgPool) {
    let (app, clock) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;
    stub_orders(&app, ALICE, vec![]).await;
    stub_wallet(&app, ALICE, 10_000_000.0).await;
    clock.advance(Duration::hours(1));
    let first = clock.now();
    app.state.wealth.poll(ALICE, Kind::Wallet).await;
    stub_wallet(&app, ALICE, 15_000_000.0).await;
    clock.advance(Duration::hours(1));
    let second = clock.now();
    app.state.wealth.poll(ALICE, Kind::Wallet).await;
    clock.advance(Duration::minutes(10));

    let html = page(&app, &browser, "/dashboard?range=24h").await;

    let (_, line) = chart_series(&html, "wealth-history");
    let times = line["times"].as_array().unwrap();
    let values = line["series"][0]["values"].as_array().unwrap();
    let at = |t: time::OffsetDateTime, from: bool| {
        let ms = t.unix_timestamp() * 1000;
        let found = if from {
            times.iter().position(|x| x.as_i64().unwrap() >= ms)
        } else {
            times.iter().rposition(|x| x.as_i64().unwrap() < ms)
        };
        values[found.unwrap()].as_i64().unwrap()
    };
    assert_eq!(at(first, false), 0);
    assert_eq!(at(first, true), 1_000_000_000);
    assert_eq!(at(second, false), 1_000_000_000);
    assert_eq!(at(second, true), 1_500_000_000);
}

#[sqlx::test]
async fn without_any_wealth_figure_the_graph_says_there_is_no_wealth_history(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;

    let html = page(&app, &browser, "/dashboard").await;

    assert!(html.contains("No wealth history yet."));
    assert!(!html.contains(r#"data-chart="wealth-history""#));
}

#[sqlx::test]
async fn the_summary_also_refreshes_on_the_wealth_signal_and_keeps_the_range(pool: PgPool) {
    let (app, _) = app_at(pool).await;
    let browser = signed_in_with(&app, ALICE, &[]).await;

    let html = page(&app, &browser, "/dashboard?range=30d").await;

    assert!(html.contains("sse:orders-updated, sse:wealth-updated, htmx:sseOpen from:body"));
    assert!(html.contains(r#"hx-get="/dashboard/summary?range=30d""#));
}
