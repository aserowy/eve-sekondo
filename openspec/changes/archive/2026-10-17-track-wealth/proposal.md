# Proposal

## Why

Sekondo shows what a trader has tied up in market orders, but not what they are worth overall. ISK in the wallet and stock waiting in hangars are just as much part of a trader's capital, and over time the total is the number that tells whether trading pays off. Sekondo already polls each character and keeps history, so wealth can be tracked the same way.

## What Changes

- **BREAKING**: Login additionally asks for `esi-wallet.read_character_wallet.v1` and `esi-assets.read_assets.v1`, and both become required. Every character linked before this change counts as "needs re-login" until it logs in again. Until then, its order polling pauses and its last known data stays visible (existing rule).
- New polling per character of the wallet balance (on the order poll's cadence) and of the assets (at most hourly, honouring ESI's cache). Also new: a daily, public, account-independent refresh of ESI's market average prices, which values the items.
- Wealth per character = wallet ISK + value of personal sell orders + escrow of personal buy orders + inventory value. Corporation orders, the corporation wallet and corporation assets are not counted.
- Inventory value = the sum over assets of quantity × ESI average price. Items without a price, including blueprint copies, count as zero and are listed as unpriced.
- Wealth history is recorded from this release on. Past wallet and asset values cannot be reconstructed, so there is no backfill.
- Dashboard: a new "Wealth" tile with the current total for the selected characters, linking to the wealth page. Also a new total-wealth-over-time graph that follows the dashboard's existing range control together with the other over-time graphs.
- New wealth page `/wealth`:
  - a stacked graph of wallet, sell, escrow and inventory over time, with its own range control that only affects that graph
  - the list of current inventory items, one row per character, item and location, sortable and sorted by value descending by default
  - wallet and asset freshness per character
- A new `wealth-updated` live signal. The wealth page and the wealth parts of the dashboard refresh on it and on `orders-updated`.
- The navigation gains a "Wealth" link that carries the character selection.
- ESI contracts, stub routes and real-EVE verification are added for the wallet, assets and market prices calls.

## Capabilities

### New Capabilities
- `wealth-sync`: polling the wallet balance and the assets of each character, refreshing market prices, valuing the inventory, and recording wealth history.
- `wealth-overview`: the wealth page with its stacked over-time graph, its range control, the inventory list and freshness.

### Modified Capabilities
- `character-auth`: the login asks for and requires four scopes instead of two.
- `dashboard`: adds the wealth tile and the total-wealth graph. The range control now also drives the wealth graph, and the live refresh also reacts to `wealth-updated`.
- `navigation`: adds the "Wealth" link.
- `live-updates`: adds the `wealth-updated` signal.
- `esi-contract`: the contracted calls now include the wallet, assets and market prices. The stub serves them, and the verification covers them.

## Impact

- **Code**: `oauth.rs` (scopes), `esi.rs` (three new calls), new wealth poller(s) next to `poller.rs`/`market.rs`, `dashboard.rs` and its templates, a new `wealth.rs` with templates, `events.rs`, `nav.html`, `static/app.js` (stacked area chart).
- **Database**: new migrations for wallet history, current assets, item prices, inventory value history and wealth sync state. All per-character tables cascade on character deletion.
- **ESI**: three new routes. The authenticated ones (wallet, assets) add per-character load, while prices is a single public call per day.
- **Stub / contracts**: `eve-stub` gains routes and controls. `esi-contract` gains contracts, which changes the fingerprint, so the user has to run a new real-EVE verification with a character that has wallet ISK and at least one asset.
- **Tests**: Rust integration tests for polling and queries, plus Playwright tests for the dashboard additions, the wealth page, navigation and live refresh.
- **Users**: everyone has to log in once per character after the deploy.
