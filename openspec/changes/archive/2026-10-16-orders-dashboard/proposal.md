# Proposal

## Why

The orders table answers "what exactly do I have listed?". A trader also needs to see at a glance how much ISK is tied up: how much is listed for sale and how much is locked in buy-order escrow, across all characters or a chosen subset. They also need to see how that has developed over time and where it is concentrated. Depends on `market-orders`.

## What Changes

- The stub dashboard is replaced by a dashboard page with summary tiles:
  - total sell order value (sum of `price * volume_remain`)
  - total buy order escrow (sum of `escrow`)
  - number of sell orders and buy orders
  - corporation orders are included in the totals, and each amount tile also shows the share placed on behalf of the corporation
- A per-character breakdown table with the same figures, plus each character's last and next update, a stale badge, and a "Log in again" link when the character needs re-login. Stale characters still count with their last known orders.
- Graphs on the dashboard:
  - **Value over time**: total sell value and buy escrow as lines.
  - **Order count over time**: number of sell and buy orders as lines.
  - **Per-character split**: sell value and escrow per selected character as bars (current data).
  - **Top items**: the ten items with the most ISK in sell orders and the ten with the most in escrow, as bars (current data).
  - The over-time graphs cover a selectable range of 24 hours, 7 days (default), 30 days or 90 days, kept in the URL as `range`.
- Every poll now records its per-character totals, including polls that return no orders. History then drops to zero when all orders close, instead of keeping the last value. Existing snapshots are backfilled into this history.
- Respects the `?chars=` character filter (with its own filter control) and updates live through the existing SSE signals, graphs included.
- Large ISK numbers are shortened and show the exact value on hover, in the graphs too.

## Capabilities

### New Capabilities
- `dashboard`: aggregated sell value, buy escrow, order counts, the corporation share, the per-character breakdown, freshness indicators, and the graphs.

### Modified Capabilities
- `navigation`: the "Signed-in home" requirement no longer describes a placeholder page saying the dashboard is coming.
- `order-sync`: every successful poll records the character's totals as history, including polls with no orders.

## Impact

- New page and fragment handlers that read aggregates of `current_orders`, `order_sync` and the new poll-totals history.
- New migration: a `order_poll_totals` TimescaleDB hypertable written in the poll transaction and backfilled from `order_snapshots`. Polls in the past that returned no orders cannot be recovered.
- New vendored front-end dependency: a JavaScript chart library served from `/static`. It must work under the existing CSP (`script-src 'self'; style-src 'self'`).
- The character filter script currently always targets `/orders`. It must target the page the filter is on and keep `range`.
- The existing stub-dashboard tests (Rust and Playwright `navigation.spec.ts`) change with the page.
- Reuses `character-filter`, `live-updates` and the Playwright/ESI stub test harness.
