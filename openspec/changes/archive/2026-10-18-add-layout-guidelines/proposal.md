# Proposal

## Why

The opportunities page breaks almost every convention the dashboard, orders, wealth and characters pages follow. It has a page heading, loose paragraphs outside panels, an inline freshness list, four tables of up to 50 rows that stretch the page, and a floating detail window full of tables and charts. Those conventions are only implicit today, spread over page-specific requirements, so nothing stops the next page from drifting the same way. This change writes them down as general layout guidelines and brings every page in line with them.

## What Changes

- **New general layout guidelines** in `ui-layout`:
  - Two page types. *Window pages* fill the window and never scroll as a whole (orders, wealth). *Scrolling pages* stack full-width rows and scroll (dashboard, characters, opportunities, opportunity detail).
  - On scrolling pages, a table shows at most 20 rows and then scrolls inside its panel with a sticky header. On window pages, tables fill their panel.
  - A table that is too wide scrolls sideways inside its panel. Cells do not wrap, and the page never scrolls sideways.
  - No page headings: every content block is a titled panel.
  - One shared data-table style, with the same striping, hover and clickable rows everywhere.
  - Details are always shown on a page of their own, never in a floating window.
- **BREAKING (layout)**: the top bar holds only the brand, the navigation, and the characters and sign-out icons. The character filter and the dashboard time range leave the top bar.
- **New filter box**: a thin full-width strip below the bars. It holds the filters that affect more than one content block, plus page-wide context text and validation errors. A filter that affects a single content block sits in that block's panel title. This moves the character filter on the dashboard, orders, wealth and opportunities pages, and the dashboard time range. The wealth time range stays in its graph panel.
- **New section sub-menu**: scrolling pages with more than one row get a bar of links that jump to each row through URL fragments. A scroll observer marks the row in view. The dashboard gets one too.
- On scrolling pages, the top bar, the sub-menu and the filter box stay at the top while the page scrolls.
- The collapsible freshness column becomes a general pattern for status and freshness. On scrolling pages it is sticky and as tall as the window below the bars.
- **Opportunities page reworked**:
  - Four stacked table panels, each showing at most 20 rows, with a sub-menu entry per table that shows its row count.
  - The filter settings move into the filter box. The Apply button goes, because a change already saves.
  - The station name and the "fees of X" note move to the right end of the filter box. The heading and the loose paragraphs go.
  - The freshness moves into the collapsible column, with a warning count.
- **BREAKING**: the opportunity detail floating window is replaced by a detail page at `/opportunities/<table>/<type id>` that a row click navigates to. It shows the figures and each chart as rows, and it has its own sub-menu with a link back to the opportunities page.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `ui-layout`:
  - Adds the page types, row limit, wide tables, filter box, sub-menu, sticky bars, no-heading and shared table style guidelines.
  - Narrows the top bar.
  - Generalises the scrolling panel, freshness column and collapsible freshness requirements to any page.
  - Changes the dashboard breakdown limit to 20 rows.
  - Lays out the opportunities and detail pages.
- `station-opportunities`:
  - Changes the page layout: filter box, sub-menu, stacked capped tables and freshness column.
  - Removes the Apply button.
  - Replaces the floating detail window with a detail page.
  - Live refresh covers the detail page.
- `live-updates`: the opportunities signal keeps refreshing the opportunities page, now including its sub-menu counts. It also refreshes an open opportunity detail page, in place of the old detail window.

## Impact

- **Templates**: `base.html` gains a sub-menu block and a filter box block. `dashboard.html`, `orders.html`, `wealth.html`, all `opportunities*.html` and the shared table templates change, and there is a new opportunity detail page template.
- **`static/app.css`**: page-type layouts, sticky bars, filter box, sub-menu, a `table.data` style that replaces `table.orders` and `table.opportunities`, a 20-row cap, and the removal of the floating-window styles.
- **`static/app.js`**: a scroll observer for the sub-menu. The floating-window code goes, and scroll restoration and the freshness toggle are adjusted for scrolling pages.
- **Rust (`crates/sekondo/src/opportunities/page.rs`, `app.rs`)**: a detail page route and handler that replace the detail fragment window. Row links become navigation. The views gain row counts, and the opportunities freshness gets a warning count.
- **Playwright tests (`e2e/tests`)**: layout, navigation, opportunities and live tests are updated, and new tests cover the filter box, the sub-menu and the detail page.
