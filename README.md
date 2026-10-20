# eve-sekondo

Sekondo ("second", as in corner man) is a personal helper for EVE Online. You sign in with EVE SSO, link
several characters to one account, and Sekondo keeps their ESI credentials valid and their market orders
current: one page lists every active sell and buy order of your characters (or a selection of them),
refreshing on its own when new data arrives.

Everything speaks **HTTPS only**, including on `localhost`, and the app connects to its database over
TLS with full certificate and hostname verification. There is no switch to turn any of that off.

## Prerequisites

The Nix flake provides everything: `nix develop` (or direnv) gives you Rust, `mkcert`, `sqlx-cli`,
`podman` and `podman-compose`, the PostgreSQL client tools, `openssl`, Node.js and a Playwright-compatible Chromium.

Without Nix you need the same tools installed yourself. Below, `podman-compose` can be replaced by
`docker compose`.

## 1. Register an EVE application

At <https://developers.eveonline.com/> create an application with:

- **Callback URL:** `https://localhost:8443/auth/callback`
- **Scopes:** `esi-markets.read_character_orders.v1`, `esi-universe.read_structures.v1`, `esi-wallet.read_character_wallet.v1`, `esi-assets.read_assets.v1`, `esi-skills.read_skills.v1`, `esi-characters.read_standings.v1` and `esi-location.read_location.v1`. Tick every one of them on the application, or EVE's login answers `invalid_scope`. The first six are required: characters linked with fewer scopes (every character linked before the skills and standings scopes were added) show "Needs re-login" until they log in once more. The location scope is optional: without it a character's fee cards use Jita 4-4 and offer to log in again, and nothing else changes.

Keep the client ID and the secret for `.env` below.

## 2. Certificates

```sh
mkcert -install            # once: trust the local CA in your browser and system
scripts/dev-certs.sh       # writes certs/{dev,dev-key,db,db-key,rootCA}.pem
```

`certs/` is gitignored. Run the script again if a certificate expires.

## 3. Settings

```sh
cp .env.example .env
```

Then edit `.env`:

- `EVE_CLIENT_ID` and `EVE_CLIENT_SECRET` from step 1.
- `TOKEN_ENCRYPTION_KEY`: `openssl rand -base64 32`. **Losing it makes every stored token unreadable**;
  all characters then show "Needs re-login" until you log in with them again.
- `POSTGRES_PASSWORD`: `openssl rand -hex 24` (hex keeps it URL-safe, because Compose puts it in a URL).
- `DATABASE_URL` is for tools on your machine (cargo, sqlx, tests). It must say `sslmode=verify-full`
  and give an **absolute** `sslrootcert` path to `certs/rootCA.pem`, e.g.
  `postgres://sekondo:<password>@localhost:5432/sekondo?sslmode=verify-full&sslrootcert=/home/me/eve-sekondo/certs/rootCA.pem`.
  The app refuses to start with anything weaker. Inside Compose the app uses `db:5432` instead; Compose
  sets that itself.

- `ORDER_POLL_INTERVAL_MINUTES` (optional, default 20, whole minutes from 1 to 1440): how often each
  character's orders are polled. Any other value refuses to start.
- `STATION_SYNC` (optional, `on` or `off`, default `on`): whether the background sync of the trading
  station's market (the order book, its snapshots and ESI's market history) runs. The browser tests turn
  it off and drive it through the test hooks. Any other value refuses to start.
- `ESI_CONTACT` (optional): an email address or EVE character name. It is appended to the `User-Agent`
  of every ESI request (`Sekondo/<version> (+<PUBLIC_BASE_URL>; <contact>)`), so CCP can reach you if
  your installation misbehaves.

Startup errors name the setting that is missing or invalid.

## What the poller does

The app polls ESI in the background, with nobody signed in, for every linked character that does not
need a re-login:

- Each character is polled at most once per `ORDER_POLL_INTERVAL_MINUTES`, never before the cache expiry
  (`Expires`) ESI reported for its previous answer, and polls are spread across the interval. A
  character that was just linked gets its first poll at a stable offset inside the next interval.
- A poll replaces the character's current orders with exactly what ESI returned and appends one row per
  order to the TimescaleDB hypertable `order_snapshots`, so history accumulates from the first poll. It is
  kept uncompressed and without a retention limit; views never read it. Unlinking a character deletes its
  snapshots.
- The poller follows ESI's rules: it backs off when the error limit runs low or ESI answers 420, and
  pauses a rate-limited route group until `Retry-After` has passed. Failures are recorded, leave the
  current orders alone and are retried with a growing delay, never longer than the interval.
- Item, region, station and structure names are looked up through ESI once and cached in the database.
  A structure ESI will not describe shows as "Unknown structure (id)" and is tried again after a day.

## Market fees

For every linked character the app also polls its skills and its NPC standings, each at most once an hour
and never before ESI's cache expiry, and shows what the market charges it on the orders page: under each
character, one row per station or structure where it has orders, with the sales tax, the broker fee and the
minimum margin of a station trade (hover a figure for the exact rate and its inputs).

- Sales tax: 7.5% × (1 − 0.11 × Accounting level). Broker fee at an NPC station: 3% − 0.3% × Broker
  Relations − 0.03% × faction standing − 0.02% × corporation standing, with the standings towards the
  station owner's faction and the owner corporation as ESI reports them. In a player structure the fee is
  "0.5% + owner fee": the owner's part is not exposed by ESI. The minimum margin is the markup on a buy
  price at which a buy order resold through a sell order breaks even.
- The owner corporation of an NPC station comes from ESI once per station. ESI does not report the faction of
  an NPC corporation, so the app keeps a table from CCP's static data export,
  `crates/sekondo/data/npc_corporation_factions.csv` (its header names the SDE build). A migration loads it
  into the database. To update it, run `scripts/npc-corporation-factions.sh` (needs `curl`, `unzip` and
  `python3`; pass a downloaded SDE zip to skip the download), then write a new migration with the changed
  rows. A corporation missing from the table counts as having no faction.
- The sources of the formulas, and what was checked in game, are in
  `openspec/changes/show-market-fees/design.md`.

The orders pages (`/orders/sell` and `/orders/buy`) take `?chars=<id>,<id>` to select characters and `sort`/`dir` (sell table)
and `bsort`/`bdir` (buy table) for the sorting; the URL can be bookmarked. Open pages are told about new
data over a server-sent event stream (`/events`) and re-fetch the affected tables themselves. A skills or
standings poll that changes a fee re-fetches the fee rows and the order tables, because the margins depend
on the fees.

## Undercut volume and margin

The orders tables show, next to "Undercut N", the **undercut volume**: the remaining units of exactly the
orders counted in "Undercut N" (empty for a top order, "—" without market data). The sell table also shows
the **margin** of each sell order: what its remaining units earn after fees, minus what they cost.

- **Transactions poll.** Once an hour per character (never before ESI's cache expiry) the app reads the
  wallet transactions. It uses the wallet scope the app already has, so no new login is needed. A poll
  reads from the newest transaction back until an answer holds a transaction it already stored, ESI
  returns nothing older, or 50 answers were read. Every personal purchase is stored permanently as a
  purchase lot, so the history grows beyond ESI's window of about 30 days. Personal sales are stored
  the same way (see "Trades"); corporation transactions are not stored.
- **First in, first out.** Every personal sell order is remembered with the time it was first issued and
  its total volume, also after it closes (a reprice does not change it). Per character and item, the
  orders in the order they were first issued take units from the oldest lots bought at or before the order
  was issued that earlier orders did not take, at any location. Repricing an order does not change what it
  took, and a purchase that is stored late is picked up because the allocation is redone from all lots.
  Another character's purchases are never used.
- **Margin formula.** `remaining volume × price × (1 − sales tax − broker fee) − remaining volume ×
  average cost per unit`, with the character's current fees at the order's location (see "Market fees")
  and the average cost per unit being the order's known cost over its total volume. It is rounded half up
  to the cent and can be negative (shown in the error style). Hover it for the exact amount and the cost
  per unit. The margin reads "—" when the cost is not known for every unit of the order (hover: "cost known
  for X of Y units"), when the fees are not known (including in a player structure), and for corporation
  orders.
- **Instant sales are not tracked.** Selling straight into someone else's buy order takes no lots, so a
  later sell order may get older, cheaper lots than it really did. By decision there is no attempt to
  tell those sales apart.
- The design and the specifications are in `openspec/changes/order-undercut-volume-and-margin/`.

## Trades

The **Trades** page (`/trades`, between Wealth and Opportunities) lists the personal wallet transactions
of the selected characters, newest first: date (EVE time), character, item, location, buy/sell,
quantity, unit price, value (quantity × unit price, before fees) and margin. The range control in the
table's title (`24h`, `7d` default, `30d`, `90d`) limits the rows, and a sticky totals row shows the bought
value, the sold value and the sum of the margins that are known (hover it for how many sales it leaves
out). The page refreshes live when a transactions poll stores something or a fee changes.

- **Storing sales.** The transactions poll above also stores every personal sale permanently, like the
  purchases (`sales` next to `purchases`). It stops at a transaction it already stored, whether a
  purchase or a sale.
- **First in, first out for sales.** Per character and item, the sales in the order of their date (the
  transaction ID breaking ties) take units from the oldest purchase lots bought at or before the sale
  that earlier sales did not take, at any location. This is separate from the sell-order allocation: a
  unit one takes is still free for the other, so a sale's cost and a sell order's cost are not additive.
  It is redone from all lots and sales whenever either changes.
- **Sale margin formula.** `quantity × unit price × (1 − sales tax − broker fee) − known cost`, with the
  character's current fees at the sale's location, rounded half up to the cent and negative when the sale
  lost money. It reads "—" when the known cost does not cover the whole quantity (hover: "cost known for X
  of Y units") or when the fees are not known (including in a player structure). A purchase has no
  margin. Fee changes show in old sales too, because the margin is worked out when the page is read.
- **Instant sales pay a broker fee here.** The wallet transactions do not tell a sale into a buy order
  from a sell-order fill, so the broker fee is charged on every sale and the margin of an instant sale is
  understated by `broker fee × value`. By decision there is no attempt to tell them apart.
- **One-time full read.** Sales were not stored before this page existed. After the upgrade each existing
  character's next transactions poll reads back through everything ESI still offers (about 30 days, at
  most 50 answers) once, ignoring what is stored, to fill in the sales in that window. Older sales are
  gone, so the table can show purchases without their sales.
- The design and the specifications are in `openspec/changes/trades-overview/`.

## Station trading opportunities

The app keeps a public record of one market, Jita IV - Moon 4 - Caldari Navy Assembly Plant in The Forge,
for every account, and the Opportunities page (`/opportunities`) shows what is worth buying there. The
sync can be switched off with `STATION_SYNC=off`.

- **Order book.** At most every 5 minutes, and never before ESI's cache expiry, the whole public order
  book of The Forge is read (a few hundred pages, eight at a time through the same ESI gate as every
  other call). A read whose pages belong to different cache generations (they must share one `Expires`)
  is repeated at once, at most twice. A refresh where any page fails keeps the previous book, records
  nothing and is retried with the usual backoff. The orders at the station replace `station_orders`, and
  every item that has a buy and a sell order gets one row in the `station_snapshots` hypertable: best
  prices, spread, units and orders within 1% per side, total units, and the units taken since the last
  refresh. Snapshots are compressed after a day and deleted after 30.
- **Rollups.** `station_hourly` and `station_daily` are hierarchical continuous aggregates of the
  snapshots (only completed periods are read). `station_daily_figures` joins them with ESI's daily market
  history: a day's low, high and spread come from the own data when it covers at least 20 of 24 hours,
  otherwise from the history, and every figure says which.
- **Market history.** Once a day per tradable item, two requests at a time, the last 30 days of ESI's
  market history of The Forge are stored. A failing item is retried with a growing delay while the others
  go on; an item without history is asked again the next day.
- **Per-item statistics** (`station_item_stats`) hold what does not depend on the account: the book
  figures after every refresh, the indicators (volume, volatility, SMA 5/20, Bollinger %B, RSI 14, the
  fresh SMA crossing) after a history pass, and the recurring spread window when the hourly rollup
  advanced. All maths are pure functions in `crates/sekondo/src/opportunities/analysis.rs`.
- **The pages** show one table each: `/opportunities/expansion`, `/opportunities/classic`,
  `/opportunities/reversion` and `/opportunities/trend` (`/opportunities` goes to the first). A sub-menu
  switches between them without row counts, and every table lists every item that passes its rules,
  paged 50 at a time after sorting by ISK/day unless you sort a column (each table keeps its own sorting
  in the URL: `xs`/`xd`, `cs`/`cd`, `ms`/`md`, `ts`/`td`). The freshness footer holds the
  station sync's status. For every row the request works out the cost
  `c = B (1 + b)`, the revenue `r = S (1 - b - t)`, the margin `m`, the order size, the fill time, the
  allowed holding time `min(3, (m / 2σ)^2)` days and ISK/day, with the fees of the selected character that
  has the lowest minimum margin at the station (base rates of 3% and 7.5% while none is known). An item
  whose fill time exceeds its holding time, whose margin is not positive, or that has fewer than 10 daily
  returns is in no table. The filter section (the character filter, then minimum margin, minimum ISK/day,
  an item name search and the capital per item, which are saved per account as soon as one changes) also
  names the station and the fees in use on its right. A click on a row opens the detail page
  `/opportunities/<table>/<type id>` with the fees, every figure and how it follows from the others, and
  each chart of the table on a row of its own; its sub-menu leads back to the table with the selection
  and sorting kept.
- The owner of the station is looked up once, for every account, so a character has fees there whether or
  not it has an order in Jita.
- The design and the specifications are in `openspec/changes/station-trading-opportunities/`.

## Page layout

Every signed-in page follows the layout guidelines in `openspec/specs/ui-layout/spec.md`, styled with
Pico CSS (vendored in `crates/sekondo/static/`) and a small stylesheet of our own. A collapsible sidebar
holds the main menu (icons and text, icons only when collapsed; the state is kept per browser). Every
icon starts at one left edge, and so does every text; the collapse toggle sits at the right edge of the
expanded sidebar and joins the icon column when collapsed. The text stays at Pico's unscaled 16px on
every screen. Every page starts with its tab-style sub-menu (if it has siblings or sections; it sticks to the top
of the window; its entries are packed at the left with a fixed gap), then its title (a page without a
sub-menu starts with the title) and the filter section. The filter section follows Pico's form
structure: the character filter is a group with a "Characters" legend over side-by-side checkboxes,
text and number fields have their label above the input in columns of equal width (every text filter,
the item name search included, is a plain input without a search pill), the time range is a labelled
group of joined buttons, and a rejected value marks its own input invalid with its message right below
it. The filter groups are set apart by spacing alone (3rem across, 0.75rem between wrapped lines), with
no divider. A table page continues with the KPI cards, **one** table (cells padded 0.3rem by 0.75rem,
text 0.875rem, header text never underlined, not even the sort links) and its pagination (page
size 25, 50, 100 or 200, in the URL as `page` and `per_page`), and ends with the freshness as a quiet
footer. The pagination is the last block of the main content: on a short page it sits at the bottom of
the window directly above the footer, on a long page it follows the table. Every graph is a card of its
own (an `<article class="graph">`) holding its title, the chart and its empty-state text. Sorting,
filters, ranges and pages are server-rendered and swapped in place; a live refresh keeps the page, the
sorting and the scroll position. The orders are split into `/orders/sell` and `/orders/buy` (`/orders`
goes to the sell page), the fee cards show each selected character's fees at the station it is docked
in (Jita 4-4 until its location is known). Read the spec before adding a page.

## 4. Run with Compose

```sh
podman-compose up --build        # or: docker compose up --build
```

Open <https://localhost:8443/>. Both services listen on `127.0.0.1` only.

**Your data persists.** The database lives in the named volume `sekondo-db-data`. It survives
`down`/`up`, `restart`, `--build` and container recreation, and migrations run on every start (a no-op
when nothing is new). Things to know:

- `podman-compose down -v` (or removing the volume, or `volume prune`) **wipes the database**.
- `POSTGRES_USER`, `POSTGRES_PASSWORD` and `POSTGRES_DB` only take effect when the volume is first
  created. Changing them in `.env` later does not change the existing database.
- The database image tag is pinned. A data directory only starts with the PostgreSQL major version that
  created it, so changing the major version is a deliberate step (`pg_upgrade`, or dump and restore).

**Backups** over TLS from your machine, with the same verification as the app:

```sh
PGPASSWORD=<password> pg_dump \
  "sslmode=verify-full sslrootcert=certs/rootCA.pem host=localhost user=sekondo dbname=sekondo" \
  > sekondo-backup.sql
```

Restore into an empty database with `psql` and the same connection string.

The database image is TimescaleDB, so `pg_dump` prints warnings about the extension's internal tables;
they are expected. To restore a dump into a TimescaleDB database, wrap it in
`SELECT timescaledb_pre_restore();` before and `SELECT timescaledb_post_restore();` after.

Run only the database (for development and tests) with `podman-compose up -d db`.

## 5. Tests

`cargo test` needs no database, no `.env` and no running container. The app reaches its data through
one store trait per domain (`crates/sekondo/src/store`): Postgres in production, an in-memory copy
(`MemoryStores`, feature `memory-store`) in the tests. Only the browser tests and the checks of the
Postgres side below need the database (`podman-compose up -d db`), a `.env` as above and the certificates
(the Playwright ones).

```sh
cargo test --workspace --features sekondo/test-hooks   # unit and integration tests, no database
cd e2e && npm install && npx playwright test           # browser tests, on Postgres
```

### The in-memory store must be verified against Postgres

A test on the in-memory store only means something if it behaves like Postgres. `crates/store-contract`
holds one contract case per behavior of the stores. The cases run on the in-memory store in
`cargo test -p store-contract --test memory`, and `store-verify` runs the same cases on a real Postgres.

A fingerprint of the store sources, the migrations and the cases is committed in
`crates/store-contract/verified-store.json`. **If any of them changes, every test of `sekondo` fails**
with "the store changed since its last verification against PostgreSQL" until you run, with the database up
(`podman-compose up -d db`, `DATABASE_URL` as in `.env`, with `sslmode=verify-full`):

```
cargo run -p store-contract --bin store-verify
git add crates/store-contract/verified-store.json
```

`store-verify` creates the database `sekondo_store_verify`, migrates it, runs every case on Postgres and on
the in-memory store, and records the fingerprint only when all of them pass. It never touches the app's own
database. A new store method needs a Postgres implementation, an in-memory implementation and a contract
case; a test in `store-contract` checks that every method has one.

The Postgres schema and the migrations are checked on their own, in throwaway databases that
`#[sqlx::test]` creates and drops:

```sh
cargo test -p store-contract --features postgres
```

The Playwright setup recreates a dedicated `sekondo_e2e` database.

The `test-hooks` feature adds `POST /_test/refresh/{id}`, `POST /_test/poll/{id}`, `POST /_test/wealth-poll/{id}/{kind}` (`wallet`, `assets` or `transactions`) and `POST /_test/fee-poll` (a signed-in
account's own characters only; CSRF-protected like any other POST). The first forces a token refresh, the
second runs one order poll right away, ignoring the schedule but not ESI's limits, so browser tests do not
have to wait for the scheduler. The third polls the skills and standings of all the account's characters and looks up its station owners. `POST /_test/station-book`, `/_test/station-history`, `/_test/station-rollups` and `/_test/station-seed` (a JSON body of synthetic snapshots and history days for past times) drive the trading station's sync. All are compiled out of everything else. The Playwright setup builds with
the feature and sets `ORDER_POLL_INTERVAL_MINUTES=1440` and `STATION_SYNC=off` so the background schedulers stay out of the way.
Without the feature, `cargo test` skips the tests of the hooks.

Playwright needs port 8443 for its own app instance, so stop the Compose `app` container first
(`podman-compose stop app`); the database must keep running.

Rust tests start the app and a stand-in EVE SSO (`crates/eve-stub`) in-process with per-run certificates.
The Playwright suite builds and starts both with the mkcert certificates and pins Chromium to that one
certificate's SPKI hash; it never turns certificate checks off.

`@playwright/test` in `e2e/package.json` is pinned to the same version as nixpkgs' `playwright-driver`
(the browsers in `PLAYWRIGHT_BROWSERS_PATH`). Bump both together.

### The stub must be verified against real EVE

Every test that talks to "EVE" talks to `eve-stub`, so a green run only means something if the stub behaves
like real SSO and ESI. `crates/esi-contract` holds one contract per call the app makes (status, the headers
the app reads, the JSON fields and which may be missing). The stub's answers are checked against them in
`cargo test -p eve-stub --test contract`, and `esi-verify` checks the real services' answers against the
same contracts.

A fingerprint of the stub's source and the contracts is committed in
`crates/esi-contract/verified-stub.json`. **If the stub or a contract changes, every stub-backed test
(Rust and Playwright) fails** with "the stub changed since it was last verified against real EVE" until you:

```
cargo run -p esi-verify      # logs in on real EVE in your browser, then checks every call
git add crates/esi-contract/verified-stub.json
```

Preconditions of `esi-verify`:

- A dedicated **verification character** with at least one sell order, one buy order, one asset and one NPC
  corporation or faction standing. It needs a browser and a person, so it cannot run headless. The application
  needs all six scopes.
- `ESI_VERIFY_STRUCTURE_ID` (a structure the character can open; any public one from
  `GET https://esi.evetech.net/universe/structures/?filter=market` works) and
  `ESI_VERIFY_FORBIDDEN_STRUCTURE_ID` (one it cannot, an unknown ID answers 403 too) in `.env`, plus
  `PUBLIC_BASE_URL`, `TLS_CERT_PATH` and `TLS_KEY_PATH` as in `.env.example`.
- The Compose `app` container stopped (`podman-compose stop app`): the login callback uses port 8443.
- `EVE_SSO_BASE_URL` and `ESI_BASE_URL` at their real defaults; it refuses to run against a stand-in.

Tokens only live in memory during the run; nothing but the fingerprint, the time and the verified variant
names is written. `cargo run -p eve-stub` for local development is not gated.

## Running against the stand-in instead of EVE

To log in without an EVE application, run the stub and point the app at it:

```sh
cargo run -p eve-stub -- --cert certs/dev.pem --key certs/dev-key.pem --listen 127.0.0.1:9443 \
  --client-id stub-client --client-secret stub-secret
# in .env:  EVE_CLIENT_ID=stub-client  EVE_CLIENT_SECRET=stub-secret
#           EVE_SSO_BASE_URL=https://localhost:9443  TLS_EXTRA_CA_PATH=./certs/rootCA.pem
cargo run -p sekondo
```

Also set `ESI_BASE_URL=https://localhost:9443` to let the stub stand in for ESI as well. The stub's login
page lists fake characters, like EVE's character picker. Alice, Bob and Carol come with fixture orders
(Alice: two sell orders and a buy order in a structure; Bob: a sell order; Carol: none). The poller picks
them up after the first interval.

Control endpoints under `/_stub/` (POST with JSON and `Authorization: Bearer <control token>`; `reset`
restores everything) change how the stub behaves:

| Endpoint | Body | Effect |
|---|---|---|
| `orders` | `{"character_id", "orders": [...]}` | replaces the orders ESI returns for a character |
| `esi-expires` | `{"seconds"}` | how far ahead the `Expires` header points (default 1200) |
| `structure-access` | `{"structure_id", "allowed"}` | allows or refuses (403) a structure lookup |
| `esi-fail-next` | `{"status", "count"?, "route"?, "headers"?}` | the next calls (of one route, or any) answer 5xx, 420 or 429 with the given headers |
| `esi-delay` | `{"millis"}` | the orders route answers slowly |
| `skills` | `{"character_id", "skills": [...]}` | replaces the skills ESI returns for a character (ESI's objects with `skill_id`, `active_skill_level`, ...) |
| `standings` | `{"character_id", "standings": [...]}` | replaces the standings (`from_id`, `from_type`, `standing`) |
| `location` | `{"character_id", "location"}` | replaces where a character is: `{"solar_system_id", "station_id"}`, `{..., "structure_id"}` or just `{"solar_system_id"}` (open space); everybody is docked in Jita 4-4 by default |
| `stations` | `{"station_id", "owner"}` | sets the owner corporation of an NPC station |
| `revoke`, `scopes`, `owner-hash`, `fail-next-token`, `rotate-key` | | SSO behaviour, see `crates/eve-stub` |
| `stats` | (GET) | call counters per route |

ESI routes served: `GET /characters/{id}/orders` (needs a stub-issued token of that character with the
orders scope), `POST /universe/names` (a fixture table; one unknown ID fails the whole request with 404)
`GET /universe/structures/{id}` (needs the structures scope), `GET /characters/{id}/skills` and
`GET /characters/{id}/standings` and `GET /characters/{id}/location` (each needs its scope) and `GET /universe/stations/{id}` (public).

## Database queries

Queries are plain `sqlx::query` calls in `crates/sekondo/src/store/pg`, checked by `store-verify` and the schema tests, so there is no `.sqlx/` cache.
If you add `query!` macros, run `cargo sqlx prepare --workspace` and commit `.sqlx/`. The Dockerfile builds
with `SQLX_OFFLINE=true`, so a stale or missing cache fails the image build. CI should run
`cargo sqlx prepare --workspace --check`.

## Layout

| Path | What |
|---|---|
| `crates/sekondo` | the app: library with all logic, thin `main.rs`, migrations, templates, static files |
| `crates/eve-stub` | a stand-in for EVE SSO and the ESI routes the app uses, for tests and local development |
| `crates/esi-contract` | contracts for the calls to ESI and SSO, the stub fingerprint and the gate |
| `crates/esi-verify` | `cargo run -p esi-verify`: verifies the contracts against real EVE |
| `crates/store-contract` | contract cases for the stores, the fingerprint and the gate, `store-verify`: runs them on Postgres |
| `e2e/` | Playwright tests |
| `docker/` | database and app container support files |
| `openspec/` | specs, designs and tasks |
