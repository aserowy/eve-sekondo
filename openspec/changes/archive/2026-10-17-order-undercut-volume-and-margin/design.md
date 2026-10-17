# Design

## Context

- Standing and gap come from one SQL fragment, `orders::STANDING_JOIN`. It is a lateral `count(*)`/`min|max(price)` over `market_orders`, used both by `list_orders` and by `market::snapshot`. `market::Snapshot` compares `(account, order, undercut, gap)` tuples before and after a refresh to decide who gets `market-updated`. `market_orders` stores no volume today.
- Fees come from one SQL statement, `fees::FEES_SQL`, as `numeric` percent per `(character_id, location_id)` of the current orders. Sales tax is `NULL` until skills are polled. Broker fee is `NULL` in structures and until standings and the station owner are known.
- `WealthPoller` is the pattern for scheduled per-character ESI work. It has a `Kind` enum and `wealth_sync (character_id, kind)` with `CHECK (kind IN ('wallet','assets'))`. It also handles claim/record success/failure/deferral, the shared `EsiGate`, one transaction per store, and a signal after commit. The wallet scope `esi-wallet.read_character_wallet.v1` covers `GET /characters/{id}/wallet/transactions/`.
- The order poll in `poller.rs` replaces `current_orders` and appends `order_snapshots` per character in one transaction. `order_snapshots` holds every sell order ever seen, with its `issued` at each poll.
- The orders page refreshes `#order-tables` on `orders-updated` and `market-updated`, and `#freshness` on `orders-updated` and `fees-updated`.
- ESI sends prices as floats. The crate has no decimal type. Money is `numeric(20,2)` in the database and cents (`i64`) in Rust.

## Goals / Non-Goals

**Goals:**
- What the tables show, how they sort and when they are re-fetched all come from SQL, as for standing and fees, so sorting and change detection stay consistent.
- The FIFO allocation is a pure, unit-tested function. Its result is stored, so that listing and sorting are plain SQL.

**Non-Goals:**
- Tracking sales into buy orders, wallet journal data, or the actual broker fee paid when an order was placed (the current fee at the location is used).
- Corporation wallets and purchases made for the corporation.
- Matching lots to locations, or hauling costs.
- Per-unit FIFO inside a sell order: the remaining units are valued at the order's average cost.

## Decisions

### 1. Undercut volume in `STANDING_JOIN`
- Migration: `ALTER TABLE market_orders ADD COLUMN volume_remain bigint NOT NULL DEFAULT 0`. `market.rs` stores `volume_remain` from ESI on every refresh.
- The existing lateral select gains `sum(m.volume_remain) AS undercut_volume`, gated by `available` like the other columns. A top order has `undercut = 0` and shows no volume. The listing exposes it as `undercut_volume`. `Snapshot` rows grow to `(account, order, undercut, gap, undercut_volume)`, so a change in volume alone sends `market-updated`.
- Until the first refresh after the deploy, the default of 0 is shown as a too-low volume for up to one refresh interval. This is accepted instead of a nullable column with an extra code path.

**Alternative:** a second lateral join just for volume. It was rejected because it would run the reach rules twice and could drift.

### 2. Wallet transactions call
`EsiClient::wallet_transactions(character_id, token, from_id: Option<i64>)` calls `GET /characters/{id}/wallet/transactions/` (`?from_id=` when given). It gets its own gate route and contract.

`EsiTransaction` reads these fields: `transaction_id`, `date`, `type_id`, `location_id`, `quantity`, `unit_price`, `is_buy`, `is_personal`. `unit_price` is a float and is parsed through its decimal text form into `numeric`, not via `f64` arithmetic, as for standings.

### 3. Polling as a third `wealth_sync` kind
`Kind::Transactions` gets a one-hour interval. The migration widens the `CHECK` constraint to include `'transactions'`. `run_poll` for this kind does the following:
- Fetches the newest answer.
- While the answer is non-empty and contains no already stored `transaction_id`, fetches again with `from_id = min(id) − 1`. It stops after a bound of 50 requests and logs it.
- Inserts personal purchases with `ON CONFLICT (transaction_id) DO NOTHING`.
- If any row was inserted, recomputes the allocation for the touched `(character, type)` pairs in the same transaction (§5), then publishes `OrdersUpdated` instead of `WealthUpdated`.

The expiry is taken from the first answer.

**Alternative:** a separate poller module. It was rejected because scheduling, backoff and sync-state handling would be duplicated. The kind enum already covers this case.

### 4. Storage
- `purchases (transaction_id PK, character_id → characters ON DELETE CASCADE, type_id, location_id, quantity bigint, unit_price numeric(20,2), bought_at timestamptz)`, with an index on `(character_id, type_id, bought_at, transaction_id)`.
- `sell_order_history (order_id PK, character_id → characters ON DELETE CASCADE, type_id, first_issued timestamptz, volume_total bigint)`. The order poll inserts every personal sell order it stores with `ON CONFLICT DO NOTHING`, in its existing transaction. The backfill in the migration uses `SELECT DISTINCT ON (order_id) … FROM order_snapshots WHERE NOT is_buy_order AND NOT is_corporation ORDER BY order_id, polled_at`.
- `sell_order_costs (order_id PK → sell_order_history ON DELETE CASCADE, covered_units bigint, cost numeric(24,2))` is the stored result of the allocation.

Unlink and change of owner go through the existing `DELETE FROM characters`, so everything cascades.

### 5. FIFO as a pure function, stored per (character, item)
`trade::allocate(lots: &[Lot], orders: &[RememberedOrder]) -> Vec<OrderCost>` works on lots sorted by `(bought_at, transaction_id)` and orders sorted by `(first_issued, order_id)`. It keeps a cursor into the lots and a remainder of the current lot. For each order it takes from lots with `bought_at ≤ first_issued` up to `volume_total`. Costs are summed in cents as `i128`, because quantity × unit price can exceed `i64`.

`trade::recompute(tx, character_id, type_ids)` loads both sides for those pairs, runs `allocate`, and replaces their `sell_order_costs` rows. It runs in two places:
- the transactions poll, for the types of the inserted purchases
- the order poll, for the types of newly remembered sell orders

The allocation for a pair is fully redone each time, so late purchases are picked up (spec: "Purchase stored late").

**Alternative:** pure SQL with recursive CTEs. It was rejected because the "bought at or before" cap makes the running sums depend on each other, which is hard to read and test. Computing on read was also rejected, because the tables could then not sort by margin in SQL.

### 6. Margin in `list_orders`
`list_orders` left-joins `sell_order_costs` and a fee subquery built from `FEES_SQL`'s inner rates (character, location, `sales_tax`, `broker_fee` in percent). It computes `margin_cents` as:

```
CASE WHEN NOT o.is_buy_order AND NOT o.is_corporation
          AND sc.covered_units = o.volume_total
          AND f.sales_tax IS NOT NULL AND f.broker_fee IS NOT NULL
     THEN round(o.volume_remain * o.price * (1 - f.sales_tax/100 - f.broker_fee/100)
                - o.volume_remain * sc.cost / o.volume_total, 2) * 100
END
```

`numeric` `round` is half away from zero, which is half up for positive values. It also returns `covered_units`, `volume_total` and `cost_per_unit_cents` for the hover text. To share the rates without duplicating the formulas, `FEES_SQL` is split: its rates part becomes a `pub(crate) const FEE_RATES_SQL` that both use.

**Alternative:** use `sell_order_history.volume_total` instead of the current `volume_total`. The two are equal, because ESI's `volume_total` does not change on reprice. The current row is used for readability.

### 7. Columns, sorting and signals
- New `SortKey`s: `UndercutVolume` (both sides) and `Margin` (sell only). They use the same "missing last in both directions" `NULLS LAST` pattern as `Gap`. For undercut volume, a top order sorts as 0.
- Formatting: `undercut_volume` uses a new `format::thousands(i64)`, and the margin uses `isk_short`/`isk_exact`. A negative margin gets the existing error badge class.
- `orders.html`: `#order-tables` also triggers on `sse:fees-updated`.

## Risks / Trade-offs

- [Instant sales don't take lots] → Later sell orders can show an older, cheaper cost. This is documented in the proposal as a known limitation. A later change could use the wallet journal to tell order fills from instant sales.
- [Only about 30 days of history at first link] → Older stock shows "—" with a coverage hint and does not guess. History builds up from then on.
- [`from_id` paging behaves differently on real ESI than assumed] → The contract has a variant for it, and the real-EVE verification exercises it before the stub is trusted. The 50-request bound caps the damage.
- [The undercut volume reads 0 until the first market refresh after the deploy] → This lasts at most one refresh interval (5 minutes by default).
- [Margin uses today's broker fee, not the one paid when the order was placed] → Accepted. Fee data has no history.
- [The allocation is recomputed per pair on every new lot] → The work is bounded by one character's lots and orders of one item, which is small.

## Migration Plan

1. A single migration adds `market_orders.volume_remain` and widens the `wealth_sync.kind` check. It creates `purchases`, `sell_order_history` (backfilled from `order_snapshots`) and `sell_order_costs`.
2. On startup, existing characters get a `transactions` sync row through the usual `schedule_new`, so the first polls are spread across the hour. Each first poll recomputes the costs for its character.
3. The stub fingerprint changes, so a real-EVE verification is required before stub-backed tests run, with a character that has at least one purchase in the last 30 days.

Rollback: the migration only adds things. The previous app version ignores the new tables and column, except for the widened `CHECK`, which it does not violate.

## Open Questions

- Resolved by the real-EVE verification: `from_id` is **inclusive** (asking from the oldest ID returned that same transaction), so paging with `from_id = min(id) − 1` in §3 is right, and the stub does the same. The page size could not be observed: the verification character has only 10 transactions in the window, all returned in one answer, so a page holds at least 10. The stub uses 1,000 as for assets. The stop rule ("stop at a known ID or an empty answer") holds either way.
