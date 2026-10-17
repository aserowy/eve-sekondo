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
- **Scopes:** `esi-markets.read_character_orders.v1`, `esi-universe.read_structures.v1`, `esi-wallet.read_character_wallet.v1`, `esi-assets.read_assets.v1`, `esi-skills.read_skills.v1` and `esi-characters.read_standings.v1`. All six are required: tick every one of them on the application, or EVE's login answers `invalid_scope`. Characters linked with fewer scopes (every character linked before the skills and standings scopes were added) show "Needs re-login" until they log in once more.

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

The orders page (`/orders`) takes `?chars=<id>,<id>` to select characters and `sort`/`dir` (sell table)
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
  purchase lot, so the history grows beyond ESI's window of about 30 days. Sales and corporation
  purchases are not stored.
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

The tests need the database running (`podman-compose up -d db`), a `.env` as above, and the certificates.
They never touch the app's own database: `#[sqlx::test]` creates and drops a throwaway database per test,
and the Playwright setup recreates a dedicated `sekondo_e2e` database.

```sh
cargo test --workspace --features sekondo/test-hooks   # unit and integration tests
cd e2e && npm install && npx playwright test           # browser tests
```

The `test-hooks` feature adds `POST /_test/refresh/{id}`, `POST /_test/poll/{id}`, `POST /_test/wealth-poll/{id}/{kind}` (`wallet`, `assets` or `transactions`) and `POST /_test/fee-poll` (a signed-in
account's own characters only; CSRF-protected like any other POST). The first forces a token refresh, the
second runs one order poll right away, ignoring the schedule but not ESI's limits, so browser tests do not
have to wait for the scheduler. The third polls the skills and standings of all the account's characters and looks up its station owners. All are compiled out of everything else. The Playwright setup builds with
the feature and sets `ORDER_POLL_INTERVAL_MINUTES=1440` so the background scheduler stays out of the way.
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
| `stations` | `{"station_id", "owner"}` | sets the owner corporation of an NPC station |
| `revoke`, `scopes`, `owner-hash`, `fail-next-token`, `rotate-key` | | SSO behaviour, see `crates/eve-stub` |
| `stats` | (GET) | call counters per route |

ESI routes served: `GET /characters/{id}/orders` (needs a stub-issued token of that character with the
orders scope), `POST /universe/names` (a fixture table; one unknown ID fails the whole request with 404)
`GET /universe/structures/{id}` (needs the structures scope), `GET /characters/{id}/skills` and
`GET /characters/{id}/standings` (each needs its scope) and `GET /universe/stations/{id}` (public).

## Database queries

Queries are plain `sqlx::query` calls, checked by the integration tests, so there is no `.sqlx/` cache.
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
| `e2e/` | Playwright tests |
| `docker/` | database and app container support files |
| `openspec/` | specs, designs and tasks |
