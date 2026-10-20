# Tasks

## 1. Filter groups aligned at the top

- [x] 1.1 Set `.filter-box` to `align-items: flex-start` in `app.css` and check that the range label keeps its buttons level with the inputs. Verify with Playwright tests in `e2e/tests/filter.spec.ts`:
  - "Groups aligned at the top" on the trend page at 1920×1080: the "Characters" legend and the "Minimum margin (%)" label share their top edge (the "Fees" legend is added to this test in 4.1);
  - "Character legend level with the range label" on the trades page;
  - "Wrapped fields do not move the character filter" on the trend page in a window where the opportunity filters wrap to two rows beside the character filter;
  - "Error does not move neighbours": entering "-1" as the minimum margin leaves the character filter's top edge where it was.
  
  Also check that `opportunities-layout.spec.ts` "Narrow window wraps" and the other filter tests still pass.

## 2. Station name as subtitle

- [x] 2.1 Add an optional `page_subtitle` block to `base.html` that renders `<hgroup><h1>…</h1><p>…</p></hgroup>` only when filled, and move the title margin rule so it holds for both forms. Fill it with the station name in `opportunities.html` and `opportunity_detail.html`. Verify with Playwright tests in `e2e/tests/page-structure.spec.ts`: "Station as subtitle" on the trend page and a trend detail page (dimmed, body font size, one `h1`, no other heading above the filter section) and "No subtitle elsewhere" on the sell orders page. "Title on every page" and "Sub-menu at the top of the page" must still pass.

## 3. Stored fee character and the fee-choice rule

- [x] 3.1 Add the migration that adds a nullable `fee_character_id BIGINT` to `opportunity_settings`. Add `load_fee_character` and `save_fee_character` to `OpportunityStore`, with pg and memory implementations, and make sure `save_settings` leaves the column alone and saving a fee character creates a default settings row when none exists. Add contract cases in `crates/store-contract` (round trip, per-account isolation, filters saved after a fee character keep it, and the other way round). Verify with `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test`, including the Postgres-verified contract run.
- [x] 3.2 Extend `analysis::choose_fees` with a preferred character: it wins when selected (base rates if its fees are unknown), otherwise the lowest-minimum-margin rule applies. Pass the stored choice through `tables::choose_station_fees` into the page `Environment` for table and detail pages. Verify with unit tests in `crates/sekondo/tests/analysis.rs`: preferred character wins over a better one, unknown fees of the preferred character give base rates, a preferred character that is not selected falls back to the best one, and no characters gives base rates.

## 4. Fees group in the filter section

- [x] 4.1 Replace `opportunities_filter_context.html` with an `opportunities_fee_filter.html` group (`<form id="fee-filter">` with the "Fees" legend, one radio per selected character with the fee character checked, and helper text with the rates or the base-rates note). Include it in the table and detail pages and push it out-of-band from both content fragments in place of `#filter-context`. Delete the `#filter-context` CSS. Update the Rust tests that assert `filter-context` or "Fees of" (`opportunities_settings.rs`, `opportunities_detail.rs`, `opportunities_tables.rs`) to assert the radios and note instead. Verify with Playwright tests in `e2e/tests/opportunities.spec.ts` / `opportunities-layout.spec.ts`:
  - "Two characters": the best one is checked by default;
  - "No fee data yet": the helper text says base rates;
  - "Nobody selected": no radios and a base-rates note;
  - "Chosen character deselected" via the character filter;
  - add the "Fees" legend to "Groups aligned at the top" from 1.1;
  - rewrite the existing `#filter-context` assertions.
- [x] 4.2 Add `POST /opportunities/fee-character` in `page.rs` and `app.rs`: CSRF-checked like the settings endpoint, 422 without saving for a character not linked to the account, saves the choice, then answers with page 1 of the content fragment (`HX-Replace-Url`) or, when `type_id` is posted, with the detail fragment. Verify with Rust integration tests (saves and recomputes, foreign character rejected and stored choice unchanged, missing CSRF token rejected, choice stays within the account) and with Playwright tests:
  - "Choose another character" on the trend page: table recomputed, helper text updated, still in use on the classic page after signing in again;
  - "Choose fees on the detail page": URL kept, figures recomputed, trend page uses it;
  - "Choice applies on the detail page".

## 5. Docs and integration

- [x] 5.1 Update `README.md`: the layout section (top-aligned filter groups, subtitle) and the opportunities section (the Fees group and its default rule). Check it against both spec deltas.
- [x] 5.2 Run the full suites and confirm they are green, including the CSP test across all pages: `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test` and `npx playwright test --workers=2`.
