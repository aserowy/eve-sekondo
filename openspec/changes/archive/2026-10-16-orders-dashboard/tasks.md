# Tasks

## 1. Shared freshness

- [x] 1.1 Move `FreshnessRecord`, `is_stale` and the `FreshnessView` construction from `orders.rs` into a new `freshness` module (design §2), with `orders.rs` using it and its behaviour unchanged. Add unit tests for `is_stale` covering needs-re-login, never polled, exactly 2× the interval and just over it. Verify `cargo test -p sekondo` passes, with `tests/freshness.rs` and `tests/orders_page.rs` unchanged

## 2. Poll totals history

- [x] 2.1 Add the migration from design §3: the `order_poll_totals` hypertable (30-day chunks, `ON DELETE CASCADE`, unique `(character_id, polled_at)`, index `(character_id, polled_at DESC)`) and the backfill from `order_snapshots`. Verify `tests/schema.rs` tests that:
  - the table is listed in `timescaledb_information.hypertables`
  - deleting a character removes its entries
  - the "Backfill from snapshots" scenario holds: insert snapshots for two poll times, run the migration's backfill statement, and compare the sums
- [x] 2.2 Insert the poll's entry in `Poller::store`'s transaction, computed from the just-written `current_orders` rows. Verify `tests/poll.rs` integration tests for the `order-sync` scenarios "Entry after a poll with orders" (including the corp escrow share), "Entry after a poll without orders", "Failed poll" (via `/_stub/esi-fail-next`) and "Entries are removed with the character"

## 3. Current dashboard figures

- [x] 3.1 Add `dashboard.rs` with the grouped per-character query from design §1. It returns sell value, escrow, both corp shares (in cents), sell/buy counts and the freshness fields for every selected character, including characters without orders, in link order. The totals are Rust sums over the rows. Verify integration tests in `tests/dashboard.rs` (seeded with the stub and `/_test/poll`) for the `dashboard` scenarios "Totals across characters", "No orders", "Only own orders", "Breakdown rows" and "Row figures add up to the totals", and that a missing escrow counts as zero
- [x] 3.2 Cover corp shares and stale data in the query results. Verify `tests/dashboard.rs` tests for "Corporation sell order" (700m personal + 300m corp → 1b total, 300m share), a corp buy order's escrow share, "No corporation orders" (both shares zero), and that a character marked needs-re-login still contributes its last known orders
- [x] 3.3 Add the two top-items queries from design §5. Verify `tests/dashboard.rs` tests for "Item listed by two characters", "More than ten items" (twelve items → ten, highest first), ties ordered by name, an unresolved name labelled `#<type id>`, and that another account's orders for the same item are not summed in

## 4. History series

- [x] 4.1 Add `Range` (`24h`, `7d`, `30d`, `90d`, with fallback to `7d`) to `ViewQuery`, plus the bucket widths from design §4. Verify unit tests for parsing each value, a missing value and `1y`
- [x] 4.2 Implement the gap-filled series query from design §4 (per-character `locf` with `prev` carry-in, summed across the selection, bucket ends as timestamps, last point clamped to now). Verify `tests/dashboard.rs` tests driven by `TestClock` and repeated `/_test/poll` with changing stub orders for "Value develops over polls", "All orders close", "Characters polled at different times", "Stale character is carried forward", "No history in range", a carry-in from before the range start, and that the last point equals the tile totals

## 5. Dashboard page

- [x] 5.1 Replace the stub: `GET /dashboard` renders the tiles (with the "of which … corp" line only when the share is non-zero) and the breakdown table, through a new `GET /dashboard/summary` fragment included in the page (design §8). Amounts go through `format::isk_short`/`isk_exact` with the exact value on hover. Breakdown rows show last and next update in EVE time, the stale badge, "Log in again" and "Waiting for first update", with the orders page's markup. Remove `DashboardTemplate` from `pages.rs` and route both paths. Verify:
  - Rust tests in `tests/dashboard.rs` for the rendered tiles, the corp line shown and hidden, "Shortened total", each freshness scenario with `TestClock`, and "Unauthenticated access" for both paths
  - a Playwright test in `e2e/tests/dashboard.spec.ts` that seeds orders for two characters and checks the tiles, the breakdown rows, the corp line and the hover text under the CSP guard
- [x] 5.2 Rewrite the stub-dashboard checks for the new home: replace `tests/navigation.rs::the_stub_dashboard_says_it_is_coming_and_links_to_the_orders` with a test for "Dashboard after sign-in". Update the opening block of `e2e/tests/navigation.spec.ts` to expect the dashboard tiles and reach the orders page through the navigation link. Verify `cargo test -p sekondo --test navigation` and `npx playwright test navigation.spec.ts` pass

## 6. Graphs

- [x] 6.1 Vendor Chart.js 4 (design §6): `static/chart.umd.min.js` with its licence and the version and SHA-256 recorded. Load it on the dashboard only. Add the `htmx:load` initialiser in `app.js` that destroys and recreates charts on `[data-chart]` canvases from `data-series` (design §7), with animation off and the external HTML tooltip styled in `app.css`. Pick the sell/buy colours with the `dataviz` skill. Draw the per-character split graph from the design §1 rows. Verify:
  - a Rust test that the summary fragment's `data-series` for the split graph holds the breakdown figures, and that a character name containing `"<>&` is HTML-escaped in the attribute
  - a Playwright test in `dashboard.spec.ts` for "Two characters" that hovers each bar and reads the tooltip text, under the CSP guard
- [x] 6.2 Add the two top-items bar graphs, including their "no sell orders" / "no buy orders" empty states. Verify a Rust test that `data-series` matches the design §5 query results, and a Playwright test in `dashboard.spec.ts` that hovers the top bar of each graph and reads its item name and exact value, plus the "No buy orders" empty state
- [x] 6.3 Add the value-over-time and order-count-over-time line graphs: a linear epoch axis with EVE-time ticks, shortened ISK axis labels, and tooltips with EVE time, series name and exact value. Include the "no history yet" empty state and the `aria-label` with the latest values. Verify a Rust test that `data-series` timestamps are bucket ends and amounts are cents, and a Playwright test in `dashboard.spec.ts` for "Hover a point" (seed polls through `/_test/poll`, hover the last point, read "Sell value" and the exact amount) and the empty state for a character never polled

## 7. Range control

- [x] 7.1 Add the range control links (design §9) with `aria-current` on the active range, swapping `#dashboard-view` and pushing the URL. Verify:
  - Rust tests for "Default range", "Unknown range" (7d marked, 7d series) and that the tiles, breakdown and bar graphs are identical across ranges
  - a Playwright test in `dashboard.spec.ts` for "Switch range" (URL carries `chars` and `range=30d`, no full reload checked with a `window` marker) and "Graphs under the policy"

## 8. Character filter on the dashboard

- [x] 8.1 Extract the checkbox fieldset into a shared `character_filter.html` partial (design §9). The orders page keeps its sort inputs, and the dashboard adds a hidden `range` input and targets `#dashboard-view` with `hx-select-oob="#main-nav"`. Change `static/app.js` to take the path from the form's `hx-get` and to append sort fields and `range` only when present and not default. Verify:
  - a Rust test that `/dashboard?chars=<first>` covers only the first character in every figure and graph, and that a foreign ID falls back to all characters
  - a Playwright test in `dashboard.spec.ts` for "Narrow the selection" (starting from `?range=30d`, the URL keeps `range=30d`) and "Selection carried to the orders page"
  - the unchanged `e2e/tests/filter.spec.ts` still passes

## 9. Live dashboard

- [x] 9.1 Give the summary wrapper `hx-get="/dashboard/summary{query}"` and `hx-trigger="sse:orders-updated, htmx:sseOpen from:body"` inside the `sse-connect="/events"` container, and make sure re-initialised charts replace the old ones (no leaked Chart instances). Verify:
  - Playwright tests in `dashboard.spec.ts` for "Totals refresh after a poll" (tile, row, split bar tooltip and the last line point change without a reload, checked with a `window` marker) and "Selection and range kept by a live refresh"
  - a check that after three refreshes `Chart.instances` holds exactly four charts
  - the unchanged `e2e/tests/live.spec.ts` still passes

## 10. Integration check

- [x] 10.1 Run the full suite and the change checks together. Verify that `cargo clippy --workspace --all-targets`, `cargo test --workspace` and the whole Playwright suite pass with the stub fingerprint unchanged (no stub or contract edits in this change), and that `openspec validate orders-dashboard --strict` reports the change as valid
