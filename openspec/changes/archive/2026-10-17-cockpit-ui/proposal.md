# Proposal

## Why

Pico CSS is a reading-oriented framework: large type, roomy padding, card-like articles, a centred max-width container and 18rem charts stacked one under another. A trading tool needs the opposite: one glance at the screen should show as many figures as possible. Today the dashboard needs several screens of scrolling and the orders tables spend a full line on location and region that are rarely needed.

## What Changes

- **BREAKING (look & feel)**: Remove Pico CSS and replace it with a small, self-written, dense stylesheet based on CSS custom properties (dark cockpit theme, ~13px type, ~20px table rows, tabular numbers, thin panel borders, small uppercase panel titles, one accent colour plus success/error colours). No third-party CSS.
- Use the full viewport width on every page instead of a centred max-width container; desktop-first for widths of 1280px and more, narrower widths only wrap.
- Replace the navigation with a single thin top bar that also holds the page's controls (the character filter and, on the dashboard, the time range control). Drop the per-page `<h1>` headers.
- Characters and Sign out in the top bar become icon-only controls (with their text as accessible name and hover title). Other actions (Unlink, Add character, Log in again) get an icon in front of their text. Icons come from Lucide, vendored as inline SVG.
- Lay the dashboard out as a panel grid that fits a 1920×1080 viewport without page scrolling; panels scroll internally, chart heights follow the grid.
- Orders page: freshness and fees move into a narrow right-hand side column; sell and buy tables are stacked, scroll internally with sticky headers, and each order is shown on two lines — figures under sortable headers on the first, `Issued · Character [Corp] · Location · Region` as one dimmed line on the second.
- **BREAKING**: Orders can no longer be sorted by character, issued date, location or region; a URL that asks for one of those falls back to the default sort (item ascending).
- Wealth page uses the same pattern: graph panel on top, items table filling the rest with sticky headers, freshness in a side column.
- Chart.js charts get smaller, dimmer axes, grid lines and legends; data and tooltips are unchanged.

## Capabilities

### New Capabilities
- `ui-layout`: The application-wide visual layout: dense theme without a third-party CSS framework, full-width pages, the top bar holding page controls, the no-scroll dashboard grid, internally scrolling panels with sticky table headers, side columns for freshness, and the icon rules.

### Modified Capabilities
- `navigation`: Characters and Sign out are icon-only controls that keep their names for assistive technology and on hover.
- `orders-overview`: Order rows are split into a figures line and a dimmed details line (issued, character, corp marker, location, region); sorting is limited to the figure columns; the standing and margin styles no longer refer to "the page's CSS framework".

## Impact

- `crates/sekondo/static/pico.min.css` removed; `crates/sekondo/static/app.css` rewritten; `crates/sekondo/static/app.js` chart defaults adjusted.
- All templates in `crates/sekondo/templates/` (base, nav, dashboard, orders, wealth, characters, landing, error and their fragments); new `templates/icons/` with vendored Lucide SVGs and the ISC license.
- `crates/sekondo/src/orders.rs` sort keys and table headers; the nav view model in `pages.rs` gains page-control slots.
- Rust tests touching sort keys or headers (`tests/orders_page.rs`, `tests/navigation.rs`) and Playwright tests that assume the column layout (`e2e/tests/orders.spec.ts`) or Pico markup.
- No API, database or dependency changes; the Content Security Policy stays unchanged.
