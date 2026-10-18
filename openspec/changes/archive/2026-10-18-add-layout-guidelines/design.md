# Design

## Context

See proposal.md for why. The current state that shapes the approach:

- `base.html` renders the top bar (`nav#main-nav`) with a `controls` block. Each page puts its character filter form, plus the dashboard range control, into that block. The filter forms swap the page's view with `hx-select` and refresh the navigation links with `hx-select-oob="#main-nav"`.
- **Window pages** (orders, wealth) get their height from `.fill` and a CSS grid. The freshness column is `[data-freshness]`. `app.js` keeps the open/closed state in memory and restores the inner scroll of `[data-scroll-key]` elements around live swaps.
- **Opportunities:**
  - The page stacks a heading, a filter form panel, a freshness panel, a fees paragraph and four table panels inside `#opportunities-view`.
  - The settings form posts to `/opportunities/settings` and swaps `#opportunity-tables`.
  - Rows load `/opportunities/detail/{table}/{type_id}` into the fixed `aside#opportunity-detail`.
- **Tables:** `table.orders` is styled for orders, the breakdown and inventory. `table.opportunities` has its own hover style and no striping.

## Goals / Non-Goals

**Goals:**
- Two shared page skeletons in `base.html` and `app.css` (window page and scrolling page), so a new page picks one instead of writing its own grid.
- One filter box block and one sub-menu block that any page fills in.
- Keep htmx swaps and the CSP as they are. No inline scripts or styles, and no new dependencies.

**Non-Goals:**
- Reworking table columns. Over-wide tables only get sideways scrolling, and columns get fixed later, in a change per content block.
- Changing what any table, graph or analysis computes.
- A responsive or mobile layout below the existing "usable at narrow widths by wrapping" rule.

## Decisions

### Layout skeleton in `base.html`
`base.html` gains two blocks after the top bar: `subnav` and `filters`. Each renders its wrapper (`nav#section-nav`, `div#filter-box`) only when the page fills it, so the characters page shows neither. The `controls` block and the top bar's `.controls` container go away.

All bars sit in one `header.bars` wrapper. On scrolling pages the wrapper is `position: sticky; top: 0`, so the three bars move as one unit and their combined height is a single measurable box.

`main` gets a page-type class, `page-window` or `page-scroll`, from a `{% block page_type %}`. The window type moves today's `.fill` sizing onto `main`: its height is `100dvh` minus the measured bars.

A small script sets `--bars-height` on `:root` from the bars wrapper with a `ResizeObserver`. That height changes when the filter box wraps, so it can't be a constant. It drives the window page height, the sticky freshness column's `top` and height, and the rows' `scroll-margin-top`.
- *Alternative:* fixed pixel constants. Rejected because the filter box wraps at narrow widths and when there are many characters.

### Filter box contents and swapping
The character filter forms move unchanged into the `filters` block. Their `hx-select-oob` list grows to `#main-nav, #section-nav, #filter-context`, so navigation links, sub-menu counts and the fees text follow a selection change.

On the dashboard, the range control moves next to the filter form inside the box. The wealth range stays in its graph panel.

On opportunities, the settings inputs become a second form in the same box. It keeps `hx-post="/opportunities/settings"` and `hx-trigger="change"`, and the Apply button is removed. Validation errors render into `#filter-error` inside the box.

The settings response still swaps `#opportunity-tables`. It also returns `#section-nav` (the new counts) and `#filter-error` (cleared) as `hx-swap-oob`. A rejected value answers 422 with only the error out of band. The existing 422 handling in `app.js` is adapted to target `#filter-error`.

The right-end context text (`#filter-context`) is plain markup rendered from the view: the station name and the fees note.

### Sub-menu and scroll observer
Each row panel gets a stable `id` that serves as its fragment:
- **Dashboard:** `totals`, `characters`, `split`, `value`, `orders`, `wealth`.
- **Opportunities:** `expansion`, `classic`, `reversion`, `trend`. These replace the `opportunity-<id>` ids, and tests are updated.
- **Detail page:** `figures`, one id per chart kind, and `next`.

Sub-menu links are plain `href="#id"` anchors, so the browser handles jumping and sets the fragment. CSS `scroll-margin-top: calc(var(--bars-height) + 4px)` keeps titles clear of the bars.

The observer in `app.js` works like this:
- It runs on scroll, throttled with `requestAnimationFrame`, plus `hashchange`, `load` and `htmx:load`.
- While a jump is in progress (a `hashchange`, or a sub-menu click until the next user scroll input such as wheel, key or touch), it marks the fragment's link.
- Otherwise it marks the last row whose top is at or above the bars' bottom edge, or the last row when `scrollY` has reached the page end.
- The marker is `aria-current="location"` on the link.
- It never calls `history.replaceState`, so scrolling doesn't change the URL.
- *Alternative:* `IntersectionObserver`. Rejected because the "top-most row past a line" rule and the end-of-page case are simpler to get right with one geometric pass over a handful of rows.

On opportunities, `#section-nav` is swapped out of band with new counts. After every `htmx:load` the observer recomputes the marker, so swapped markup gets its marker back.

### Row cap on scrolling pages
A table's `.scroll` wrapper on a scrolling page gets `max-height: calc(var(--row) * 20 + header)`. Rows are a fixed single-line height under the dense theme, so this shows exactly 20 rows; it's set in CSS from the existing `--row` token plus borders.

The orders page keeps its two-row `tbody` records, but it's a window page, so the cap doesn't apply there. The dashboard breakdown's `max-height: 20rem` becomes the same 20-row rule.
- *Alternative:* the server renders only 20 rows. Rejected, because the spec keeps up to 50 rows reachable by scrolling.

### Wide tables
`.scroll` already uses `overflow: auto`. `table.data` gets `white-space: nowrap` on cells and `width: max-content; min-width: 100%`, so a table is as wide as the panel or wider and scrolls sideways inside it. Scroll restoration already records `scrollLeft`.

### Shared table style
`table.data` replaces `table.orders` and `table.opportunities` for header, row height, striping (`tbody tr:nth-child(even)`) and hover. Rows that link get `tr[data-href]` with a pointer cursor.

The orders tables keep their extra class (`table.data.records`) for the two-row-per-order striping by `tbody`. The breakdown's id selector and the inventory's id selector stay.

### Freshness column on scrolling pages
The opportunities view becomes a two-column grid (`minmax(0, 1fr) auto`), like orders. The freshness section gets `data-freshness` and the same toggle markup as orders. The column is `position: sticky; top: var(--bars-height); height: calc(100dvh - var(--bars-height) - 8px)` with its own inner scroll.

`opportunities_freshness.html` is rewritten in the orders pattern: head, toggle, warning count and body. The view gains a `warnings` count: book stale, a book error present and a history error present.

### Opportunity detail page
- **Route:** a new route `GET /opportunities/{table}/{type_id}` renders a full page, `opportunity_detail.html`, which extends `base.html`. axum routes `/opportunities/tables` (one segment) and `/opportunities/{table}/{type_id}` (two segments) separately, and `/opportunities/detail/{table}/{type_id}` keeps its distinct literal prefix.
- **Content fragment:** the existing handler `/opportunities/detail/{table}/{type_id}` is kept as the content fragment for live refresh. It renders `#opportunity-detail-content`, which holds the figures panel, chart rows and next occurrences. The page includes that same template.
- **Navigation:** rows become `tr[data-href]`. A delegated click handler in `app.js` does `location.assign(href)` (not inline JS, so the CSP holds). The item cell also wraps its text in a real `<a href>`, so keyboard users and middle-click work.
- **Back link:** the view builds "← Opportunities" from the current `chars` and the four sort parameters. The page's character filter form targets the detail page itself with `hx-select="#opportunity-detail-content"` plus the out-of-band list.
- **Live refresh:** the content `div` listens to `sse:opportunities-updated`, `sse:orders-updated`, `sse:fees-updated` and `htmx:sseOpen` as now, and swaps itself. The window scroll stays because the swap is in place, and chart rows have fixed heights so the page height doesn't jump.
- **Removed:** `aside#opportunity-detail`, its CSS, the close and Escape handlers, and the margin rule that made room for it.

## Risks / Trade-offs

- [The opportunities filter box carries the character filter, four inputs and a long station name, so it won't fit on one line at 1280px] → The spec lets it wrap. `--bars-height` follows the wrap, so the sticky offsets stay right, and a Playwright test at 1024px checks there's no horizontal scroll.
- [Moving the character filter out of the top bar breaks many existing selectors in the e2e tests (`.main-nav #character-filter`)] → The form keeps its `id="character-filter"`. Tests that scope to the nav are updated in the same task as the template move.
- [Page height changes during a live refresh, for example when a table grows from 4 to 20 rows above the reader, shift the viewport] → Rows above the reader only change height within the 20-row cap. The existing "keep scroll position" tests on opportunities guard the common case.
- [The sticky bars eat vertical space on the dashboard's first screen] → The bars total about 36 + 24 + 28px. The dashboard first-screen test at 1920×1080 keeps the check.
- [The out-of-band sub-menu swap could drop the scroll observer's marker] → The observer recomputes on `htmx:load`.

## Migration Plan

This is a pure UI change with no data migration. The old `/opportunities/detail/...` URL stays valid as a fragment endpoint, but it's no longer linked as a page. Roll back by reverting the commit.
