mod common;

use common::PollEnv;
use sekondo::{
    orders::{Side, Sort},
    selection::{CharacterSelection, ViewQuery},
};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

async fn two_accounts() -> (PollEnv, uuid::Uuid, uuid::Uuid) {
    let env = PollEnv::new().await;
    let mine = env.new_account().await;
    let theirs = env.new_account().await;
    env.link(mine, ALICE).await;
    env.link(mine, BOB).await;
    env.link(theirs, CAROL).await;
    // The stub's fixture: Alice has two sell orders and a buy order, Bob one sell order. Give
    // Carol an order of her own.
    env.control(
        "orders",
        serde_json::json!({"character_id": CAROL, "orders": [{
            "order_id": 7001, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
            "is_buy_order": false, "is_corporation": false, "price": 1.0,
            "volume_remain": 1, "volume_total": 1, "min_volume": 1, "range": "station",
            "issued": "2026-10-01T12:00:00Z", "duration": 90
        }]}),
    )
    .await;
    for character in [ALICE, BOB, CAROL] {
        env.poller.poll_character(character).await;
    }
    (env, mine, theirs)
}

async fn all_orders(
    env: &PollEnv,
    account: uuid::Uuid,
    selection: &CharacterSelection,
) -> Vec<(i64, i64)> {
    let mut rows = Vec::new();
    for side in [Side::Sell, Side::Buy] {
        for row in env
            .stores
            .orders
            .list_orders(account, selection.ids(), side, Sort::default())
            .await
            .unwrap()
        {
            rows.push((row.character_id, row.order_id));
        }
    }
    rows.sort_unstable();
    rows
}

#[tokio::test]
async fn a_foreign_character_id_never_yields_that_characters_orders() {
    let (env, mine, _) = two_accounts().await;
    let own = [ALICE, BOB];

    let query = ViewQuery {
        chars: Some(CAROL.to_string()),
        ..ViewQuery::default()
    };
    let selection = query.selection(&own);

    // The foreign ID is ignored: the user's own characters are shown as if there were no filter.
    assert_eq!(selection.ids(), &own);
    let rows = all_orders(&env, mine, &selection).await;
    assert_eq!(
        rows,
        vec![(ALICE, 5001), (ALICE, 5002), (ALICE, 5003), (BOB, 6001)]
    );
}

#[tokio::test]
async fn the_query_itself_is_scoped_to_the_account_even_if_given_a_foreign_id() {
    let (env, mine, _) = two_accounts().await;
    // Defence in depth: a selection built from the wrong list still cannot read Carol's orders.
    let selection = CharacterSelection::resolve(&[ALICE, CAROL], Some("90000003"));
    assert_eq!(selection.ids(), &[CAROL]);

    assert!(all_orders(&env, mine, &selection).await.is_empty());
}

#[tokio::test]
async fn a_subset_shows_only_the_selected_characters_orders() {
    let (env, mine, _) = two_accounts().await;
    let selection = CharacterSelection::resolve(&[ALICE, BOB], Some("90000002"));

    assert_eq!(all_orders(&env, mine, &selection).await, vec![(BOB, 6001)]);
}
