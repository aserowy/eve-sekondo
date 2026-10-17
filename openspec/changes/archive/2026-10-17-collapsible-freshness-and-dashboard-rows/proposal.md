# Proposal

## Why

The freshness side columns on the orders and wealth pages permanently take about 24rem of width for information that matters only when something is wrong. On the dashboard, the per-character breakdown is squeezed into half the width, and the over-time graphs share a row with each other and with the top-item graphs, so long time ranges are hard to read.

## What Changes

- On the orders page and the wealth page, the freshness column (with the fees per location on the orders page) starts collapsed to a narrow strip. An icon button in the strip unfolds it, and the same button folds it again. While collapsed, the tables and the graph use the freed width.
- The collapsed strip shows the number of characters with a problem (stale, or needing re-login), so problems stay noticeable without unfolding.
- The unfolded state lasts until the page is reloaded or the user navigates to another page. Live refreshes and in-page view changes (filter, sorting, range) keep it. Every page load starts collapsed. Nothing is stored in the browser or on the server.
- **BREAKING (layout)**: The dashboard is rearranged into rows: the tiles; the characters breakdown across the full width; the per-character split, top items by sell value and top items by buy escrow side by side on one row; then value over time, order count over time and wealth over time, each on its own full-width row. The dashboard now scrolls vertically; the "fits one screen" rule is replaced. The tiles, the breakdown and the split/top-items row stay on the first screen at 1920×1080.
- The freshness toggle is an icon-only control outside the top bar, which the icon rule currently forbids. That rule gets this one exception, with an accessible name and a hover title.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `ui-layout`: "Dashboard fits one screen" is replaced by a row layout that scrolls. The scrolling-panels rule no longer forbids page scrolling on the dashboard. The freshness side column becomes collapsible, collapsed by default, with a warning count. The icon rule allows the freshness toggle as an icon-only control.

## Impact

- Templates: `orders.html`, `orders_freshness.html`, `wealth.html`, `wealth_freshness.html`, `dashboard_summary.html`. Two new vendored Lucide icons (`panel-right-open`, `panel-right-close`) under `templates/icons/`.
- `static/app.css` (dashboard rows, collapsed side column) and `static/app.js` (toggle state for the page session, re-applied after htmx swaps).
- View models for the warning count: `orders.rs` freshness view and `wealth_page.rs` freshness view.
- Tests: the Rust page tests for the warning count; Playwright `layout.spec.ts` (dashboard rows, collapsed columns) and the specs that read freshness content (`orders.spec.ts`, `fees.spec.ts`, `wealth.spec.ts`, `live.spec.ts`, `relogin.spec.ts`, `scroll.spec.ts`), which now unfold the column first.
- No API, database or dependency changes; the CSP stays unchanged.
