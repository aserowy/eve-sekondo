# Proposal

## Why

The legends on the opportunity pages are a definition list: each term is a bold label followed by a paragraph that runs the full name, the meaning and the formula together. With 15 to 20 terms this is hard to scan, while the worked example right below it, a small table with the label on the left, reads easily. The legends should read the same way.

## What Changes

- Both legends, the one in the "How to read this table" card on every table page and the Legend card on every detail page, become a table instead of a definition list.
- The table has a header row **Label | Name | Meaning | Formula** and one row per term. The label is the row's header, as in the worked example. The name is plain text.
- The Formula cell holds only the right-hand side of the formula in code style, for example `B × (1 + b)` for c and `min(3, (m / 2σ)²) days` for T_max. A term without a formula has an empty Formula cell.
- The legend table looks like the worked example: one shared style for both explainer tables, and never the `data` class, so the one-data-table-per-page rule and the sort and row-click scripts ignore it.
- In a narrow window the meaning wraps inside its cell, the labels and formulas stay on one line, and the page never scrolls sideways.
- The wording of the entries, their order and the cards around them do not change.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `station-opportunities`: the Legend requirement says how a legend is laid out: a table with a header row and one row per term, labels as row headers, the formula's right-hand side, and an empty cell where there is no formula.

## Impact

- `crates/sekondo/templates/opportunities_legend.html`: a table instead of a `<dl>`. It is shared by the explainer card and the detail legend card.
- `crates/sekondo/static/app.css`: the legend and the worked example share one table style; the `.legend dt`/`dd` rules go.
- Tests that read `<dl>`, `<dt>` or `<dd>`: `tests/opportunities_page.rs`, `tests/opportunities_detail.rs`, `e2e/tests/opportunities.spec.ts` and `e2e/tests/opportunities-layout.spec.ts`.
- `README.md`, if it describes the legend's layout.
- No change to `legend.rs`, the handlers, the analysis or the routes.
