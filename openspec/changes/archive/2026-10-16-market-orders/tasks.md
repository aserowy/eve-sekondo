# Tasks

## 1. Schema and configuration

- [x] 1.1 Add the migration from design §1: the `timescaledb` extension, `current_orders`, the `order_snapshots` hypertable (7-day chunks), `order_sync` and `names`, with every per-character table referencing `characters ON DELETE CASCADE`. Verify with `tests/schema.rs` tests that:
  - the migration runs on a fresh test database
  - `order_snapshots` is listed in `timescaledb_information.hypertables`
  - deleting a character removes its rows from all three per-character tables
- [x] 1.2 Add `ORDER_POLL_INTERVAL_MINUTES` (default 20, 1–1440) and the optional `ESI_CONTACT` to `Config` and `.env.example`. Verify unit tests for the default, 0, 1441 and a non-number (each rejected with the variable named), and that `env_example_tests` passes

## 2. ESI stub

- [x] 2.1 Add `GET /characters/{id}/orders` to `eve-stub`. It requires a stub-issued bearer token for that character with the orders scope (403 otherwise), returns fixture orders with `Expires`, error-limit and rate-limit headers, and adds the `/_stub/orders` and `/_stub/esi-expires` controls and per-route call counters. Verify with stub integration tests for a valid token, a token of another character, a token without the scope, replaced orders and a custom `Expires`
- [x] 2.2 Add `POST /universe/names` (a fixture table; any unknown ID gives 404 for the whole request) and `GET /universe/structures/{id}` (authenticated; 403 for IDs on the deny list), plus the `/_stub/structure-access` and `/_stub/esi-fail-next` (5xx, 420 or 429 with headers) controls, all cleared by `reset`. Verify with stub integration tests for each route and control

## 3. ESI client

- [x] 3.1 Implement `EsiClient` (orders, names, structures) on the shared `reqwest::Client`, with the `User-Agent` from design §3, `X-Compatibility-Date`, and `Expires` converted using ESI's `Date` header. Map responses to `EsiError`. Verify with integration tests against the stub that orders, names and structures parse, that the `User-Agent` names Sekondo, its version, the public URL and the contact, and that 403, 404 and 5xx map as designed
- [x] 3.2 Implement `EsiGate`: a global block on error limit below 20 or on 420, a per-group block on 429 with `Retry-After`, and "deferred" instead of a request while blocked, all driven by the injected clock. Verify with unit tests of the header parsing and with stub-backed tests that no request is sent during a block and that requests resume after it

## 4. Order sync

- [x] 4.1 Implement a single-character poll: get the token through `TokenService`, fetch the orders, replace `current_orders` and insert the snapshot rows in one transaction, and update `order_sync`. Keep corporation orders flagged. Verify integration tests for the `order-sync` scenarios "Orders are fetched", "An order closes", "An order changes", "Corporation order", "Snapshot per order and poll", "Snapshots are removed with the character" and "Status after a successful poll"
- [x] 4.2 Add the failure paths to the poll: `NeedsRelogin` (no ESI call, orders kept), temporary token and ESI failures (orders kept, failure recorded, exponential retry capped at the interval), deferred by the gate (rescheduled, not a failure), and the character unlinked mid-poll (dropped without error). Verify an integration test per path, using `/_stub/revoke`, `/_stub/fail-next-token` and `/_stub/esi-fail-next`
- [x] 4.3 Implement name resolution (design §4): batched `/universe/names` with halving on 404, structures with the polling character's token, `NULL` name with `retry_after = +24h` on 403 or 404, and nothing cached on temporary failure. Verify integration tests for "Names are resolved once" (stub call counter), "Inaccessible structure", a batch containing one invalid ID, and a retry after a temporary failure
- [x] 4.4 Implement the scheduler loop (design §2): claim due characters, bounded concurrency, `next_poll_at = max(interval, Expires) + jitter` with hash-based first offsets, skip `needs_relogin`, and start it from `main::serve`. Verify with `TestClock` integration tests for "Next poll waits for the interval", "Next poll waits for ESI's cache expiry", "Polls are spread out", "Character needing re-login is skipped", and a re-login picking polling back up
- [x] 4.5 Add the `POST /_test/poll/{id}` test hook (own characters only, `test-hooks` feature), and verify integration tests that it polls an own character and returns 404 for a foreign one

## 5. Live updates

- [x] 5.1 Implement the `EventBus` trait and `InProcessBus` (filtered per account; lag mapped to `OrdersUpdated`), and publish from the poll after a success, a status change or needs-re-login. Verify unit tests for account isolation and lag handling, and an integration test that a poll publishes to the right account only
- [x] 5.2 Implement `GET /events`: SSE, 401 without a session, 15 s keep-alive, and closing the stream once the session is gone or past its lifetime (re-checked per tick). Verify integration tests (with a shortened tick) for "Signal after a poll", "Signals stay within the account", "Unauthenticated stream request", "Signals carry no data" and "Sign out in another tab"

## 6. Navigation and landing

- [x] 6.1 Add the `nav` partial (Dashboard, Orders, Characters, Sign out; current page marked), the stub `/dashboard` page, and redirects to `/dashboard` from `/`, from `/auth/login` when signed in, and from the login callback (adding a character still returns to `/characters`). Update the Rust callback tests and the Playwright `loginWith` helper for the new landing. Verify the `navigation` scenarios and the modified `character-auth` scenarios with integration tests and a Playwright test `navigation.spec.ts` (navigating between pages, sign out from the nav, the stub dashboard linking to orders), and verify the existing e2e suite still passes

## 7. Character filter

- [x] 7.1 Implement `CharacterSelection::resolve` and the `ViewQuery` parsing, used by every account-scoped query. Verify unit tests for missing, empty, subset, foreign, malformed and mixed `chars`, and an integration test that a foreign ID never yields that character's data
- [x] 7.2 Add the filter control partial (checkboxes; all checked means no `chars`; `hx-push-url`; includes the sort fields) and make nav links carry `chars`. Verify with a Playwright test `filter.spec.ts` for "Narrow the selection", "Bookmark", "Selection carries across pages" and a foreign ID in the URL

## 8. Orders overview

- [x] 8.1 Implement and unit-test the formatting helpers: shortened ISK with the exact `title` value, time left in whole days and hours, and UTC dates
- [x] 8.2 Implement `GET /orders` and the `/orders/tables` fragment: sell and buy tables with the columns from the spec, the "Corp" marker, name placeholders (`#id`, `Unknown structure (id)`), empty states, and whitelisted SQL sorting by `sort`/`dir` and `bsort`/`bdir` (default item name ascending). Verify integration tests for "Sell and buy orders are separated", "No orders", "Only own orders", "Unauthenticated access" and the sort whitelist, and a Playwright test `orders.spec.ts` (log in, poll through the hook, see sell and buy rows with correct values, sort by price and reverse it, hover the exact ISK value) with no CSP violations
- [x] 8.3 Implement the freshness strip fragment: last and next update per selected character, stale after twice the interval, needs re-login with a "Log in again" link, and "waiting for first update". Verify integration tests for the four `Freshness` scenarios, and a Playwright test that revokes a character's refresh token through the stub, polls, and sees the stale badge while the orders stay listed

## 9. Live refresh in the browser

- [x] 9.1 Vendor `htmx-ext-sse` (the 2.x release matching htmx 2.0.4) into `static/`, wire `sse-connect` on the orders page, and refresh the tables and freshness strip on `sse:orders-updated` and on reconnect, keeping `chars` and the sorting. Verify with a Playwright test `live.spec.ts` for "Table refreshes after a poll", "Filter and sorting are kept" and "Missed signals after a reconnect" (poll while offline using `context.setOffline`), with no CSP violations

## 10. Documentation and integration check

- [x] 10.1 Update `README.md` (what the poller does, the new settings, the ESI stub controls and the `/_test/poll` hook), and verify that the documented commands and settings work from `.env.example`
- [x] 10.2 Run a manual check against real ESI with a registered application and a character that has sell, buy and structure orders: orders appear within one interval, names resolve (record any inaccessible structure), and the `Expires`, error-limit and rate-limit headers match what `EsiGate` parses. Record any differences as stub or gate fixes
  - Result (2026-10-16, compose app against real EVE, one character): the poll stored 2 sell and 5 buy orders without error. All 9 names resolved (7 types, 1 station, 1 region); no inaccessible structure. `next_poll_at` was ESI's `Expires` plus jitter, as designed.
  - Differences found, both already fixed through `esi-contract-verification`: ESI leaves `is_buy_order` (and `min_volume`) out of sell orders, and sends no `X-Ratelimit-*` headers on the orders, names and structures routes (only a 429 may carry them).
  - Not covered by the poll: the character had no structure orders. Structure lookups were verified by `esi-verify` instead (`structures/found`, `structures/forbidden`).
  - Since ESI caches orders for about 20 minutes and polls never run before `Expires`, intervals below 20 minutes don't speed up polling against real ESI.
