# Tasks

## 1. Layout skeleton: bars, page types, shared table style

- [x] 1.1 In `base.html`, add the `header.bars` wrapper, the `subnav`, `filters` and `page_type` blocks, and remove the top bar's `controls` block. Add the `--bars-height` ResizeObserver in `app.js`, and add the `page-window`/`page-scroll` and sticky-bar CSS. Verify with a Playwright test in `layout.spec.ts`: the top bar holds only brand, navigation and icons, and on the dashboard the bars stay at the top after scrolling to the end.
- [x] 1.2 Add `table.data` (header, row height, striping, hover, nowrap, `max-content` width, `tr[data-href]` cursor), plus the 20-row cap on `.page-scroll .scroll`. Switch the breakdown and inventory tables to it, and the orders tables to `table.data.records`. Verify with Playwright tests: the second rows of the breakdown and the inventory share one background, an orders row is at most 22px high, and the dashboard breakdown with 30 characters shows 20 rows and scrolls inside itself.

## 2. Filter box on existing pages

- [x] 2.1 Move the character filter of the orders and wealth pages into the `filters` block. Put their tables and graph into the window-page skeleton in place of `.fill`, and extend `hx-select-oob` to the new bars. Verify with Playwright tests: at 1280×800 the orders filter box is one line at most 28px high below the top bar, the wealth range stays in the graph panel title, the orders page doesn't scroll vertically at 1920×1080, and the existing filter, scroll and freshness specs pass with updated selectors.
- [x] 2.2 Move the dashboard's character filter and range control into the filter box, and check that the characters page renders no filter box. Verify with Playwright tests: dashboard bar and filter box contents, no filter box on the characters page, the first-screen check at 1920×1080 still passes with the bars, and no page scrolls horizontally at 1024px.

## 3. Section sub-menu

- [x] 3.1 Render the `subnav` block on the dashboard with the row ids `totals`, `characters`, `split`, `value`, `orders` and `wealth`, and add the `scroll-margin-top` rule. Verify with Playwright tests: the six links are in order, activating "Wealth" scrolls the wealth panel's title into view below the bars and sets the fragment, and opening `/dashboard#value` starts scrolled to the value panel.
- [x] 3.2 Add the scroll observer to `app.js`: it marks the jump target after a jump, otherwise the top-most row past the bars or the last row at the page end, sets `aria-current="location"`, recomputes on `htmx:load`, and never touches the URL. Verify with Playwright tests: scrolling by hand to the value panel marks "Value" with the URL unchanged, scrolling to the end marks "Wealth", and the marker survives a live `orders-updated` refresh.

## 4. Opportunities page

- [x] 4.1 Restructure `opportunities.html` as a scrolling page:
  - The filter box holds the character filter form, a settings form without the Apply button with `#filter-error`, and `#filter-context` with the station name and the fees note.
  - The four table panels get ids `expansion`, `classic`, `reversion` and `trend`, and switch to `table.data`.
  - Remove the heading, the loose fees paragraph and the freshness panel.

  Verify with Playwright tests: no `h1` or loose text, the filter box contents, no apply button, a capped 20-row classic table, and no horizontal page scroll at 1280px.
- [x] 4.2 Add the sub-menu with row counts and return `#section-nav`, `#filter-context` and `#filter-error` out of band from the tables, settings and filter responses. Adapt the 422 handling to `#filter-error`. Verify with Playwright tests: the counts match the tables, raising the minimum margin updates a count to 0, a capital of 0 shows the message in the filter box, and deselecting a character updates the fees text.
- [x] 4.3 Rewrite `opportunities_freshness.html` in the collapsible-column pattern: add a warning count to the freshness view (stale book, book error, history error) and make the column sticky on the scrolling page. Verify with a Rust unit test of the warning count, and Playwright tests: the strip starts collapsed, shows "2" for a stale book with a failed refresh, and stays inside the window when unfolded and scrolled to the trend table.

## 5. Opportunity detail page

- [x] 5.1 Add the route `GET /opportunities/{table}/{type_id}` with the page template `opportunity_detail.html`, which reuses the content template.
  - Filter box: the character filter and `#filter-context`.
  - Sub-menu: "← Opportunities" carrying `chars` and the four sorts, "Figures", one entry per chart, and "Next occurrences" on expansion pages.
  - Rows: the figures panel and full-width chart rows.
  - Navigation marks "Opportunities" as current.
  - An unknown table answers 404, and a signed-out request redirects.

  Verify with Rust handler tests for 404 and redirect, and Playwright tests for the sub-menu entries, the back link keeping the sorting, and charts on their own rows.
- [x] 5.2 Make rows navigate: render `tr[data-href]` plus an item `<a href>`, add the delegated click handler, and remove `aside#opportunity-detail` with its CSS, close/Escape code and the margin rule. Keep `/opportunities/detail/...` as the live-refresh content fragment, and make the detail page's character filter swap only the content. Verify with Playwright tests: clicking a mean reversion row navigates to its page without any floating window, deselecting a character recomputes the fees on the detail page, and a dropped-out item shows its reason.
- [x] 5.3 Live refresh of the detail page. Verify with Playwright tests in `live.spec.ts`: an `opportunities-updated` signal updates the figures while the page stays scrolled to the volume chart, and the opportunities tables keep their scroll position.

## 6. Documentation and integration

- [x] 6.1 Update the README's opportunities section (filter box, sub-menu, detail page in place of the floating window) and add a short layout-guidelines paragraph pointing to the `ui-layout` spec. Verify the README no longer mentions a filter bar or a detail window.
- [x] 6.2 Run `cargo test`, `cargo clippy` and the full Playwright suite, and verify that all pass and the CSP check covers every page, including the detail page and sub-menu jumps.
