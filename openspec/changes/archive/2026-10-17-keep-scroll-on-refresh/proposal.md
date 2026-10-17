# Proposal

## Why

The cockpit layout (change `cockpit-ui`) puts the orders tables, the dashboard breakdown, the wealth items and the freshness columns into panels that scroll on their own. Live refreshes replace those panels' content, which resets each panel to the top. A trader who scrolled to order 150 of 200 jumps back to the start whenever a poll, market or fee signal arrives, which happens several times an hour.

## What Changes

- When a page refreshes a part of itself because of a live signal, or after the stream reconnects, every scrolling panel in that part keeps its vertical and horizontal scroll position.
- Changes the user makes (character filter, sorting, time range, navigation) still show the new view from the top. Restoring a position in a different view would be misleading.
- If the refreshed content is shorter than the old position, the panel shows its end. No anchoring to a specific row.
- Depends on `cockpit-ui`, which introduces the scrolling panels. That change must be applied first (it is implemented but not yet archived).

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `live-updates`: adds a requirement that live refreshes keep the scroll position of scrolling panels, and that user-initiated view changes do not.

## Impact

- `crates/sekondo/static/app.js`: records panel scroll positions before a signal-triggered swap and restores them after it.
- Templates of the scrolling panels (`orders_table.html`, `orders_freshness.html`, `dashboard_summary.html`, `wealth_items.html`, `wealth_freshness.html`) get a stable key per panel.
- New Playwright scenarios for the orders, dashboard and wealth pages. No server, API, database or dependency changes. The Content Security Policy is unchanged; no inline script or style is added.
