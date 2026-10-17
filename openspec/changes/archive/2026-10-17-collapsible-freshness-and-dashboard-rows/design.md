# Design

## Context

- The orders page is `#orders-view` = grid `minmax(0,1fr) 24rem`, with `#order-tables` on the left and `#freshness` (`.panel`) on the right. The wealth page is the same with `#wealth-graph`/`#wealth-items` on the left and `#wealth-freshness` on the right.
- `#freshness` and `#wealth-freshness` refresh by swapping their `innerHTML` with `orders_freshness.html` / `wealth_freshness.html` (on signals and reconnects). Filter, sort and range changes replace the whole `#…-view` (outerHTML), so the side sections are replaced too.
- Freshness view models come from `freshness.rs`. `FreshnessView` has `stale` and `needs_relogin`, and `WealthFreshnessView` has `needs_relogin` plus `wallet.stale`/`assets.stale`. A character that needs re-login is already counted as stale.
- The dashboard is `.fill` (100dvh minus the bar) holding `#dashboard-summary` as a 12-column grid with rows `auto 2fr 3fr`. The keep-scroll hook in `app.js` restores `[data-scroll-key]` positions after signal-triggered swaps.
- The CSP forbids inline script and style, so all toggling happens through `app.js` event listeners and CSS attribute selectors.

## Goals / Non-Goals

**Goals:**
- Toggle state lives for one page session and survives every htmx swap on that page.
- The warning count is always current, because it is rendered by the server in the refreshed fragment.

**Non-Goals:**
- Persisting the state (localStorage, cookie, server). The user chose "only until reload".
- Collapsing anything on the dashboard: its breakdown keeps its own "Updates" column.
- Changing chart data, series or the over-time graphs themselves.

## Decisions

### 1. Toggle inside the refreshed fragment, state in `app.js`
Each freshness fragment renders its own title row: a `<button class="freshness-toggle" aria-controls="…-body" aria-expanded="false" aria-label="Freshness" title="Show freshness">` with the `panel-right-open` icon and, when the count is above zero, a `<mark class="badge stale">N</mark>`. The list and fee tables follow in a body element with `hidden`. The server always renders the collapsed state, so a full page load starts collapsed with no flash.
`app.js` keeps one module-level boolean, `freshnessOpen` (false at load). A delegated `click` listener on `.freshness-toggle` flips it and calls `applyFreshness(document)`. An `htmx:load` listener calls `applyFreshness` on swapped content. `applyFreshness` sets `data-open` on each `[data-freshness]` section, `aria-expanded`, the title ("Show freshness"/"Hide freshness"), the icon (both icons are rendered and CSS shows one by `data-open`), and the body's `hidden`.
- Because the boolean outlives swaps, live refreshes and outerHTML view changes keep the state, and a reload resets it.
- *Alternative:* the toggle outside the refreshed region (in `orders.html`), with the count fetched separately. The count must refresh with the freshness itself, and the view swap replaces the toggle anyway, so splitting them buys nothing.
- *Alternative:* `<details>/<summary>`. Its open state is lost on every innerHTML swap, and it can't collapse a grid column's width without the same JS.

### 2. Accessible name and count
`aria-label="Freshness"` fixes the name required by the spec. The count badge gets an id and the button `aria-describedby` points at a visually hidden text "N characters need attention", so assistive technology hears the count without changing the name. The `hidden` utility class needed for that goes into `app.css` (no inline style).

### 3. Warning count in the view models
`orders::freshness` / `orders::view` add `warnings: usize` = number of entries with `stale` (re-login counts as stale). `wealth_page` adds `warnings` = number of entries with `needs_relogin || wallet.stale || assets.stale`. "Waiting for first update" counts only when it also makes the entry stale. For orders it does (never polled ⇒ stale), and that matches the existing stale badge.

### 4. Grid columns follow the state
`#orders-view` and `#wealth-view` use `grid-template-columns: minmax(0,1fr) auto`. The side section is `width: 28px` when collapsed and `width: 24rem` with `[data-open]`. The strip shows only the toggle (and badge), stacked vertically.

### 5. Dashboard rows
`dashboard.html` drops `.fill`, so the page scrolls. `#dashboard-summary` becomes a 3-column grid with `grid-auto-rows: auto`:
- tiles `1 / -1`
- breakdown `1 / -1`, its `.scroll` with `max-height: 20rem` (about 14 rows), growing with its content below that
- split, top-sell and top-buy one column each, height `22rem`
- value, count and wealth history `1 / -1`, each `height: 40dvh; min-height: 16rem`. The no-history placeholder spans the full width.

At 1080px: bar 31 + tiles ~62 + breakdown ≤ 20rem (260) + row 22rem (286) + gaps ≈ 650px, which is inside the window. The breakdown keeps `data-scroll-key="breakdown"`, so the keep-scroll scenario still holds.
- *Alternative:* keep `.fill` and let `#dashboard-summary` scroll internally. That is one more nested scroller, and the window scrollbar is what users expect for a long page.

### 6. Icons
Vendor Lucide `panel-right-open` and `panel-right-close` into `templates/icons/` in the same inline-SVG form as the others. The ISC licence file is already there.

## Risks / Trade-offs

- [Collapsed by default hides a "Log in again" link one click away] → The warning count in the error style keeps the problem visible, and the dashboard breakdown still shows the link directly.
- [Many Playwright specs read freshness content] → A shared `unfoldFreshness(page)` helper in `e2e/support/fixtures.ts`, used wherever a spec reads `#freshness`/`#wealth-freshness`. Updated in the same task as the template change.
- [The keep-scroll test for the freshness column] → It unfolds first. Scroll keeping is unchanged, because the body stays the `[data-scroll-key]` element.

## Migration Plan

Front-end and view-model only, shipped with the next release. Rollback is reverting the commit.
