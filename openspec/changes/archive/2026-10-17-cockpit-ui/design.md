# Design

## Context

- Styling is `static/pico.min.css` plus a 64-line `static/app.css`. Templates are askama (`crates/sekondo/templates/`) and rely on Pico's element styles and a few Pico idioms (`<article>` cards, `role="button"` links, `class="secondary outline"`, `<hgroup>`, `.container`).
- The CSP is `default-src 'self'; img-src 'self' https://images.evetech.net; script-src 'self'; style-src 'self'`. Inline `style` attributes and `data:` images are blocked; `app.css` already works around Pico's `data:` checkbox images.
- The nav (`nav.html`, `#main-nav`) is rendered by `base.html`. Each page's filter form lives inside its view (`#dashboard-view`, `#orders-view`, `#wealth-view`), is swapped with `hx-select` and refreshes the nav out of band (`hx-select-oob="#main-nav"`) so the nav links carry the new selection.
- Orders tables are built from `orders::headers()` and `orders_table.html`; `Sort::parse` already falls back to the default sort for unknown keys.
- Charts are drawn by `static/app.js` with Chart.js, `maintainAspectRatio: false` inside a `.chart` box of fixed 18rem height.

## Goals / Non-Goals

**Goals:**
- One stylesheet, written for this app, driven by a small set of design tokens.
- Layouts built from CSS grid "panels" that fill the viewport (`100dvh` minus the bar) at ≥1280px.
- Keep every existing element ID, `data-*` hook, htmx attribute and SSE trigger working, so live updates and most Playwright locators are unaffected.

**Non-Goals:**
- No new data, figures or endpoints. No light theme or theme switcher. No mobile-specific layout below 1280px beyond wrapping.
- No change to Chart.js itself or to chart data/series.

## Decisions

### 1. Own stylesheet instead of a framework
Remove `pico.min.css`; rewrite `app.css` from scratch (target a few hundred lines). Structure: tokens (`:root` custom properties: colours, `--fs` 13px, `--row` ~20px, `--gap` 4–6px, `--border`), a minimal reset, element defaults (tables, links, buttons, checkboxes, `mark` badges), layout primitives (`.bar`, `.grid`, `.panel`, `.panel-title`, `.scroll`, `.side`), and page grids.
- *Alternatives:* Bootstrap 5.3 (232 KB), Halfmoon 2 (369 KB, densest of the family, last release 2024), Tabler 1.6 (635 KB dashboard kit) — all would need density overrides anyway, require class-heavy markup rewrites, and ship `data:` images that collide with the CSP. Classless sets (Water, Simple, concrete, launch) are as roomy as Pico.
- Native form controls with `accent-color` replace the checkbox workaround.

### 2. Top bar with a page-controls slot
`nav.html` becomes the bar: brand · Dashboard/Orders/Wealth text links · page controls · Characters icon · Sign out icon. `base.html` gains a `{% block controls %}` rendered inside `#main-nav`. Dashboard, orders and wealth move their filter `<form id="character-filter">` (and the dashboard its range control) into that block. The filter's checkboxes become a compact inline list (no `<legend>` text in view; it stays as an accessible name via `aria-label`).
- Because the form now lives inside `#main-nav`, filter and range requests keep `hx-target="#…-view" hx-select="#…-view"` and keep `hx-select-oob="#main-nav"`; that out-of-band swap now refreshes the controls (checked state, hidden `range`/`sort` inputs) together with the links. The dashboard range links gain the same `hx-select-oob`.
- `app.js` code that finds `[data-character-filter]` keeps working because the attribute stays on the form.
- The wealth page's range control stays in the wealth graph panel's title row, since the graph fragment swaps itself (`#wealth-graph`) independently of the view.
- *Alternative:* keep controls in the views and only restyle — rejected, costs a full row on every page.

### 3. Panel grids per page
- **Dashboard**: `#dashboard-view` is a grid filling `calc(100dvh - bar)`: row 1 the KPI tile strip (5 tiles in one row); rows 2–3 a 12-column grid: breakdown (scrolling) + split chart, two top-item charts, three over-time charts. `.chart` loses its fixed height and becomes `height: 100%` of a panel with `min-height: 0` so Chart.js resizes into it.
- **Orders**: `#orders-view` = `grid-template-columns: 1fr <side ~22rem>`; left column two rows (sell, buy) each `minmax(0,1fr)` with an internal `overflow:auto` scroller; right column `#freshness` scrolls.
- **Wealth**: same two columns; left = graph panel (fixed fraction) above `#wealth-items` scroller; right = `#wealth-freshness`.
- **Characters / landing / error**: simple panels in the full-width page; characters as a dense list of rows (portrait 32px, name, badges, actions).
- Sticky headers: `thead th { position: sticky; top: 0 }` inside the scroller, with an opaque background.

### 4. Two-line order rows
Each order renders as its own `<tbody id="order-{id}" class="order">` with two `<tr>`s: the figures row and `<tr class="details"><td colspan="N">issued · character [Corp] · location · region</td></tr>`. Multiple `<tbody>` elements are valid HTML and give a natural hover/zebra unit (`tbody.order:hover`, `tbody.order:nth-of-type(even)`). `N` comes from the header count already in the table view.
- `SortKey::Character`, `Location`, `Region` and `Issued` are removed from the enum, `ALL`, `name()` and `expression()`; `headers()` drops their columns. `Sort::parse` then already maps the old names to the default sort.
- *Alternative:* keep the extra fields in hidden columns or `title` attributes — rejected; the user wants them visible but secondary.

### 5. Icons: vendored Lucide, inline SVG via askama
Copy the needed Lucide SVGs (`users`, `log-out`, `user-plus`, `unlink`, `log-in`) into `templates/icons/*.html` with Lucide's ISC licence next to them (`templates/icons/LICENSE`). Templates `{% include "icons/users.html" %}`. Each SVG uses `stroke="currentColor"`, `width/height="14"`, `aria-hidden="true"`; no `style` attributes (CSP). Icon-only controls carry `aria-label` and `title`.
- *Alternatives:* SVG sprite with `<use href="/static/icons.svg#…">` (extra request, same result), icon font (CSP font rules, more weight), npm package (no JS build in this project).

### 6. Chart defaults
In `app.js`, set `Chart.defaults.font.size` (~10–11px), dimmer `INK`/`GRID` constants, smaller legend boxes, and `maxTicksLimit` on axes. Series colours and tooltip callbacks stay.

## Risks / Trade-offs

- [Playwright and Rust tests bound to Pico markup or column positions break] → Update them in the same task as the template they cover; keep IDs and `data-*` hooks stable. `orders.spec.ts` "location and region are the last two columns" is replaced by the details-line test.
- [Out-of-band swap of the bar replaces the checkbox the user just clicked, losing focus] → Acceptable today already for the view; if it hurts keyboard use, swap only `#main-nav ul.links` out of band in a follow-up.
- [No-scroll dashboard on 1080p squeezes charts when the browser has toolbars] → Use `100dvh`, give charts `min-height` so they never collapse below readability; panels scroll instead of the page.
- [Removing sort keys silently changes bookmarked URLs] → Spec'd fallback to default sorting; no error.
- [Own CSS has to style every form element] → The app has few: checkboxes, buttons, links-as-buttons. Covered by the CSP-violation test visiting every page.

## Migration Plan

Pure presentation change, deployed with the next release. Rollback is reverting the commit; no data or configuration involved.
