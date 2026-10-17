# Proposal

## Why

Sekondo shows how a trader's existing orders are doing, but not what is worth trading next. Station traders pick items by reading spreads, volume and price history. In Jita that means thousands of items, far too many to do by hand. Sekondo already talks to ESI, works out each character's fees and stores time series in TimescaleDB. It can screen the Jita 4-4 market with established trading analyses and show what to buy.

## What Changes

- A new public **station book sync** for one built-in station, Jita IV - Moon 4 - Caldari Navy Assembly Plant (60003760, The Forge):
  - Every ESI cache expiry (~5 min) it reads the whole The Forge order book and keeps the orders placed at the station.
  - For every item with both a buy and a sell order there, it records one **top-of-book snapshot** per refresh. Each snapshot keeps every figure in its own column: best buy, best sell, units and order count within 1% of the best price on each side, total units per side, and the units taken from each side since the last refresh.
  - Hourly and daily rollups keep min, max and average of each figure in separate columns. Raw snapshots are compressed after one day. Everything older than 30 days is dropped.
  - Once a day after downtime, it reads ESI's **market history** of each of those items for The Forge and keeps 30 days. Daily low/high come from our own snapshots, and history fills in only the days our snapshots are missing. Traded volume always comes from history.
  - Storage is keyed by region and station, so more hubs can be added later. There is no station selector yet.
- A new **Opportunities** page `/opportunities`, signed in only, with one table per analysis on one scrolling page:
  1. **Recurring spread expansion**:
     - Finds hour-of-day or weekday × hour windows in which the spread is regularly ≥ 1.5× its 30-day median: on ≥ 60% of the last 14 days for daily windows, in ≥ 3 of the last 4 weeks for weekly ones.
     - Until 7 days of snapshots exist, only weekday windows from the daily figures are found.
     - The target sell price is the 25th percentile of past peaks in that window, one tick below. The table also shows when the next window starts.
  2. **Classic spread trade**: sell at best sell − one tick.
  3. **Mean reversion**: price below the lower Bollinger band (20, 2σ) or RSI 14 < 30. The target is a return to SMA 20.
  4. **Trend**: SMA 5 has recently crossed above SMA 20, with rising volume.
- What every table has in common:
  - Margins are calculated against the **current best buy**. No buy price is suggested.
  - Fees are those of the lowest-fee selected character at Jita 4-4, with base rates as the fallback.
  - Every row shows liquidity (7 and 30 days), volatility, competition, time-to-fill and estimated ISK/day.
  - A **hard exclusion** drops items that would not sell fast enough for their volatility: allowed holding time T_max = min(3 days, (net margin / 2σ)²). The item must be able to fill an order of "capital per item" within T_max, given a market share of 1 / (competing orders within 1% + 1) per side.
  - Rows are marked when the account already has a buy or sell order for the item at the station.
- A shared filter bar is stored per account: minimum net margin (5%), minimum ISK/day (5M), item name search, capital per item (100M ISK). Each table shows its top 50 rows, sorted by its own key, and every column can be sorted.
- Clicking a row opens a non-modal **floating detail window** at a fixed position, which cannot be dragged. Its content depends on the table: spread by hour/weekday with past peaks, spread history and book depth, Bollinger/RSI chart, or SMA/volume chart. It always shows the fees used and how T_max was worked out. Esc or × closes it, and it refreshes on live updates.
- A new `opportunities-updated` live signal after every station book refresh and history run. Only the Opportunities page listens to it.
- The navigation gains an "Opportunities" link that carries the character selection.
- Fees: the owner of Jita 4-4 is always known, even when no character has an order there.
- New ESI contracts, stub routes and real-EVE verification for the region-wide order book and the market history call.

## Capabilities

### New Capabilities
- `station-market-sync`: reading the region-wide order book for the built-in station, top-of-book snapshots and their rollups, daily market history, how our daily figures and history are merged, and 30-day retention.
- `station-opportunities`: the Opportunities page, its four analysis tables, the shared figures and the hard exclusion, the per-account filter settings, sorting, and the floating detail window.

### Modified Capabilities
- `navigation`: adds the "Opportunities" link.
- `live-updates`: adds the `opportunities-updated` signal.
- `fee-sync`: the owner and faction of the built-in trading station are always known, and characters get fees there without having an order at it.
- `esi-contract`: the covered calls now include the public order book of a whole region (all items) and the market history of a region for one item.

## Impact

- **Code**:
  - `esi.rs`: region-wide orders and history calls
  - a new station book poller and a history poller next to `market.rs`
  - a new `opportunities` module (analysis maths as pure functions, page and fragments)
  - `fees.rs` (Jita 4-4 owner), `events.rs`, `app.rs` routes, the nav template
  - `static/app.js` (floating window, detail charts with the bundled Chart.js), `app.css`
- **Database**: new migrations for the current station book, the snapshot hypertable (compression and 30-day retention), hourly and daily continuous aggregates, market history, sync state, and per-account opportunity settings (cascading on account deletion).
- **ESI load**:
  - a few hundred paged public requests every ~5 minutes for The Forge
  - one history request per item per day (on the order of 10k), paced through the existing rate-limit gate
  - no new scopes, so no re-login
- **Stub / contracts**: `eve-stub` gains the history route, the region-wide order book variant and controls to seed them. New contracts change the fingerprint, so a new real-EVE verification run is needed.
- **Storage**: roughly 10k items × 288 snapshots/day × 30 days ≈ 85M raw rows, kept small by compression.
- **Tests**: Rust tests for the analysis maths, the sync and the queries. Playwright tests for the page, its tables, filters, the detail window, navigation and live refresh.
