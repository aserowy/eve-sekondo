# Proposal

## Why

The self-written cockpit stylesheet has proved flaky: every new page brings layout regressions (window vs. scrolling pages, inner-scrolling panels, side columns, sticky bars), and keeping them working costs more than it gives. Going back to Pico CSS gives a stable, maintained base, while table density can be kept with a few targeted overrides. At the same time, cramming several tables, a freshness column and filters into one screen wastes space: one table per page, with its filters and KPIs on top and freshness at the end, uses the screen better.

## What Changes

- **BREAKING (look & feel)**: Vendor Pico CSS v2 again and drop the self-written theme. Everything uses Pico's defaults (dark theme, `container-fluid` width, badges, chart sizes) except tables, which get compact overrides (smaller text, tight cell padding, no wrapping, sideways scroll inside their container).
- Replace the top bar with a **collapsible left sidebar** as the main menu: Dashboard, Characters, Wealth, Trades, Orders, Opportunities, and Sign out pinned at the bottom. Each entry has an icon and text. When collapsed, only the icons show, and each keeps its name for hover and assistive technology. The state is remembered per browser.
- **One table per page.** A text-only sub-menu at the top of the page reaches its sibling pages:
  - **BREAKING (URLs)**: Orders splits into `/orders/sell` and `/orders/buy`, and `/orders` redirects to Sell.
  - **BREAKING (URLs)**: Opportunities splits into one page per table (`/opportunities/expansion|classic|reversion|trend`), and `/opportunities` redirects to the first. The sub-menu shows no row counts.
  - Wealth (graph and items table) and Trades stay single pages without a sub-menu. The dashboard and the opportunity detail page keep their section sub-menus.
- Each table page is laid out top to bottom:
  - the **filter section**, which holds all of its filters, including the time range now in a panel title;
  - the **KPI cards**: square cards that wrap onto more rows;
  - the **table**;
  - **pagination**: server-side `page` and `per_page` parameters, 25/50/100/200, default 50;
  - **freshness**: always shown, no longer collapsible, no warning-count strip.
- New and moved KPIs, each covering all filtered rows, not just the current page:
  - **Sell**: order count, order value, net margin (the sum of the order margins and its share of the known cost).
  - **Buy**: order count, escrow.
  - **Sell and Buy**: one card each for every selected character's sales tax, broker fee and minimum margin at the station where the character currently is. These replace the fees listed in the freshness.
  - **Trades**: bought, sold and margin, moved out of the table footer.
  - **Wealth**: total, wallet, sell orders, buy escrow and inventory.
  - The dashboard tiles become the same cards.
- Poll each character's **current location** from ESI (`/characters/{id}/location/`) together with the fee polls, using a new **optional** scope `esi-location.read_location.v1`. A character without the scope or without a known docked location uses Jita 4-4 for its fee KPIs and shows a "Log in again" hint. A missing location scope does not mark the character as needing re-login.
- **BREAKING (behaviour)**: tables no longer scroll inside their own panel vertically, and no longer cap at 20 or 50 rows; pagination replaces both.

## Capabilities

### New Capabilities
- `character-location`: Polling a character's current location (the last station or structure it was docked in) through the optional location scope, and choosing the fee location from it with Jita 4-4 as the fallback.

### Modified Capabilities
- `ui-layout`: Pico-based styling with only table overrides; the sidebar, the sub-menu, the per-page layout (filter section, KPI cards, table, pagination, freshness) and pagination replace the top bar, panels, page types, side columns and inner-scrolling tables.
- `navigation`: A collapsible sidebar with icon and text entries in the new order, Sign out at the bottom, and the new sell, buy and opportunity table URLs.
- `dashboard`: The tiles become KPI cards, and the Orders link carries the selection to the sell page.
- `orders-overview`: Separate sell and buy pages, pagination, count, value and net-margin KPIs, and per-character fee KPIs at the current station instead of fees per order location in the freshness.
- `wealth-overview`: Wealth KPIs, the range in the filter section, a paginated items table with the totals moved into KPIs, and freshness at the bottom.
- `trades-overview`: The range in the filter section, totals as KPIs, pagination, and freshness at the bottom.
- `station-opportunities`: One page per table with a sub-menu without counts, pagination instead of the 50-row cap, and freshness at the bottom.
- `fee-sync`: Fees are also known at a character's current station, and location polls feed the fee signal.
- `character-auth`: The login asks for the optional location scope in addition to the six required scopes.
- `esi-contract`: The location call is contracted and served by the stub.
- `live-updates`: The refresh behaviour applies to the new pages, keeps the current page of the table, and a location change raises `fees-updated`.

## Impact

- `crates/sekondo/static/pico.min.css` added back (v2, MIT). `app.css` is reduced to the table, sidebar, sub-menu, KPI card and pagination rules. `app.js` drops the freshness toggle, the side-column and inner-scroll handling, and gains the sidebar toggle.
- All page templates and `base.html`, plus new Lucide icons for the menu entries.
- Routes in `app.rs`; `orders.rs`, `opportunities/*`, `trades_page.rs`, `wealth_page.rs`, `dashboard.rs` and `pages.rs` (nav model) gain pagination, KPIs and the new URLs.
- `oauth.rs` and `sso.rs` scopes (optional scope handling), `fee_poller.rs` and `fees.rs` (location kind, fee at the current station), `esi.rs`, and a new migration for character locations.
- `esi-contract` contracts, `eve-stub` routes and fixtures. The verified stub fingerprint changes.
- Rust tests and most Playwright tests (layout, subnav, freshness, scroll, orders, opportunities, trades, wealth, navigation) need updating. `scroll.spec.ts` and the freshness-collapse tests go away or shrink.
- Content Security Policy unchanged: Pico needs no inline styles or scripts.
