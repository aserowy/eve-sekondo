# Design

## Context

See proposal.md for motivation. Relevant current state from `character-auth`:

- `TokenService::access_token(character_id)` (`crates/sekondo/src/tokens.rs`) returns a valid access token. It refreshes under a per-character lock, marks "needs re-login" on rejection, and returns `TokenError::Temporary` for network or SSO trouble. The poller only consumes it.
- `AppState` holds `config`, `pool`, a rustls-only `reqwest::Client` that trusts `TLS_EXTRA_CA_PATH`, `clock: Arc<dyn Clock>`, `repo`, `sso` and `tokens`. `App::new_with` takes an injected clock and extra routes for tests.
- Migration convention: every per-character table references `characters(character_id) ON DELETE CASCADE`. Unlinking a character and an ownership change both go through `delete_in_tx`, so cascading tables are cleaned up for free.
- The database is `timescale/timescaledb:2.17.2-pg17`, but no migration has run `CREATE EXTENSION timescaledb` yet. The app connects as `POSTGRES_USER`, which the image makes a superuser, so a migration can create the extension. The Rust tests (one database per test) and the e2e database (recreated per run) both run migrations.
- The CSP is `default-src 'self'`, so a same-origin `EventSource` is allowed without changes. Inline scripts and `eval` are forbidden, which rules out `hx-on` and inline handlers. htmx 2.0.4 is vendored.
- `eve-stub` already serves SSO over HTTPS with `/_stub/*` control endpoints and was built to grow ESI routes. The e2e build enables the `test-hooks` feature (`/_test/refresh/{id}`).
- After login, the callback and `/auth/login` currently redirect to `/characters`, and `/` sends signed-in users there too.

## Goals / Non-Goals

**Goals:**
- One poller per process that is correct under restarts. The schedule lives in the database, not in memory.
- Views read only from small "current" tables. History is only written, never read, in this change.
- Every external behaviour (ESI failures, rate limits, inaccessible structures) can be driven from the stub, so Rust tests and Playwright cover it without touching real ESI.

**Non-Goals:**
- Running several app instances at once. Nothing prevents it later (see the poller and event bus decisions), but it is not designed or tested here.
- Snapshot compression, retention or history queries.
- Wallet, transaction or fill data. Corporation endpoints.

## Decisions

### 1. Data model (one migration)

```
CREATE EXTENSION IF NOT EXISTS timescaledb;

current_orders   (order_id bigint PK, character_id → characters CASCADE,
                  type_id, region_id, location_id, is_buy_order, is_corporation,
                  price numeric(20,2), volume_remain, volume_total, min_volume, range,
                  escrow numeric(20,2) NULL, issued timestamptz, duration int,
                  updated_at)                      -- index (character_id)
order_snapshots  (polled_at timestamptz, character_id → characters CASCADE, order_id,
                  same order fields)               -- hypertable on polled_at, 7-day chunks,
                                                   -- index (character_id, polled_at DESC)
order_sync       (character_id PK → characters CASCADE, next_poll_at, last_success_at NULL,
                  last_attempt_at NULL, last_error text NULL, esi_expires_at NULL)
names            (id bigint PK, category text, name text NULL, resolved_at,
                  retry_after timestamptz NULL)    -- NULL name = unresolvable structure
```

- ISK is stored as `numeric(20,2)` and handled as integer cents in Rust (`i64`), never `f64`. ESI sends prices as JSON numbers. They are parsed with `serde_json`'s arbitrary-precision feature, or as `f64` and rounded to cents straight away, since EVE prices have at most 2 decimals. Either way, sums for value and escrow happen in SQL on `numeric`.
- `current_orders` is replaced per character inside one transaction: delete the character's orders, then insert the returned ones. That makes "exactly what ESI returned" trivially true. Order IDs are globally unique in EVE, so `order_id` alone is the PK. An order moving between characters (it can't) would surface as a PK violation, which is logged.
- The hypertable has no PK. TimescaleDB would require the time column in it, and nothing reads by key. A hypertable foreign key to a plain table with `ON DELETE CASCADE` is supported. Because chunks stay uncompressed, cascading deletes are ordinary row deletes.
- The `order_sync` row is created on first poll scheduling, not by `character-auth` code. A character without a row is "never polled" and is scheduled immediately, with jitter.
- "Polled with zero orders" is visible through `order_sync.last_success_at`. No sentinel snapshot row is needed.
- `names` is global, not per account. Item, region and station names are public. A structure name only reaches an account whose character has an order in that structure, so that player could already see the name in game. *Alternative:* per-account structure names. Rejected because it duplicates lookups for no confidentiality gain.

### 2. Poller: a database-driven scheduler loop

A single tokio task, started from `main::serve` (not from `App::new`, so tests opt in), wakes every few seconds (configurable for tests) and claims due characters:

```sql
SELECT c.character_id FROM characters c LEFT JOIN order_sync s USING (character_id)
WHERE NOT c.needs_relogin AND (s.next_poll_at IS NULL OR s.next_poll_at <= now())
ORDER BY s.next_poll_at NULLS FIRST LIMIT $batch
```

Each claimed character is polled with bounded concurrency (for example 4). After each attempt:

- **Success:** `next_poll_at = max(polled_at + interval, esi_expires_at) + jitter`.
- **Temporary failure:** `next_poll_at = now + min(interval, backoff)`, where the backoff starts at 1 minute and doubles per consecutive failure. `last_success_at` stays unchanged.
- **`NeedsRelogin`:** nothing is scheduled. The query skips the character until a login clears the flag, and the next tick picks it up.

Spreading: when a character is first scheduled, `jitter = hash(character_id) mod interval`. On later polls a small random jitter of ±30 s is added. Characters linked at the same moment therefore land at different offsets, and the ESI `Expires` values, which differ per character, keep them apart.

*Alternatives:* a timer per character in memory, which loses the schedule on restart and makes "next update" unknowable to the views; or `tokio-cron`-style ticks, which poll everyone at once. The database query also lets a later multi-instance setup use `FOR UPDATE SKIP LOCKED` without redesign.

A `clock: Arc<dyn Clock>` drives every time decision, so the existing `TestClock` can advance time in Rust tests.

### 3. ESI client and etiquette

`EsiClient` wraps the shared `reqwest::Client` and owns one `EsiGate`. The gate is a shared `Mutex` holding "blocked until" instants: one global entry for the error limit, and one per rate-limit group.

- Base URL: `config.esi_base_url`. Paths: `GET /characters/{id}/orders`, `POST /universe/names` (up to 1000 IDs per call, unauthenticated), `GET /universe/structures/{id}` (authenticated). Every request sends `X-Compatibility-Date` pinned to a constant, so ESI route changes are opt-in.
- `User-Agent: Sekondo/<CARGO_PKG_VERSION> (+<PUBLIC_BASE_URL>[; <ESI_CONTACT>])`.
- Before every request, the client waits or skips while the gate is blocked. After every response it reads:
  - `X-ESI-Error-Limit-Remain` and `X-ESI-Error-Limit-Reset`: when the remaining count is below 20 (the safety margin), it blocks globally until the reset. HTTP 420 does the same.
  - `X-Ratelimit-Group`, `X-Ratelimit-Remaining`, and 429 with `Retry-After`: it blocks that group until `Retry-After`. When `Remaining` is 0 without a 429, it blocks for 1 second.
  - `Expires`: returned to the caller for scheduling.
- A blocked gate makes a poll attempt return "deferred". The character's `next_poll_at` is moved to the unblock time and nothing counts as a failure.
- Errors map to `EsiError::{Temporary, Forbidden, NotFound, Deferred}`. Only the structure lookup treats `Forbidden`/`NotFound` as final ("unknown structure"). A 403 on orders is recorded as a failure. It is not treated as needs-re-login, because token validation already handles scope loss.

*Alternative:* a crate like `rfesi`. Rejected: it doesn't model the newer rate-limit groups, and it would add a second HTTP and TLS stack next to the existing rustls client.

### 4. Name resolution inside the poll

After storing orders, the poll collects the `type_id`, `region_id` and `location_id` values that are missing from `names`, or whose `retry_after` has passed:

- IDs below 100,000,000 (types, regions, NPC stations) go to `POST /universe/names` in one batch. If a batch fails because one ID is invalid, it is split in half and each half retried, so one bad ID never blocks the rest.
- Higher location IDs are player structures. They go to `GET /universe/structures/{id}` with the polling character's token. On 403 or 404, the `names` row gets `name = NULL, retry_after = now + 24h`. On a temporary failure, no row is written, so the next poll retries.

Resolution happens before the `orders-updated` event is published, so the first render after a poll already has the names. Views render a missing name as `#<id>`, and a NULL structure name as `Unknown structure (<id>)`.

### 5. Event bus and SSE

```rust
#[async_trait] pub trait EventBus: Send + Sync {
    fn publish(&self, account: Uuid, event: Event);
    fn subscribe(&self, account: Uuid) -> BoxStream<'static, Event>;
}
```

`InProcessBus` uses one `tokio::sync::broadcast` channel of `(Uuid, Event)`, and subscribers filter by account. A lagged receiver yields `Event::OrdersUpdated` instead of an error, because refetching is idempotent. `Event` is a plain enum (`OrdersUpdated`) that maps to the SSE event name. It carries no payload. The poller publishes to the character's account after a successful poll that changed anything, or after the first success. It also publishes after a status change (failure or needs-re-login), so freshness badges update too.

`GET /events` (axum `Sse`) answers 401 without a signed-in session, instead of the usual redirect: an `EventSource` would follow a redirect to an HTML page. It merges the account stream with a 15 s keep-alive. On every keep-alive tick it re-checks that the session still exists and is within its lifetime: it loads the session ID from the store and applies the same idle and absolute checks as `session::enforce_lifetime`. When the check fails it ends the stream, which meets the 60 s close requirement. The htmx SSE extension keeps retrying with exponential backoff, and every retry gets a 401. That is harmless, and the next full page load redirects to the landing page anyway.

*Alternative:* Postgres `LISTEN/NOTIFY` right away. Deferred because one process doesn't need it, and the trait keeps the swap local.

### 6. htmx wiring, filter and sorting

- Vendor `htmx-ext-sse` (2.2.x, the version matching htmx 2) as `static/htmx-ext-sse.min.js`. The orders page body contains `<div hx-ext="sse" sse-connect="/events">`. The table container uses `hx-get="/orders/tables?{query}" hx-trigger="sse:orders-updated, htmx:sseOpen from:body"`, so it refetches on signals and after every (re)connect. That covers missed signals. The freshness strip works the same way.
- All view state lives in the query string: `chars`, `sort`, `dir` (with a separate `bsort` and `bdir` for the buy table). Fragments are rendered by the same handler as the page. The handler parses one `ViewQuery` and passes it on, so the tables are always fetched with the page's own query. Sort headers are plain links with `hx-get`, `hx-target` and `hx-push-url`, so the URL updates. Sorting runs in SQL through a whitelist of column names, never through interpolated input.
- `CharacterSelection::resolve(account_characters, raw_chars)` is the only place that parses `chars`. It drops anything that isn't an owned character ID and falls back to all characters. Every query uses the resolved ID list (`character_id = ANY($1)`) and also joins `characters.account_id = $account`.
- The filter control is a form of checkboxes with `hx-get` on `change`, `hx-push-url`, and `hx-include` of the sort fields. When every box is checked it submits no `chars`, so a plain URL means "all characters". Nav links are rendered with the current `chars` attached.
- Formatting runs in Rust and is unit-tested: shortened ISK (4 significant digits with a k/m/b/t suffix, full value in `title`), time left (whole days and hours, rounded down), and dates in UTC as "EVE time".

### 7. Navigation and landing

A `nav.html` partial in `base.html` is rendered when the template's `nav: Option<Nav>` is set. `Nav` holds the current page and the `chars` query. A new `/dashboard` handler renders the stub. `/`, `/auth/login` (when already signed in) and the login callback redirect to `/dashboard`. The callback after "add character" still redirects to `/characters`, so the existing flash message keeps working. The Playwright `loginWith` helper expects `/dashboard`.

### 8. Configuration

New settings, all documented in `.env.example` (`env_example_tests` enforces this):

- `ORDER_POLL_INTERVAL_MINUTES`: default 20, valid range 1–1440. Any other value refuses to start, naming the setting.
- `ESI_CONTACT`: optional, appended to the `User-Agent` (for example an email address or EVE character name).

The scheduler tick and the concurrency limit are constants, overridable only from tests.

### 9. Stub (`eve-stub`) additions

ESI routes on the same HTTPS server (`ESI_BASE_URL` points to the stub in tests and e2e):

- `GET /characters/{id}/orders`: requires a bearer token issued by the stub for that character with the orders scope (otherwise 403). Returns that character's fixture orders with an `Expires` header (default now + 1200 s, controllable) and error-limit and rate-limit headers.
- `POST /universe/names`: answers from a fixture name table. Any unknown ID makes the whole request return 404, like ESI does.
- `GET /universe/structures/{id}`: answers from fixture structures. IDs on a deny list return 403.

New controls: `/_stub/orders` (replace a character's orders), `/_stub/esi-fail-next` (next N ESI calls return 5xx, 420 or 429 with given headers), `/_stub/esi-expires` (seconds to report), `/_stub/structure-access` (deny or allow). There are also call counters per ESI route for "requested at most once" assertions. `reset` clears all of it.

A test hook, `POST /_test/poll/{id}` (signed-in account's own character only), runs one poll immediately, ignoring the schedule but not the ESI gate. Playwright uses it instead of waiting for the scheduler.

## Risks / Trade-offs

- **ESI header names and compatibility date drift.** The rate-limit system and `X-Compatibility-Date` are recent and documented loosely. → Header parsing is isolated in `EsiGate` with unit tests. The final integration task runs against real ESI and records any differences as stub and gate fixes, as `character-auth` did for SSO.
- **Uncompressed snapshot growth** (about 22k rows per character per day at worst, roughly 4–6 GB a year for three busy characters). → Views never read the hypertable, so query speed is unaffected. Compression or retention is a later, additive policy change that needs no schema change.
- **Long-lived SSE connections.** → They don't hold a database connection: the handler loads the session once, and per-tick checks borrow a pool connection only briefly. The browser's limit of 6 connections per host over HTTP/1.1 doesn't apply, because `tls::server_config` already advertises `h2` through ALPN, so many tabs share one connection.
- **The poller and a login racing on tokens.** → Already handled by `TokenService`'s per-character lock. A character marked needs-re-login during a poll just has its next tick skipped.
- **A character unlinked during its poll.** → Inserting into `current_orders` or `order_snapshots` then fails with an FK violation. The poll is dropped and nothing is published. The test covers this.
- **Clock skew against ESI `Expires`.** → ESI's `Date` header is used to turn `Expires` into a duration, which is added to the local clock.

## Migration Plan

- One new migration. It is additive: it creates the extension and new tables, and alters no existing ones. Rolling back means deploying the previous binary. Its migration history won't know the new migration, which sqlx tolerates only when the extra migration is reverted by hand (`DROP` of the new tables and the extension). This is acceptable before the first real deployment.
- No backfill: history starts with the first poll after deployment.
