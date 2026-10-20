# Tasks

## 1. Pin the example numbers

- [x] 1.1 In `crates/sekondo/tests/analysis.rs`, add one test per table that runs its worked example through `figures` and `evaluate`. Shared inputs: b = 1.41%, t = 3.375%, capital 100,000,000, V = 80,000, ρ = 0.5, 3 buy / 1 sell near orders. Per table: B, S and σ as in the spec's example table. Assert every row of that table (c, r, Net, m, U, q_buy, q_sell, T_fill, T_max, V_min, ISK/d), that the row is not excluded and clears m ≥ 5% and ISK/d ≥ 5M, and the outbid price one tick above B with its c and m. Verify the tests pass.
- [x] 1.2 Add assertions for the signal inputs: `tick_below` of 11,500.00 gives 11,490.00 and of 11,800.00 gives 11,790.00, `round_down_valid` of 10,537.40 gives 10,530.00, and the expansion peaks give S = 12,520.00 (through `recurring_windows`, or the design's fallback). Verify the tests pass.

## 2. Legend source and short headers

- [x] 2.1 Add `opportunities/legend.rs` with `Term` (short label, full name, meaning, optional formula) for every term the spec's Legend requirement lists, plus a function giving a table's terms in column order and one giving the detail figures' terms in group order. Add unit tests in `tests/` that every `SortKey` maps to a term, that B's name says "not a suggested price", and that the T_max formula reads "min(3, (m / 2σ)²) days". Verify with `cargo test`.
- [x] 2.2 Switch `SortKey::label` to the short labels, add `title` to `HeaderView`, and render `<abbr title>` headers in `opportunities_table.html` (not for Item). Update the Rust page tests and existing Playwright tests that match the old header text. Add a Playwright test in `e2e/tests/opportunities.spec.ts`: the trend headers read Item, B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, SMA5, SMA20, Cross; the B header's `<abbr>` title says "not a suggested price"; clicking "σ" still sorts. Verify with `cargo test` and `npx playwright test --workers=1`.

## 3. Explainer card on the table pages

- [x] 3.1 Add `legend: Vec<LegendEntry>` to `OpportunitiesView`. Add `opportunities_legend.html` (a `<dl>` of entries), `opportunities_explainer.html` (an `<article class="explainer">` with a closed `<details>`, the summary "How to read this table", the legend and the table's part by id), `opportunities_explainer_shared.html` (intermediate figures with formulas and the hard exclusion) and `opportunities_explainer_playbook_common.html`. Add the classic part `opportunities_explainer_classic.html` (signal, shared include, worked example as a two-column table without `class="data"`, and playbook). Include the card after `<section id="content">` in `opportunities.html`. Add a Rust page test in `tests/opportunities_page.rs`:
  - the card is after `#content` and before the freshness on every table page;
  - its legend lists that table's columns only;
  - it holds no `table.data`;
  - the classic card contains its exact example and playbook strings (11,490.00, 10,141.00, 7.88%, 9,860, 39,440, 5.33M, 10,010.00, 7.77%).

  Verify with `cargo test`.
- [x] 3.2 Add `opportunities_explainer_{expansion,reversion,trend}.html`, each with its signal, worked example and playbook from the spec's example table and playbook list. Extend the Rust page test:
  - each page shows its own example strings (expansion 12,520.00, 17.55%, 11.87M, "daily 18:00"; reversion 9,000.00, 10,530.00, 9.85%, 2.70 days, 48,756, 5.99M, 9,001.00; trend 11,790.00, 10.70%, 7.23M, 10.59%) and none of the other tables';
  - the shared figures part is byte-identical on all four;
  - an empty table shows "No opportunities." followed by the card.

  Verify with `cargo test`.
- [x] 3.3 Add the card's spacing in `static/app.css`, matching the main content's block gap, with no change to the `#content` flex rules. Add Playwright tests in `e2e/tests/opportunities-layout.spec.ts`:
  - the card is closed on load and opens to show legend, signal, shared figures, example and playbook, and that the reversion and trend pages show different examples;
  - on a short page at 1920×1080, the pagination is directly above the card and the card directly above the freshness footer, with free space between the table and the pagination;
  - after opening, a live `opportunities-updated` refresh, page 2, a sort change and a minimum-margin change leave it open;
  - no horizontal overflow.

  Adjust the existing opportunity pagination placement tests. Verify with `npx playwright test --workers=1`.

## 4. Detail page: grouped figure cards and legend

- [x] 4.1 Rework `detail_figures` into `FigureGroup`s (Fees, Prices, Margin, Size & liquidity, Timing & risk, Result) of `FigureView { key, term, value, detail }`. Add the b and t figures, build the item-number detail lines from the design's table, and drop the "how" prose. Update the Rust tests in `tests/opportunities_detail.rs` and `tests/opportunities_fees.rs`: group order, c's detail line "100.00 × 1.0141", no card containing "B × (1 + b)", and no detail line on b, t, B, Best sell, V7, V30, ρ and σ. Verify with `cargo test`.
- [x] 4.2 Render the groups in `opportunities_detail.html` as an `h3` per group and a `.kpis` grid of cards. Each card has an `<abbr>` name, `data-figure`, a `data-value` value and a dimmed detail line. Remove `table.data.figures`. Add group heading styles in `static/app.css`. Update the Playwright tests that read the "how" text (e.g. `e2e/tests/opportunities.spec.ts` "(r − c) / c"). Add a Playwright test that a classic detail page shows the six groups in order, the c card's detail line, and no horizontal overflow at 1280px. Verify with `npx playwright test --workers=1`.
- [x] 4.3 Add the detail legend card (`<article id="legend" hx-preserve="true">` with a closed `<details><summary>Legend</summary>` and `opportunities_legend.html`) between the figures and the first chart. Add "Legend" after "Figures" in the section sub-menu. Update the Rust sub-menu test (expansion: "← Recurring spread expansion", "Figures", "Legend", charts, "Next occurrences"). Add Playwright tests:
  - the legend is closed on load and lists the detail terms with names and formulas;
  - the "Legend" sub-menu jump shows its title below the sub-menu;
  - after opening it, an `opportunities-updated` refresh and a fee character change keep it open while the figures change.

  Verify with `cargo test` and `npx playwright test --workers=1`.

## 5. Docs and integration

- [x] 5.1 Document the short headers, the explainer card, the detail figure groups and the legend in the Station trading opportunities section of `README.md`. Verify that the README matches what the pages show.
- [x] 5.2 Run the full `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test` and `npx playwright test --workers=1`. Verify both are green.
