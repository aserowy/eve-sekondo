# Tasks

## 1. Table cells and pagination placement

- [x] 1.1 Change `th, td` padding to `0.3rem 0.75rem`. Verify with a Playwright test in `e2e/tests/tables.spec.ts` that checks the cell padding on the trades page and a row height of at most 1.75rem on the sell orders page, and rewrite the old 28px row-height assertion in `pico.spec.ts` in rem.
- [x] 1.2 Make `main` and `section#content` flex columns and give `.pagination` `margin-top: auto`, so the pagination sits above the footer on short pages and follows the table on long ones. Verify with Playwright tests in `e2e/tests/freshness.spec.ts` or `page-structure.spec.ts` for these scenarios: short sell page at 1920×1080, 200 orders at 1280×800 scrolled to the end, page 2 of 60 orders, and pagination inside `main`.
- [x] 1.3 Replace the "Even spacing" test with the wealth-page version (gap from the KPI cards to the graph equals the gap from the graph to the items table), and check that the "footer at the bottom" and "footer scrolls in" tests still pass with the flex `#content`. Verify with `npx playwright test --workers=2 page-structure freshness`.

## 2. Graph cards

- [x] 2.1 Turn every graph `<section>` in `dashboard_summary.html`, `wealth_content.html` and `opportunities_detail.html` into an `<article class="graph">` that keeps its id, title, chart and empty-state text. Add the margin and row-stretch rules to `app.css`. Verify with Playwright tests for the "Dashboard graphs in cards", "Wealth graph in a card", "Empty graph keeps its card" and "Shared row" scenarios in `dashboard.spec.ts` and `wealth.spec.ts`.
- [x] 2.2 Check that section sub-menu jumps and the scroll marker still land on the card titles. Verify that `subnav.spec.ts` and `scroll.spec.ts` pass, plus a new assertion that "Wealth" lands with the card's title below the sub-menu. Also update any Rust template tests in `crates/sekondo/tests` that match the old `<section>` markup, and verify they pass.

## 3. Main menu alignment and sub-menu packing

- [x] 3.1 Reset Pico's button centring on the sidebar controls so that every icon starts on one left edge and every label on one left edge. Put the toggle at the right edge when the sidebar is expanded and in the icon column when it is collapsed. Verify with Playwright tests in `e2e/tests/sidebar.spec.ts` for "Expanded menu lines up", "Toggle on the right" and "Collapsed menu lines up", using bounding boxes.
- [x] 3.2 Pack the entries of every sub-menu at the left with one fixed gap, overriding Pico's `nav ul` spreading where it applies. Verify with a Playwright test in `e2e/tests/subnav.spec.ts` for "Entries packed at the left": "Sell" at the left edge, the same gap as on the dashboard, and "Buy" ending in the left half at 1920px.

## 4. Filter structure (Pico forms)

- [x] 4.1 Change the character filter to a fieldset with the legend "Characters" and the checkboxes side by side. Remove the `.filter-box` label and input overrides and set `align-items: flex-end`. Verify with a Playwright test in `e2e/tests/filter.spec.ts` for "Characters legend" and with the "Form fields keep Pico's size" test in `pico.spec.ts`. Also update `filter.spec.ts` and the Rust tests that select the fieldset by `aria-label`.
- [x] 4.2 Lay out the opportunity filters as a grid of label-above-input fields of equal width. Verify with a Playwright test in `e2e/tests/opportunities-layout.spec.ts` for "Labels above inputs" at 1920×1080, and check that the existing "Narrow window wraps" test passes.
- [x] 4.3 Turn the dashboard, wealth and trades range controls into a `role="group"` group of `a[role=button]` links with a visible "Range" label, the current range highlighted and `aria-current`, and the others secondary outline. Verify with a Playwright test in `e2e/tests/trades.spec.ts` for "Range as a button group". Also update the dashboard and wealth range tests and check that live range switching still works (`npx playwright test --workers=2 dashboard wealth trades`).

## 5. Per-field filter errors

- [x] 5.1 Make `Form::validate` return the rejected field together with its message, and keep the existing messages. Verify with Rust unit tests in `crates/sekondo/src/opportunities/settings.rs` (or `tests/opportunities_settings.rs`) that each invalid input names its own field (`CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test`).
- [x] 5.2 Render the rejected filter form with `aria-invalid`, `aria-describedby` and a `<small>` message under the bad input. Point `data-invalid-target` at the form. Make a successful save return the form in its clean state out of band, and remove the `#filter-error` span. Verify with Playwright tests in `e2e/tests/opportunities.spec.ts` for "Error at its field" and "Error cleared". Also add a test that text typed into another field during a save is kept, and update the Rust tests that assert on `filter-error`.

## 6. Docs and integration

- [x] 6.1 Update the layout section of `README.md` (sidebar, graph cards, pagination placement, filter structure) and check it against the spec delta.
- [x] 6.2 Run the full suites and confirm they are green: `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test` and `npx playwright test --workers=2`. This includes the CSP test across all pages.
