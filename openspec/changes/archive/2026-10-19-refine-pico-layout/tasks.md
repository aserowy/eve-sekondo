# Tasks

## 1. Base font and page skeleton

- [x] 1.1 Write failing Playwright tests in `e2e/tests/pico.spec.ts`: root and body font size are 16px at 1024, 1280 and 1920 widths on the characters page, and the sell table stays at 0.875rem. Then add `:root { --pico-font-size: 100%; }` to `app.css` and verify the tests pass.
- [x] 1.2 Write failing Playwright tests in a new `e2e/tests/page-structure.spec.ts` for "Title on every page" and "Header order" (title above sub-menu above filter section on the buy page). Restructure `base.html` into `.page` with `header.page-header > h1` (`page_title` block), the `subnav`/`filters` blocks, `<main>` and a `footer` block, plus a `page_attrs` block. Fill `page_title` in every signed-in page template. Remove the headings that duplicate the title (characters `h2`, the opportunity table's `h2` on table pages, the item name in the detail `h2`). Verify the new tests pass.
- [x] 1.3 Update the tests that encode the old skeleton (`h1` count 0 in navigation, opportunities and opportunities-layout; `main.container-fluid` in pico; `main > nav.sub-menu#section-nav` in opportunities; `#classic h2`), so they assert the new title and structure. Run the touched specs and the sidebar width test, and verify they pass.

## 2. Freshness footer

- [x] 2.1 Write failing Playwright tests in `e2e/tests/freshness.spec.ts`: on a short sell page at 1920×1080 the footer's bottom edge equals the window's bottom edge and the page has no vertical overflow. With 200 sell orders at 1280×800 the footer is out of view until the page is scrolled to its end, then it is in view below the pagination. The dashboard and characters pages have no `contentinfo` landmark, and the trades page has one `main` and one `contentinfo` landmark.
- [x] 2.2 Move `sse-connect` to `.page` through `page_attrs` on the orders, trades, wealth, opportunity table, opportunity detail and dashboard pages, and move each `#freshness` section into the `footer` block. Add the `.page` flex column (`min-height: 100dvh`, `main { flex: 1 0 auto }`) to `app.css`. Verify the 2.1 placement tests pass, and that `live.spec.ts`, `scroll.spec.ts` and `fees.spec.ts` still pass.
- [x] 2.3 Write a failing Playwright test for compact entries: on the trades page at 1920×1080 with two characters, the footer has no heading and each `li` is one line high. Remove `<h2>Freshness</h2>` from the four freshness partials. Style `.page-footer` (top divider, 0.875rem, muted) and `ul.freshness li` as wrapping inline rows, with the wealth `[data-kind]` segments inline. Update the old "Freshness" heading assertion in `freshness.spec.ts`, then run `freshness`, `wealth`, `trades`, `opportunities` and `opportunities-layout` specs and verify they pass.

## 3. Header, sub-menu and rhythm

- [x] 3.1 Write failing Playwright tests: on the buy page the "Buy" sub-menu link has a bottom border in the primary colour and "Sell" has none. After scrolling the sell page to its end, the sub-menu is at the window top and the `h1` is out of view. Then restyle `.sub-menu` as a tab bar in `app.css`, and verify these tests, `subnav.spec.ts` and the section-jump tests in `dashboard.spec.ts` pass.
- [x] 3.2 Write failing Playwright tests: the dashboard "Per character" `h2` is smaller than the `h1`, and on the sell page the KPI→table gap equals the table→pagination gap. Add the heading sizes and the flex-gap rhythm on the content containers, and remove the per-component `margin-bottom` from `.kpis`, `.table-wrap` and `.pagination`. Verify the tests and `layout.spec.ts` pass.

## 4. KPI stat tiles

- [x] 4.1 Write failing Playwright tests: at 1920px on the buy page and the dashboard, every `.kpi` is wider than high, all have equal width and none contains a `header`. Replace the card `header` with `small.name` in `kpi_card.html`, drop `aspect-ratio` and set the `minmax(12rem, 1fr)` grid and compact padding in `app.css`. Update the square-card assertions and the `.kpi header` locators (e.g. `wealth.spec.ts`), then verify the KPI, wealth and dashboard specs pass, including "Cards wrap" at 1280px.

## 5. Integration

- [x] 5.1 Run the full Playwright suite and `cargo test`, and verify both pass. Run `openspec validate refine-pico-layout --strict` and verify it reports no errors.
