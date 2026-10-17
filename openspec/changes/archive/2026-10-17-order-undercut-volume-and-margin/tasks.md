# Tasks

## 1. Contracts and stub

- [x] 1.1 Add `volume_remain` (integer, always present) to the market orders contract variants and to the stub's market fixtures and control. Verify: the stub conformance and contract unit tests pass.
- [x] 1.2 Add the contracts `wallet-transactions/newest`, `wallet-transactions/from-id` and `wallet-transactions/forbidden` with the fields `transaction_id`, `date`, `type_id`, `location_id`, `quantity`, `unit_price`, `is_buy`, `is_personal`, plus `Expires` etc. Verify: the contract unit tests pass.
- [x] 1.3 Add the stub route `GET /characters/{id}/wallet/transactions` with the following, and verify it with a new `eve-stub/tests/esi_transactions.rs`, plus passing coverage and conformance tests:
  - newest first, with a per-answer limit
  - `from_id` paging, and an empty list past the oldest
  - a 403 without the wallet scope
  - a `/_stub/transactions` control
  
  The test must cover paging, the empty tail and 403.
- [x] 1.4 Extend `esi-verify`:
  - check the transactions call (newest, then one `from_id` page)
  - fail with "a purchase is needed" when there is none
  
  Verify: `crates/esi-verify/tests/verify.rs` covers the missing-purchase failure.
- [x] 1.5 Run the real-EVE verification with the user's verification character (it needs a purchase in the last 30 days and a sell order). Record the observed page size and `from_id` inclusiveness in design.md's Open Questions, and commit the new `verified-stub.json`. Verify: stub-backed tests run again.

## 2. Undercut volume

- [x] 2.1 Write a migration that adds `market_orders.volume_remain bigint NOT NULL DEFAULT 0`, and make `EsiClient::market_orders` and the market refresh store it. Verify: a `tests/market.rs` case shows that stored volumes follow a refresh (spec: "Volumes follow the refresh").
- [x] 2.2 Add `sum(m.volume_remain) AS undercut_volume` to `orders::STANDING_JOIN`, gated by `available`, and expose it in `OrderRow`. Verify: `tests/market.rs` cases for the market-position "Undercut volume" scenarios (sell 25, buy 1,300 with the out-of-system jump order excluded, top has none, no market data has none).
- [x] 2.3 Extend `market::Snapshot` rows with the undercut volume. Verify: a `tests/events_stream.rs` case where only an undercutting order's remaining volume changes delivers `market-updated`, and an unchanged refresh delivers nothing.
- [x] 2.4 Add the undercut volume column after the gap in both tables, with the following:
  - `format::thousands`
  - empty for top, "—" without standing
  - `SortKey::UndercutVolume` with top as 0 and missing data last
  
  Verify by all of:
  - `format` unit tests
  - `tests/orders_page.rs` sorting cases
  - a Playwright test in `e2e/tests/orders.spec.ts` covering a sell row "1,250", a buy row "1,300", a top row empty, "—" without data, and sorting
  - a Playwright test in `e2e/tests/live.spec.ts` where the volume changes after a market refresh without a reload

## 3. Purchases and remembered sell orders

- [x] 3.1 Write a migration for the following, verified by `tests/schema.rs` (the backfill keeps the earliest `issued` per order and skips corp and buy orders) and `tests/unlink.rs` (the rows cascade on unlink and owner change):
  - `purchases`, `sell_order_history` and `sell_order_costs` (design §4)
  - the `wealth_sync.kind` check widened to `'transactions'`
  - the `sell_order_history` backfill from `order_snapshots`
- [x] 3.2 Make the order poll insert every personal sell order into `sell_order_history` with `ON CONFLICT DO NOTHING` in its existing transaction. Verify: `tests/poll.rs` cases where a repriced order keeps its first `issued`, a closed order stays remembered, and a corp sell order is not remembered.
- [x] 3.3 Add `EsiClient::wallet_transactions(character_id, token, from_id)` with a gate route and decoding, with `unit_price` parsed from decimal text. Verify: `tests/esi_client.rs` cases against the stub, including a unit price with two decimals kept exactly and the `from_id` query.

## 4. FIFO allocation

- [x] 4.1 Implement the pure `trade::allocate` (design §5) with unit tests for each trade-margin "First in, first out" scenario:
  - one order from two lots → 1,600.00 for 15
  - the later order gets the rest → 5 of 10
  - purchases after the order are not taken
  - ties broken by IDs
  - a lot split across orders
  - `i128` sums for huge amounts
  
  Verify: `cargo test -p sekondo trade` passes.
- [x] 4.2 Implement `trade::recompute(tx, character_id, type_ids)`, which replaces `sell_order_costs` for those pairs. Call it from the order poll for newly remembered sell orders. Verify: a `tests/trade.rs` case against the database. A second character's lots are not used ("Other characters' purchases do not count").

## 5. Transactions poll

- [x] 5.1 Add `Kind::Transactions` to `WealthPoller` with the following:
  - an hourly interval
  - the newest answer, then `from_id` paging until a known ID or an empty answer, capped at 50 requests
  - personal purchases inserted with `ON CONFLICT DO NOTHING`
  - `trade::recompute` for the touched types in the same transaction
  - `OrdersUpdated` published only when something was inserted
  
  Verify: `tests/wealth_poll.rs` cases for the trade-margin polling scenarios (first poll over several requests, stop at known, sales and corp purchases skipped, re-login skip, failure keeps the data), "Purchase stored late", and `tests/events_stream.rs` cases for the signal sent on new purchases and none on a repeat.
- [x] 5.2 Make the test-hooks wealth poll route accept the `transactions` kind, and schedule the kind for existing characters on startup. Verify: `tests/poll_hook.rs` and `tests/no_test_hooks.rs` cases.

## 6. Margin on the orders page

- [x] 6.1 Split the rate part of `fees::FEES_SQL` into `FEE_RATES_SQL` without changing its results. Verify: the existing `tests/fees.rs` passes unchanged.
- [x] 6.2 Add `margin_cents`, `covered_units` and `cost_per_unit_cents` to `list_orders` (design §6). Verify with `tests/trade.rs` cases for the "Margin of a sell order" scenarios:
  - 542.34
  - 180.78 when partly sold
  - partial cost → none, with 5 of 10 covered
  - a structure or unpolled skills → none
  - a corp order → none
  - a negative margin
- [x] 6.3 Add the margin column after the value in the sell table, with the following:
  - shortened ISK, and an exact `title` with the cost per unit
  - the error style when negative
  - "—" with "cost known for X of Y units" when the cost is partial
  - `SortKey::Margin`, with missing values last
  
  Verify by all of:
  - `tests/orders_page.rs` sorting cases
  - a Playwright test in `e2e/tests/orders.spec.ts` covering a shown margin with its hover, a negative margin, the partial-cost hover, sorting descending, and no margin column in the buy table
- [x] 6.4 Add `sse:fees-updated` to the `#order-tables` trigger. Verify with Playwright tests in `e2e/tests/live.spec.ts` for these two cases, each updating without a reload:
  - a higher Accounting level set through the stub and `/_test/fee-poll` changes the margin
  - a missing purchase added through the stub and the transactions poll hook makes the margin appear

## 7. Docs and integration

- [x] 7.1 Update the README with the transactions poll, the FIFO rules, the margin formula and the instant-sale limitation. Verify: the README mentions all four.
- [x] 7.2 Run the full suite (`cargo test --workspace` and the Playwright suite against the verified stub). Verify: both pass.
