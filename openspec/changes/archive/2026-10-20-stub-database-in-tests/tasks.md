# Tasks

No task in this change touches the UI. The existing Playwright suite is run unchanged in group 6 as a
regression check. Groups 1–2 keep the current `#[sqlx::test]` suite as the safety net, so run them
with the Compose database up.

## 1. Store scaffold (Postgres-backed, behaviour unchanged)

- [x] 1.1 Add `crates/sekondo/src/store/{mod.rs,pg/mod.rs}` with an empty `Stores` bundle and
      `PgStores::connect(&Config)` (pool, verified TLS, `sqlx::migrate!`). Switch `main` to it, make
      `App::new_with_options` take `Stores` plus the pool (the pool is removed in 2.10), and drop the
      migration from `App`. Wrap the pool in `PgStores` in `tests/common/mod.rs`. Verify that
      `cargo test -p sekondo` passes unchanged and the app starts against Compose Postgres.

## 2. Extract the Postgres stores (SQL moved verbatim)

Each task defines the domain trait and domain row types in `store/mod.rs`, moves the module's SQL
verbatim into `store/pg/<domain>.rs`, makes the module, its poller and its page call the trait, and
verifies that the existing `cargo test -p sekondo` tests for that domain pass unchanged.

- [x] 2.1 `AccountStore`: `repo.rs` (accounts, characters, encrypted tokens, unlink) and its callers
      (`tokens`, `auth`, `characters`, `account`). Verify with `tests/{repo,tokens,unlink,callback,characters_page}.rs`.
- [x] 2.2 Sessions: move `PgSessionStore` to `store/pg/session.rs` behind the session data trait.
      Verify with `tests/{sessions,logout,csrf,login_start}.rs`.
- [x] 2.3 `OrderStore`: `poller.rs`, `orders.rs`, `freshness.rs`, and the order reads in
      `dashboard.rs`. Verify with `tests/{poll,poll_hook,scheduler,orders_page,freshness,dashboard}.rs`.
- [x] 2.4 `MarketStore`: `market.rs`, `prices.rs`, `names.rs`. Verify with `tests/{market,prices}.rs`.
- [x] 2.5 `StationStore`, book and history: `station_book.rs`, `station_history.rs`. Verify with
      `tests/{station_book,station_history}.rs`.
- [x] 2.6 `StationStore`, item stats and rollups: `opportunities/stats.rs`, plus `refresh_rollups`
      used by `test_hooks.rs` in place of its SQL `CALL`. Verify with `tests/{station_stats,station_hooks}.rs`.
- [x] 2.7 `OpportunityStore`: `opportunities/{page,tables,settings}.rs`. Verify with
      `tests/{opportunities_page,opportunities_tables,opportunities_detail,opportunities_settings,opportunity_charts,analysis}.rs`.
- [x] 2.8 `WealthStore`: `wealth.rs`, `wealth_poller.rs`, `wealth_page.rs`. Verify with
      `tests/{wealth,wealth_poll,wealth_page}.rs`.
- [x] 2.9 `FeeStore` and `TradeStore`: `fees.rs`, `fee_poller.rs`, `trade.rs`, `trades.rs`,
      `trades_page.rs`. Verify with `tests/{fees,fee_poll,fees_page,trade,trades_page}.rs`.
- [x] 2.10 Remove `pool` from `AppState`, from every poller constructor and from `App`. Verify that
      `grep -rn "PgPool\|sqlx::query" crates/sekondo/src` only hits `store/pg/` and `PgStores::connect`,
      and that the full `cargo test -p sekondo` passes.

## 3. `store-contract` crate

- [x] 3.1 Create `crates/store-contract` (workspace member, depends on `sekondo` with feature
      `memory-store`). Add `[features] memory-store` and the `store/memory/mod.rs` skeleton
      (`MemoryStores::new(clock)`, `Mutex<Tables>`) to `sekondo`, and enable it through `sekondo`'s
      self-dev-dependency. Verify that `cargo build --release -p sekondo` does not compile
      `store::memory` (`cargo tree -e features -p sekondo --edges normal` does not list `memory-store`).
- [x] 3.2 Add `fingerprint.rs` (store sources, migrations, contract cases; length-prefixed sections
      as in `esi-contract`) with unit tests for: tree order independence, a changed, added or moved
      file, a changed migration, and an edit outside the covered paths that must not count. Verify
      with `cargo test -p store-contract`.
- [x] 3.3 Add `gate::require_verified_store()` and `verified-store.json` handling, with unit tests:
      the matching fingerprint passes, and a stale or missing record fails with a message naming
      `cargo run -p store-contract --bin store-verify`. Verify with `cargo test -p store-contract`.
- [x] 3.4 Add the case registry and `tests/memory.rs` (one test per registered case, ungated), and
      `src/bin/store-verify.rs`: `verify-full` check of `DATABASE_URL`, (re)create
      `sekondo_store_verify`, migrate once, reset before each case, run every case against Postgres
      and memory, write the record only when all pass. Verify by running it with zero cases: it
      writes the record, and a deliberately failing dummy case leaves the record unchanged and exits
      non-zero.
- [x] 3.5 Move `crates/sekondo/tests/schema.rs` to `crates/store-contract/tests/schema.rs` under
      `#![cfg(feature = "postgres")]` with the migrator pointing at `../sekondo/migrations`. Verify
      that `cargo test -p store-contract --features postgres --test schema` passes and that plain
      `cargo test -p store-contract` skips it.

## 4. In-memory stores and contract cases

Each task implements the domain trait in `store/memory/<domain>.rs`, adds the `seed_*` methods its
tests will need to both stores, and writes contract cases for every method of the trait, edge cases
included. It is verified by `cargo test -p store-contract --test memory` passing for that domain and
by `store-verify` passing for that domain's cases against Compose Postgres.

- [x] 4.1 Accounts and characters: token ciphertext round trip, upsert of an existing character,
      unlink outcome, account deletion.
- [x] 4.2 Sessions: `MemorySessionStore` with idle and absolute expiry identical to `PgSessionStore`,
      and expired deletion.
- [x] 4.3 Orders: replacing current orders appends snapshots, poll state and backoff, poll totals,
      freshness reads, and the dashboard order reads.
- [x] 4.4 Market, prices and names: upserts, missing names, the price map.
- [x] 4.5 Station book and history: book replace, the latest snapshot per item (`DISTINCT ON`), and
      history day upserts.
- [x] 4.6 Station rollups and item stats: buckets are empty until refreshed, hourly and daily bucket
      boundaries, a daily bucket built from hourly ones, the 30-day taken window that splits between
      daily and hourly, tradable items, and stats rows that follow the tradable set.
- [x] 4.7 Opportunities: settings defaults and updates, table reads with filters and sorting, and
      detail reads.
- [x] 4.8 Wealth: wallet balances, inventory values, and the history reads over ranges.
- [x] 4.9 Fees and trades: fee inputs (skills, standings, location), transactions from-id paging,
      trade margin reads.
- [x] 4.10 Coverage check: a test in `store-contract` fails when a trait method has no case in the
      registry (via a hand-maintained per-trait method list that sits next to the traits and is
      covered by the fingerprint). Run `cargo run -p store-contract --bin store-verify` with every
      case and commit `verified-store.json`. Verify that the gate passes with the committed record.

## 5. Switch the tests to the in-memory store

- [x] 5.1 Harness: in `tests/common/mod.rs`, add `stores(clock)` (gate plus `MemoryStores`) and make
      `TestPki` a shared `OnceLock`. `AppBuilder::new()` and `PollEnv::new()` take no pool. Convert
      `tests/{server,headers,no_test_hooks,icons,partials,navigation,events_stream}.rs` to
      `#[tokio::test]`. Verify with those tests passing with the Compose database stopped.
- [x] 5.2 Convert the auth and account tests: `tests/{callback,login_start,logout,csrf,sessions,tokens,unlink,characters_page,selection}.rs`.
      Move what `tests/repo.rs` covers into contract cases from 4.1 and delete the file. Verify with
      the Compose database stopped.
- [x] 5.3 Convert the order tests: `tests/{poll,poll_hook,scheduler,freshness}.rs`. Replace raw SQL
      with store reads and seeds. Verify with the Compose database stopped.
- [x] 5.4 Convert `tests/{orders_page,dashboard}.rs`. Verify with the Compose database stopped.
- [x] 5.5 Convert the station and market tests: `tests/{market,prices,station_book,station_history,station_stats,station_hooks}.rs`.
      Verify with the Compose database stopped.
- [x] 5.6 Convert the opportunity tests: `tests/{analysis,opportunities_page,opportunities_tables,opportunities_detail,opportunities_settings,opportunity_charts}.rs`.
      Verify with the Compose database stopped.
- [x] 5.7 Convert the wealth tests: `tests/{wealth,wealth_poll,wealth_page}.rs`. Verify with the
      Compose database stopped.
- [x] 5.8 Convert the fee and trade tests: `tests/{fees,fee_poll,fees_page,trade,trades_page,contract_request,esi_client,sso_validation}.rs`.
      Verify with the Compose database stopped, and check that
      `grep -rn "sqlx::test\|sqlx::query\|PgPool" crates/sekondo/tests` finds nothing.
- [x] 5.9 Document in README a "Tests" section covering: plain `cargo test` needs no database; when
      the gate fails, run `cargo run -p store-contract --bin store-verify` with the database up; the
      Postgres schema tests run with `cargo test -p store-contract --features postgres`. Verify by
      running each documented command as written.

## 6. Integration checks

- [x] 6.1 With the Compose database stopped and `DATABASE_URL` unset, `cargo test --workspace`
      passes. Record its wall time against the pre-change run in the commit message.
- [x] 6.2 Edit a comment in a migration, then confirm that a `sekondo` test fails at the gate naming
      `store-verify` while `cargo test -p store-contract --test memory` still passes. Revert the edit
      and confirm the tests pass again.
- [x] 6.3 Run `cargo clippy --workspace --all-targets` clean, and run the Playwright suite under `e2e/`
      unchanged against Compose Postgres. Verify that it passes, including the station hooks that
      refresh rollups.
