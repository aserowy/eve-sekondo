# Proposal

## Why

`cargo test` is very slow. About 750 tests in `crates/sekondo/tests` are `#[sqlx::test]`. Each one
creates a new database on the Compose TimescaleDB and runs all 14 migrations (hypertables and
continuous aggregates included) before it can start. Most of them only test handlers, pollers or page
rendering and do not care which database sits underneath. The ESI side already solved this:
`eve-stub` stands in for EVE, and contracts verified against the real service keep it honest. The
database needs the same treatment, so that unit tests run in memory and only a dedicated, explicit run
touches Postgres.

## What Changes

- Every database access in `sekondo` goes through per-domain store traits (accounts and characters,
  sessions, orders, market and prices, station market, opportunities, wealth, fees, trades). Their
  Postgres implementation holds the existing SQL unchanged. `PgPool` is no longer reachable from
  handlers, pollers or pages.
- An in-memory implementation of every store trait, behind a `memory-store` cargo feature that release
  builds never enable. It computes what SQL computes (latest snapshots, sums over windows, the hourly
  and daily rollups) naively in Rust. Rollups appear only after an explicit refresh, as with
  `materialized_only` continuous aggregates.
- A new `store-contract` crate, modelled on `esi-contract`, holding:
  - contract cases that run against any store. The in-memory run is part of plain `cargo test`.
  - a `store-verify` command that runs the same cases against a real Postgres and, when every case
    passes, records a fingerprint in `verified-store.json`.
  - a gate: tests backed by the in-memory store fail up front, naming the verification command, while
    the fingerprint is stale. The fingerprint covers the store traits, both implementations, the
    contract cases and `crates/sekondo/migrations`.
  - the migration and schema tests (today's `tests/schema.rs`). They stay on Postgres and run only
    with the crate's `postgres` feature.
- Every test in `crates/sekondo/tests` (HTTP, poller and page tests) moves from `#[sqlx::test]` to
  `#[tokio::test]` on the in-memory store. Seeding with raw SQL is replaced by store calls or by seed
  helpers that both stores implement and the contract cases cover.
- The test harness generates its TLS certificates once per test binary instead of once per test.
- Untouched: `eve-stub`, `esi-contract`, `esi-verify` and the Playwright suite under `e2e/`, which keeps
  running the real app against real Postgres.

## Capabilities

### New Capabilities

- `store-contract`: the in-memory stand-in for the database, the contract cases that keep it equal to
  Postgres, its verification against Postgres, the fingerprint gate, and the guarantee that plain
  `cargo test` needs no database.

### Modified Capabilities

None. Product behaviour does not change. The Postgres store keeps today's SQL, and the app still
requires Postgres over verified TLS to start.

## Impact

- `crates/sekondo`: a new `store` module (traits, `pg`, `memory`). Every module that holds a `PgPool`
  changes (`repo`, `session`, `poller`, `orders`, `dashboard`, `freshness`, `market`, `prices`,
  `names`, `station_book`, `station_history`, `opportunities/*`, `wealth*`, `fee*`, `trade*`,
  `test_hooks`, `app`). `App::new` takes the stores instead of a pool. `main` connects, migrates and
  builds the Postgres stores.
- `crates/sekondo/tests`: the harness (`common/mod.rs`, `AppBuilder`, `PollEnv`) and every test file;
  `schema.rs` moves to `store-contract`.
- A new workspace member `crates/store-contract`, with a binary `store-verify` and a committed
  `verified-store.json`.
- README: how to run the tests, and when and how to run `store-verify` and the Postgres schema tests.
- No new runtime dependencies. No change to the database schema, configuration or deployment.
