# Proposal

## Why

Sekondo shows what open sell orders will earn, but not what a trader actually bought and sold, or what the completed sales earned. The wallet transactions are already polled every hour, but only purchases are kept, and only as cost lots for the orders page. A trader still has to open the game's wallet to see recent trades, and the wallet doesn't show any profit.

## What Changes

- New **Trades** page (`/trades`) in the navigation, between Wealth and Opportunities. It is a window page with the character filter in the filter box and a collapsible freshness column.
- The page has one table with one row per personal wallet transaction (buys and sells), newest first. Its columns are: date (EVE time), character, item, location, buy/sell, quantity, unit price, **value** (quantity × unit price, before fees) and **margin**.
- The table panel's title has the time range control (`24h`/`7d`/`30d`/`90d`, default `7d`). Only transactions in the range are listed. A totals row shows the bought value, the sold value and the sum of the margins that are known.
- The transactions poll now also stores **personal sales**, kept permanently like purchases. Corporation transactions are still not stored. Existing characters get one full read back after the deploy, so that sales still in ESI's window (about 30 days) are filled in.
- Each sale gets a **cost by its own FIFO**. Per character and item, sales take units from the oldest purchase lots bought at or before the sale that earlier sales have not taken. The ties are broken by transaction ID. This runs separately from the existing sell-order allocation, which is unchanged.
- **Margin of a sale** = `Q × P × (1 − t − b) − cost`, using the character's current sales tax and broker fee at the sale's location, as for order margins. It is rounded half up to the cent and may be negative. It reads "—" when:
  - the cost covers only part of the units (hover: "cost known for X of Y units");
  - the fees are not known.
  
  Buys have no margin.
- Known simplification: every sale is charged the broker fee, also an instant sale into someone else's buy order, where none is paid. A transaction does not say which kind of sale it was.
- Live refresh: a transactions poll that stores a new transaction sends a new `trades-updated` signal. The Trades page re-fetches on it and on `fees-updated`. The `orders-updated` signal on new purchases stays.

## Capabilities

### New Capabilities
- `trades-overview`: the Trades page. Covers the transactions table, its columns, range, totals, freshness and live refresh.

### Modified Capabilities
- `trade-margin`: the poll also stores personal sales and reads back fully once. Adds the FIFO of sales and the margin of a sale. Sales are deleted with the character.
- `fee-sync`: station owners are also looked up for NPC stations where a stored sale was made.
- `navigation`: adds the Trades link.
- `ui-layout`: the Trades page is a window page with a freshness column, a warning count and a filter box, and its table scrolls inside its panel.
- `live-updates`: new `trades-updated` signal.

## Impact

- **Code**:
  - `wealth_poller.rs` (store sales, stop condition over purchases and sales, one-time full read, signal)
  - `trade.rs` (sale allocation and recompute)
  - `fees.rs` (rates for sale locations)
  - new `trades_page.rs` with templates `trades.html`, `trades_table.html` and `trades_freshness.html`
  - `app.rs` routes
  - `base.html` navigation
  - `events.rs`
- **Database**: a migration adds `sales` and `sale_costs` (both cascade with the character) and a `full_read` flag on `wealth_sync` for the one-time read back.
- **ESI**: no new routes or fields; `EsiTransaction` already carries `is_buy`/`is_personal`. Only the first poll after the deploy reads more pages.
- **Tests**: Rust tests for the sale FIFO, margin SQL, poll and page, plus Playwright tests for the Trades page, navigation, layout and live refresh.
