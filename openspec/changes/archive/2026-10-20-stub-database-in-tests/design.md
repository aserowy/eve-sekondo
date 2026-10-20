# Design

## Context

- `AppState` holds `pool: PgPool`. About 170 SQL statements are spread over 22 modules: `repo`,
  `session` (`PgSessionStore`, a `tower_sessions::SessionStore`), `poller`, `orders`, `dashboard`,
  `freshness`, `market`, `prices`, `names`, `station_book`, `station_history`, `opportunities/{page,
  tables,settings,stats}`, `wealth`, `wealth_poller`, `wealth_page`, `fees`, `fee_poller`, `trade`,
  `trades`, `trades_page` and `test_hooks`. Pollers take a `PgPool` in their constructors.
- `App::new` runs `sqlx::migrate!` on the pool it is given. Every test in `crates/sekondo/tests` is
  `#[sqlx::test]`, which creates a database per test on the Compose TimescaleDB and runs all 14
  migrations. About 200 statements in those tests seed or inspect tables with raw SQL (not counting
  `schema.rs`, which has another ~100).
- Some SQL works on TimescaleDB objects: hypertables (`order_snapshots`, `order_poll_totals`,
  `wallet_balances`, `inventory_values`, `station_snapshots`) and two hierarchical continuous
  aggregates, `station_hourly` and `station_daily`, with `materialized_only = true`. They are refreshed
  by a policy in production and by `CALL refresh_continuous_aggregate` in `test_hooks`. Several
  queries use `now()` instead of the app's `Clock`.
- The model to follow is `esi-contract`. `fingerprint.rs` hashes the stub source and the contracts.
  `gate::require_verified_stub()` compares that hash with `verified-stub.json`. `esi-verify` is the
  explicit command that refreshes the record.
- `TestPki::generate()` runs once per `AppBuilder::spawn` and once per `PollEnv::new`.

## Goals / Non-Goals

**Goals:**

- `cargo test` (default features) runs every `sekondo` test without a database.
- The in-memory store can't silently drift from Postgres. The gate makes sure of it, as it does for
  `eve-stub`.
- The Postgres store keeps today's SQL statement for statement. Moving the SQL is mechanical, so the
  move itself carries little risk.

**Non-Goals:**

- Faster production SQL, or schema changes.
- Modelling TimescaleDB compression, retention policies or chunking in memory. The contract cases
  don't touch time ranges where those would make a difference.
- Changing `eve-stub`, `esi-contract`, `esi-verify` or the Playwright suite.
- Making the HTTP tests in-process (no TLS). They still start the HTTPS server and, where needed,
  `eve-stub`. Only the database goes away.

## Decisions

### D1. One store trait per domain, bundled in `Stores`

`crates/sekondo/src/store/mod.rs` defines object-safe `async_trait` traits, one per domain:
`AccountStore` (accounts, characters, encrypted tokens, unlink), `SessionStore` data access,
`OrderStore` (current orders, snapshots, poll state and totals, freshness), `MarketStore`
(market orders, prices, names), `StationStore` (book, snapshots, history, item stats, rollups),
`OpportunityStore` (settings, tables, detail reads), `WealthStore`, `FeeStore` and `TradeStore`.
A `Stores` struct holds one `Arc<dyn …>` of each and replaces `AppState.pool`. Pollers and pages take
only the trait they need.

The trait methods sit at the level the module needs today, one method per current query or per tight
group of queries run in one transaction. Row types become plain domain structs in `store`, not
`sqlx::FromRow` tuples. Transactions that today span several statements (replace orders and append
snapshots, for example) become a single store method, so neither implementation leaks a transaction
type.

Alternatives: one big `Store` trait is simpler to wire but every fake and every test pulls in the
whole surface. Stubbing only some modules leaves those tests on Postgres and doesn't meet the goal.

### D2. Postgres store: `store/pg/`, SQL moved verbatim

Each domain gets `store/pg/<domain>.rs`, which implements its trait over a `PgPool` with the existing
queries copied unchanged. `PgSessionStore` moves there. `PgStores::connect(config)` builds the pool,
runs `sqlx::migrate!` and returns `Stores`. `main` calls it. `App::new_with_options` takes `Stores`
instead of a `PgPool` and no longer migrates.

Where SQL uses `now()`, it stays as it is. The in-memory store reads the injected `Clock` instead,
and the contract cases that touch those queries use a clock that matches wall time. That way neither
production nor the contract depends on the difference between the two.

### D3. In-memory store: `store/memory/`, behind feature `memory-store`

`MemoryStores::new(clock)` builds every trait over one `Mutex<Tables>` that holds plain `Vec`s or
`BTreeMap`s per table. Derived reads (`DISTINCT ON` latest snapshot, sums over the 30-day window,
tradable items, poll totals) are computed naively in Rust when they are read. Being clear matters
more than speed here.

The rollups are kept as materialized maps. `refresh_rollups(from, to)` recomputes `station_hourly`
from the snapshots and `station_daily` from `station_hourly` for the buckets in range, with the same
bucket boundaries and aggregates as the view definitions. Reads return only materialized buckets, as
`materialized_only = true` does. `test_hooks` calls `StationStore::refresh_rollups` instead of the SQL
`CALL`.

The session data uses the same tables-in-a-mutex approach. `MemorySessionStore` implements
`tower_sessions::SessionStore` with the expiry semantics of `PgSessionStore`.

`sekondo/Cargo.toml` declares `[features] memory-store = []`, and the module is
`#[cfg(feature = "memory-store")]`. The crate enables it for its own tests through a
self-dev-dependency (`sekondo = { path = ".", features = ["memory-store", …] }`). Release builds never
see it, and no config value selects it.

Alternative: share one migrated Postgres database per test binary and truncate between tests. That
is much less work, but it still needs a running database, and the user asked for stubs.

### D4. Seeding through the stores, not through SQL

Tests stop running raw SQL. When a test needs data that the app itself writes (orders after a poll,
snapshots, wallet history), it either drives the app path that writes it or calls the store's write
method. Where no app write exists for a fixture (rows that only a past migration or another domain
produces, or rows with chosen timestamps), each trait gets a small `seed_*` method group, implemented
by both stores and exercised by contract cases. A seed is therefore verified like any other call.
Seeds stay on the trait, not on the in-memory type alone, so a test can't create a state that
Postgres would reject.

### D5. `store-contract` crate, modelled on `esi-contract`

`crates/store-contract` depends on `sekondo` with `memory-store` and contains:

- `src/cases/<domain>.rs`: contract cases as `async fn(&Stores, &dyn ClockControl)`. Each writes
  through the traits and asserts on the reads. A registry lists them by name (`orders/replace-keeps-snapshots`, …).
- `src/fingerprint.rs`: SHA-256 over `crates/sekondo/src/store/**`, `crates/sekondo/migrations/**`
  and `crates/store-contract/src/cases/**`, with relative path and contents in path order, and the
  same length-prefixed sections as `esi-contract`. It is read from the working tree.
- `src/gate.rs`: `require_verified_store()`. It panics with "the store changed since its last
  verification against PostgreSQL; run `cargo run -p store-contract --bin store-verify`" when the
  fingerprint in `verified-store.json` differs. There is no environment override.
- `tests/memory.rs`: one `#[tokio::test]` per case against `MemoryStores`, without the gate. This is
  part of plain `cargo test`.
- `src/bin/store-verify.rs`: reads `DATABASE_URL` (it must say `sslmode=verify-full`, checked the same
  way as `Config`), creates a dedicated `sekondo_store_verify` database, migrates it once, and runs
  every case against it, resetting all tables and rollups before each case (`TRUNCATE … CASCADE` plus
  a full rollup refresh). It also runs every case against `MemoryStores`. Only when all cases pass
  does it write `verified-store.json` (`fingerprint`, `verified_at`, `cases`). Otherwise it prints
  each failing case with the difference and exits non-zero.
- `tests/schema.rs`: today's `crates/sekondo/tests/schema.rs`, moved, under
  `#![cfg(feature = "postgres")]` and still `#[sqlx::test(migrator = …)]` against
  `../sekondo/migrations`. It runs with `cargo test -p store-contract --features postgres`.

`crates/sekondo` dev-depends on `store-contract` for the gate. That is a dev-dependency cycle
(sekondo tests → store-contract → sekondo), which Cargo allows for integration tests in `tests/`.
The gate is called only from those tests, never from `#[cfg(test)]` modules inside `src`.

Alternative: a dedicated `store-verify` crate next to `esi-verify`. One crate is enough, because
verifying needs only the cases and a database, not a browser or secrets.

### D6. Test harness on the in-memory store, certificates once

In `tests/common/mod.rs`:

- `stores(clock)` calls `store_contract::gate::require_verified_store()` and returns
  `MemoryStores::new(clock)`.
- `AppBuilder::new()` and `PollEnv::new()` no longer take a `PgPool`. They build their stores with
  `stores(clock)` and expose `stores` (typed as `MemoryStores`, so tests can reach seeds).
- `TestPki` comes from a `static OnceLock<TestPki>`, generated on first use. Each spawn still writes
  the PEM files into its own `TempDir`, because `Config` wants paths.

Every test becomes `#[tokio::test]`. Tests that today read tables with SQL to assert on them use the
store's read methods instead.

### D7. Migration order: extract, fake, switch

1. Extract every domain's trait and Postgres implementation while tests stay on `#[sqlx::test]`. The
   test harness wraps the pool in `PgStores`. Behaviour is unchanged and the suite stays green
   throughout.
2. Add the in-memory implementations and contract cases domain by domain, then verify once.
3. Switch the harness, then convert the test files group by group.

`PgPool` disappears from `AppState` at the end of step 1, so step 3 never has a half-database state.

## Risks / Trade-offs

- [The in-memory logic for rollups and windows drifts from SQL] → Contract cases cover each derived
  read with edge cases at bucket and window boundaries, and the gate forces a Postgres run after any
  store or migration edit.
- [Moving 170 statements introduces a typo] → The SQL is copied verbatim. Step 1 runs the existing
  Postgres-backed suite unchanged before any test is converted.
- [Contract cases test less than the old per-feature tests did against Postgres] → Every store method
  needs at least one case. A task checks that coverage with a registry-against-trait list before the
  first verification.
- [`now()` in SQL versus the injected clock] → See D2. Contract cases don't freeze the clock across
  queries that use `now()`.
- [The dev-dependency cycle confuses the type identity of `sekondo` items] → The gate takes no
  `sekondo` types. If the cycle still causes trouble, split `fingerprint` and `gate` into a small
  `store-fingerprint` crate without a `sekondo` dependency.
- [The gate adds friction to editing a store] → This is intended and mirrors `eve-stub`. Only edits
  under the fingerprinted paths need the Postgres run.
- [Playwright depends on `test_hooks` refreshing rollups] → `test_hooks` keeps the same HTTP surface
  and calls the store, and the Postgres store issues the same `CALL`.

## Migration Plan

This is a developer-facing change only. There is no deployment step, no schema change and no data
migration. The README gains a "Tests" section covering plain `cargo test`, when to run `store-verify`,
and how to run the Postgres schema tests. Rollback is a revert of the change's commit.
