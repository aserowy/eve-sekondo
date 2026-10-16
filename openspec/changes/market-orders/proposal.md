# Proposal

## Why

The first job Sekondo should do is show a trader every active market order across all of their characters in one place, kept current automatically. In game, this means switching between characters and clients. Depends on `character-auth`.

## What Changes

- Background poller (in the same binary as the web server) that fetches personal market orders (`GET /characters/{id}/orders/`) for every linked character that does not need re-login.
- The refresh interval is configurable (default 20 minutes). A character is never polled before ESI's cache `Expires` time, polls are spread out across the interval, requests carry a descriptive `User-Agent`, and the poller backs off using ESI's error-limit headers.
- Every poll writes one row per order to a TimescaleDB hypertable (`order_snapshots`), so history accumulates from day one. Orders that close (filled, cancelled or expired) drop out of current views. Fill detection is out of scope.
- Item, location and region names are resolved through ESI and cached in the database. Structures that cannot be resolved show as "Unknown structure (id)".
- Live updates: an in-process event bus (behind a trait, so it can later move to Postgres LISTEN/NOTIFY) and an SSE endpoint per account that sends signals only (e.g. `orders-updated`). htmx re-fetches the affected fragments.
- Character filter: every view accepts `?chars=<ids>`, where a missing value means all characters. The filter can be bookmarked and is carried on every fragment request.
- Orders overview page with separate Sell and Buy tables showing character, item, location and region, price, volume remaining / total, value (sell) or escrow (buy), date issued and time left. Tables are sortable, large ISK numbers are shortened and show the exact value on hover, and the page shows "last updated / next update" and stale badges.
- Corporation orders are out of scope.

## Capabilities

### New Capabilities
- `order-sync`: polling ESI for character orders, cadence and ESI etiquette, snapshot storage, and name resolution with caching.
- `live-updates`: the event bus and the per-account SSE signal stream that drives htmx fragment refreshes.
- `character-filter`: selecting all characters or a subset through the URL, applied consistently across views.
- `orders-overview`: the Sell and Buy order tables and how they are presented.

### Modified Capabilities

## Impact

- New TimescaleDB hypertable and tables for current orders and the name cache.
- Uses the htmx SSE extension. Adds an outbound ESI client with rate and error-limit handling.
- The ESI stub gains order and universe-name endpoints for tests.
- `orders-dashboard` builds on `order-sync`, `live-updates` and `character-filter`.
