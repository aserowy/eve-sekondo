# Tasks

## 1. Filter input height and row gap

- [x] 1.1 Add the 2rem filter input rule (`height: 2rem; padding-block: 0` for non-checkbox and non-radio inputs in `.filter-box`) and the `0.75rem 1rem` gap on `#opportunity-filters` in `crates/sekondo/static/app.css`. Verify with a Playwright test in `e2e/tests/filter.spec.ts`: on the trend page every opportunity input's height equals 2 × the root font size and its font size equals the body font size. In a narrow window, each second-row label's top is 0.75rem below the input above it. The existing "Item name is a plain input", "Groups aligned at the top" and "Error does not move neighbours" tests still pass.

## 2. Maximum margin filter

- [x] 2.1 Add `max_margin` (default 0.80) to `Settings` and `Form` in `opportunities/settings.rs`. Add `Field::MaxMargin` (`max_margin`), and validate it in form order: a number ≥ 0 and ≥ the minimum margin. Empty is rejected with "must be a number of at least 0 percent", below the minimum with "must be at least the minimum margin". Verify with unit tests in `settings.rs` for the defaults, `Form::of`, the valid case, empty, negative, below min, equal to min, and first-rejected-field order.
- [x] 2.2 Add a migration adding `max_margin numeric NOT NULL DEFAULT 0.80` to `opportunity_settings`, and read and write it in `store/pg/opportunity.rs` and `store/memory/opportunity.rs`. Verify with a test in `tests/opportunities_settings.rs`: a saved maximum margin is read back from both stores, and an account without saved settings reads 80%.
- [x] 2.3 Drop rows with `margin > max_margin` in `tables::rows`, and give `tables::detail` the reason "Its net margin is above your maximum margin." Verify with tests in `tests/opportunities_tables.rs` and `tests/opportunities_detail.rs`: 120% is hidden at 80% and listed at 150%, 80% exactly is listed, and the detail reason is shown.
- [x] 2.4 Add the "Maximum margin (%)" field after "Minimum margin (%)" in `templates/opportunities_filters.html`, with its error helper text. Verify with a Playwright test in `e2e/tests/opportunities.spec.ts`: the default shows 80%; entering 5 with a minimum of 10 marks only that input invalid with the message and keeps the table; clearing it shows the empty-value message. Update the "Labels above inputs" test in `filter.spec.ts` to five equally wide inputs.

## 3. Page sizes and default

- [x] 3.1 Change `PER_PAGE_SIZES` to `[10, 25, 50, 100, 200]` and `DEFAULT_PER_PAGE` to 25 in `pagination.rs`. Add `Paging::parse_or(page, per_page, fallback)` and `Paging::query_or(fallback)`, with `parse`/`query` delegating to them with the default. Verify with the `pagination.rs` unit tests, updated for 25 and the new size list, plus new tests for the fallback on a missing `per_page`, 25 on an unknown one, and `query_or` leaving out the fallback size.
- [x] 3.2 Update the tests that rely on 50 rows by default, so the full suite is green with the new default. Use an explicit `per_page` where a test needs a large page, or change the expected counts to 25. This covers the Rust integration tests for orders, wealth, trades and opportunities, and the e2e `layout`, `live`, `subnav` and `opportunities-layout` specs. Add a Playwright test in `e2e/tests/layout.spec.ts`: the sell orders page's page-size control offers 10, 25, 50, 100, 200, and 120 orders show 25 rows with "Page 1 of 5". Choosing 10 shows "Page 1 of 12" with `per_page=10` in the URL.

## 4. Stored opportunity page size

- [x] 4.1 Add a migration adding nullable `per_page integer` to `opportunity_settings`. Add `load_page_size`/`save_page_size` to `OpportunityStore`, with PG and memory implementations that leave the filters and the fee character untouched. Verify with tests in `tests/opportunities_settings.rs`: round trip in both stores; saving the filters keeps the page size and saving the page size keeps the filters; account A's size does not change account B's.
- [x] 4.2 Add `POST /opportunities/page-size` (`per_page=<n>`): store it when `n` is in `PER_PAGE_SIZES` and answer 204, answer 422 without storing otherwise, and require CSRF like the other opportunity POSTs. Verify with tests in `tests/opportunities_settings.rs` for accepted sizes, an unknown size, a missing CSRF token (rejected, stored size unchanged) and an unauthenticated request.
- [x] 4.3 Load the stored size in the opportunity `environment()`. Parse paging with `parse_or(stored or 25)` and build every view-state link with `query_or(stored)`, for both the page and the content fragment. Verify with tests in `tests/opportunities_page.rs`: without `per_page` the stored size is used; `per_page` in the URL wins and is not saved; the size-option links leave out `per_page` for the option equal to the stored size; and the sell orders page ignores the stored size and shows 25.
- [x] 4.4 Add `save: Option<String>` to `PageView` and render it as `data-save` on the select in `pagination.html`, set only by the opportunity page. In `app.js`, when `data-save` is present, POST the size with `source: select` (inheriting the CSRF header) before the existing content GET and `pushState`. On failure, skip the GET and reset the select. On opportunity pages, build each size option's link for that option as the stored size. Verify with a Playwright test in `e2e/tests/opportunities.spec.ts`:
  - choosing 10 on the trend page, opening a row's detail and activating "← Trend" shows 10 rows per page with 10 selected;
  - choosing 50 on classic and switching to "Mean reversion" in the sub-menu shows up to 50 rows;
  - the size is kept after signing out and in;
  - a live `opportunities-updated` refresh keeps it;
  - the explainer's open state and the pagination placement are unchanged by a size change.

## 5. Integration check

- [x] 5.1 Run the full suites sequentially (`CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test` and `npx playwright test --workers=1`). Verify that all tests are green and that `openspec validate tune-opportunity-filters-and-page-size --strict` passes.
