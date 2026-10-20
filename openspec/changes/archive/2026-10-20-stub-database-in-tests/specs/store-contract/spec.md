# Spec Delta

## Purpose

Lets the app's tests run against an in-memory stand-in for the database, and keeps that stand-in equal
to the real PostgreSQL and TimescaleDB store by checking both against the same contract.

## ADDED Requirements

### Requirement: Every database access goes through the store contract

The app SHALL reach the database only through its stores. No handler, poller, page or test hook SHALL
hold a database connection or run SQL itself. The Postgres store SHALL be the only code that talks to
PostgreSQL, and it SHALL keep the app's behaviour, schema and queries as they were before this change.

#### Scenario: App starts against Postgres

- **WHEN** the app starts with a valid configuration
- **THEN** it connects to PostgreSQL over verified TLS, applies pending migrations and serves every
  page and poll as before

#### Scenario: Nothing outside the Postgres store runs SQL

- **WHEN** the app's source is searched for SQL execution or for database connection handles
- **THEN** the only hits are in the Postgres store and in the app's startup code that connects and
  migrates

### Requirement: Plain test runs need no database

Running the workspace's tests with the default features SHALL NOT require a database, a database URL
or a running container. Tests that need PostgreSQL SHALL run only when that is explicitly requested,
through a separate feature or command. The Playwright suite SHALL keep running the real app against
real PostgreSQL.

#### Scenario: Tests without a database

- **WHEN** `cargo test` runs with no database reachable and no database URL set
- **THEN** every test runs and none fails for lack of a database

#### Scenario: Postgres tests on request

- **WHEN** the developer runs the explicit Postgres test command with the database up
- **THEN** the migration and schema tests run against a freshly migrated PostgreSQL

### Requirement: The in-memory store conforms to the contract

An in-memory store SHALL implement every store the app uses. A shared set of contract cases SHALL
describe what each store must return for given writes and reads, including derived results such as
the latest snapshot per item, sums over time windows and the hourly and daily rollups. The in-memory
store SHALL pass every contract case. That check SHALL be part of the plain test run.

#### Scenario: In-memory store checked on every test run

- **WHEN** `cargo test` runs
- **THEN** every contract case runs against the in-memory store and must pass

#### Scenario: A deviating in-memory store

- **WHEN** the in-memory store returns something a contract case does not expect
- **THEN** that contract case fails and names the store call and the difference

### Requirement: Rollups appear only after a refresh

In both stores, the hourly and daily rollups of the station snapshots SHALL show only buckets that an
explicit refresh has materialized. Snapshots written after the last refresh SHALL NOT appear in the
rollups until the next refresh covers them.

#### Scenario: Snapshots before a refresh

- **WHEN** station snapshots are written and no refresh has run
- **THEN** both stores return no rollup buckets for them

#### Scenario: Snapshots after a refresh

- **WHEN** a refresh covers the snapshots' time range
- **THEN** both stores return the same hourly and daily buckets for them

### Requirement: Postgres is verified against the contract

A verification command SHALL run every contract case against a real, freshly migrated PostgreSQL with
TimescaleDB, and SHALL start each case from an empty store. It SHALL connect with the same TLS
verification the app requires. When every case passes against both stores, it SHALL record the current
store fingerprint, the time and the list of cases in a committed file. When any case fails, it SHALL
name the case and the difference and SHALL leave the recorded file unchanged.

#### Scenario: Verification passes

- **WHEN** the verification command runs and every case passes against both stores
- **THEN** the recorded file holds the current fingerprint and the list of verified cases

#### Scenario: Verification fails

- **WHEN** a case fails against PostgreSQL
- **THEN** the command exits with failure, names the case and the difference, and the recorded file
  is unchanged

### Requirement: The verified store is recorded by fingerprint

The store fingerprint SHALL be a hash over the store definitions, the in-memory store, the Postgres
store, the contract cases and every migration, taken from the working tree so uncommitted edits count.
Changing, adding, moving or removing any of these files SHALL change the fingerprint. Files outside
them SHALL NOT affect it.

#### Scenario: A migration changes

- **WHEN** a migration is added or edited
- **THEN** the fingerprint changes

#### Scenario: App code changes

- **WHEN** only a handler, a page template or a test outside the stores changes
- **THEN** the fingerprint stays the same

### Requirement: In-memory-backed tests require a verified store

No test that uses the in-memory store SHALL run while the recorded fingerprint differs from the
current one. The failure SHALL say that the store changed since its last verification against
PostgreSQL, name the verification command, and happen before the test starts any server or poller.
There SHALL be no setting or flag that lets these tests run with an unverified store. The contract
cases' own run against the in-memory store SHALL NOT be gated, so the fake can be developed before it
is verified.

#### Scenario: Store unchanged

- **WHEN** the tests run and the recorded fingerprint matches the current one
- **THEN** they run without a database

#### Scenario: Store changed

- **WHEN** a store file, a contract case or a migration was edited and the tests run before a new
  verification
- **THEN** every in-memory-backed test fails with a message naming the verification command, while
  the contract cases still run against the in-memory store

### Requirement: Production never uses the in-memory store

The in-memory store SHALL be compiled only into test builds. A release build of the app SHALL NOT
contain it, and no configuration setting SHALL select it.

#### Scenario: Release build

- **WHEN** the app is built for release with its default features
- **THEN** the in-memory store is not part of the binary and the app requires PostgreSQL to start
