# Proposal

## Why

The opportunity tables show many derived figures (m, σ, T_max, fill time, minimum daily volume, ISK/day, %B, RSI, SMA crossings) under long headers that make the tables wide. Nothing on the page explains what the figures mean or what to do with a row. The detail page squeezes a long label, the value and a mixed formula/explanation text into every figure row, which is hard to scan. A trader has to read the spec to know how S is chosen, why an item is listed, or how to turn a row into a buy order and a sell order.

## What Changes

- **Short headers**: the opportunity table headers use the symbols of the formulas (B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, Window, Next (UTC), %B, RSI, SMA5, SMA20, Cross). Each is an abbreviation with its full name on hover, so no information is lost. B's full name says it is the current best buy and not a suggestion.
- **One legend**: every short label has one entry (full name, meaning, formula), worded the same on the table pages and the detail pages.
- **Explainer card on each table page**, below the table and pagination and above the freshness footer. It is a closed "How to read this table" disclosure holding, in order:
  1. the table's legend;
  2. the table's signal and how S is derived;
  3. the intermediate shared figures (c, r, U, market share, ρ, q) and the hard exclusion, the only part worded the same on every page, because the maths is the same;
  4. a static worked example of a row of that table, continuing its signal into every column;
  5. that table's order playbook: the common steps (buy U units one tick above B, re-check m, T_fill against T_max), then how to act on that table's own signal (expansion window timing, classic outbid and undercut, reversion sell at SMA 20 without averaging down, trend exit on the SMA crossing back).
  Each table works differently, so its signal, example and playbook are unique.
- **Detail page redesign**: the figures become KPI cards grouped as Fees (b, t, new on the page), Prices, Margin, Size & liquidity, Timing & risk, and Result. Each card shows the short label, the value and, for computed figures, a detail line with this item's numbers. Formulas and meanings move into a closed "Legend" card below the figures, which gets a "Legend" entry in the detail sub-menu.
- The example numbers are static. The cards keep their open state across live refreshes and in-page changes, and the state is not stored.
- The pagination is no longer required to be the last block of the main content: the explainer card may follow it.
- The tables still show no suggested buy price per row. Buy-side guidance is only the explainer's general rule.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `station-opportunities`: short headers with full names on hover, a shared legend, an explainer card on every table page, and the detail page's figures as grouped KPI cards with a legend card.
- `ui-layout`: the table page layout and the pagination requirements allow an explainer card between the pagination and the page footer.

## Impact

- `crates/sekondo/src/opportunities/tables.rs`: short header labels and full names per column.
- A new legend source shared by the headers, the table legend and the detail legend.
- `crates/sekondo/src/opportunities/page.rs`: the detail figures become grouped KPI views with item-number detail lines. The "how" text is replaced, fees are added, and a "Legend" section is added to the sub-menu.
- Templates: `opportunities.html` gets the explainer after `#content`, with new explainer and legend templates. `opportunities_table.html` gets `<abbr>` headers. `opportunities_detail.html` gets grouped KPI cards and the legend card. `static/app.css` gets the card spacing and the group headings.
- Tests: Rust page and analysis tests. Playwright tests for the headers, the explainer, the detail cards and the legend. Existing tests that read `[data-figure] [data-value]` or the "how" text on the detail page, or header labels, are adjusted.
- `README.md`, opportunities section.
- No change to the analysis, the store or the handlers' routes.
