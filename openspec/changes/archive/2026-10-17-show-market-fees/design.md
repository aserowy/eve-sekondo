# Design

## Context

- `oauth::REQUIRED_SCOPES` is the single scope list. Adding the two scopes there makes characters linked earlier fail the scope check on their next refresh and get the existing "needs re-login" treatment, exactly as in track-wealth.
- `WealthPoller` sets the pattern for scheduled per-character ESI work:
  - a `Kind` enum
  - a `*_sync` table keyed by `(character_id, kind)` with `next_poll_at`, `esi_expires_at` and `failures`
  - `schedule_new` and `claim_due`
  - `record_success`/`record_failure`/`record_deferral` with the shared `EsiGate`
  - one transaction per successful store
  - a signal published after commit
- The orders code already treats `location_id < 100000000` as an NPC station and anything else as a player structure (`orders::STANDING_JOIN`). Station names come from the `names` cache.
- Fee-relevant computations follow the market-position precedent: one SQL fragment used by both the listing and the change detection, so a signal is sent exactly when what is shown changes. PostgreSQL `numeric` keeps the arithmetic exact. The crate has no decimal type.
- The orders page's `#freshness` section is a fragment (`/orders/freshness`) refreshed on `sse:orders-updated` and on reconnect. It renders `orders_freshness.html` from `FreshnessView`s.

## Fee math and its sources

All sources were fetched on 2026-10-17. Anything marked *derived* is algebra on the cited rules, not a quote.

| Figure | Formula | Source |
|---|---|---|
| Sales tax | 7.5% × (1 − 0.11 × Accounting) | CCP patch notes 22.02, 2025-03-12.1: "Sales Tax has been increased from 4% to 7.5%" (https://www.eveonline.com/news/view/patch-notes-version-22-02). Accounting (type 16622) live skill text from ESI `/universe/types/16622`: "Each level of skill reduces sales tax by 11%. Sales tax starts at 7.5%." |
| NPC broker fee, base and skill | 3% − 0.3% × Broker Relations | Broker Relations (type 3446) live skill text: "subtracts a flat 0.3% … which usually come to 3% of the order's total value. This can be further influenced by the player's standing towards the owner of the station." |
| NPC broker fee, standings | − 0.03% × faction standing − 0.02% × corporation standing | EVE University, https://wiki.eveuniversity.org/Trading (rev. 232073, 2026-08-04). CCP does not publish the coefficients. CCP's stated 1.0% minimum (https://www.eveonline.com/news/view/restructuring-taxes-after-relief) equals the formula at Broker Relations 5 with 10.0/10.0 standings, which corroborates them. |
| Unmodified standings | Connections, Diplomacy and Criminal Connections do not lower the fee | EVE University Trading and Standings pages. No CCP statement. See Open Questions. |
| Structure broker fee | 0.5% SCC surcharge + owner fee; skills have no effect | Viridian expansion notes, 2023-06-09 (https://www.eveonline.com/news/view/viridian-expansion-notes), and patch notes 21.05 (owner minimum 0%). Broker Relations text says "non-player station". |
| Minimum margin | (1 + b) / (1 − b − t) − 1 | *Derived*: buying through a buy order costs P_b·(1 + b), and selling through a sell order yields P_s·(1 − b − t). The trade breaks even when they are equal, and the markup is P_s/P_b − 1. |

The broker fee and sales tax on the sale are both charged on the sale value, so they combine as 1 − b − t (EVE University Trading: the broker fee is "paid every time a sell or buy order … is created"; the sales tax is paid "when an item is sold"). The buyer pays no sales tax.

## Goals / Non-Goals

**Goals:**
- Every figure is reproducible from a cited rule and the character's own ESI data.
- Fee figures shown on the page and the `fees-updated` change detection come from the same SQL fragment.

**Non-Goals**, by decision during planning:
- The 100 ISK minimum broker fee, because it does not apply to a percentage.
- ISK rounding of charges, because no source documents it.
- Relist and modification fees.
- The Exordium starter-region surcharge ("Additional 5% market fees added to non-instant orders", patch 23.02), because its mechanics are undocumented.
- The owner fee of player structures, which ESI does not expose. A later change could read it from the wallet journal or let the user enter it.
- Per-item net margins and the other ways to trade (instant buy or instant sell).

## Decisions

### 1. Three new ESI calls
`EsiClient` gains the following, each with its own gate route and the usual error mapping and `Expires` handling:
- `skills(character_id, token)`: `GET /characters/{id}/skills/`. `EsiSkill` has `skill_id` and `active_skill_level`. `trained_skill_level` is not read, because the active level is the one that applies (it differs for Alpha clones and expert systems).
- `standings(character_id, token)`: `GET /characters/{id}/standings/`. `EsiStanding` has `from_id`, `from_type` (`agent`, `npc_corp`, `faction`) and `standing` (float).
- `station(station_id)`: `GET /universe/stations/{id}/`, public. Only `owner` is read.

**Alternative:** fetch all skills and keep them. Only two skills matter here, so less data is stored.

### 2. Storage
A single migration:
- `character_skills (character_id, skill_id, active_level)`: only Accounting (16622) and Broker Relations (3446) are kept. Replaced as a whole on each successful poll.
- `character_standings (character_id, from_id, from_type, standing numeric)`: only `npc_corp` and `faction` rows; agents are dropped. Replaced as a whole on each successful poll. `numeric` keeps the reported value exact for the SQL math. ESI sends a float, which is parsed through its decimal text form, not via `f64` arithmetic.
- `station_owners (station_id PK, owner_id, resolved_at)`: public, belongs to no account.
- `fee_sync (character_id, kind CHECK IN ('skills','standings'), …)`: same columns as `wealth_sync`.
- All per-character tables `ON DELETE CASCADE` from `characters`.

### 3. NPC corporation → faction table
ESI's `/corporations/{id}` does not return `faction_id` for NPC corporations. Research checked seven NPC corporations, including 1000035. So the mapping is taken from CCP's static data export (`npcCorporations`, `factionID`):
- It is checked in as `crates/sekondo/data/npc_corporation_factions.csv` (`corporation_id,faction_id`).
- A header comment states the SDE build number and download URL.
- It is loaded into a `npc_corporation_factions` table by the migration (`COPY`-free: generated `INSERT`s in the migration), so the fee SQL can join it.
- A task fetches the SDE, generates the file, and records its version. Updating it later is a new migration.

**Alternative considered:** embed the table with `include_str!` and pass it to SQL as arrays. Rejected because the join belongs in the fee fragment.

### 4. Fee poller
`fee_poller.rs` mirrors `WealthPoller` with `Kind::{Skills, Standings}`, an interval of one hour each, and `ESI Expires` respected.
- Each successful store runs the fee fragment for the character before and after the write in the same transaction. If the result differs, `Event::FeesUpdated` is published after commit.
- **Station owners:** each scheduler tick resolves up to N unknown NPC station IDs from `current_orders` (`location_id < 100000000`, not in `station_owners`). For every character with orders at a resolved station, it publishes `FeesUpdated` to that character's account.
- **Failures:** a failed station lookup is retried on a later tick with the gate's backoff.

**Alternative considered:** adding kinds to `wealth_sync`. Rejected because the table and poller are named for wealth, and fees are their own concern.

### 5. One fee fragment
`fees::FEES_SQL` produces one row per (character, location) from the character's current orders (`DISTINCT character_id, location_id`), joined to skills, standings, `station_owners` and `npc_corporation_factions`. Its columns:
- `accounting`, `broker_relations` (NULL when skills were never polled; 0 when polled but untrained)
- `faction_standing`, `corp_standing` (NULL when standings were never polled; 0 when polled and absent)
- `is_structure` (`location_id >= 100000000`)
- `sales_tax`: `7.5 * (1 - 0.11 * accounting)`
- `broker_fee`: NULL for structures, NULL when an input is unknown, else `3 - 0.3*br - 0.03*f - 0.02*c`
- `min_margin`: `((1 + b/100) / (1 - b/100 - t/100) - 1) * 100` when `broker_fee` is not NULL

All values are `numeric`. No clamp is applied: with standings at most 10 and Broker Relations at most 5, the formula cannot fall below CCP's stated 1.0%.

### 6. Orders page
- `FreshnessView` gains `locations: Vec<FeeLocationView>`, built from `FEES_SQL` for the selected characters and ordered by location name.
- `orders_freshness.html` renders a small table under each character with Location, Sales tax, Broker fee and Min. margin columns.
- Formatting is done in Rust from the `numeric` values (as text):
  - shown: half-up to two decimals
  - `title`: four decimals plus the inputs
  - structure broker fee: "0.5% + owner fee"
  - unknown: "—"
- The `#freshness` section's `hx-trigger` gains `sse:fees-updated`. `#order-tables` does not.

### 7. Stub, contracts, verification
- `eve-stub` gains the three routes and the controls `/_stub/skills`, `/_stub/standings` and `/_stub/stations`, with scope checks giving 403.
- `esi-contract` gains the contracts `skills/levels`, `skills/forbidden`, `standings/list`, `standings/forbidden` and `stations/station`.
- `esi-verify` checks the following, then re-records the fingerprint:
  - skills
  - standings (failing when there is no `npc_corp` or `faction` row)
  - the station of a character NPC order, or Jita IV - Moon 4 - Caldari Navy Assembly Plant (60003760) when there is none

## Risks / Trade-offs

- **[Standing coefficients are community-documented]** → They are cited, corroborated by CCP's 1.0% minimum, and checked manually in game (task 1.2). The 0.03% faction coefficient and the raw-standings reading were checked in game (task 1.2, Open Questions); the 0.02% corporation coefficient and the Accounting step were not.
- **[ESI standings might be effective, not raw]** → This is undocumented. The manual check (task 1.2) uses a character with Connections trained. If ESI turns out to report effective standings, implementation stops and the plan is revisited, because the raw value can't be recovered without knowing the character's Connections level and whether the standing is friendly or hostile.
- **[Skills are stale until the character logs in or the cache expires]** → This is accepted: the figure follows what ESI reports, with the poll time visible in the freshness entry.
- **[SDE table drifts]** → New NPC corporations are rare. An unknown corporation falls back to faction standing 0, which overstates the fee rather than understating it when the standing is positive.
- **[Another forced re-login]** → Accepted during planning.
- **[Structure fee unknown]** → The figure says so explicitly instead of showing a misleading number.

## Migration Plan

1. Deploy. The migration creates the tables and loads the faction table.
2. Each character fails its next token refresh's scope check and shows "Log in again". After the user logs in again, skills and standings are polled within the hour.
3. Rollback: the previous release ignores the new tables, and tokens with the two extra scopes still pass its scope check, so no further re-login is needed.

## Open Questions

None open. Answered in task 1.2 on 2026-10-17, from real ESI and the game, with one character (Broker Relations 2, Connections 1, no Accounting):

- **Raw or effective?** ESI reports **raw** standings. ESI said faction standing 0.15000000000000002 (Caldari State) with Connections at level 1 and no corporation standing towards the station owner (Caldari Navy). A sell order worth 81 × 284.4 = 23,036.40 ISK at Jita IV - Moon 4 was charged a broker fee of 551 ISK in game. The formula gives 551.84 for raw standings, 549.11 for effective ones (0.15 + (10 − 0.15) × 0.04) and 552.87 for no standing effect. The reading is whole ISK and the market window appears to truncate, so the check only rules out effective standings and "no effect"; it does not prove the 0.03% coefficient to more digits.
- **Broker Relations and base.** Seven earlier broker fees in the wallet journal (2026-10-14 and 2026-10-15, Jita IV - Moon 4) were each exactly 2.40000% of the order value: 3% − 0.3% × 2. This confirms the 3% base and 0.3% per level.
- **Sales tax base.** Two earlier `transaction_tax` entries were exactly 7.5% of the sale value (recovered from the matching broker fee entries), with Accounting untrained. The 11% per Accounting level was not checked in game, because no available character has Accounting trained. It rests on CCP's skill text (above).
- **Not covered:** the corporation coefficient 0.02%, and Diplomacy and Criminal Connections. The journal showed a relist or modification fee at half the rate (1.2%), which stays a non-goal.
