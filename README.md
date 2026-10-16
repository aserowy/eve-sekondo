# eve-sekondo

Sekondo ("second", as in corner man) is a personal helper for EVE Online. This first version lets you
sign in with EVE SSO, link several characters to one account, and keeps their ESI credentials valid.

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
- **Scopes:** `esi-markets.read_character_orders.v1` and `esi-universe.read_structures.v1`

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

Startup errors name the setting that is missing or invalid.

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
cargo test --workspace                        # unit and integration tests
cd e2e && npm install && npx playwright test  # browser tests
```

Rust tests start the app and a stand-in EVE SSO (`crates/eve-stub`) in-process with per-run certificates.
The Playwright suite builds and starts both with the mkcert certificates and pins Chromium to that one
certificate's SPKI hash; it never turns certificate checks off.

`@playwright/test` in `e2e/package.json` is pinned to the same version as nixpkgs' `playwright-driver`
(the browsers in `PLAYWRIGHT_BROWSERS_PATH`). Bump both together.

## Running against the stand-in instead of EVE

To log in without an EVE application, run the stub and point the app at it:

```sh
cargo run -p eve-stub -- --cert certs/dev.pem --key certs/dev-key.pem --listen 127.0.0.1:9443 \
  --client-id stub-client --client-secret stub-secret
# in .env:  EVE_CLIENT_ID=stub-client  EVE_CLIENT_SECRET=stub-secret
#           EVE_SSO_BASE_URL=https://localhost:9443  TLS_EXTRA_CA_PATH=./certs/rootCA.pem
cargo run -p sekondo
```

The stub's login page lists fake characters, like EVE's character picker.

## Database queries

Queries are plain `sqlx::query` calls, checked by the integration tests, so there is no `.sqlx/` cache.
If you add `query!` macros, run `cargo sqlx prepare --workspace` and commit `.sqlx/`. The Dockerfile builds
with `SQLX_OFFLINE=true`, so a stale or missing cache fails the image build. CI should run
`cargo sqlx prepare --workspace --check`.

## Layout

| Path | What |
|---|---|
| `crates/sekondo` | the app: library with all logic, thin `main.rs`, migrations, templates, static files |
| `crates/eve-stub` | a stand-in for EVE SSO (and later ESI) for tests and local development |
| `e2e/` | Playwright tests |
| `docker/` | database and app container support files |
| `openspec/` | specs, designs and tasks |
