# Design

## Context

- `market.rs` (`MarketPoller`) refreshes public orders per `(region, type)` pair that a linked character trades. It reads every page through `EsiClient::market_orders` and stores them in `market_orders`, with the sync state in `market_sync`. The gate, backoff, claim/lease and scheduler tick come from `poller.rs`.
- `fees.rs` works out sales tax, broker fee and minimum margin per `(character, location)` of current orders. The station owner is only looked up for stations where a character has an order.
- TimescaleDB is already in use (`order_snapshots`, `wallet_balances`, `inventory_values` are hypertables; image `timescale/timescaledb:2.17.2-pg17`). Continuous aggregates, compression and retention policies are not used yet.
- `EventBus::publish(account, event)` only signals per account. Every page is htmx with fragments that are re-fetched on SSE signals. Charts use the bundled Chart.js, driven by `static/app.js` from data attributes, because of the CSP (no inline script).
- Character-scoped views parse `ViewQuery` (`chars`, per-table sort keys). Money is `numeric(20,2)` in the database and cents in Rust.

## Goals / Non-Goals

**Goals:**
- Data that does not depend on the account is computed once per refresh: snapshots, rollups, indicators and recurring windows. Each page request then applies only what depends on the account: fees, capital, exclusion, filters, sorting, top 50.
- All analysis maths are pure Rust functions with unit tests: ticks, fees, fill time, T_max, SMA, Bollinger, RSI, σ, recurrence, percentiles. SQL only collects and aggregates.

**Non-Goals:**
- Merging the per-pair `MarketPoller` with the region-wide book. Both run side by side for now, even though The Forge pairs are then read twice.
- Buy orders placed at other stations whose range reaches Jita 4-4, as competition or as a buyer.
- Hauling, multi-station, or structure markets.
- Leaving the account's own orders out of the competition count n.

## Decisions

### 1. Region-wide book: one poller, pages read in parallel
A new `station_book.rs` (`StationBookPoller`) reads `GET /markets/{region}/orders?order_type=all&page=N` through a new `EsiClient::region_orders_page(region, page)`. The first page gives `X-Pages`, then the remaining pages are read with a concurrency of 8 through the shared gate.

A refresh is only valid if every page has the same `Expires`. If ESI's cache turned over during the read, the refresh is retried at once, at most twice. Otherwise one book could mix two cache generations.

Sync state lives in `station_sync (region_id, station_id, kind 'book'|'history', next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures)`. It uses the same claim/backoff helpers as `market_sync`.

**Alternative:** fetching per type with `type_id`, as `MarketPoller` does. Rejected because it would be ~10k requests every 5 minutes instead of a few hundred.

### 2. Station book and snapshot written in one transaction
`station_orders (region_id, station_id, order_id PK, type_id, is_buy_order, price numeric(20,2), volume_remain bigint)` holds the current book of the station. A refresh does the following in one transaction:
1. Load the new orders into a temporary table with `unnest`.
2. `INSERT INTO station_snapshots … SELECT … GROUP BY type_id`:
   - best prices and spread
   - the within-1% units and orders, through a join on the best price per side
   - total units
   - units taken, as `sum(greatest(old.volume_remain − new.volume_remain, 0))` over orders present in both books
   - only for types that have both sides
3. Replace `station_orders`.
4. Record success.

After the commit, `OpportunitiesUpdated` goes to all accounts (§8).

`station_snapshots` is a hypertable on `recorded_at` with one-day chunks. Every figure is its own column, as the specs require. Compression is `segmentby = type_id`, `orderby = recorded_at`, after 1 day, and lossless. Retention is 30 days.

### 3. Rollups as hierarchical continuous aggregates
- `station_hourly`: a continuous aggregate over `station_snapshots`, with `time_bucket('1 hour')`. Each figure has its own min, max and avg column, the taken units are summed, and `count(*)` is kept as `snapshots`.
- `station_daily`: a continuous aggregate over `station_hourly`, with `time_bucket('1 day')` in UTC. It keeps min, max and avg the same way, plus `count(*) AS hours_covered`. The "covered day" rule (≥ 20 hours) is then a plain comparison.
- Policies:
  - hourly is refreshed every 15 minutes with an end offset of 1 hour
  - daily is refreshed every hour with an end offset of 1 day
  - both use `materialized_only = true`, so only completed periods are read
  - both have 30-day retention

**Alternative:** computing rollups in Rust after each refresh. Rejected because TimescaleDB does this incrementally and keeps the rollups consistent with the raw data.

### 4. History poller per item
`market_history (region_id, type_id, day date, average numeric(20,2), highest, lowest, volume bigint, order_count bigint, PK (region_id, type_id, day))` is a plain table of about 300k rows. A daily job deletes days older than 30.

`history_sync (region_id, type_id, next_poll_at, …)` works like `market_sync`:
- Tradable types without a row are inserted with `next_poll_at = now`.
- A success sets `next_poll_at` to ESI's `Expires`, which is after the next downtime.
- A failure backs off.
- A 404 or 400 for a type ESI has no history for sets `next_poll_at` to the next day.

The poller has a concurrency of 2 and relies on the gate's `X-Ratelimit` handling for pacing. Answers are upserted, keeping only the last 30 days. When a pass has stored new rows and no due items are left, it publishes `OpportunitiesUpdated` once.

### 5. Daily figures as a SQL view
`station_daily_figures` joins `market_history` with `station_daily` per `(type, day)`:
- `low`, `high` and `max_spread` come from `station_daily` where `hours_covered ≥ 20`, otherwise from history.
- `source` records which one.
- `average` and `volume` always come from history.

Indicators (SMA, Bollinger, RSI, σ, trend) use `average` and `volume` only, so the series never mixes transaction prices with book prices. Low, high and spread feed the recurring-expansion logic only.

### 6. Precomputed per-item statistics, account-specific maths per request
`station_item_stats (region_id, station_id, type_id PK…, computed_at, …)` holds everything that does not depend on the account:
- from the latest snapshot: best buy/sell, n per side
- ρ over 30 days
- V7 and V30
- σ, SMA 5/20 and the crossing day, Bollinger %B, the RSI 14 value
- the selected expansion window (kind, weekday, hour) and its target sell price, or none

It is recomputed by a pure Rust function at three points:
- book fields after every book refresh
- indicator fields after a history pass
- expansion fields when the hourly rollup advanced (checked each book refresh: the latest `station_hourly` bucket changed)

The opportunities request loads all rows (≈10k) plus the account's settings and fees. In Rust it computes c, r, m, U, the q values, T_fill, T_max, the minimum volume and ISK/day. It then applies the exclusion, the per-table selection, filters and sorting, and takes 50 rows per table. This takes well under 50 ms for 10k rows.

Recurrence (`analysis::recurring_windows`) takes an item's hourly rollups (or its daily figures in weekday mode) and returns the windows with their peaks. P25 uses linear interpolation between ranks.

**Alternatives:**
- Computing everything per request. Rejected because it would read ~7M hourly rows.
- Storing per-account results. Rejected because capital and filters change often and there are few accounts.

### 7. Fees at the built-in station
`fees::ensure_station_owner(station_id)` is called by the fee poller every tick for the built-in station (a no-op once known). A new `fees::at_location(characters, location_id)` reuses the existing fee SQL with a fixed location instead of the order locations. The opportunities module chooses the lowest minimum margin, falling back to base rates per character. The orders page's location list stays driven by current orders.

### 8. Broadcast signal
`EventBus` gets `publish_all(event)`. `InProcessBus` sends `(Target::All, event)`, and every subscriber stream lets `Target::All` through as well as its own account. The new `Event::OpportunitiesUpdated` is named `opportunities-updated`. **Alternative:** looping over all account IDs from the database. Rejected because it needs a query per refresh and misses nothing the broadcast would not.

### 9. Page, fragments and routes
| Route | What it does |
|---|---|
| `GET /opportunities` | the page |
| `GET /opportunities/freshness` | freshness fragment |
| `GET /opportunities/tables` | all four tables in one fragment, so one re-fetch keeps them consistent |
| `GET /opportunities/detail/{table}/{type_id}` | detail fragment with chart data in `data-*` JSON |
| `POST /opportunities/settings` | CSRF-checked; validates, stores, returns the tables fragment, or the filter bar with an error and HTTP 422 |

- Sort keys are per table in the query: `xs/xd`, `cs/cd`, `ms/md`, `ts/td`, added to `ViewQuery`.
- Settings are stored in `opportunity_settings (account_id PK → accounts ON DELETE CASCADE, min_margin numeric, min_isk_per_day numeric, search text, capital numeric)`, with defaults applied when there is no row.
- The page listens to `sse:opportunities-updated` on the tables and the freshness. The existing scroll-keeping code in `app.js` already handles re-fetched panels.

### 10. Floating detail window
- `<aside id="opportunity-detail" role="dialog" aria-modal="false">` is `position: fixed`: right-aligned below the top bar, max 40 vw wide, scrolling inside.
- A row click loads the detail fragment into it with htmx. It has no drag handlers.
- `app.js` handles:
  - closing on Esc and the × button
  - drawing charts from the fragment's data attributes with Chart.js, reusing the existing chart styling helpers
  - re-fetching the open window's URL on `opportunities-updated`
- If the item no longer qualifies, the fragment says "no longer listed" and still renders its figures from `station_item_stats` and history.

### 11. Testing seams
- The stub gets:
  - the region-wide orders variant on the existing path when no `type_id` is given, with pages of 1000
  - `/markets/{region}/history`
  - controls to set the region book, set history per type, and fail a page or a history type
- Test hooks (test builds only):
  - `/_test/station-book` runs one refresh
  - `/_test/station-history` runs history until nothing is due
  - `/_test/station-rollups` calls `refresh_continuous_aggregate` for both aggregates
  - `/_test/station-seed` inserts synthetic snapshots or history for past days, because Playwright cannot wait 7 days for hourly patterns
- Rust integration tests insert directly into the tables.

## Risks / Trade-offs

- [The Forge book is a few hundred pages every 5 minutes] → Pages are read in parallel within the gate, and a refresh that overruns the interval simply delays the next one. A failure keeps the old book.
- [~10k history requests per day may hit ESI rate limits] → The concurrency is low and the gate defers on 429/420. The pass spreads over hours after downtime, and the indicators use whatever history is stored.
- [Snapshot storage, ~85M rows over 30 days] → Compression is segmented by type, there is 30-day retention, and only tradable types are stored.
- [Units taken are a lower bound: completely filled orders vanish, just like cancelled ones] → They are only used for the side ratio ρ, not for volume. Volume comes from history.
- [Own low/high are book prices, while history low/high are transaction prices] → The mix is limited to the spread/peak logic, and each day records its source. Indicators use history only.
- [n counts the user's own orders as competition] → Accepted for now (Non-Goals). The own-order markers make it visible.
- [Hourly patterns need 7 days of own data after the deploy] → Weekday windows from daily figures work immediately, and the table shows the collecting progress.

## Migration Plan

1. Deploy the migrations: tables, hypertable, compression/retention policies, continuous aggregates. There is no backfill, so snapshots start with the first refresh.
2. On startup, the book poller runs at once. The history poller starts with every tradable type due, and fills 30 days over the first hours.
3. Contracts change, so before merging the stub fingerprint must be re-verified against real EVE with `esi-verify`.
4. Rollback: the migrations are forward-only. Rolling back means deploying the previous release plus a follow-up migration that drops the new tables, aggregates and policies. Nothing existing depends on them.

## Open Questions

- Settled by probing real ESI: a real type without market history in the region answers 200 with `[]`, an unknown type answers 404 ("Type not found!"). Both are handled as "no history, retry tomorrow".
