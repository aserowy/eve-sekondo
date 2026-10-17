# Tasks

## 1. ESI contracts and stub

- [x] 1.1 Add contracts for the region-wide market orders call (200 with `X-Pages`, plus each order's `type_id`, `location_id` and `volume_remain`) and for the market history call (200; plus whatever ESI answers for a type without history), together with the shared not-verifiable 420/429/5xx variants, in `esi-contract`. Verify: the contract unit tests pass, and the coverage test names the new routes until 1.2 lands.
- [x] 1.2 Add to `eve-stub`:
  - the region-wide variant of `/markets/{region_id}/orders` (no `type_id`, paged at 1,000)
  - `/markets/{region_id}/history`
  - controls to set the region book and the history per type, and to fail a chosen page or history type

  Verify: new `tests/esi_market.rs` cases cover several pages, a failing page, history per type and failure injection, and the coverage and conformance tests pass.
- [x] 1.3 Extend `esi-verify` with the first page of The Forge's region-wide book and the history of the item already used for the market orders check. Verify: `crates/esi-verify/tests/verify.rs` covers the history call, and the scrub test shows that no prices appear in the output.
- [x] 1.4 **Needs the user:** run the real-EVE verification. Settle the design's open question (how ESI answers a type without history) and adjust the contract variant if needed. Verify: `verified-stub.json` holds the new fingerprint, and the stub-backed tests start again.

## 2. ESI client

- [x] 2.1 Add `EsiClient::region_orders_page(region, page)` (returning orders with `type_id`, the `X-Pages` count and `Expires`) and `EsiClient::market_history(region, type_id)`, each with its own gate route and decoding. Verify: `tests/esi_client.rs` cases against the stub decode both answers, read `X-Pages` and `Expires`, and map 404/5xx/420/429 as for the other market calls.

## 3. Schema

- [x] 3.1 Add the migration (design §2–§5) for:
  - `station_sync`, `station_orders`, the `station_snapshots` hypertable with compression after 1 day and 30-day retention
  - `market_history`, `history_sync`, `station_item_stats`
  - `opportunity_settings` (cascading on account deletion)

  Verify: `tests/schema.rs` checks the tables, the hypertable, and the compression and retention policies, and that deleting an account removes its settings row.
- [x] 3.2 Add the continuous aggregates `station_hourly` (on snapshots) and `station_daily` (on hourly, with `hours_covered`), with their refresh and retention policies and `materialized_only`, plus the `station_daily_figures` view. Verify: `tests/schema.rs` cases show that:
  - an hour with best sell 110/130/120 rolls up to min 110, max 130 and avg 120
  - a day with 24 covered hours takes its low/high from own data
  - a day with 5 covered hours takes them from history, with the source recorded
  - average and volume always come from history

## 4. Analysis maths (pure functions)

- [x] 4.1 Implement `opportunities::analysis` prices and fees: valid prices and ticks (one tick below, rounded down), cost, revenue, m, U, and the choice of the lowest-fee character with the base-rate fallback. Verify: unit tests for 1,234,000 → 1,233,000, 9.99 → 9.98, 120.00 → 119.90, 150.37 → 150.30, and the fee-choice and base-rate scenarios.
- [x] 4.2 Implement the liquidity and holding maths: market share per side, ρ with its 0.5 fallback, q_buy/q_sell, T_fill, σ (needs ≥ 10 returns), T_max, the minimum daily volume, ISK/day and the hard exclusion. Verify: unit tests reproduce the worked example in the spec (T_fill ≈ 5.92, T_max = 3, excluded), plus the "volatile and thin" and "not enough history" scenarios.
- [x] 4.3 Implement the indicators: SMA 5/20, Bollinger %B, Wilder RSI 14, the SMA crossing within 3 days, and the 5- vs 20-day volume check. Verify: unit tests against hand-computed series, covering oversold, inside the band, a fresh crossing with volume, and a crossing without volume.
- [x] 4.4 Implement `recurring_windows`:
  - hour-of-day: 60% of the last 14 days, at least 7 days with data
  - weekday × hour: 3 of the last 4 weeks
  - weekday-only mode below 7 days of snapshots
  - peaks of the expanded occurrences, P25 with linear interpolation, one tick below
  - choice of the window with the highest S, and the next start in UTC

  Verify: unit tests for the "daily evening expansion", "too irregular" and "first days" scenarios.

## 5. Station sync

- [x] 5.1 Implement `StationBookPoller`:
  - claim, backoff and expiry handling on `station_sync`
  - parallel pages, with the "same `Expires` on every page" retry
  - the one-transaction snapshot insert and book replacement (design §2)

  Verify: `tests/station_book.rs` cases for:
  - a successful refresh keeping only Jita 4-4 orders
  - an order at another Jita station being dropped
  - the snapshot content scenario
  - units taken
  - a failing page keeping the book with no snapshot
  - no refresh before the cache expiry
- [x] 5.2 Implement the history poller on `history_sync`:
  - schedule newly tradable types
  - concurrency 2 through the gate
  - upsert the last 30 days and delete older ones
  - "no history" pushes the type to the next day

  Verify: `tests/station_history.rs` cases for a new tradable item getting 30 days, a failing item being retried while others update, and sell-only items not being fetched.
- [x] 5.3 Fill `station_item_stats` after a book refresh (book fields), after a history pass (indicator fields), and when the hourly rollup advanced (expansion fields), using the 4.x functions. Verify: `tests/station_stats.rs` cases show each set of fields updating only on its trigger and matching the pure-function results for seeded data.
- [x] 5.4 Add `fees::ensure_station_owner` for Jita 4-4 in the fee poller tick, and `fees::at_location`. Verify: `tests/fees.rs` cases for "character without orders in Jita" (fee at Jita known, the orders page lists only Amarr) and "owner looked up once".
- [x] 5.5 Add `EventBus::publish_all` and `Event::OpportunitiesUpdated` (`opportunities-updated`). Publish it after a successful book refresh and after a history pass that stored rows. Add the test hooks `/_test/station-book`, `/_test/station-history`, `/_test/station-rollups` and `/_test/station-seed`, and start both pollers in the app. Verify:
  - `events.rs` unit tests show the broadcast reaching two accounts
  - `tests/events_stream.rs` shows the signal after a refresh and none after a failed refresh
  - `tests/no_test_hooks.rs` still passes

## 6. Opportunities page

- [x] 6.1 Add `/opportunities` (page shell, station heading, freshness fragment with stale marking) and the "Opportunities" nav link, which carries `chars`. Verify:
  - a Rust test for the unauthenticated redirect and freshness staleness
  - Playwright `e2e/tests/opportunities.spec.ts` opens the page from the orders page with a narrowed selection and sees the four table titles and freshness
  - `navigation.spec.ts` is extended to the five-link walk with "Opportunities" marked current
- [x] 6.2 Implement the `/opportunities/tables` fragment for the classic spread and trend tables:
  - per-request maths from `station_item_stats` plus fees and settings
  - top 50 by ISK/day
  - shared columns, own buy/sell markers, `Type <id>` placeholders, per-table sort keys in `ViewQuery`

  Verify:
  - Rust tests for the classic row and "more than 50 matches"
  - Playwright tests: a seeded classic row shows S 119.90 and current best buy 100.00, sorting the trend table by σ keeps the other table's sorting and updates the URL, and an own sell order is marked
- [x] 6.3 Add the recurring-expansion and mean-reversion tables to the fragment, with their own columns (window and next start; %B and RSI) and the "Collecting hourly data (n/7 days)" note. Verify: Playwright tests with seeded snapshots and history show the "daily 18:00" row, the collecting note at 3 days, and an oversold row with S 150.30.
- [x] 6.4 Implement the filter bar and `POST /opportunities/settings`:
  - CSRF
  - validation with a 422 response and a message
  - per-account storage with defaults
  - the tables are re-rendered after a change

  Verify:
  - Rust tests for invalid capital, a missing CSRF token, and account isolation
  - Playwright tests for the defaults on first visit, raising capital removing a slow item and persisting after signing in again, and the name search narrowing every table

## 7. Detail window

- [x] 7.1 Add `/opportunities/detail/{table}/{type_id}` and the fixed, non-draggable `#opportunity-detail` aside:
  - htmx loading on a row click, replacing any open window
  - Esc and × close it
  - the shared figures block (fees, B, S, m, σ, T_max, fill time and their derivation)
  - "no longer listed" when the item has dropped out

  Verify: Playwright tests for open and close with Esc, a drag attempt leaving the window in place, replacement from another table, and no CSP violations.
- [x] 7.2 Add the per-table detail charts in `app.js` from `data-*` JSON, using the existing chart styling:
  - expansion: spread by hour and weekday, peaks with an S line
  - classic: best buy/sell and spread, and depth within 1%
  - mean reversion: average with SMA 20, Bollinger bands and RSI
  - trend: average with SMA 5/20 and volume

  Verify: Playwright tests use `e2e/support/charts.ts` to check that each window draws its charts with the seeded data points.

## 8. Live updates

- [x] 8.1 Wire `sse:opportunities-updated` to re-fetch the freshness, the tables and an open detail window, keeping `chars`, the filters, the per-table sorting and the scroll position. Verify: Playwright `live.spec.ts`/`scroll.spec.ts` cases:
  - a book refresh while the trend table is sorted by σ, the page is scrolled and a window is open: new figures show without a reload, with sorting, scroll and window kept
  - the orders page does not re-fetch on the signal

## 9. Integration

- [x] 9.1 Run the full suite (`cargo test --workspace`, Playwright `npm test` in `e2e/`) and `openspec validate station-trading-opportunities --strict`. Verify: all pass. With the local stack running, the book refresh time and the compressed chunk sizes after a day are checked manually against design §2 and the Risks.
