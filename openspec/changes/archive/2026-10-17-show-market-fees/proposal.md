# Proposal

## Why

A station trader has to know what the market takes before an order can pay off: the broker fee on every order placed and the sales tax on every sale. Both depend on the character's skills and, at NPC stations, on its standings with the station owner, so they differ per character and per station. Today Sekondo shows the orders but not what they cost, and the trader has to work the fees out in game.

## What Changes

- **BREAKING**: Login additionally asks for `esi-skills.read_skills.v1` and `esi-characters.read_standings.v1`, and both become required. Every character linked before this change counts as "needs re-login" until it logs in again (existing rule).
- New hourly polling per character of its skills and its NPC standings, honouring ESI's cache.
- The owner corporation of each NPC station where a character has orders is looked up from ESI once and cached. The faction of each NPC corporation comes from a table taken from CCP's static data export, because ESI does not report it for NPC corporations.
- On the orders page, each character's entry lists every station or structure where the character currently has orders, with:
  - the **sales tax** rate: 7.5% × (1 − 0.11 × Accounting level);
  - the **broker fee** rate: at an NPC station, 3% − 0.3% × Broker Relations level − 0.03% × faction standing − 0.02% × corporation standing; in a player structure, "0.5% + owner fee (unknown)";
  - the **minimum margin** of a buy order resold through a sell order, as a markup on the buy price: (1 + b) / (1 − b − t) − 1, shown only where the broker fee is known.
- Hovering a figure shows the exact rate and its inputs (skill levels and standings).
- A new `fees-updated` live signal refreshes the character entries when a skills or standings poll changes a figure.
- The ESI contracts, stub routes and real-EVE verification are extended to cover the skills, standings and station calls.
- Not covered, by decision: the 100 ISK minimum broker fee, ISK rounding, relist fees, the undocumented Exordium market surcharge, and the owner fee of player structures.

## Capabilities

### New Capabilities
- `fee-sync`: polling skills and standings per character, resolving station owners and their factions, and computing the sales tax, broker fee and minimum margin per character and location.

### Modified Capabilities
- `character-auth`: login asks for and requires six scopes instead of four.
- `orders-overview`: the freshness entry of each character gains the fee figures per location.
- `live-updates`: adds the `fees-updated` signal.
- `esi-contract`: the contracted calls include skills, standings and stations. The stub serves them, and the verification covers them.

## Impact

- **Code**: `oauth.rs` (scopes), `esi.rs` (three new calls), a new fee poller and a fee module next to `wealth_poller.rs`, `freshness.rs`/`orders.rs` and `orders_freshness.html`, `events.rs`, `orders.html` (signal trigger).
- **Data**: a checked-in NPC corporation → faction table from the SDE, with its source and version noted.
- **Database**: a new migration for character skills, character standings, station owners and the fee sync state. All per-character tables cascade on character deletion.
- **ESI**: two authenticated routes per character per hour, plus one public station lookup per new station.
- **Stub / contracts**: `eve-stub` gains routes and controls, and `esi-contract` gains contracts. That changes the stub fingerprint, so a new real-EVE verification is needed, with a character that has at least one NPC standing.
- **Tests**: Rust tests for the fee math and the polling, plus Playwright tests for the orders page.
- **Users**: everyone has to log in once more per character after the deploy.
