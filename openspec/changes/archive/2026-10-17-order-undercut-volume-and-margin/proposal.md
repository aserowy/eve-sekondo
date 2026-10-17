# Proposal

## Why

"Undercut 3" doesn't tell a trader whether three units or three thousand units sit in front of their order, and that decides whether waiting is fine or a reprice is needed. The orders page also shows what a sell order is worth, but not what it earns: the trader has to remember what they paid for the items, which is hard when one sell order holds items bought at different prices.

## What Changes

- The public market copy also keeps the remaining volume of every market order.
- Sell and buy tables gain an **undercut volume** column: the sum of the remaining volumes of exactly the orders counted in "Undercut N". It is empty for a top order and "—" without market data.
- New hourly poll per character of ESI's **wallet transactions**. It uses the wallet scope the app already has, so no new login is needed. Personal purchases are stored permanently as purchase lots, so the history builds up beyond ESI's roughly 30-day window.
- Every sell order is remembered with the time it was first issued and its total volume, even after it closes. Per character and item, sell orders take units from the oldest purchase lots bought before them that earlier sell orders haven't taken yet (**FIFO**, at any location). Repricing an order doesn't change what it took.
- The sell table gains a **margin** column: `remaining volume × price × (1 − sales tax − broker fee) − remaining volume × average cost per unit`, using the character's current fees at that location from fee-sync. Hovering shows the exact ISK and the cost per unit.
- Margin reads "—" when:
  - not every unit of the order has a known purchase cost (hover: "cost known for X of Y units");
  - the fees are not known, including in a player structure;
  - it is a corporation order.
- The `market-updated` signal also fires when an undercut volume changes. A transactions poll that stores new purchases sends `orders-updated`.
- The ESI contracts, the stub and the real-EVE verification cover the wallet transactions call.
- Known limitation, by decision: sales straight into someone else's buy order do not take lots, so a later sell order may get older, cheaper lots than it really did.

## Capabilities

### New Capabilities
- `trade-margin`: polling wallet transactions into purchase lots, remembering sell orders, the FIFO allocation of lots to sell orders, and the margin of a sell order.

### Modified Capabilities
- `market-position`: the market copy keeps volumes; adds the undercut volume of an order.
- `orders-overview`: new undercut volume column in both tables and margin column in the sell table, with their sorting.
- `live-updates`: `market-updated` also covers undercut volume; transactions polls signal `orders-updated`.
- `esi-contract`: the wallet transactions call is contracted, served by the stub and verified.

## Impact

- **Code**:
  - `market.rs` (store volume, snapshot includes undercut volume)
  - `orders.rs` (`STANDING_JOIN`, new columns and sort keys)
  - `orders_table.html`
  - `esi.rs` (transactions call)
  - `wealth_poller.rs` (new `transactions` kind)
  - `poller.rs` (remember sell orders)
  - a new margin/FIFO module
  - `events.rs` usage
- **Database**: a migration adds `volume_remain` to `market_orders`, adds `purchases` and `sell_order_history` tables (both cascade on character deletion), and extends the `wealth_sync` kinds. `sell_order_history` is backfilled from `order_snapshots`.
- **ESI**: one more authenticated route per character per hour (more pages on the first poll).
- **Stub / contracts**: a new route, control and contract. The stub fingerprint changes, so a new real-EVE verification is needed.
- **Tests**: Rust tests for FIFO and margin math, the poll and the SQL, plus Playwright tests for the orders page.
