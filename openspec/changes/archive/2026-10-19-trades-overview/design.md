# Design

## Context

- **Transactions poll.** `wealth_poller.rs` polls the transactions as `Kind::Transactions`, every hour per character.
  - `fetch_transactions` reads the newest answer and then pages with `from_id = min(id) − 1`. It stops when an answer holds an ID already in `purchases`, or the answer is empty, or after 50 requests.
  - `store_transactions` keeps only `is_buy && is_personal` rows in `purchases`, runs `trade::recompute` for the new types and publishes `OrdersUpdated` when something was new.
  - `EsiTransaction` already decodes `is_buy`, `is_personal`, `location_id`, `quantity`, `unit_price` (cents) and `date`. No ESI contract or stub change is needed; the stub's transactions control can already serve sales.
- **FIFO.** `trade::allocate` is a pure FIFO of lots to remembered sell orders. `trade::recompute` reloads both sides for some `(character, type)` pairs and rewrites `sell_order_costs`.
- **Fees.** `fees.rs` builds the rate SQL from a macro: `fee_rates_sql!($locations)` gives `sales_tax`/`broker_fee` as `numeric` percent for any `(character_id, location_id)` row set. `order_locations_sql!` and `fixed_location_sql!` are the existing row sets.
  - The fee poller looks up unknown station owners only for `current_orders` locations (`fee_poller.rs` around the `unknown` query). `store_owner` signals `fees-updated` to the accounts whose fees changed.
- **Pages.** Each page module (`wealth_page.rs`, `orders.rs`) has a `page` handler plus fragment handlers that htmx re-fetches on `sse:*` triggers.
  - Selection parsing is in `selection.rs` (`ViewQuery`, `CharacterSelection`), the range in `range.rs` (`Range::parse`, `span`, labels), freshness records and the stale rule in `freshness.rs`, ISK formatting in `format.rs`.
  - The wealth page is the closest template: a window page, a filter box with the character filter, a range control in a panel title, and a collapsible freshness column.
- **Sync state.** `wealth_sync (character_id, kind)` holds the sync state of the transactions kind.

## Goals / Non-Goals

**Goals:**
- Sales are stored like purchases, and the sale FIFO is a pure, unit-tested function whose result is stored, as for sell orders.
- The margin is computed in SQL at read time from the stored cost and the current fee rates. Fee changes then show up without recomputing anything.
- The page reuses the wealth page's layout pieces, selection, range, freshness and formatting.

**Non-Goals:**
- Wallet journal data (actual tax and broker fee paid), and telling instant sales apart from sell-order fills.
- Sorting or paging the trades table, and per-item aggregation.
- Corporation transactions.
- Changing the sell-order allocation or the orders page.

## Decisions

### 1. Storage: a `sales` table next to `purchases`
Migration:
- `sales (transaction_id PK, character_id → characters ON DELETE CASCADE, type_id, location_id, quantity bigint, unit_price numeric(20,2), sold_at timestamptz)`, indexed on `(character_id, type_id, sold_at, transaction_id)` for the allocation and on `(character_id, sold_at)` for the page.
- `sale_costs (transaction_id PK → sales ON DELETE CASCADE, covered_units bigint, cost numeric(24,2))`.
- `wealth_sync ADD COLUMN full_read boolean NOT NULL DEFAULT false`, then `UPDATE wealth_sync SET full_read = true WHERE kind = 'transactions'`.

`purchases` stays as it is, because it is what the sell-order FIFO reads. The page lists `purchases UNION ALL sales`.

**Alternative:** one `wallet_transactions` table with `is_buy`. It was rejected because it means migrating `purchases`, `trade::recompute` and the existing tests for no visible gain.

### 2. Poll: stop condition and the one-time full read
- The stop check becomes `EXISTS (SELECT 1 FROM purchases WHERE transaction_id = ANY($1)) OR EXISTS (SELECT 1 FROM sales WHERE …)`.
- `fetch_transactions` takes a `full_read: bool` read from the sync row. When it is set, it ignores the stop check and pages until an empty answer, still bounded by the 50 requests. `record_success` clears the flag in the same transaction that stores the result.
- A new character's row is created without the flag. Its first poll reads back fully anyway, because nothing of it is stored yet.
- `store_transactions` also inserts `!is_buy && is_personal` rows into `sales` with `ON CONFLICT DO NOTHING RETURNING type_id`. It then runs one `trade::recompute` for the union of the types of the new purchases and the new sales.
- Signals after commit: `TradesUpdated` when anything new was stored, and also `OrdersUpdated` when a new purchase was stored. `publish` gets a list of events instead of an `Option`.

### 3. Sale FIFO: `trade::allocate_sales`
`allocate_sales(lots: &[Lot], sales: &[Sale]) -> Vec<SaleCost>` works like `allocate`, with sales sorted by `(sold_at, transaction_id)` and the cap `bought_at ≤ sold_at`, taking up to `quantity`. The cursor loop is shared, not copied: both `allocate` and `allocate_sales` map their inputs to a private `(id, at, wanted)` demand list and call one `take_fifo` helper.

`trade::recompute` already loads the lots for the given types. It also loads the sales, runs both allocations, and replaces `sell_order_costs` and `sale_costs` for those pairs. Callers are unchanged. The order poll recomputes sale costs too, which is harmless and keeps one entry point.

**Alternative:** compute the sale cost on read with window functions. It was rejected for the same reason as for orders: the "bought at or before" cap makes the running sums depend on each other.

### 4. Margin on read
New `fees` row-set macro `sale_locations_sql!()`: `SELECT DISTINCT s.character_id, s.location_id FROM sales s JOIN characters c … WHERE s.character_id = ANY($1) AND ($2::uuid IS NULL OR c.account_id = $2)`. `pub const SALE_FEE_RATES_SQL = fee_rates_sql!(sale_locations_sql!())`.

The trades listing selects, for the range `[now − span, now]`:

```
SELECT 'buy' AS side, p.transaction_id, p.character_id, p.type_id, p.location_id,
       p.quantity, p.unit_price, p.bought_at AS at,
       NULL margin, NULL covered_units, NULL cost
FROM purchases p …
UNION ALL
SELECT 'sell', s.transaction_id, …,
       CASE WHEN sc.covered_units = s.quantity
                 AND f.sales_tax IS NOT NULL AND f.broker_fee IS NOT NULL
            THEN round(s.quantity * s.unit_price * (1 - f.sales_tax/100 - f.broker_fee/100) - sc.cost, 2)
       END,
       sc.covered_units, sc.cost
FROM sales s LEFT JOIN sale_costs sc … LEFT JOIN (SALE_FEE_RATES_SQL) f USING (character_id, location_id)
ORDER BY at DESC, transaction_id DESC
```

`numeric` `round` is half away from zero, as for order margins. Totals (bought, sold, margin sum, sales without a margin) are summed in Rust over the listed rows in cents (`i128`, saturated to `i64` for `format::isk_short`), so they always match the table.

### 5. Station owners for sale locations
The fee poller's `unknown` query becomes `current_orders` locations `UNION` `sales` locations below `STRUCTURE_ID_MIN`. `store_owner` takes its characters from both sources, and the before/after comparison uses `fees::for_characters` plus the sale rates for those characters. An account whose sale-location rates change gets `fees-updated`, which the trades table listens to.

### 6. Page: `trades_page.rs`
- Routes: `GET /trades` (page), `/trades/table` and `/trades/freshness` (fragments). They are built like `wealth_page.rs`: `CurrentAccount`, `selection::{ViewQuery, CharacterSelection}`, `Range::parse`.
- Templates:
  - `trades.html`, extending `base.html`, with the filter box (character filter only) and the window-page layout with the collapsible freshness column, as `wealth.html`.
  - `trades_table.html`, with the panel title holding the range control and a `<tfoot>` totals row that is sticky at the bottom.
  - `trades_freshness.html`.
- htmx triggers:
  - table: `sse:trades-updated, sse:fees-updated, htmx:sseOpen from:body`
  - freshness: `sse:trades-updated, htmx:sseOpen from:body`
  
  Scroll keeping comes from the existing refresh script, which covers any panel that scrolls on its own.
- Row cells:
  - date: `format::eve_time`
  - character and item names, and the location name, via `names::resolve`, as on the orders page
  - "Buy"/"Sell"
  - quantity: `format::thousands`
  - unit price and value: `isk_short` with the exact amount in `title`
  - margin: `isk_short`, with a `title` holding the exact amount and the cost per unit, or "cost known for X of Y units"; the error class when negative
- Freshness: a `freshness::list_transactions_freshness` that reads `wealth_sync` kind `transactions` plus `needs_relogin`. The stale rule is `is_stale(record, now, 1h)`, which applies "older than twice the interval". The warning count is the number of stale characters.
- Navigation: the "Trades" link in `base.html` between Wealth and Opportunities, carrying `chars` like the others.
- `events.rs`: `Event::TradesUpdated` → `"trades-updated"`.

## Risks / Trade-offs

- **Instant sales are charged a broker fee.** Their margin is understated by `b × value`. The user accepted this; it is documented in the README.
- **Current fee rates, not historic ones.** Old sales' margins shift when skills or standings change. This is accepted and consistent with order margins.
- **History before the deploy only reaches ESI's window.** Older sales are gone. Purchases older than that are still listed, so the table can show buys without their matching sells.
- **A sale and a sell order can take the same lots.** The two allocations are independent by decision, so their costs are not additive.
- **The first poll after the deploy reads up to 50 pages per character.** This is bounded and runs once, through the shared ESI gate.

## Migration Plan

1. Migration adds `sales`, `sale_costs` and the `full_read` flag (set for existing transactions rows).
2. On the next hourly poll each existing character reads its full ESI window, stores its sales, recomputes costs and clears the flag.
3. Rollback: drop `sale_costs` and `sales` and the column. `purchases` and the order margins are untouched.

## Open Questions

None; all decisions were settled with the user before the proposal.
