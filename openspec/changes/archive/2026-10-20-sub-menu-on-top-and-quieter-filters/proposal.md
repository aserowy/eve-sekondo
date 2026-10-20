# Proposal

## Why

The page header still distracts from the content. The sub-menu hangs below the page title, so it moves down whenever a title is long, and it only reaches the top of the window after the title has scrolled away. Sortable column names are underlined like links in running text. The "Item name" filter looks different from the fields next to it, because it is drawn as a rounded search pill. The character filter runs into the filters beside it, so the filter section reads as one blur.

## What Changes

- On pages with a sub-menu, the sub-menu is the first thing in the page column, at the top of the page. The page title follows below it, then the filter section. The sub-menu keeps sticking to the top of the window. Pages without a sub-menu (characters, wealth, trades) keep the title at the top.
- Table header text, including the sort links, is never underlined, not even on hover. The header row keeps its thicker bottom line, its background and its bold text.
- Every text filter is a plain text input that looks like the other fields. The "Item name" filter loses the search-field styling (rounded shape, magnifier icon, built-in clear button). Its filtering is unchanged.
- The character filter is set apart from the filters beside it by a horizontal gap of 3rem, twice the gap used today. When the filter section wraps, its lines are 0.75rem apart. There is no divider line or border.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ui-layout`:
  - the page structure and the sub-menu: the sub-menu comes above the page title;
  - the shared table style: header text is not underlined;
  - the filter section: plain text inputs for every text filter, and wider spacing around the character filter.

## Impact

- `crates/sekondo/templates/base.html`: order of the sub-menu and the page header.
- `crates/sekondo/templates/opportunities_filters.html`: the "Item name" input type.
- `crates/sekondo/static/app.css`: header link decoration and filter-section gaps.
- `crates/sekondo/static/app.js`: checked for assumptions about the sub-menu's position (the published sub-menu height and the scroll marker).
- `e2e/tests`: page-structure, sub-menu, tables, filter and opportunity tests.
