# Proposal

## Why

The orders tables show what a trader has listed, but not whether an order is still competitive. For station trading the question that matters is "am I the best price at my station, and if not, by how much am I undercut?". Today the trader has to open the market window in game for every order. Depends on `market-orders`.

## What Changes

- In both order tables, the **Location** and **Region** columns move to the end of the row (after "Time left"). They stay sortable.
- Two new columns after **Price**, in both tables:
  - **Standing**: "Top" (success colour) when no other order has a better price, otherwise "Undercut N" (error colour), where N is the number of orders with a strictly better price. A tie with the best price is "Top". "—" while there is no market data for the order.
  - **Behind by**: for an order that is not top, the per-unit price difference to the best price plus its percentage of the best price (for example "12.5k (1.3%)"), shortened with the exact value on hover. Empty for a top order, "—" without market data.
- What is compared, per side:
  - **Sell order**: other sell orders for the same item at the same station or structure.
  - **Buy order**: other buy orders for the same item in the same region that reach the station of the order: any order at the same station, a `region` range from anywhere, and a `solarsystem` or jump range from the same solar system. Jump-range orders in other systems are not counted. The tool is for station trading, so the order's own range is ignored and it is always compared at its own station.
  - The character's own other orders, and the orders of all linked characters, count as part of the market. Price is the only criterion: minimum volume and remaining volume are ignored.
- Both new columns use the existing success and error styles of the page's CSS framework, no colours of their own. "Standing" sorts by its text with numbers in natural order ("Top", "Undercut 1", "Undercut 2", … "Undercut 12", "—" last). "Behind by" sorts by its numeric difference.
- A new market poller fetches the public market orders from ESI for every (region, item) pair that the current orders of any linked character use, on its own schedule (every 5 minutes, never before ESI's cache expiry).
- When a poll changes the standing of an account's order, that account's open orders pages receive a new `market-updated` signal and re-render the tables, keeping filter and sorting.
- The ESI stub gains the market orders route with a control, and the contracts and the real-ESI verification cover the new call, as for every ESI call before.

## Capabilities

### New Capabilities
- `market-position`: the market data poller and its cadence, and how an order's standing and gap to the best price are determined from them.

### Modified Capabilities
- `orders-overview`: column order, the Standing and Behind by columns, and their sorting.
- `live-updates`: the `market-updated` signal and the orders page refreshing on it.
- `esi-contract`: the market orders call gets a contract, is served by the stub and is verified against real ESI.

## Impact

- New migration: tables for the cached public market orders and the per-pair market sync state.
- New outbound ESI call (`GET /markets/{region_id}/orders/`) behind the existing ESI client, gate and etiquette.
- `eve-stub`, `esi-contract` and `esi-verify` change, so **the stub fingerprint changes**. Every stub-backed test (Rust and Playwright) fails until the person who owns the EVE credentials runs `cargo run -p esi-verify` once the stub and contract are in place. The tasks put this step right after the stub work.
- `orders.rs` list query and view model, the orders templates and `app.js`/`app.css` (columns, badges), the event bus (a second event).
- The dashboard is not affected: it does not listen to `market-updated`.
