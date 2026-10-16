# Proposal

## Why

The orders table answers "what exactly do I have listed?". A trader also needs to see at a glance how much ISK is tied up: how much is listed for sale and how much is locked in buy-order escrow, across all characters or a chosen subset. Depends on `market-orders`.

## What Changes

- Dashboard page with summary tiles:
  - total sell order value (sum of `price * volume_remain`)
  - total buy order escrow (sum of `escrow`)
  - number of sell orders and buy orders
- A per-character breakdown table with the same figures.
- "Last updated / next update" indicator and stale badges for characters that need re-login.
- Respects the `?chars=` character filter and updates live through the existing SSE signals.
- Large ISK numbers are shortened and show the exact value on hover.
- Historical charts are out of scope (snapshot data is already being collected).

## Capabilities

### New Capabilities
- `dashboard`: aggregated sell value, buy escrow, order counts, the per-character breakdown, and freshness indicators.

### Modified Capabilities

## Impact

- New page and fragment handlers that read the latest order data. No new external dependencies.
- Reuses `character-filter`, `live-updates` and the Playwright/ESI stub test harness.
