# Design

## Context

An opportunity table page (`templates/opportunities.html`) renders its table and pagination inside `<section id="content">`. That section is replaced by every live refresh, pagination and sorting link, the opportunity filters form and the fee character form (`hx-target="#content"`). The character filter selects `#content > *`. The sub-menu links between tables are plain links, so switching tables loads the whole page. The pagination stays at the bottom of a short page because `#content` is a flex column taking main's spare height and `.pagination` has `margin-top: auto`.

The headers come from `SortKey::label` in `opportunities/tables.rs`.

The detail page renders `opportunities_detail.html` inside `#opportunity-detail-content`. Live refreshes, the character filter and the fee character form all replace that element's children. Its figures are built by `detail_figures` in `opportunities/page.rs` as `FigureView { key, label, value, how }` and shown as a 3-column `table.data.figures`. Tests find figures by `[data-figure="<key>"] [data-value]`.

KPI cards already exist (`kpi_card.html`, `kpi::Kpi`, `.kpis` grid). The shared figures are computed by `opportunities::analysis::figures`, and valid prices by `tick_below` and `round_down_valid`. htmx is 2.x and supports `hx-preserve`.

## Goals / Non-Goals

**Goals:**
- One source of wording per short label, used by the headers' hover text, the table legend and the detail legend.
- The explainer and the legend keep their open state across every in-page swap, without storing it.
- The explainer's static numbers cannot drift from the real formulas without a test failing.

**Non-Goals:**
- Live or per-row examples in the explainer, localisation, and storing the open state.
- Changes to the analysis, the charts, or the detail page's other sections.

## Decisions

### A legend module is the single source of the wording
A new `opportunities/legend.rs` holds a `Term` enum (Item, b, t, B, BestSell, S, c, r, Net, m, U, V7, V30, ρ, q_buy, q_sell, σ, T_fill, T_max, V_min, ISK/d, Near, Window, Next, %B, RSI, SMA5, SMA20, Cross). Each term gives `short()`, `name()`, `meaning()` and `formula() -> Option<&str>`. `SortKey` maps to its `Term`, and `SortKey::label` returns `term.short()`. `HeaderView` gains `title` (the full name). Each table's legend is its columns' terms in column order. The detail legend is the terms of the figures shown, in group order.
*Alternative*: write the wording in templates. That duplicates it between the hover text, two legends and the detail cards.

### Headers as `<abbr>`
`opportunities_table.html` renders `<abbr title="{{ h.title }}">{{ h.label }}</abbr>` inside the sort link, except for Item. Sorting keys and URLs are unchanged.

### The table explainer sits outside `#content`
The card is placed after `<section id="content">` in the `content` block of `opportunities.html`, inside `<main>`. No swap targets it, so a `<details>` the user opened stays open without JS. `#content` keeps `flex: 1 0 auto`, so the spare height still goes between the table and the pagination, and the card follows the pagination directly. The legend part needs the table's terms, so `OpportunitiesView` gains `legend: Vec<LegendEntry>`. Everything else in the card is static template text.
*Alternative*: put it inside `#content` and restore `open` in `app.js` after each swap. That needs more code and makes every fragment carry the text.

### The detail legend uses `hx-preserve`
The detail legend must sit between the figures and the charts, which are both inside the swapped `#opportunity-detail-content`. The legend card gets a stable id (`legend`) and `hx-preserve="true"`. htmx then keeps the existing element, open state included, on every swap of its parent. The character filter's `hx-select` swap also keeps it. Its content never changes for a page, because a detail page's table, and so its terms, is fixed by the URL. The figure set can shrink when an item drops out of the table, but the legend still lists every term the table can show, which is acceptable.
*Alternative*: split the detail content into two swap targets around the legend. That doubles the live-refresh wiring.

### Detail figures as grouped KPI cards
`detail_figures` returns `Vec<FigureGroup { title, figures: Vec<FigureView> }>`. `FigureView` becomes `{ key, term, value, detail: Option<String> }`: `term` gives the card name and hover text, and `detail` is the computation with the item's numbers. The "how" prose is removed. Each group renders as an `h3` plus a `.kpis` grid of cards. The cards use their own markup rather than `kpi_card.html`, because they need `data-figure`/`data-value` hooks and an `<abbr>` name, so existing test selectors keep working. Fees b and t come from the fee character's `Fees` already in `Context`.

Detail lines:

| Figure | Detail line |
|---|---|
| c | `B × (1 + b)` with numbers, e.g. "100.00 × 1.0141" |
| r | "S × 0.95215" |
| Net | "r − c" with numbers |
| m | "Net / c" with numbers |
| U | "⌊capital / c⌋" |
| q_buy, q_sell | "1/(n + 1) × V × ρ" with numbers |
| T_fill | "U / q_buy + U / q_sell" with numbers |
| T_max | "min(3, (m / 2σ)²)" with numbers |
| ISK/d | "Net × U / T_fill" with numbers |
| S | the table's derivation with its input, e.g. "1 tick below 11,500.00", "SMA 20 10,537.40 rounded down", "p25 of peaks of daily 18:00" |

b, t, B, Best sell, V7, V30, ρ and σ get no detail line. V_min's formula is not a short expression, so its detail is left out and its meaning is in the legend.

### Explainer templates
- `opportunities_explainer.html` is the card shell: `<article class="explainer"><details><summary>How to read this table</summary>`. It includes `opportunities_legend.html` (a `<dl>` over the entries, also used by the detail legend card), then the table's own part chosen with an `if`/`else if` chain on `view.table.id`.
- `opportunities_explainer_{expansion,classic,reversion,trend}.html` each hold that table's signal, worked example and playbook, and include `opportunities_explainer_shared.html` between signal and example.
- `opportunities_explainer_shared.html` holds only the intermediate figures and the hard exclusion.
- `opportunities_explainer_playbook_common.html` holds the common playbook steps, included at the start of each playbook.
- A worked example is a small two-column table (figure, value with its working) without the `data` class, so the "one data table per page" rule and the sort and row-click scripts ignore it.

Each example shares the fees, capital, V7, ρ and near-order counts. Only B, S, σ and the table's own context vary, so the four examples are easy to compare. The numbers were chosen so every example clears the default filters (m ≥ 5%, ISK/d ≥ 5M) and fills within T_max. The reversion example uses a lower B (9,000.00) and a higher σ (3%), so its T_max is below the 3-day cap and shows the formula at work.

### Pinning the static numbers
A test in `tests/analysis.rs` feeds each of the four worked examples through `figures` and asserts every value its card prints, as listed in the spec's example table, and that each passes `evaluate` and the default filters. It also checks the signal examples via `tick_below` (11,500.00 → 11,490.00, 11,800.00 → 11,790.00), `round_down_valid` (10,537.40 → 10,530.00) and the expansion percentile (through `recurring_windows`, or `tick_below(1_252_500)` with the percentile arithmetic written in the test). It checks the outbid prices one tick above B (10,010.00 and 9,001.00) with the resulting c and m. A page test asserts that each rendered card contains its own exact strings and none of the other tables' examples.

## Risks / Trade-offs

- [Short headers are cryptic to new users] → `<abbr>` hover on every header, the legend first in the explainer, and the B entry says "not a suggested price".
- [`hx-preserve` keeps stale legend content] → The detail legend's content is fixed per URL, so nothing is stale. A future change that makes it dynamic must drop `hx-preserve`.
- [Existing tests rely on header text, the "how" text or `table.figures`] → Tasks adjust them in the group that changes the markup. The `data-figure`/`data-value` hooks are kept.
- [Static explainer text drifts from the spec] → The analysis test pins the numbers, and the wording of the terms lives in one module.
