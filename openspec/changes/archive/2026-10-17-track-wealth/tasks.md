# Tasks

## 1. Scopes

- [x] 1.1 Extend `oauth::REQUIRED_SCOPES` with `esi-wallet.read_character_wallet.v1` and `esi-assets.read_assets.v1`, and make the stub's tokens carry the requested scopes. Verify: `tests/login_start.rs` asserts exactly the four scopes, a new `tests/tokens.rs` case shows that a two-scope token refresh marks "needs re-login", and logging in again with four scopes clears it.
- [x] 1.2 Update `e2e/tests/relogin.spec.ts` with a character linked with the old two scopes. Verify: the Playwright test shows the "needs re-login" badge on the characters page, and logging in again clears it.

## 2. ESI contracts and stub

- [x] 2.1 Add contracts for wallet (200, 403), assets (200 with `X-Pages`, 403) and market prices (200), plus the shared not-verifiable 420/429/5xx variants, in `esi-contract`. Verify: the contract unit tests pass, and the coverage test fails for the new routes until 2.2 lands.
- [x] 2.2 Add the stub routes `/characters/{id}/wallet/`, `/characters/{id}/assets/` (paged at 1,000) and `/markets/prices/`, with the controls `POST /wallet`, `/assets` and `/prices` and scope checks. Extend `esi-fail-next`/`esi-expires` to the new routes. Verify: new `eve-stub` tests (`tests/esi_wealth.rs`) cover pages, 403 without the scope and failure injection, and the contract coverage and conformance tests pass.
- [x] 2.3 Extend `esi-verify` to call wallet, assets and prices, and to fail with "an asset is needed" on an empty asset list. Verify: `crates/esi-verify/tests/verify.rs` covers the empty-assets failure, and the scrub test shows that no balance or asset value appears in output.
- [x] 2.4 **Needs the user:** run the real-EVE verification with a character that has ISK, at least one asset, and buy and sell orders. While running it, list a stack and confirm that the listed units are absent from the assets answer. Verify: `verified-stub.json` holds the new fingerprint, and the stub-backed tests start again. The listed-stack check was not done and is documented as a possible double count in design.md (Risks).

## 3. ESI client

- [x] 3.1 Add `EsiClient::wallet`, `assets` (paged) and `prices`, with `EsiAsset`/`EsiPrice` decoding and gate route names. Verify: `tests/esi_client.rs` cases against the stub decode each answer, read every page of assets, carry `Expires`, and map 403/5xx/420/429 as for orders.

## 4. Schema and shared modules

- [x] 4.1 Add the migration for `wallet_balances`, `current_assets`, `item_prices`, `price_sync`, `inventory_values` and `wealth_sync` (hypertables, indexes and cascades as in design §2). Verify: `tests/schema.rs` checks that the tables exist, that the hypertables are hypertables, and that unlinking a character removes its rows from every new table (`tests/unlink.rs`).
- [x] 4.2 Move name resolution from `Poller` into a shared `names` module, and move `Range` and the bucket ends from `dashboard.rs` into a `range` module, without changing behaviour. Verify: `tests/poll.rs`, `tests/dashboard.rs` and `cargo test` pass unchanged.

## 5. Wealth polling

- [x] 5.1 Implement `WealthPoller` scheduling (`schedule_new`, `claim_due` per kind, spread and jitter, skipping "needs re-login") and the wallet poll and store (balance row plus `wealth_sync`, next poll at `max(interval, Expires)`, failure and deferral handling). Verify: new `tests/wealth_poll.rs` cases for a balance after a poll, the re-login skip, a failed poll keeping the balance, and a wallet failure not affecting the order poll.
- [x] 5.2 Implement the assets poll: top-level location resolution (containers, ships in space, structures, cycle guard, unknown), replacing `current_assets`, the in-transaction `inventory_values` row and the one-hour minimum, with an index-safe store. Verify: a unit test for location resolution, plus `tests/wealth_poll.rs` cases for two pages, an item being gone, a failure on page 2 keeping the assets, and the next poll lying at least 1 hour out.
- [x] 5.3 Resolve names for asset types and locations after the assets poll through the shared `names` module. Verify: `tests/wealth_poll.rs` shows a station name, a solar system name and "Unknown structure (<id>)" for a 403 structure.
- [x] 5.4 Implement `PricePoller` (daily, honouring `Expires`, failure keeping prices), which writes `inventory_values` rows for all characters with assets in the same transaction. Verify: new `tests/prices.rs` cases for a single fetch for two accounts, a failed refresh keeping prices, a doubled price giving a doubled inventory entry, and blueprint copies and missing prices adding nothing.
- [x] 5.5 Add `Event::WealthUpdated`, publish it after the wallet and assets polls and per affected account after a price refresh, and add the test hooks `/_test/wealth-poll/{id}/{kind}` and `/_test/price-refresh`. Start both pollers in the app. Verify: `events.rs` unit tests cover the name and account scoping, `tests/events_stream.rs` shows the signal after a wallet poll and none for another account, and `tests/no_test_hooks.rs` still passes.

## 6. Wealth queries

- [x] 6.1 Implement `wealth::current` (per-character wallet, personal sell and escrow, inventory, total) and `wealth::history` (carry-forward over three sources, personal orders only). Verify: new `tests/wealth.rs` cases for the 200m all-components example, the corp exclusion, assets not fetched yet, components recorded at different times, the last point equalling current, no history, and foreign account data never counting.
- [x] 6.2 Implement the inventory list query (grouped by character, item and top location; sort whitelist; unpriced last; unpriced count; total). Verify: `tests/wealth.rs` cases for the two-containers merge, a row per character, default value-descending order with unpriced last, sorting by item, and a foreign account excluded.

## 7. Wealth page

- [x] 7.1 Add `/wealth` with the page, the filter control, `/wealth/freshness` and the "Wealth" nav link (which carries `chars` and never `range`). Add `data-default-sort` support to the filter script. Verify: a Rust test for unauthenticated redirect and freshness staleness (3-hour-old assets, re-login), plus a Playwright `e2e/tests/wealth.spec.ts` test that navigates there from the dashboard with a narrowed selection, sees freshness, and gets no CSP violations. The existing `filter.spec.ts` passes unchanged.
- [x] 7.2 Add the `/wealth/items` fragment: the inventory table with sortable headers, ISK shortening with hover, "—" for unpriced rows, the unpriced count, the total, the empty state and `#<type id>` placeholders. Verify: Playwright tests in `wealth.spec.ts` for default sorting, sorting by item with the URL reflecting it, a blueprint copy row showing "—", and the hover showing the exact amount.
- [x] 7.3 Add the `/wealth/graph` fragment with the stacked-area chart kind in `app.js`, the tooltip with the four values and the total, colours picked with the `dataviz` skill, and the page's own range control targeting the graph only. Verify: Playwright tests for the stack's hover total of 200m, switching to 30d updating the URL and graph while the list stays unchanged, `?range=1y` falling back to 7d, coming from `/dashboard?range=30d` showing 7d, and "no history yet".
- [x] 7.4 Live refresh of all wealth fragments on `wealth-updated`, `orders-updated` and reconnect, keeping the selection, range and sorting. Verify: Playwright tests in `e2e/tests/live.spec.ts` for a wallet poll updating the last wallet point, the 24h + item-sort + one-character state being kept, and `market-updated` not causing a re-fetch.

## 8. Dashboard

- [x] 8.1 Add the wealth tile to the summary fragment, linked to `/wealth` with `chars`. Verify: a `tests/dashboard.rs` case for the tile total across two characters, and a Playwright case in `dashboard.spec.ts` for "250m" with the exact hover and the link carrying `chars`.
- [x] 8.2 Add the total-wealth line graph to the summary fragment, driven by the existing range control, and add `sse:wealth-updated` to the fragment trigger. Verify: Playwright cases for one 30d click updating the value, count and wealth graphs, the wealth line stepping from 10m to 15m, the last point equalling the tile, a wallet poll refreshing the tile and line live, and no CSP violations.

## 9. Integration

- [x] 9.1 Run the full Rust and Playwright suites and `openspec validate track-wealth --strict`. Verify: everything passes, and the existing order, dashboard, navigation and live tests pass without changes beyond those listed above.
