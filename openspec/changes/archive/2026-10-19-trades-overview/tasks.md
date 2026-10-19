# Tasks

## 1. Storing sales

- [x] 1.1 Write a migration that adds `sales`, `sale_costs` and `wealth_sync.full_read`, and sets the flag for existing `transactions` rows (design §1). Verify:
  - `tests/schema.rs`: the flag is set for an existing transactions row and not for the wallet and assets rows
  - `tests/unlink.rs`: sales and sale costs cascade on unlink and on owner change ("Trade data is removed with the character")
- [x] 1.2 Make `store_transactions` also insert personal sales with `ON CONFLICT DO NOTHING`, and widen the stop check in `fetch_transactions` to purchases or sales (design §2). Verify with `tests/wealth_poll.rs` cases for:
  - the first poll stores purchases and sales
  - stopping at a stored sale
  - corporation sales are not stored
  - a sale is not a purchase lot
  - re-login skip and failure keep the sales
- [x] 1.3 Add the one-time full read: read `full_read` from the sync row, page past known IDs until an empty answer (capped at 50), and clear the flag on success. Verify: a `tests/wealth_poll.rs` case for "Sales filled in once after the upgrade". The first poll reads past a stored purchase, and the second stops at it.

## 2. Sale FIFO and cost

- [x] 2.1 Extract the cursor loop of `trade::allocate` into a shared `take_fifo` helper and add the pure `trade::allocate_sales` (design §3). Verify with unit tests in `trade.rs`:
  - one sale from two lots → 1,600.00 for 15
  - a later sale gets the rest → 5 of 10
  - purchases after the sale are not taken
  - ties broken by IDs
  - a lot split across sales
  - `i128` sums
  - every existing `allocate` test still passes
- [x] 2.2 Make `trade::recompute` also load the sales of the types and replace their `sale_costs`, and call it for the types of new sales in `store_transactions`. Verify with `tests/trade.rs` cases against the database:
  - other characters' purchases are not taken
  - a sale and a sell order both take the same lot ("Independent of sell orders"), and the order's cost is unchanged
  - a purchase stored late feeds an earlier-stored sale

## 3. Sale margin and fees

- [x] 3.1 Add the `sale_locations_sql!` row set and `SALE_FEE_RATES_SQL` to `fees.rs`, and a `trades::list` query returning buys and sells in a range with `margin`, `covered_units` and `cost` (design §4). Verify with `tests/trade.rs` cases for:
  - 428.23
  - the −143.07 loss
  - partial cost → none with 4 of 10 covered
  - a structure or unpolled skills → none
  - a higher Accounting level changes the margin
  - only personal buys and sells of the account's selected characters, newest first, within the range
- [x] 3.2 Extend the fee poller's unknown-station query to sale locations and include characters with sales there in `store_owner`'s before/after comparison (design §5). Verify: a `tests/fee_poll.rs` case where a sale at a new NPC station gets its owner looked up and the account receives `fees-updated`.

## 4. Signal

- [x] 4.1 Add `Event::TradesUpdated` (`trades-updated`). Make the transactions poll publish it when anything new was stored, plus `OrdersUpdated` when a purchase was new. Verify with `tests/events_stream.rs` cases for:
  - a new sale gives only `trades-updated`
  - a new purchase gives both
  - a repeat gives none
  - no cross-account delivery

## 5. Trades page

- [x] 5.1 Add `trades_page.rs` with `GET /trades` and `/trades/table`, and the templates `trades.html` and `trades_table.html`. The page gets the filter box with the character filter, the window-page layout, and the range control in the panel title. The table gets the columns, the formatting and hovers, the empty state, and the sticky totals `tfoot` (design §6). Verify:
  - `tests/trades_page.rs`: unauthenticated redirect; default and unknown range give 7d; foreign `chars` are ignored; totals with "1 sale left out"
  - a Playwright test `e2e/tests/trades.spec.ts` covering the "Sale row" and "Purchase row" values, the margin hovers (exact amount and partial cost), the negative margin in the error style, a range switch to 30d that keeps `chars` in the URL, and narrowing the selection
- [x] 5.2 Add `/trades/freshness` with `trades_freshness.html` and a `freshness::list_transactions_freshness`, in the collapsible freshness column with the warning count. Verify:
  - `freshness.rs` unit tests: stale after two hours, re-login stale, never polled stale
  - a Playwright test in `e2e/tests/freshness.spec.ts`: the trades strip shows "1" for one stale character, and none when all are fresh; unfolding shows the "Log in again" link
- [x] 5.3 Add the "Trades" link to the navigation between Wealth and Opportunities, carrying `chars`. Verify: Playwright cases in `e2e/tests/navigation.spec.ts` for the extended navigate-through scenario, selection carried to Trades and range not carried; plus a `tests/navigation.rs` case for the link and its current marker.
- [x] 5.4 Wire the live refresh: the table on `sse:trades-updated`, `sse:fees-updated` and reconnect; the freshness on `sse:trades-updated` and reconnect. Verify with Playwright tests in `e2e/tests/live.spec.ts` for:
  - a new sale through the stub and the transactions poll hook appears without a reload
  - a higher Accounting level through `/_test/fee-poll` changes a sale's margin without a reload
  
  Also add a case in `e2e/tests/scroll.spec.ts`: a scrolled trades table keeps its position on `trades-updated`.
- [x] 5.5 Cover the layout rules for the trades page. Verify with Playwright cases in `e2e/tests/layout.spec.ts`:
  - with 300 transactions at 1920×1080 the page does not scroll, and the header and totals rows stay visible while the table scrolls
  - the freshness is right of the table when unfolded
  - the range control is in the panel title and the character filter in the filter box
  - no `h1`
  - no CSP violations on the trades page

## 6. Docs and integration

- [x] 6.1 Update the README: the trades page, storing sales, the sale FIFO, the sale margin formula, the instant-sale broker-fee simplification and the one-time full read. Verify: the README mentions all six.
- [x] 6.2 Run the full suite (`cargo test --workspace` and the Playwright suite against the verified stub). Verify: both pass.
