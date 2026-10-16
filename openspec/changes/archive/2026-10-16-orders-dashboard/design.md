# Design

## Context

- `/dashboard` is a stub served by `pages::dashboard` with `templates/dashboard.html`. It already resolves the `chars` selection, but only to carry it into the navigation.
- The orders overview (`orders.rs`) has the patterns this change reuses. Queries are scoped by `account_id` and `selection.ids()`. Freshness comes from `list_freshness`, `is_stale` and `freshness_view`. It serves a page plus fragments (`/orders/tables`, `/orders/freshness`), and they refresh on `hx-trigger="sse:orders-updated, htmx:sseOpen from:body"`.
- The filter control is the `data-character-filter` form in `orders_filter.html`. `static/app.js` rebuilds its request URL and hardcodes the path `/orders` along with the sort fields, so it cannot be reused on another page as it stands.
- Amounts are `numeric(20,2)` in `current_orders` and `order_snapshots`. The views work in integer cents (`format::isk_short` / `isk_exact`).
- `Poller::store` replaces `current_orders`, inserts `order_snapshots` rows and updates `order_sync` in one transaction. A poll with no orders inserts no snapshot rows and leaves no trace except `order_sync.last_success_at`, which the next poll overwrites.
- The database is `timescale/timescaledb:2.17.2-pg17`, the community build, so `time_bucket_gapfill`, `locf` and `last` are available.
- The CSP is `script-src 'self'; style-src 'self'`: no inline scripts, no `<style>` elements and no `style=""` attributes in markup. Setting `element.style` from a script file is allowed. Front-end libraries (htmx, the htmx SSE extension, pico) are vendored minified files in `crates/sekondo/static/`.

## Goals / Non-Goals

**Goals:**
- Aggregate in the database, never in another account's scope.
- Make the rows add up to the totals by construction, and make the last history point match the totals.
- Use one stale rule for the orders page and the dashboard.
- Render graphs under the existing CSP without loosening it.

**Non-Goals:**
- Sorting the breakdown table.
- Linking tiles, rows or bars to a pre-filtered orders page.
- Zooming or panning graphs, history per item or per location, corp-share lines in the graphs, and export.
- A retention policy or continuous aggregates for the history.

## Decisions

### 1. A `dashboard` module with one grouped query for current figures
New `crates/sekondo/src/dashboard.rs` holding the page and fragment handlers. One query reads from `characters c` with `LEFT JOIN order_sync s` and a `LEFT JOIN` on `current_orders o`, filtered by `c.account_id = $1 AND c.character_id = ANY($2)` and grouped by character. It uses aggregate `FILTER` clauses:
- sell value: `SUM(o.price * o.volume_remain) FILTER (WHERE NOT o.is_buy_order)`
- escrow: `SUM(COALESCE(o.escrow, 0)) FILTER (WHERE o.is_buy_order)`
- corp share of each: the same sums with `AND o.is_corporation` added
- counts: `COUNT(o.order_id) FILTER (...)`

Each sum is `COALESCE(...,0)` and converted to cents only once, after summing (`(sum * 100)::bigint`). Rows are ordered by `c.created_at, c.character_id`. The tile totals are the sums of the rows in Rust, so "rows add up to the totals" always holds and there is no second query. The per-character split graph is drawn from these same rows.

*Alternatives:* a separate totals query, which could disagree with the rows while a poll writes between the two queries. Summing all orders in Rust, which moves a whole table over the wire for four numbers.

### 2. Freshness is shared
`is_stale` and the construction of `FreshnessView` move out of `orders.rs` into a small `freshness` module that both views use. The dashboard query already returns `needs_relogin`, `last_success_at` and `next_poll_at`, so it builds a `FreshnessRecord` from them and calls no separate `list_freshness`.

### 3. History: an `order_poll_totals` hypertable
New migration with these columns:
- `character_id bigint REFERENCES characters ON DELETE CASCADE`
- `polled_at timestamptz`
- `sell_value`, `escrow`, `sell_corp`, `escrow_corp`, all `numeric(20,2)`
- `sell_count`, `buy_count`, both `integer`

It is a hypertable on `polled_at` (30-day chunks), with an index on `(character_id, polled_at DESC)` and a unique constraint on `(character_id, polled_at)`, which TimescaleDB allows because it contains the time column.

`Poller::store` inserts one row in its existing transaction. The row is computed in SQL from the rows just written to `current_orders` (`INSERT … SELECT … FROM current_orders WHERE character_id = $1`), so the history and the current figures cannot disagree, and an empty poll yields zeros. The migration backfills with `INSERT … SELECT … FROM order_snapshots GROUP BY character_id, polled_at`.

*Alternatives:*
- Aggregating `order_snapshots` at read time. It cannot see empty polls (the "All orders close" scenario would be wrong), and it scans every order row of the range.
- A separate "polls" table with only timestamps, which still needs the snapshot scan.
- A continuous aggregate, which can't be filled from inside the poll transaction and still can't see empty polls.

### 4. History series: gap-filled buckets, last value carried forward
Per range, a fixed bucket width: 24h → 30 min, 7d → 2 h, 30d → 6 h, 90d → 1 day. That gives 48 to 120 points.

For each selected character, `time_bucket_gapfill(width, polled_at, start, now)` with `locf(last(<figure>, polled_at), prev => (<the character's last entry before start>))` gives the figure in force at the end of every bucket. The outer query sums across characters per bucket. Buckets before a character's first entry are NULL and count as zero.

Each point's timestamp is its bucket's end, so its value is "the most recent poll at or before that time". The last bucket's end is clamped to `now`. That last point therefore equals the current totals, because every character's latest entry was written in the same transaction as its current orders.

Account scoping uses the same `c.account_id = $1 AND character_id = ANY($2)` join as decision 1.

*Alternative:* fetching raw entries and stepping through them in Rust. At 20-minute polls, 90 days of history is about 6,500 rows per character, so that moves far more data, for a result the database computes directly.

### 5. Top items: two grouped queries over current orders
`GROUP BY o.type_id` (joined to `names` for the label) with `SUM(price * volume_remain)` for sell orders and `SUM(COALESCE(escrow,0))` for buy orders. Each is ordered by sum descending, then lower-cased name, then type id, with `LIMIT 10`, and scoped as in decision 1. Labels fall back to `#<type id>` as in `orders.rs`.

### 6. Chart library: Chart.js 4, vendored
`chart.umd.min.js` is pinned to the current 4.x release at implementation time and committed to `static/` together with its MIT licence text. The version and SHA-256 are recorded in a comment at the top of `app.js`'s chart section. It is loaded with a plain `<script src="/static/chart.umd.min.js">` on the dashboard page only.

Why it is safe under the CSP: Chart.js 4 draws on `<canvas>`, uses no `eval` and injects no `<style>` elements, so the CSP stays as it is. Configuration choices:
- **Time axis:** a `linear` axis over epoch milliseconds, with a tick callback that formats EVE time. That avoids a date-adapter dependency.
- **Tooltips:** the external (HTML) tooltip handler renders into a `<div class="chart-tooltip">` styled in `app.css` and positioned through `element.style`. Playwright can read its text, and the exact values come from the data, not from canvas pixels.
- **Animation:** off, so live refreshes don't flicker and tests are deterministic.
- **Colours:** sell and buy each get one colour, used the same way in every graph. Pick them with the `dataviz` skill so they read in pico's light and dark themes.

*Alternatives:*
- uPlot: smaller and very fast for time series, but bars need its paths plugin, and its cursor/legend model fits two of the four graphs.
- Server-side SVG: no dependency, but rejected in favour of a library for richer interaction (HTML tooltips, hover across series).
- A date adapter for Chart.js's `time` scale: another vendored file for formatting we already do.

### 7. Chart data travels in an HTML-escaped attribute, not inline JSON
Each graph is a `<canvas data-chart="<kind>" data-series="<json>" role="img" aria-label="…">` inside the summary fragment. The JSON is built with `serde_json` and placed in the attribute through askama's normal HTML escaping (no `|safe`). It holds:
- timestamps in epoch ms
- amounts in cents as integers
- labels: character and item names
- the pre-formatted exact strings for the tooltip, built by `format::isk_exact`, so the Rust and JS formatting can't diverge

`app.js` listens for `htmx:load` and handles every `[data-chart]` it finds. It destroys any previous chart on that canvas (`Chart.getChart`), parses `dataset.series` with `JSON.parse`, and creates the chart.

*Alternative:* a `<script type="application/json">` block. HTML escaping does not apply inside `<script>`, so it would need `|safe` plus hand-escaping of `</script>` sequences in ESI-supplied item and character names. That is a needless injection surface.

### 8. One live fragment for tiles, breakdown and graphs
`GET /dashboard/summary?chars=…&range=…` renders the tiles, the breakdown, the four graphs and the range control. The page wraps it in an element with `hx-get="/dashboard/summary{query}"` and the orders page's SSE trigger. A single fragment keeps every figure on screen from the same request.

### 9. Range and filter controls
- `ViewQuery` gains `range`, and a `Range` enum parses it with fallback to `7d`, the same way `Sort::parse` falls back.
- The range control is a group of links (`/dashboard?…&range=30d`) with `hx-get`, `hx-target="#dashboard-view"`, `hx-select="#dashboard-view"`, `hx-push-url="true"`, and `aria-current` on the active range.
- The checkbox fieldset moves to a shared `character_filter.html` partial. The form's `hx-get`, `hx-target` and `hx-select` come from view fields.
- `app.js` takes the path from the form's `hx-get` instead of the literal `/orders`. It appends `sort/dir/bsort/bdir` only when those inputs exist and are not the default, and `range` only when its hidden input exists and is not `7d`.
- Both pages keep `hx-select-oob="#main-nav"`. The navigation carries only `chars`, so following "Orders" and back resets the range to the default. That is accepted.

*Alternative:* a second copy of the filter script for the dashboard. It would drift from the first.

## Risks / Trade-offs

- [The vendored chart library breaks under the CSP or a future browser] → The Playwright CSP guard already fails any test on a violation. The dashboard tests exercise every graph, a range switch and a live refresh under it.
- [The backfill is slow on a large `order_snapshots`] → One grouped insert per migration. At personal scale it's seconds. It runs once, inside the migration transaction.
- [Past empty polls are lost] → Accepted. The history is exact from this release on. The backfilled part may keep showing a character's last non-empty figures across a period in which it had no orders. Recorded in the proposal.
- [Unlinking deletes history, so totals over time change retroactively] → This is what the existing unlink requirement asks for ("all data stored for it"). Accepted.
- [Changing `app.js` affects the orders filter] → Run the existing `filter.spec.ts` and `live.spec.ts` unchanged as regression tests.
- [Moving freshness code could subtly change the orders page] → Keep `tests/freshness.rs` passing unchanged.
- [`numeric` → `bigint` overflow] → 9.2e18 cents is about 9.2e16 ISK, far above any account. The conversion runs in SQL, so an overflow fails loudly.
- [Canvas graphs are opaque to screen readers] → Each canvas carries `role="img"` and an `aria-label` that states the latest values (over-time graphs) or the top entry (bars). The breakdown table already gives the current per-character figures as text. Full tabular alternatives for the history are deferred.
- [Removing the stub breaks existing tests] → `tests/navigation.rs::the_stub_dashboard_says_it_is_coming_…` and the first block of `e2e/tests/navigation.spec.ts` are rewritten, not deleted.

## Migration Plan

1. The new migration creates `order_poll_totals`, makes it a hypertable and backfills it from `order_snapshots`. It runs automatically on startup, like the existing migrations.
2. From then on every successful poll writes one entry.

Rollback: revert the release, then drop `order_poll_totals` by hand if needed. The old code ignores the table, so leaving it in place is harmless. No other table changes.
