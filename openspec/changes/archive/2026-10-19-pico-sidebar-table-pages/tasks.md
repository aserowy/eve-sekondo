# Tasks

## 1. Pico shell and sidebar

- [x] 1.1 Restore `static/pico.min.css` (v2.0.6, MIT header) from commit `d2aa6d5^`, load it before `app.css` in `base.html` and wrap `main` in `.container-fluid`; verify with a Playwright test that every page loads exactly `/static/pico.min.css` and `/static/app.css` from the app's origin with no CSP violation, and that the characters page body font size equals Pico's default
- [x] 1.2 Vendor the Lucide icons `layout-dashboard`, `wallet`, `arrow-left-right`, `list`, `lightbulb`, `panel-left-close` and `panel-left-open` (drop `panel-right-*`), and extend `tests/icons.rs` so that every icon template is a `currentColor` line icon; verify with `cargo test --test icons`
- [x] 1.3 Replace the top bar in `base.html` with `aside.sidebar` (entries Dashboard, Characters, Wealth, Trades, Orders, Opportunities, Sign out at the bottom, each an icon plus a `.label`, with `aria-label` and `title`) and add `Page::OrdersSell`, `Page::OrdersBuy` and `Page::Opportunities(table)` with hrefs to `/orders/sell` and `/opportunities/expansion`; verify with `tests/navigation.rs` (order, current marking, `chars` carried, no range, page or per-page) and a Playwright test of entry order and navigation
- [x] 1.4 Add the menu toggle (`aria-expanded`, accessible name "Menu"), the `localStorage` state and its early `data-menu` initialisation in `app.js`, and the collapsed CSS; verify with a Playwright test that collapsing hides the labels, widens `main`, keeps the accessible names and hover titles, persists across navigation, and starts expanded in a fresh context
- [x] 1.5 Strip `app.css` down to compact tables (0.875rem, rows of at most 28px, nowrap, `.num` tabular and right-aligned, zebra and hover, `.table-wrap` with sideways scroll), the sidebar, the sticky `nav.sub-menu`, `.kpis`/`.kpi` square cards and the pagination bar, and remove panel, bars, page-type and freshness-column rules and custom chart heights; verify with Playwright tests for row height, font size, identical striping across tables, a wide table scrolling sideways without page scroll at 1280px, and no horizontal scroll at 1024px
- [x] 1.6 Remove `watchBars`, `--bars-height`, the freshness toggle and the inner-panel scroll keeping from `app.js`; publish `--sub-menu-height` for `scroll-margin-top` and measure the section marker against the sub-menu; verify with the updated `subnav.spec.ts` (jump, open with a fragment, marker follows scrolling, end of page, sub-menu stays sticky)

## 2. Shared pagination and KPI cards

- [x] 2.1 Add `pagination.rs` with `Paging` parsing (`page`, `per_page` ∈ {25, 50, 100, 200}, defaults 1 and 50), clamping to `1..=last` and a `PageView` (previous and next hrefs, "Page x of y", page-size options); verify with unit tests for defaults, malformed and non-positive values, past-the-end clamping, an empty list and query building that omits defaults
- [x] 2.2 Add the `pagination.html` partial (previous, next, "Page x of y", a page-size `<select>`, `hx-get` on the content fragment with `hx-push-url` and `show:#content:top`, hidden when there are no rows) and `kpi_card.html` (name, value with the exact `title`, optional detail line clamped to three lines, optional link, optional error style); verify with a template rendering test in `tests/server.rs` or a dedicated test
- [x] 2.3 Update the live-refresh handling in `app.js`: keep `window.scrollY` and the `.table-wrap` `scrollLeft` across SSE-triggered swaps, and scroll the table top into view on user-triggered swaps; verify with the rewritten `scroll.spec.ts` (refresh keeps the page scroll and the sideways scroll, sort and filter show the table top)

## 3. Character location (backend)

- [x] 3.1 Add the `character_location` contract (station, structure, in space, 403) to `esi-contract`, and the stub route `/characters/{id}/location/` (scope-checked) with a fixture control, with the location scope in the stub's default character scopes; verify with the contract coverage and stub conformance tests and a stub test that a token without the scope gets 403
- [x] 3.2 Split `oauth.rs` into `REQUIRED_SCOPES` (6) and `OPTIONAL_SCOPES` (location), request both, and keep `missing_scopes()` on the required ones only; verify with `tests/login_start.rs` (exactly seven scopes) and `tests/callback.rs` or `tests/tokens.rs` (a token without the location scope is not marked "needs re-login")
- [x] 3.3 Add the migration for `character_locations` and the widened `fee_sync.kind` CHECK including `'location'`, the change-hands cleanup, and `EsiClient::character_location`; verify with `tests/schema.rs`, `tests/unlink.rs` (no location rows remain) and `tests/esi_client.rs` against the stub
- [x] 3.4 Add `Kind::Location` to `fee_poller`: skip characters without the scope or needing re-login, upsert when docked, keep the row in space, honour `Expires` and back off on failure; verify with `tests/fee_poll.rs` scenarios (docked station, structure, in space keeps the last one, hourly wait, failure keeps the last location, without the scope it is not polled)
- [x] 3.5 Add `fee_location(character)` (no scope → Jita with the re-login hint; docked → that location; otherwise Jita), extend `resolve_stations` to docked NPC stations, and send `fees-updated` when the fees at the fee location change after a location poll; verify with `tests/fees.rs` and `tests/events_stream.rs` (moving sends the signal, the same station does not, the owner of a docked station is looked up)
- [x] 3.6 Re-record `verified-stub.json` with `esi-verify` against real EVE for the new location contract, and verify that the stub gate passes with `cargo test` in the stub-backed suites

## 4. Orders: sell and buy pages

- [x] 4.1 Add the routes `/orders/sell`, `/orders/buy`, `/orders/{side}/content` and the `/orders` → `/orders/sell` redirect keeping the query; render the sub-menu ("Sell", "Buy", carrying `chars` and both sortings, without page or per-page) and the filter section; verify with `tests/orders_page.rs` (redirect, each side lists only its orders, unauthenticated redirect, sortings carried) and a Playwright test of the sub-menu switching sides
- [x] 4.2 Extract the order aggregates (count, value or escrow, corp share) shared with `dashboard.rs`, and add the net margin (Σ margin, Σ known cost, percentage, left-out count); verify with unit tests (the 25.00% example, negative, none with a margin → "—") and a test that the sell KPIs equal the dashboard totals for the same selection
- [x] 4.3 Render the sell and buy KPI cards and the per-character fee cards (sales tax, broker fee, min. margin, with the character and station detail, hover inputs, structure "0.5% + owner fee" / "—", unknown "—", "Log in again" without the scope); remove the fee table from `orders_freshness.html`; verify with `tests/fees_page.rs` and a Playwright test (`fees.spec.ts`) for NPC station, structure and missing scope
- [x] 4.4 Paginate both order tables after sorting, keeping the page in the live-refresh URL with clamping; move the freshness below the table; verify with `tests/orders_page.rs` (page 2 contents, past-the-end clamp, sorting resets to page 1) and Playwright tests in `orders.spec.ts` and `live.spec.ts` (next page, page size 200, `orders-updated` and `fees-updated` keep page 2 and the scroll position, freshness below the pagination with no toggle)

## 5. Trades page

- [x] 5.1 Move the range control into the filter section, render Bought, Sold and Margin as KPI cards over the whole range (with the left-out count on hover and the error style when negative), and remove the `tfoot`; verify with `tests/trades_page.rs` (totals across 80 rows with 50 shown) and a Playwright test in `trades.spec.ts`
- [x] 5.2 Paginate the trades table (newest first), reset to page 1 on range and filter changes, keep the page on `trades-updated` and `fees-updated`, and move the freshness to the bottom; verify with `tests/trades_page.rs` (page 3 of 120) and Playwright tests for the range reset and the live refresh keeping the page

## 6. Wealth page

- [x] 6.1 Move the range control into the filter section and render the KPI cards (Wealth, Wallet, Sell orders, Buy escrow, Inventory with the unpriced count) from the current figures, independent of the range; verify with `tests/wealth_page.rs` (the 200m example, equal to the dashboard wealth card, unchanged by the range) and a Playwright test in `wealth.spec.ts`
- [x] 6.2 Order the content as KPIs, graph, items table and pagination, then freshness; paginate the items after sorting and keep the page on `wealth-updated`; verify with `tests/wealth_page.rs` (300 rows → "Page 1 of 6") and Playwright tests for the layout order and the live refresh keeping page 2

## 7. Opportunities pages

- [x] 7.1 Add `/opportunities/{table}` and `/opportunities/{table}/content`, the `/opportunities` → `/opportunities/expansion` redirect keeping the query, and 404 for unknown tables; render the sub-menu without counts (carrying `chars` and all sortings) and the filter section with the settings form targeting the current table; verify with `tests/opportunities_page.rs` and `tests/opportunities_tables.rs` and a Playwright test in `opportunities-layout.spec.ts` (switch table, no numbers in the sub-menu)
- [x] 7.2 Drop the 50-row cap, paginate after sorting, reset to page 1 on filter changes, keep the page on `opportunities-updated`, and move the freshness and the expansion data note into place; verify with `tests/opportunities_tables.rs` (80 matches → 50 and 30) and Playwright tests in `opportunities.spec.ts` (shared filters across tables, live refresh keeps the sorting and page 2)
- [x] 7.3 Change the detail page's back link to "← <table title>" pointing at its table page, and move it under the new shell (section sub-menu, filter section, sections instead of panels); verify with `tests/opportunities_detail.rs` and a Playwright test of the back link and the section jumps

## 8. Dashboard and characters under Pico

- [x] 8.1 Render the dashboard tiles as KPI cards (wealth linked), remove the breakdown's inner scroll and the first-screen sizing, keep the row order and the section sub-menu; verify with `tests/dashboard.rs` and a Playwright test in `dashboard.spec.ts` (cards are square and equal-sized, row order, 30 characters without pagination, jump and marker)
- [x] 8.2 Move the characters, landing and error pages onto Pico defaults (no panels, icon-and-text actions kept); verify with `characters.spec.ts`, `landing.spec.ts` and `unlink.spec.ts`

## 9. Integration

- [x] 9.1 Remove the obsolete freshness-collapse and window-page tests and dead templates and fragment routes (`/orders/tables`, `/opportunities/tables`, `opportunities_section_nav` counts), then run `cargo test --workspace`, `cargo clippy --workspace -- -D warnings` and the full Playwright suite; verify that all pass
- [x] 9.2 Run a Playwright CSP sweep over the dashboard, characters, wealth, trades, sell, buy, all four opportunity pages and a detail page, using every sub-menu link and pagination control; verify that no CSP violation is reported
