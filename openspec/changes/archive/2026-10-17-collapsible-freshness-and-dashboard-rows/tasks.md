# Tasks

## 1. Warning counts

- [x] 1.1 Add `warnings` to the orders freshness view (count of stale entries) and to the wealth freshness view (count of entries needing re-login or with stale wallet or assets). Verify: Rust tests in `tests/freshness.rs`/`tests/wealth_page.rs` with one fresh and one re-login character expect 1, and with all fresh expect 0. A never-polled order character counts as 1.

## 2. Collapsible freshness on the orders page

- [x] 2.1 Vendor Lucide `panel-right-open` and `panel-right-close` as `templates/icons/*.html`, and extend `tests/icons.rs` to cover them. Verify: `cargo test -p sekondo --test icons` passes.
- [x] 2.2 Render the toggle title row and the hidden body in `orders_freshness.html` (`data-freshness` on `#freshness`, badge only when `warnings > 0`, `aria-describedby` text). Add the `app.js` state (`freshnessOpen`, delegated click, `htmx:load` re-apply) and the CSS for the collapsed strip and the open column with `#orders-view` columns `minmax(0,1fr) auto`. Add `unfoldFreshness(page)` to `e2e/support/fixtures.ts` and use it in `orders.spec.ts`, `fees.spec.ts`, `live.spec.ts` and `layout.spec.ts` wherever they read `#freshness`. Verify: new Playwright tests in `e2e/tests/freshness.spec.ts`: starts collapsed (body hidden, strip right of the tables, sell table reaches the strip), unfold/fold toggles `aria-expanded` and the "Show/Hide freshness" title, accessible name "Freshness", a re-login character shows badge "1" in the error colour, and the state survives an `orders-updated` refresh and a sort change but not a reload. The updated specs pass.

## 3. Collapsible freshness on the wealth page

- [x] 3.1 Apply the same toggle to `wealth_freshness.html`/`#wealth-freshness` and the `#wealth-view` columns, and update `wealth.spec.ts`, `layout.spec.ts` (wealth columns) and `scroll.spec.ts` (freshness keep-scroll unfolds first) to unfold. Verify: Playwright tests in `freshness.spec.ts` show the wealth page starts collapsed with no badge when all are fresh, and that unfolding survives a character filter change. `wealth.spec.ts` and `scroll.spec.ts` pass.

## 4. Dashboard rows

- [x] 4.1 Rearrange `dashboard_summary.html`/`app.css` into rows: tiles; full-width breakdown (max-height scroller); split, top-sell and top-buy in one row; value, count and wealth history each on a full-width row. Drop `.fill` from `dashboard.html`. Replace the `layout.spec.ts` tests "fits a 1920x1080 window" and "twenty characters" with: at 1920×1080 with three characters, the tiles, breakdown, split and top-item canvases are inside the window, the three share one row, the breakdown is as wide as the tile row, each over-time canvas's panel is as wide as the tile row and below the previous one; with twenty characters the breakdown scrolls inside itself, its header stays visible, and the split row is inside the window. Verify: these tests, `dashboard.spec.ts` (tooltips) and the breakdown test in `scroll.spec.ts` pass.

## 5. Integration

- [x] 5.1 Run the full suites (`cargo test`, and `npx playwright test` in `e2e/` with `E2E_APP_PORT`/`E2E_STUB_PORT` if the dev container holds 8443). Verify: both green with no CSP violations, including the CSP walk-through in `layout.spec.ts`, which now also unfolds the freshness on the orders and wealth pages.
