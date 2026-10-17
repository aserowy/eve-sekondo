# Design

## Context

- `oauth::REQUIRED_SCOPES` is the single list that the authorize request, the callback's scope check (`VerifiedToken::missing_scopes`), the token refresh and `esi-verify` all use. A refreshed token keeps the scopes it was granted with, so characters linked before this change fail the check on their next refresh and get the existing "needs re-login" treatment without any extra code.
- `Poller` (orders, per character) and `MarketPoller` (public, per region and item pair) set the pattern for scheduled ESI work:
  - a `*_sync` table with `next_poll_at`, `esi_expires_at` and `failures`
  - `claim_due` and `schedule_new`
  - `spread`/`jitter` for spacing polls
  - `record_failure`/`record_deferral` with the shared `EsiGate`
  - one transaction per successful store
  - a signal published after commit
- `EsiClient::fetch_paged` already reads every page using `X-Pages`. The `cents` deserializer turns ESI floats into integer cents.
- `Poller::resolve_names` resolves type, region, station and structure names: public IDs go through `/universe/names` in self-splitting batches, structures go through `/universe/structures/{id}` with a token, and unresolvable ones are retried daily. It is private to the order poller.
- `order_poll_totals` already records `sell_value`, `escrow`, `sell_corp` and `escrow_corp` per poll, so personal sell value and escrow are `sell_value - sell_corp` and `escrow - escrow_corp` with no new column.
- `dashboard::history` builds the over-time series. It generates bucket ends over the range with the last one clamped to `now`, and for each selected character it takes the latest row at or before each end (`LEFT JOIN LATERAL … ORDER BY polled_at DESC LIMIT 1`). This carry-forward is what the specs mean by "most recent recorded figure at or before".
- The dashboard serves one `/dashboard/summary` fragment, refreshed on `sse:orders-updated, htmx:sseOpen from:body`. Graphs are Chart.js canvases fed through an escaped `data-series` attribute and set up by `app.js` on `htmx:load`.
- `app.js`'s filter script takes the path from the form's `hx-get` and appends `sort`/`dir` and `range` only when those hidden inputs exist and differ from the defaults (`item`/`asc`, `7d`).

## Goals / Non-Goals

**Goals:**
- Wealth on the dashboard and on the wealth page comes from one query path, so the dashboard line equals the stack's top edge and the tile equals the last point, by construction.
- Wallet, assets and orders fail independently.
- Valuing the inventory reuses one public price list for all accounts.

**Non-Goals:**
- A region column in the inventory list. ESI assets carry no region. Getting one would need three more ESI lookups (station → system → constellation → region) with contracts and verification, so the list shows the location only.
- Cost basis, Jita or best-price valuation, and per-item price history.
- Wealth of corporations, wallet journal or transaction views, and asset search or grouping beyond the spec.
- Backfilling history, retention policies and continuous aggregates.

## Decisions

### 1. Three new ESI calls on the existing client
`EsiClient` gains the following, each with its own gate route name and the same error mapping and `Expires` handling as `orders`:
- `wallet(character_id, token) -> EsiResponse<i64>`: `GET /characters/{id}/wallet/`. The body is a bare JSON number, read with `cents`.
- `assets(character_id, token) -> EsiResponse<Vec<EsiAsset>>`: `GET /characters/{id}/assets/`, through `fetch_paged`. `EsiAsset` has `item_id`, `type_id`, `location_id`, `location_type`, `location_flag`, `quantity`, `is_singleton` and `is_blueprint_copy`, where a missing `is_blueprint_copy` counts as false.
- `prices() -> EsiResponse<Vec<EsiPrice>>`: `GET /markets/prices/`, public. `EsiPrice` has `type_id` and `average_price: Option<cents>`, and `adjusted_price` is ignored.

### 2. Data model (one migration)
- `wallet_balances(character_id → characters ON DELETE CASCADE, polled_at, balance numeric(20,2), UNIQUE(character_id, polled_at))`. A hypertable with 30-day chunks and an index on `(character_id, polled_at DESC)`. The current balance is the latest row, so the history is the source of truth and nothing duplicates it.
- `current_assets(item_id PK, character_id → characters CASCADE, type_id, location_id, location_type, location_flag, quantity bigint, is_singleton, is_blueprint_copy, top_location_id bigint NULL)`, with an index on `character_id`. `top_location_id` is NULL when the location is unknown.
- `item_prices(type_id PK, average_price numeric(20,2) NULL)` plus `price_sync`, a single-row table shaped like `market_sync` without the key columns. Both hold public data and have no account column.
- `inventory_values(character_id → characters CASCADE, recorded_at, value numeric(20,2), UNIQUE(character_id, recorded_at))`, a hypertable like `wallet_balances`.
- `wealth_sync(character_id → characters CASCADE, kind text CHECK (kind IN ('wallet','assets')), next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures, PRIMARY KEY (character_id, kind))`.

Unlinking a character and an ownership change both delete the `characters` row, which cascades to every new table. That covers the deletion scenarios.

*Alternative:* one wide `wealth_snapshots` row per character and moment, with all four components. The components arrive at different times from different sources, so every row would copy three stale values. Separate per-source rows with carry-forward at read time (decision 6) match the dashboard's existing model.

### 3. `WealthPoller`: one scheduler, two kinds
A new `wealth.rs` module mirrors `Poller`.
- `schedule_new` inserts a `wallet` and an `assets` row for every character without one. The first offsets come from `spread(character_id ‖ kind, interval)`.
- `claim_due` claims due `(character_id, kind)` pairs from characters that are not marked "needs re-login".
- `poll(character_id, kind)` gets a token, makes the call, and stores the result in one transaction:
  - **wallet**: insert the `wallet_balances` row and update `wealth_sync`. The next poll is `max(polled_at + refresh_interval, Expires) + jitter`.
  - **assets**: replace `current_assets` with the computed top locations (decision 4), insert an `inventory_values` row computed in SQL from the rows just written (decision 5), and update `wealth_sync`. The next poll is `max(polled_at + 1 h, Expires) + jitter`.
- After commit it resolves names for the asset locations and types (decision 4) and publishes `WealthUpdated` to the character's account.

It has its own `spawn_scheduler` and tick loop and is started next to the other pollers in `main.rs`/`app.rs`. A failure in one kind never touches the other kind or `order_sync`.

*Alternative:* fetching the wallet inside `Poller::run_poll`. The wallet would share the order poll's schedule for free, but a wallet error would then have to be kept from failing or deferring the order poll, and the spec wants independent failure and status. It also couples a second kind of ESI data to the order transaction for no gain.

### 4. Top-level location and names
Locations are resolved in Rust over the polled list before storing:
- Build `item_id → (location_id, location_type)` from the list.
- For each asset, follow `location_type = 'item'` parents until the type is `station`, `solar_system` or `other`, or the parent is missing from the list.
- The result is the outermost item's `location_id`, which counts as a structure when it is ≥ `STRUCTURE_ID_MIN`, even with `location_type = 'item'` (ESI reports structures that way). A missing parent or `other` gives `NULL`.
- A hop limit of 16 guards against cycles.

Name resolution moves out of `Poller` into a shared `names` module, `resolve(ids, structure_token, now)`, which keeps the batching, the self-splitting and the daily retry. Both pollers call it. For assets it gets the distinct `type_id`s and top locations, with the owning character's token for structures. The view shows `Unknown location (<id>)` for a NULL top location, and `Unknown structure (<id>)` as on the orders page.

### 5. Valuation is SQL, recorded on every change
The inventory value of a character is:
`SUM(a.quantity * p.average_price) FILTER (WHERE NOT a.is_blueprint_copy AND p.average_price IS NOT NULL)` over `current_assets a LEFT JOIN item_prices p USING (type_id)`.
- The assets poll inserts it for its character in the same transaction.
- `PricePoller` (a new `prices.rs`, shaped like `MarketPoller` with a single key, refreshing daily but never before `Expires`) replaces `item_prices`. In the same transaction it inserts one `inventory_values` row for every character with current assets (`INSERT … SELECT … GROUP BY character_id`). After commit it publishes `WealthUpdated` to each distinct account of those characters.

Because an entry is written in every transaction that changes either input, the latest `inventory_values` row always equals the live valuation, the same invariant `order_poll_totals` relies on.

*Alternative:* computing inventory history at read time from asset snapshots and price history. That needs both histories stored in full, which adds far more rows for the same answer.

### 6. One wealth history query
`wealth::history(pool, account, selection, range, now) -> Vec<WealthPoint { at, wallet, sell, escrow, inventory, contributors }>` reuses `dashboard::history`'s bucket ends and `chars` CTE, with three lateral carry-forward joins per character:
- the latest `wallet_balances.balance`
- the latest `order_poll_totals` as `sell_value - sell_corp` and `escrow - escrow_corp`
- the latest `inventory_values.value`

Each is taken at or before the bucket end, summed per bucket, and missing values count as 0. `contributors` counts the characters with any of the three, which drives "no history yet".
- The wealth page draws the four series stacked.
- The dashboard calls the same function and draws `wallet + sell + escrow + inventory` as one line, so it equals the stack's top edge.
- `wealth::current(pool, account, selection)` is the same expression with the bucket end replaced by "latest" (no time bound). The tile and the inventory total therefore match the last point, whose end is clamped to `now`.

`Range` and the bucket generation are moved from `dashboard.rs` into a small shared `range` module, since both pages use them.

### 7. Wealth page: page plus three fragments
- `GET /wealth`: the full page, with the filter control, freshness, graph section and inventory section.
- `GET /wealth/graph?chars&range`: the graph and its range control. The range links target `#wealth-graph` only, with `hx-push-url`, so switching the range never re-renders the list.
- `GET /wealth/items?chars&sort&dir`: the inventory table, with sort headers like the orders tables. The header links keep `range` in the pushed URL through a hidden input, the same way the orders page keeps the other table's sorting.
- `GET /wealth/freshness?chars`.

Each fragment refreshes on `sse:wealth-updated, sse:orders-updated, htmx:sseOpen from:body` and carries the full query. The list query groups `current_assets` by `(character_id, type_id, top_location_id, is_blueprint_copy)` (blueprint copies are unpriced and get a row of their own, so they never add to the quantity an original is valued with; an asset whose top location is unknown is grouped under its own location id) and is scoped `account_id = $1 AND character_id = ANY($2)`. Sorting is whitelisted like `orders::Sort`, and unpriced rows sort last via `NULLS LAST` in both directions. Freshness reuses `freshness::is_stale` with the threshold `2 × interval` per kind.

`app.js` changes:
- The filter script's default sort becomes per form: a `data-default-sort` and `data-default-dir` on the form, `value`/`desc` for wealth and `item`/`asc` for orders. Without this the wealth page's default sort would be written into URLs.
- The chart setup gains a `stacked-area` kind: Chart.js `line` datasets with `fill: true` on a stacked y-axis. The external tooltip lists the four series and a total, which comes pre-formatted in the series JSON, as for the other graphs.
- Colours: four categorical colours chosen with the `dataviz` skill. Sell and escrow keep the dashboard's existing colours.

### 8. Dashboard additions
- `SummaryView` gains a wealth `AmountTile`, linking to `/wealth{chars query}`, and a `wealth` line chart from `wealth::history`, rendered in the same summary fragment so one range click updates all three over-time graphs.
- The fragment's `hx-trigger` adds `sse:wealth-updated`.
- The nav adds "Wealth" with `nav.wealth_href()`, which carries `chars` only.

### 9. Signal
`Event::WealthUpdated` → `"wealth-updated"`. A lagged subscriber still falls back to `OrdersUpdated`, which every wealth view also listens to, so no refresh is lost.

### 10. Stub, contracts, verification
- `eve-stub` adds `GET /characters/{id}/wallet/`, `GET /characters/{id}/assets/` (paged, 1,000 per page, `X-Pages`) and `GET /markets/prices/`.
- Controls: `POST /wallet`, `/assets` and `/prices`, with the existing `esi-fail-next` and `esi-expires` controls extended to the new routes.
- Wallet and assets check the bearer token's scopes and answer 403 without the matching scope. Tokens issued by the stub's `/authorize` get the scopes that were requested.
- `esi-contract` gains contracts:
  - wallet: 200 and 403
  - assets: 200 with `X-Pages`, and 403
  - prices: 200
  - plus the shared 420/429/5xx variants, marked as not verifiable
- `esi-verify` calls all three with the verification character and fails with "an asset is needed" if the assets list is empty.
- The fingerprint changes, so the user has to run a real verification before stub-backed tests can run (decision 11).
- Test hooks: `/_test/wealth-poll/{id}/{kind}` and `/_test/price-refresh`, behind the same test-only build flag as the existing hooks.

### 11. Order of work
The contracts and stub (decision 10) come first, and a verification run against real EVE happens before any stub-backed test of the new calls. It needs a person, so it is an explicit task that blocks the later groups.

## Risks / Trade-offs

- [Every user has to log in again after the deploy] → Accepted (proposal, **BREAKING**). The existing "needs re-login" badge and link guide them, and last known data stays visible.
- [ESI average price misvalues thin-market or freshly listed items] → Accepted for this change. Per-row unit prices are shown so the user can see the basis, and cost basis is a possible follow-up.
- [Items in sell orders could also appear in assets and be counted twice] → In EVE, items in a sell order leave the hangar and ESI does not list them as assets. The verification task includes a manual check: list an item, run the verification, and confirm that the listed units are absent from the assets answer.
  - **Status: not verified, documented as current behavior.** The manual check in task 2.4 was not done. If ESI does list units that are in a sell order, wealth counts them twice (once as inventory, once as personal sell value). This is a possible bug. A fix, if needed, is a separate change.
- [Large asset lists (tens of thousands of rows) make the assets poll and the list heavy] → One page per 1,000 assets with hourly polls is within ESI etiquette. The list query groups in SQL. Pagination of the HTML list is deferred until it hurts.
- [Price refresh writes one inventory row per character with assets, daily] → That's a few rows per day per character, negligible next to the order snapshots.
- [A container's own value: containers and ships are assets too and are valued at their average price] → This is the intended reading of "everything ESI returns" (Q4). Fitted ships count at hull price, plus their modules as separate assets.
- [Moving `resolve_names` and `Range` could change order and dashboard behaviour] → `tests/poll.rs`, `tests/dashboard.rs` and the existing Playwright suites run unchanged as regression tests.
- [Stacked area hides small components under large ones] → The tooltip lists every component and the total.

## Migration Plan

1. Deploy: the migration creates empty tables, so there is no backfill. The pollers start, and every existing character is marked "needs re-login" on its next token refresh.
2. Users log in again per character, and wallet and assets polls start within one interval.
3. Rollback: revert the release. The new tables are inert without the code and can be dropped by a down migration. Tokens with four scopes still satisfy the old two-scope check, so no second re-login is needed.
