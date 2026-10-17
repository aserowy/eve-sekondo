# Design

## Context

- The orders page (`orders.rs`) reads `current_orders` through `list_orders`, one query per side, ordered by `SortKey::expression()`. Columns are a list in `headers()` and the cells are in `orders_table.html`.
- The ESI client (`esi.rs`) has three routes behind one `fetch` that applies the gate, the `User-Agent` and the compatibility date. `EsiResponse` carries the body and the cache expiry, but no other header.
- `Poller` claims due characters with a lease and polls them. It publishes `Event::OrdersUpdated` to the owning account. The bus and the SSE handler treat events generically by name; a lagging subscriber is given `OrdersUpdated`.
- The orders page refreshes `#order-tables` and `#freshness` on `sse:orders-updated, htmx:sseOpen from:body`.
- `current_orders` rows carry `region_id`, `location_id`, `type_id`, `range` and the price, but not the solar system. ESI's character orders have no `system_id`; the public market orders have one.
- The stub is verified against real EVE: any change to `eve-stub` or a contract changes the fingerprint and makes every stub-backed test fail until `cargo run -p esi-verify` is run by a person with a browser and a verification character.
- The tool is for station trading: the user's own buy orders are station range. Competitors may have any range.
- ESI's region market endpoint does not list the orders of player structures, except ranged buy orders (checked against the ESI docs). The full structure market needs another scope and docking access.
- Pico defines `--pico-ins-color` (success) and `--pico-del-color` (error); the page already uses the latter for the "Stale" badge.

## Goals / Non-Goals

**Goals:**
- One shared definition of "standing" that both the page rendering and the change detection use, so a signal is sent exactly when what a user would see changes.
- Standing and gap computed in the database, so they can be sorted without loading market data into Rust.
- Market data requested once per (region, item), whatever the number of accounts.

**Non-Goals:**
- Orders in player structures, and competitors listed only in structure markets (needs `esi-markets.structure_markets.v1` and docking access). Such orders show "—".
- Taking remaining or minimum volume into account, or a price history of the market.
- Jump distances between systems. A jump-range buy order in another system is not counted even when it reaches the station in game, so no ESI route call and no distance cache are needed. A "Top" next to a neighbouring system with such orders can be wrong; the spec states this.
- The range of the user's own buy order. It is always compared at its own station, which fits station trading.
- Showing the competing orders themselves, or a link to them.
- A configurable market interval. It is 5 minutes, the cache time of ESI's market answers.
- Changing the dashboard.

## Decisions

### 1. Two small tables, no hypertable
A migration adds:
- `market_orders (order_id bigint PRIMARY KEY, region_id, type_id, location_id, system_id, is_buy_order, price numeric(20,2), range text)`, with an index on `(region_id, type_id)`. Only what the comparison reads is kept. It is a copy of public data, so it has no account or character column.
- `market_sync (region_id, type_id, PRIMARY KEY (region_id, type_id), next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures)`, mirroring `order_sync`.

No history is kept: nothing reads it, and the dashboard history is about the user's own orders.

*Alternative:* computing standings in Rust from a loaded market list. It cannot sort in SQL and would need the whole pair in memory per request.

### 2. A `market` module with its own scheduler
`market.rs` holds `MarketPoller`, built like `Poller` from the pool, `EsiClient`, event bus, clock and an interval (5 minutes by default), and started next to it in `app.rs`. One tick:
1. **Track**: insert missing `market_sync` rows for every distinct `(region_id, type_id)` in `current_orders`, spread over the interval by a hash of the pair (as `first_offset` does for characters). Delete `market_sync` and `market_orders` of pairs no longer in `current_orders`.
2. **Claim** due pairs with a lease (`FOR UPDATE SKIP LOCKED`, as `claim_due`) and refresh a few at a time.
3. **Refresh** a pair (decision 4).

A failure is recorded with the same exponential backoff as characters, and a `Deferred` from the gate reschedules without counting a failure. The next refresh is `max(now + interval, ESI's Expires) + jitter`.

*Alternative:* tracking pairs inside `Poller::store`. It would couple the character poll to market data and make the 5-minute cadence depend on 20-minute character polls.

### 3. ESI client: one public call
`EsiClient::market_orders(region_id, type_id)` requests `GET /markets/{region_id}/orders/?order_type=all&type_id=…&page=n`, no token, and reads every page named by `X-Pages`. `fetch` gains a variant that also returns the `X-Pages` value; the expiry comes from the first page. The call has its own gate route (`market`) so a failing market group does not block the character calls.

Pages are read one after another: a pair has one page unless the item is very liquid, and the gate and error limit then apply per request.

### 4. A refresh replaces the pair
For one pair:
1. Read the **before** snapshot (decision 6).
2. Fetch all pages; if any page fails, keep the old copy and record the failure.
3. In one transaction: delete the pair's `market_orders` and insert the new ones, and update `market_sync`.
4. Read the **after** snapshot, compare, and publish.

*Alternative:* counting jump-range buy orders from other systems, with distances from ESI's route call cached per pair of systems. More accurate near neighbouring systems, but a second ESI call with its contract, stub and verification, a third table, and an "unknown distance" state, for a case that rarely matters to a station trader.

### 5. Standing and gap in SQL, shared by listing and detection
One SQL fragment computes, per current order, `undercut` (count) and `best_price`, or NULL when there is no standing. `list_orders` joins it for the table; the detection in decision 6 selects from the same fragment. For an order `o`:
- **Available** only if `market_sync.last_success_at` is set for `(o.region_id, o.type_id)` and `o.location_id < 100_000_000` (not a structure).
- **Sell**: competitors are `market_orders` of the pair with `NOT is_buy_order`, `location_id = o.location_id`, `order_id <> o.order_id`, `price < o.price`; `undercut` is their count and `best_price` their minimum.
- **Buy**: the order's own `range` is not read. The order's system is `system_id` of its own row in `market_orders` (public data lists the character's own order too); no row means unavailable. Competitors are buy orders of the pair with `order_id <> o.order_id` and `price > o.price` that **reach** it: the same `location_id` (any range), or `range = 'region'`, or `range <> 'station'` (solar system or a number of jumps) and the same system. `best_price` is their maximum.
- The gap is `abs(best_price - o.price)` in cents, its percentage is computed in Rust from integers, rounded half up to one decimal (`format` gets a helper), and the whole amount keeps the `isk_short`/`isk_exact` treatment.

Sorting is in the query. Both new keys always put unavailable orders last: the `ORDER BY` starts with `(undercut IS NULL)`, so it does not flip with the direction. The standing sorts by `(undercut > 0, undercut)`, so "Top" comes first and `Undercut 3` before `Undercut 12`. The gap sorts by `COALESCE(abs(best_price - price), 0)`, a top order counting as zero. The new `SortKey` names are `standing` and `gap`; unknown names keep falling back to the default.

*Alternative:* sorting the standing as the literal text. "Undercut 12" would come before "Undercut 3", which the requirement rules out.

### 6. Change detection by before/after snapshots
The snapshot of a pair is the result of the decision 5 fragment for every current order using it, as `(account_id, order_id, undercut, gap)` rows. After a refresh (market orders replaced) the two snapshots are compared per account, and `Event::MarketUpdated` is published to every account with at least one differing row. No state is kept between refreshes, so a restart can at worst skip a signal, never send a wrong one; the page also refreshes on reconnect.

The character poll replaces `current_orders` and already signals `orders-updated`, so orders that appear or disappear there need nothing from this.

*Alternative:* signalling on every refresh. Cheap, but it would re-render every open orders page every 5 minutes per pair with nothing visible changing.

### 7. A second event, only for the orders page
`Event::MarketUpdated` has the SSE name `market-updated`. The lagging-subscriber fallback stays `OrdersUpdated`, which also makes the orders page re-fetch. `#order-tables` listens to `sse:orders-updated, sse:market-updated, htmx:sseOpen from:body`; the dashboard and `#freshness` do not. Market refreshes change no freshness, so `#freshness` stays as it is.

### 8. The page
- `headers()` and `orders_table.html` list: Character, Item, Price, Standing, Behind by, Volume, amount, Issued, Time left, Location, Region. `SortKey::ALL` gains `Standing` and `Gap`.
- `RowView` gets `standing`, `standing_class` (`top`, `undercut`, or empty), `gap` and `gap_exact`.
- The badge classes are `.badge.top { background: var(--pico-ins-color); }` and `.badge.undercut { background: var(--pico-del-color); }` beside the existing `.badge.stale`, which already pairs the background with `--pico-contrast-inverse`. No colour value is written in the CSS.

### 9. Stub, contracts and verification
- **Contracts**: `market-orders` (variants `orders` with every field the app reads and the `X-Pages` header, plus the usual failure variants). Fields the app reads from the market answer: `order_id`, `type_id`, `location_id`, `system_id`, `is_buy_order`, `price`, `range` (the app does not read volumes or the rest).
- **Stub**: `GET /markets/{region_id}/orders` filtering by `type_id`, `order_type` and `page`, with `X-Pages`. Control: `/_stub/market-orders` (replace the orders of a region; optional page size, so pagination can be tested). The default fixture has no market orders, so existing tests see every sell order as "Top" once a pair is fetched, and buy orders as "—" (their own row is missing) unless a test seeds them.
- **Verification** (`esi-verify`): after the character's orders, request the market orders for the region and item of the first sell order and of the first buy order, so real sell and buy answers are checked.
- **Test hooks** (feature `test-hooks`): `POST /_test/market-poll` runs one market tick now, ignoring `next_poll_at`, so the Playwright tests do not wait for the scheduler.

## Risks / Trade-offs

- [Every stub-backed test fails after the stub change until a person runs the real verification] → The stub and contract tasks come first and end in a manual step that names the command. Later tasks that need the stub wait for it, as the previous ESI change did.
- [Competitors listed only in player structures are invisible, so a "Top" can be wrong] → Known and documented in the spec and Non-Goals.
- [Jump-range buy orders in neighbouring systems are not counted, so a buy "Top" can be wrong] → A stated rule with its own scenario. Distances can be added later behind the same SQL fragment. Orders in structures themselves show "—" instead of a wrong standing. Structure markets could be added later with the extra scope.
- [A buy order's own row is missing from the market copy (placed since the last refresh), so its system is unknown] → Its standing is "—" until the next refresh, which is at most 5 minutes.
- [Market data several minutes old] → Accepted: the last known data is used, as the stale character's orders are.
- [Many pairs make many requests] → Personal scale (tens of pairs), one request per pair and refresh, spread over the interval, with the gate and error-limit margin as everywhere else.
- [The percentage rounding disagrees with a person's mental arithmetic] → Integer half-up rounding to one decimal of the difference over the best price, stated in the spec with an example.

## Migration Plan

1. The migration creates the two tables; it runs on startup like the others. Nothing is backfilled: pairs are tracked from the next tick on, and orders show "—" until their pair is refreshed.
2. Rollback: revert the release. The tables are ignored by the old code and can be dropped by hand.
3. Run `cargo run -p esi-verify` once after the stub and contract tasks (task 1.4), to record the new fingerprint.
