# Tasks

## 1. Sub-menu above the page title

- [x] 1.1 In `base.html`, move `{% block subnav %}` above `<header class="page-header">` and reset the title's top margin in `app.css`. Verify with Playwright tests in `e2e/tests/page-structure.spec.ts`:
  - rewrite "Header order" so the sub-menu is above "Buy orders" and the title is above the filter section;
  - add "Sub-menu at the top of the page" for the dashboard, the sell orders page, the trend page and a trend detail page;
  - add "Title at the top without a sub-menu" for the wealth page.
- [x] 1.2 Check that the sticky sub-menu, section jumps and the scroll marker still work with the new order. Verify that `subnav.spec.ts` and `scroll.spec.ts` pass ("Sub-menu stays while scrolling", "Jump to a row", "Open with a fragment"). Change their "below the page title" wording and any position assertions to "above", and update any Rust template tests in `crates/sekondo/tests` that depend on the header order (`CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test`).

## 2. Table headers without underline

- [x] 2.1 Add `text-decoration: none` for `th a` at rest, on hover and on focus-visible in `app.css`, and keep Pico's thicker header line. Verify with Playwright tests in `e2e/tests/tables.spec.ts`:
  - "Header text not underlined": the computed `text-decoration-line` of every sell-table header link is `none`, both before and while hovering "Item";
  - "Header line kept": on the trades page, the header cells' bottom border is wider than a body cell's.

## 3. Filter section

- [x] 3.1 Change the "Item name" input in `opportunities_filters.html` to `type="text"`, keeping `maxlength`, its label and its error wiring. Verify with a Playwright test in `e2e/tests/opportunities-layout.spec.ts` for "Item name is a plain input": same computed height, padding, border radius and background image as "Minimum margin (%)". Also check that "the item name search narrows every table" in `opportunities.spec.ts` and any Rust test that matches `type="search"` still pass.
- [x] 3.2 Set the `.filter-box` gap to `0.75rem 3rem` and leave the gaps inside groups as they are. Verify with Playwright tests in `e2e/tests/filter.spec.ts`:
  - "Character filter stands apart" on the trades page at 1920×1080: a 3rem gap to the range, wider than the gap between checkboxes;
  - "Character filter apart from the opportunity filters" on the trend page;
  - "No divider between groups" on the dashboard;
  - extend the "Narrow window wraps" test in `opportunities-layout.spec.ts` to check the 0.75rem line gap.

## 4. Docs and integration

- [x] 4.1 Update the layout section of `README.md`: the sub-menu comes first and the title below it, header text is not underlined, all text filters are plain inputs, and the filter groups are spaced apart. Check it against the spec delta.
- [x] 4.2 Run the full suites and confirm they are green, including the CSP test across all pages: `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test` and `npx playwright test --workers=2`.
