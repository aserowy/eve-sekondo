# Tasks

## 1. Theme foundation

- [x] 1.1 Remove `static/pico.min.css` and its `<link>` in `base.html`; update `tests/server.rs` (`vendored_static_files_are_served`) to assert `/static/app.css` instead and that `/static/pico.min.css` is gone (404); verify `cargo test -p sekondo --test server` passes
- [x] 1.2 Rewrite `static/app.css` foundation: design tokens on `:root` (colours, 13px font, row height, gaps, border), reset, element defaults (body, links, tables with right-aligned tabular `.num` cells, buttons and link-buttons, native checkboxes with `accent-color`, `mark.badge` with success/error variants), keep htmx indicator rules; drop `.container` so `<main>` spans the full width with ≤8px gutters; verify with a new Playwright spec `e2e/tests/layout.spec.ts` asserting (orders page with one sell order) body font-size ≤13px, sell row height ≤22px, only one stylesheet loaded, and order panels within 8px of the window edges at 2560px width
- [x] 1.3 Add layout primitives to `app.css` (`.panel`, `.panel-title`, `.scroll` with sticky `thead th`, `.side` column) and an e2e helper `e2e/support/layout.ts` (page scroll height vs. window height, element-inside-viewport, horizontal-overflow check); verify with a Playwright test in `layout.spec.ts` that the dashboard at 1024px width has no horizontal page scroll

## 2. Top bar and icons

- [x] 2.1 Vendor Lucide `users`, `log-out`, `user-plus`, `unlink`, `log-in` as `templates/icons/*.html` (inline SVG, `stroke="currentColor"`, 14px, `aria-hidden`, no `style` attributes) plus `templates/icons/LICENSE` (ISC); verify `cargo build -p sekondo` compiles a template that includes each icon
- [x] 2.2 Turn `nav.html` into the single-row top bar (brand, Dashboard/Orders/Wealth text links, `{% block controls %}` slot from `base.html`, Characters and Sign out as icon-only controls with `aria-label` and `title`); remove the `<h1>`/`<hgroup>` page headers from all signed-in templates and update `tests/wealth_page.rs` (`<h1>Wealth</h1>` assertion) and `tests/navigation.rs` accordingly; verify `e2e/tests/navigation.spec.ts` still passes unchanged and a new scenario there checks the icon-only controls expose "Characters"/"Sign out" as accessible name and `title`, and that the bar is one row ≤36px high with no `h1` on the dashboard
- [x] 2.3 Move the character filter form of dashboard, orders and wealth into the `controls` block as a compact inline checkbox list (accessible name "Characters"), keeping `id="character-filter"`, `data-character-filter`, hidden sort/range inputs, `hx-target`/`hx-select` of the view and `hx-select-oob="#main-nav"`; verify `e2e/tests/filter.spec.ts` and the filter scenarios in `dashboard.spec.ts`, `orders.spec.ts` and `wealth.spec.ts` pass, plus a new `layout.spec.ts` check that the filter sits inside the top bar on all three pages and is absent on the characters page

## 3. Dashboard grid

- [x] 3.1 Move the dashboard range control into the `controls` block (range links keep `hx-target="#dashboard-view"` and gain `hx-select-oob="#main-nav"`); verify the range scenarios in `e2e/tests/dashboard.spec.ts` pass and a new check asserts the range control is inside the top bar
- [x] 3.2 Restructure `dashboard_summary.html` into panels (tile strip, breakdown in a `.scroll` panel, split chart, two top-item charts, three over-time charts) and add the dashboard grid to `app.css` filling `100dvh` minus the bar; make `.chart` take its panel's height with a minimum height; verify a new `layout.spec.ts` test at 1920×1080 with three characters: page scroll height ≤ window height and every tile and canvas inside the viewport, and that `live.spec.ts` dashboard refresh still passes
- [x] 3.3 Verify overflow behaviour with a Playwright test that links twenty characters (stub) and asserts at 1920×1080 the breakdown panel scrolls internally, its header stays visible after scrolling, and the page does not scroll

## 4. Orders page

- [x] 4.1 Remove `SortKey::Character`, `Location`, `Region` and `Issued` (enum, `ALL`, `name()`, `expression()`) and their columns from `orders::headers()`; update the header expectations in `tests/orders_page.rs` (around line 523) and add a test that `sort=location&dir=desc` yields item-ascending sorting; verify `cargo test -p sekondo --test orders_page` passes
- [x] 4.2 Render each order as `<tbody id="order-{id}" class="order">` with the figures row and a `colspan` details row "issued · character [Corp] · location · region" in `orders_table.html`; style details as dimmed, hover and zebra per `tbody.order`; update `tests/orders_page.rs`/`tests/fees_page.rs` HTML assertions that relied on the old row shape; verify in `e2e/tests/orders.spec.ts` by replacing "location and region are the last two columns" with tests for the header order (sell and buy), the details line text order, the Corp marker on the details line, and that hovering the details row highlights the figures row
- [x] 4.3 Lay out `orders.html` as the two-column grid: stacked sell/buy `.scroll` panels on the left, `#freshness` (with fee tables) as a scrolling `.side` column on the right; compact the freshness/fee markup; give "Log in again" the `log-in` icon plus text; verify with Playwright at 1920×1080: freshness right of the tables, buy table below sell, and with 200 stubbed sell orders scrolling the sell panel keeps its header visible while the page and the buy panel do not move; `e2e/tests/fees.spec.ts` and `live.spec.ts` orders scenarios still pass

## 5. Wealth page

- [x] 5.1 Lay out `wealth.html` as graph panel (range control in its title row) above the `#wealth-items` `.scroll` panel with sticky headers, and `#wealth-freshness` as the right `.side` column; give "Log in again" the `log-in` icon plus text; verify `e2e/tests/wealth.spec.ts` passes and a new `layout.spec.ts` test at 1920×1080 asserts freshness right of graph and items, items below graph, no page scroll

## 6. Characters, landing and error pages

- [x] 6.1 Restyle `characters.html` as a dense list of panel rows (32px portrait, name, badges, actions), with "Unlink" (`unlink` icon) and "Add character" (`user-plus` icon) showing icon plus text, flash as a compact alert; restyle `index.html` and `error.html` without Pico idioms (`article`, `role="button"`); verify `e2e/tests/characters.spec.ts`, `unlink.spec.ts`, `landing.spec.ts` and `relogin.spec.ts` pass and a new check asserts the Add character and Unlink controls each contain an `svg` followed by their text

## 7. Charts

- [x] 7.1 Adjust `static/app.js` chart defaults: smaller font (~10–11px), dimmer axis text and grid colours, compact legends, `maxTicksLimit` on axes; keep series colours and tooltip callbacks; verify the hover-tooltip and CSP scenarios in `e2e/tests/dashboard.spec.ts` and `wealth.spec.ts` (via `e2e/support/charts.ts`) pass unchanged

## 8. Integration

- [x] 8.1 Add a Playwright test that visits dashboard, orders, wealth and characters pages (switching range and selection) and asserts no CSP violations; run the full suite (`cargo test` and `npx playwright test` in `e2e/`) and verify both are green
