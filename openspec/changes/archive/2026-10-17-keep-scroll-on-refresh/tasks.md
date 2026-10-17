# Tasks

## 1. Scroll keeping on the orders page

- [x] 1.1 Add `data-scroll-key` to the sell and buy scrollers in `orders_table.html` (key from the table side) and to the freshness list in `orders_freshness.html`. Verify: a Rust test in `tests/orders_page.rs` asserts the keys `sell-orders`, `buy-orders` and `freshness` are present in `/orders` and in the `/orders/tables` and `/orders/freshness` fragments.
- [x] 1.2 Add the `htmx:beforeSwap`/`htmx:afterSwap` handlers to `app.js` that record and restore positions by key, only for swaps triggered by `sse:*` or `htmx:sseOpen`, with positions kept per request. Verify: a new `e2e/tests/scroll.spec.ts` with 200 stubbed sell orders and a buy order: scroll the sell table halfway, poll the character, wait for the `/orders/tables` refresh, and assert the sell table's `scrollTop` is unchanged (±1px), the buy table keeps its own position, and the page did not reload (window marker).
- [x] 1.3 Cover the other orders-page scenarios in `scroll.spec.ts`: a market refresh keeps the position; scrolled to the end, then a poll leaves 5 orders, so the table shows its last row; clicking the Price header and deselecting a character each leave the sell table at `scrollTop` 0. Verify: these Playwright tests pass, and `live.spec.ts`, `orders.spec.ts` and `filter.spec.ts` still pass.

## 2. Dashboard and wealth panels

- [x] 2.1 Add `data-scroll-key="breakdown"` to the dashboard breakdown scroller. Verify: a Playwright test in `scroll.spec.ts` with twenty characters (17 inserted in the database as in `layout.spec.ts`) scrolls the breakdown down, triggers an `orders-updated` poll, and asserts the position is kept after the `/dashboard/summary` refresh. `dashboard.spec.ts` still passes.
- [x] 2.2 Add `data-scroll-key="inventory"` to the wealth items scroller and `data-scroll-key="wealth-freshness"` to the wealth freshness list. Verify: a Playwright test with enough stubbed assets to scroll keeps the items table's position across a `wealth-updated` refresh (wallet or assets poll). `wealth.spec.ts` still passes.
- [x] 2.3 Verify the freshness column on the orders page with twenty characters: scroll it down, trigger a fee poll (`fees-updated`), and assert the position is kept. Verify: the Playwright test passes.

## 3. Integration

- [x] 3.1 Run the full suites (`cargo test` and `npx playwright test` in `e2e/`, with `E2E_APP_PORT`/`E2E_STUB_PORT` set if the dev container holds 8443) and verify both are green with no CSP violations.
