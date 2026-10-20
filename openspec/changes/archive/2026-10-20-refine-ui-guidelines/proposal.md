# Proposal

## Why

The page structure from the last layout changes still has rough edges. Graphs float without a frame next to framed KPI cards. The main menu's toggle and "Sign out" do not line up with the links. Sub-menu entries are spread across the bar instead of starting at the left. The pagination jumps up and down with the table's height. Table cells feel cramped. The filter section is one loose run of inline labels and inputs, with its error as stray text. Writing these points down as layout guidelines makes every page follow them, today and for future pages.

## What Changes

- Every graph is shown inside a card that holds its section title, the chart and its empty-state text, with no header band.
- In the main menu, every entry's icon starts at the same left edge, and in the expanded menu every label too. The collapse toggle's icon sits at the right edge of the expanded sidebar and lines up with the other icons when the sidebar is collapsed.
- The entries of every sub-menu (page and section sub-menus) are packed at the left edge with a fixed gap and are not spread over the bar's width.
- The pagination of a table page sits at the bottom of the window, directly above the freshness footer, when the page is shorter than the window. When the page is taller, it follows the table and scrolls into view. It stays part of the main content, not of the footer.
- Table cells get more room: padding of 0.3rem vertically and 0.75rem horizontally. The row limit is stated in rem (1.75rem) instead of px.
- The filter section follows Pico's form guidelines:
  - labels above their inputs, with the fields in a wrapping grid;
  - the character checkboxes in a fieldset with a "Characters" legend;
  - the time range as a button group;
  - a rejected value marks its own input invalid and shows its message as helper text below that input.

  Inputs keep Pico's default sizing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ui-layout`: new requirements for graph cards and main menu alignment. Changes to:
  - the sub-menu: entries packed at the left;
  - the pagination: placement at the page bottom;
  - the shared table style: cell padding;
  - the filter section: Pico form structure and per-field errors;
  - the page structure: the even-spacing scenario no longer covers the pagination;
  - the dense theme: what the application stylesheet may set.

## Impact

- `crates/sekondo/static/app.css`: sidebar, sub-menu, table cell, pagination placement, filter section and graph card rules.
- Templates:
  - `base.html`: sidebar toggle;
  - `character_filter.html`, `opportunities_filters.html`, `opportunities_filter_error*.html`: filters and errors;
  - the dashboard, wealth and trades range controls;
  - `dashboard_summary.html`, `wealth_content.html`, `opportunities_detail.html`: graph cards;
  - the table pages' content wrappers: pagination placement.
- `crates/sekondo/src/opportunities/settings.rs` and `page.rs`: validation names the rejected field, so its input can be marked.
- Playwright tests in `e2e/tests` (layout, sidebar, subnav, tables, filter, opportunities, page-structure, freshness), plus the Rust template tests that assert on the changed markup.
