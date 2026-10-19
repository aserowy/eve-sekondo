# Design

## Context

- Styling today is one hand-written `static/app.css` (~250 lines) plus layout JavaScript in `static/app.js`. The script measures the bars (`--bars-height`), keeps panels' inner scroll positions across htmx swaps, folds the freshness column and drives the section sub-menu marker. Pico v2.0.6 was vendored before commit `d2aa6d5` and can be restored from git history.
- `base.html` renders a `header.bars` with the nav, `{% block subnav %}` and `{% block filters %}`. The page model `pages::Nav` / `pages::Page` builds the hrefs, carrying `chars`.
- Each page has fragment routes (`/orders/tables`, `/orders/freshness`, `/trades/table`, `/wealth/items`, `/opportunities/tables`, …). SSE triggers on the fragment containers re-fetch them through `hx-get` URLs, which are rendered with the current view state.
- Orders, trades, wealth items and opportunity tables are already built fully in Rust and sorted in memory. Opportunity tables truncate to 50 rows after sorting.
- Sort state lives in the query string. Orders use `sort`/`dir` for sell and `bsort`/`bdir` for buy. Opportunities use one key/dir pair per table.
- The CSP is `style-src 'self'` and `script-src 'self'`: no inline styles or scripts, no `eval`.
- Fees (`fees.rs`, `fee_poller.rs`) are computed per character and location from skills, standings and station owners. Jita 4-4 is always resolved. `fee_poller::Kind` has `Skills` and `Standings`, and `fee_sync` keeps one row per character and kind.
- `characters.scopes` stores the granted scopes. `oauth::REQUIRED_SCOPES` (6) is both what we request and what `missing_scopes()` checks.

## Goals / Non-Goals

**Goals:**
- Pico does the styling. `app.css` is limited to compact tables, the sidebar, the sub-menu, KPI cards and pagination.
- One server-rendered table per page, with pagination done on the server so that KPIs can aggregate over all filtered rows.
- Delete most of the layout JavaScript instead of adapting it.
- Fee KPIs at the character's actual station, without forcing existing users to log in again.

**Non-Goals:**
- No change to how orders, margins, wealth or opportunities are computed. Only how they are cut into pages and summarised changes.
- No client-side table rendering and no infinite scroll.
- No mobile layout beyond "wraps and does not scroll sideways".
- No per-user server-side persistence of the sidebar state.

## Decisions

### Pico restored as a vendored file, with overrides scoped to tables
Restore `static/pico.min.css` (v2.0.6) from git together with its MIT licence header. Load it before `app.css`. `<html data-theme="dark">` stays. `app.css` is reduced to:
- `table` rules: font-size `0.875rem`; cell padding through Pico's `--pico-spacing`-based padding overridden to about `0.25rem 0.5rem`; `white-space: nowrap`; tabular numbers on `.num`; `.striped`-like zebra and hover. Wide tables are wrapped in a `div.table-wrap` with `overflow-x: auto`.
- The sidebar grid, the sticky sub-menu, the KPI card grid and the pagination bar.

Everything else uses Pico's semantic elements: `<article>` for KPI cards and sections, `<nav>` lists, `<mark>` badges, and `<details>`/`<select>` for the character filter. Pico's `.container-fluid` wraps `main`. Chart containers drop their custom heights and use Chart.js's responsive default aspect ratio.

*Alternative:* Pico classless or another framework. Rejected because classic Pico was in use before and the user asked for it.

### Layout shell: CSS grid with sidebar, no measured bars
`base.html` becomes `body > div.shell` (grid: `auto 1fr`) holding `aside.sidebar` (`nav[aria-label=Main]`) and `main.container-fluid`. The sidebar is `position: sticky; top: 0; height: 100vh`, with a flex column so Sign out sits at its bottom (`margin-top: auto`). Collapsing sets `data-menu="collapsed"` on `<html>`, and CSS then hides the `.label` spans. Each entry is `<a><svg …/><span class="label">Text</span></a>` with `aria-label` and `title` set to the text, so the name survives when the label is hidden.

The state is kept in `localStorage['sekondo.menu']`. `app.js` is loaded synchronously in `<head>` already, so it sets `data-menu` on `document.documentElement` before the body renders, which avoids a flash without an inline script. The toggle is a `<button aria-expanded aria-controls aria-label="Menu">`.

The sub-menu is `nav.sub-menu`, `position: sticky; top: 0`, at the start of `main`. Its height is published as `--sub-menu-height` through a tiny `ResizeObserver` and is used as `scroll-margin-top` for jump targets. This replaces `--bars-height`.

*Alternative:* sidebar state in a cookie rendered by the server. Rejected: it needs a non-HttpOnly cookie or an extra route for a purely cosmetic preference.

### Nav model
`pages::Page` gains `OrdersSell`, `OrdersBuy` and `Opportunities(Table)`. `orders_current()` and `opportunities_current()` match on these. Hrefs: Orders → `/orders/sell?chars=…`, Opportunities → `/opportunities/expansion?chars=…`. Icons, vendored from Lucide like the existing ones: `layout-dashboard`, `users`, `wallet`, `arrow-left-right`, `list`, `lightbulb`, `log-out`, and `panel-left-close`/`panel-left-open` for the toggle. The unused `panel-right-*` icons are removed.

### One route per table page; legacy entry routes redirect
- `/orders` → 303 to `/orders/sell` with the original query. New routes are `/orders/sell` and `/orders/buy`, plus fragment routes `/orders/sell/content` and `/orders/buy/content` (KPIs, table and pagination) and `/orders/freshness`.
- `/opportunities` → 303 to `/opportunities/expansion` with the query. `/opportunities/{table}` renders a table page. `/opportunities/{table}/content` replaces `/opportunities/tables`. `/opportunities/{table}/{type_id}` stays the detail route; axum matches the two-segment route separately, and an unknown `{table}` answers 404.
- Wealth and trades keep their URLs. Their KPI cards become part of the refreshed content fragment.

Sort query parameters stay as they are (`sort`/`dir` for sell, `bsort`/`bdir` for buy, per-table pairs for opportunities). Old bookmarks therefore keep working through the redirect, and sub-menu links between siblings carry all sortings.

### Pagination: one shared helper, slicing after sort
A new `pagination.rs` provides:
- `Paging { page: u32, per_page: u32 }`, parsed from the `page` and `per_page` query parameters. `per_page` must be one of 25, 50, 100 or 200 (default 50); `page` defaults to 1 when it is malformed or below 1.
- `paginate(&[T], Paging) -> (slice, PageView)`, which clamps `page` to `1..=last` (`last` is at least 1) and returns the view for the controls: previous and next hrefs, "Page x of y", and the page-size options with their hrefs.

Each page's view state (`ViewState` in orders, trades, wealth and opportunities) gains `paging`, and its `query()` omits `page=1` and `per_page=50`. Sort, filter, range and per-page links are built with `paging.page = 1`. Pagination links and the page-size `<select>` use `hx-get` on the content fragment with `hx-push-url` set to the page URL, the same pattern as the trades range control.

The live-refresh `hx-get` on the content container is rendered with the clamped paging. A refresh therefore stays on the current page, and when the data shrinks the server clamps it.

KPIs are computed from the full filtered and sorted vector before slicing, so they always cover every page. Opportunities drop `truncate(50)` and paginate instead. The row-count template code in the sub-menu is deleted.

*Alternative:* SQL `LIMIT/OFFSET`. Rejected: all lists are already materialised and sorted in Rust (sorts depend on computed margins and standings), and the KPIs need the whole set anyway.

### Content fragment = KPIs + table + pagination
Each table page has one refreshable `section#content` holding the KPI cards, the table and the pagination, and a separate `section#freshness`. Both carry their SSE triggers as today. The filter section and the sub-menu stay outside the swap target, so filter edits are not clobbered by live refreshes. Filter forms target `#content` and use `hx-select-oob` for the sidebar (to keep `chars` in the links), the sub-menu and the freshness. The wealth graph lives inside the content fragment between the KPIs and the table, so range switches swap the graph and leave the KPIs and items untouched by the range.

### KPI cards
The partial `kpi_card.html` renders `article.kpi` with a header (the name), the value (`title` holding the exact value), an optional detail line (`small`) and an optional link. `.kpis` is `display: grid; grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr))`, and `.kpi` has `aspect-ratio: 1`. Every card therefore has the same width and height, and the cards wrap. Long detail lines, such as station names, wrap inside the card and are clipped with an ellipsis after three lines; the full text is on hover.

Order KPIs come from the same aggregates the dashboard uses. A shared function computes count, value or escrow, and the corp share, from a slice of orders. That keeps the "equals the dashboard" scenario true by construction. Net margin is Σ margin and Σ known cost over sell orders whose margin is `Some`; the percentage is Σ margin / Σ cost. Orders left out are those with `None`.

Wealth KPIs reuse the last point of each wealth series (the current figures), which the wealth page already loads. The unpriced count moves from the items header to the inventory card.

Trades KPIs reuse the existing totals computation, minus the footer row.

### Fee KPIs and the fee location
`fees.rs` already computes fees for any `(character, location)` when skills, standings and the station owner are known. The orders view now asks, for each selected character, for `fee_location(character)` instead of iterating over order locations:
1. If the character's scopes lack `esi-location.read_location.v1`: Jita 4-4, with `needs_location_scope = true` so the "Log in again" link renders.
2. Otherwise, if a `character_locations` row exists: its `location_id`, which is a station or a structure.
3. Otherwise: Jita 4-4.

The location name comes from the existing names and structures resolution used for order locations. The dashboard and opportunities fee choice ("lowest min margin at the station") is unchanged.

### Location polling as a third fee-poll kind
Add `Kind::Location` to `fee_poller`, so `fee_sync` gets rows of `kind = 'location'`. It inherits the hourly cadence, the honouring of `Expires`, the backoff and the "skip when needs re-login" rule. In addition, characters without the location scope are skipped. ESI `GET /characters/{id}/location/` answers `{ solar_system_id, station_id?, structure_id? }`:
- `station_id` or `structure_id` present: upsert into `character_locations(character_id PK → characters ON DELETE CASCADE, location_id BIGINT, kind TEXT CHECK (kind IN ('station','structure')), updated_at)`.
- Neither present: in space, so keep the row.

Afterwards, when the fee location changed and the new location is an NPC station with an unknown owner, `resolve_stations` picks it up. It is extended to include docked stations, not only order stations. When the resulting fees differ from those at the previous fee location, the poller sends `fees-updated`, through the same path as skills and standings. The migration adds the table and widens the `fee_sync.kind` CHECK constraint to include `'location'`. Unlink and owner-change already cascade from `characters` deletes; the change-hands path explicitly deletes `fee_sync` rows today and is extended to `character_locations`.

*Alternative:* poll the location with the order poll (every 20 minutes) for fresher data. Rejected for now: hourly is what was agreed, and the fee location rarely matters at minute resolution.

### Optional scope
`oauth.rs` splits the scopes into `REQUIRED_SCOPES` (the 6) and `OPTIONAL_SCOPES` (`esi-location.read_location.v1`). The authorize URL requests both. `missing_scopes()` keeps checking only the required ones, so existing characters are not marked "needs re-login". The scopes granted by the refresh token are already persisted, which is how `fee_location` and the poller know whether the location scope is present.

### ESI contract and stub
- `esi-contract/src/contracts.rs` gets a `character_location` contract with three variants: docked at a station (`station_id`), docked in a structure (`structure_id`), and in space (only `solar_system_id`). It also gets the 403 variant for a missing scope, marked verifiable.
- `eve-stub` gets the route `/characters/{id}/location/`, which needs the scope, and a control `POST /characters/{id}/location` to set the fixture. The location scope is added to the default character scopes; the existing `/scopes` control can withhold it.
- The verified-stub fingerprint changes, so `verified-stub.json` must be re-recorded with `esi-verify` against real EVE before stub-backed tests pass the gate.

### JavaScript cleanup
`app.js` loses:
- `watchBars` and `--bars-height`;
- the freshness toggle and its state carried across swaps;
- the inner-panel scroll save and restore keyed by `data-scroll-key`.

It keeps:
- the section sub-menu marker, now measuring against the sub-menu's bottom edge;
- sideways scroll keeping for `.table-wrap`;
- a window scroll keep on SSE-triggered swaps. `htmx` swaps with `show:none`, and the swap is wrapped in a save and restore of `window.scrollY` when the request was triggered by `sse:*` or `htmx:sseOpen`;
- for user-triggered swaps (filter, sort, range, page), `show:#content:top`, so the table's top becomes visible.

It also gains the sidebar toggle and the early `data-menu` initialisation.

### Tests
Every requirement scenario touched gets a Rust test (routes, redirects, pagination clamping, KPI math, scopes, poller kind, contract coverage) or a Playwright test (layout, sidebar, cards, sub-menu, pagination interactions, live refresh keeping the page), following the project's TDD flow. The obsolete Playwright specs (`freshness.spec.ts` collapse cases, `scroll.spec.ts` inner-scroll cases, `layout.spec.ts` window-page cases) are rewritten rather than deleted wholesale, so that live-refresh scroll coverage is not lost.

## Risks / Trade-offs

- [Pico's defaults make every non-table element roomier, so the dashboard no longer fits in one screen] → Accepted by the user ("leave the rest as it is"). The dashboard's first-screen guarantee is dropped from the spec.
- [Square cards with long detail lines (station names) overflow] → Clamp to three lines with an ellipsis and the full text on hover. The card's minimum width is chosen so that "Jita IV - Moon 4 - Caldari Navy…" stays legible.
- [Many selected characters produce many fee cards (3 per character)] → Accepted. The cards wrap, and the user asked for one card per fee per character.
- [The verified-stub fingerprint changes and needs a run against real EVE with a test character] → A task re-records it. Until then, stub-backed tests are gated, as designed by the esi-contract capability.
- [Existing characters lack the location scope until they log in again] → The Jita fallback plus a visible "Log in again" link on their fee cards. Nothing else breaks.
- [Hourly location polling shows the previous station for up to an hour after the character moves] → Acceptable for fees. The fee cards name the station used, so the user can see which one it is.
- [Removing inner scrolling makes long pages] → Pagination with a default of 50 bounds the page length.
- [Big-bang template rewrite] → Tasks are ordered so the shell (Pico plus sidebar) lands first with all pages still working, and then each page is converted one at a time.

## Migration Plan

1. Deploy with the new migration (`character_locations`), which is additive.
2. Old `/orders` and `/opportunities` bookmarks redirect.
3. Users see the "Log in again" hint on fee cards. Logging in again grants the location scope, and the next fee tick polls the location.
4. Rollback: revert the commit. The `character_locations` table and the `fee_sync` rows of kind `location` are inert for the old code (`Kind::parse` already returns `None` for unknown kinds; a task checks that the old due-query skips such rows).
