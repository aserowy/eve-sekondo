# Tasks

## 1. Legend as a table

- [x] 1.1 In `tests/opportunities_page.rs` and `tests/opportunities_detail.rs`, switch the legend helpers from `<dl>`/`<dt>` to the legend table's row headers. Add Rust page tests: the explainer legend and the detail Legend card each hold a `table.legend` that is not `class="data"`, with the header row Label, Name, Meaning, Formula; the classic detail legend's row c reads c, Cost per unit, its meaning and `<code>B × (1 + b)</code>` (no "c ="); the row B has an empty Formula cell; the row orders stay as before. Then rewrite `opportunities_legend.html` as a table (`<thead>` of `th scope="col"`, one `<tbody>` row per entry with the label as `th scope="row"`, the name as plain text, the formula's right-hand side in `<code>` or nothing). Verify with `cargo test`.
- [x] 1.2 Give the worked example tables and the legend table the shared `explainer-table` class (keep `example` on the worked examples), move the `article.explainer table.example` rules to `.explainer-table` so they apply in the detail Legend card too, and drop the `.legend dt`/`dd` rules. Wrap the legend table in its own full-width `overflow-x: auto` container (not `.table-wrap`); the meaning cells wrap, the label and formula cells stay on one line. Update the Playwright tests that read `dl`, `dt` or `dd` (`e2e/tests/opportunities.spec.ts` detail legend test, `e2e/tests/opportunities-layout.spec.ts` explainer test). Add Playwright tests:
  - the detail Legend card's table has the header row Label, Name, Meaning, Formula, row T_max shows `min(3, (m / 2σ)²) days` and row B an empty Formula cell, and its row headers share the worked example's computed font weight;
  - on the trend page at 600×900 with the explainer open, a meaning cell spans more than one line, every label and formula cell is one line high, and the page has no horizontal overflow.

  Verify with `npx playwright test --workers=1`.

## 2. Docs and integration

- [x] 2.1 Say in the Station trading opportunities section of `README.md` that the legends are tables (Label, Name, Meaning, Formula) styled like the worked example. Verify the README matches the pages.
- [x] 2.2 Run the full `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test` and `npx playwright test --workers=1`. Verify both are green.
