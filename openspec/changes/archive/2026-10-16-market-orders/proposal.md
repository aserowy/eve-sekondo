# Proposal

## Why

The first job Sekondo should do is show a trader every active market order across all of their characters in one place, kept current automatically. In game, this means switching between characters and clients. Builds on `character-auth`, which already provides linked characters, valid ESI access tokens on demand and the needs-re-login state.

## What Changes

- Background poller (in the same binary as the web server) that fetches personal market orders (`GET /characters/{id}/orders`) for every linked character that does not need re-login.
- The refresh interval is configurable (default 20 minutes). A character is never polled before ESI's cache `Expires` time, polls are spread out across the interval, requests carry a descriptive `User-Agent`, and the poller backs off using ESI's error-limit and rate-limit headers.
- Every poll writes one row per order to a TimescaleDB hypertable (`order_snapshots`), so history accumulates from day one. Snapshots are kept uncompressed and without a retention window for now. Orders that close (filled, cancelled or expired) drop out of current views. Fill detection is out of scope.
- Orders a character placed on behalf of their corporation (returned by the same endpoint) are shown with a "Corp" marker. Fetching the corporation's own order book (corporation endpoints and roles) is out of scope.
- Item, location and region names are resolved through ESI and cached in the database. Structures that cannot be resolved show as "Unknown structure (id)".
- Live updates: an in-process event bus (behind a trait, so it can later move to Postgres LISTEN/NOTIFY) and an SSE endpoint per account that sends signals only (e.g. `orders-updated`). htmx re-fetches the affected fragments.
- Character filter: every view accepts `?chars=<ids>`, where a missing value means all characters. The filter can be bookmarked and is carried on every fragment request. It never selects a character of another account.
- Orders overview page with separate Sell and Buy tables showing character, item, location and region, price, volume remaining / total, value (sell) or escrow (buy), date issued and time left. Tables are sortable, large ISK numbers are shortened and show the exact value on hover, and the page shows "last updated / next update" and stale badges.
- Navigation between the signed-in pages, and an empty stub dashboard at `/dashboard` that becomes the signed-in home page: login and `/` land there instead of on the characters page. `orders-dashboard` fills it in later.

## Capabilities

### New Capabilities
- `order-sync`: polling ESI for character orders, cadence and ESI etiquette, snapshot storage, and name resolution with caching.
- `live-updates`: the event bus and the per-account SSE signal stream that drives htmx fragment refreshes.
- `character-filter`: selecting all characters or a subset through the URL, applied consistently across views.
- `orders-overview`: the Sell and Buy order tables and how they are presented.
- `navigation`: the signed-in home page (an empty dashboard stub for now) and the navigation between signed-in pages.

### Modified Capabilities
- `character-auth`: logging in (first login or a returning character) lands on the dashboard instead of the characters page. Adding a character still returns to the characters page.

## Impact

- First TimescaleDB use: a migration enables the extension and adds the snapshot hypertable plus tables for current orders, per-character sync state and the name cache.
- Vendors the htmx SSE extension. Adds an outbound ESI client with rate and error-limit handling, and new configuration for the poll interval and the `User-Agent` contact.
- The ESI stub (`eve-stub`) gains order and universe-name endpoints, plus controls for ESI failure modes.
- The Playwright login helper expects the dashboard after login instead of the characters page.
- `orders-dashboard` builds on `order-sync`, `live-updates`, `character-filter` and `navigation`, and replaces the stub dashboard page.

## Verification against real ESI (mandatory)

The stub's ESI and SSO answers are only beliefs about EVE; the first real poll failed on one of them
(sell orders carry no `min_volume`). Checking every ESI and SSO call this change uses against the real
services whenever the stub changes is **mandatory**: stub-backed tests must not run against a stub that
has not been verified against real EVE. It is delivered by the change `esi-contract-verification`;
task 10.2 stays the manual first check.
