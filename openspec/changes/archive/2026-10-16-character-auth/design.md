# Design

## Context

This is a greenfield repository. It contains only a Nix flake dev shell (Rust toolchain via rust-overlay, `nodejs_24`), OpenSpec and agent skills. This change creates the crate layout, the runtime, and the test infrastructure that `market-orders` and `orders-dashboard` build on (see proposal.md). Requirements are in `specs/character-auth/spec.md`.

Relevant EVE SSO facts:
- OAuth2 authorization code flow with PKCE at `{sso}/v2/oauth/authorize` and `{sso}/v2/oauth/token`.
- The access token is an RS256 JWT, valid for about 20 minutes, verified against `{sso}/oauth/jwks`.
- Claims: `sub` = `CHARACTER:EVE:<id>`, `name`, `owner` (owner hash), `scp` (a string or an array), `iss`, `aud` (the client ID and `"EVE Online"`).
- A refresh may return a new refresh token. A rejected refresh comes back as HTTP 400 `invalid_grant`.

## Goals / Non-Goals

**Goals:**
- One `docker compose up` runs the app and the database locally.
- All behaviour, including Playwright end-to-end tests, can be tested without contacting the real EVE servers.
- The project structure stays simple enough to extend with the poller, SSE and pages in later changes.

**Non-Goals:**
- Encryption key rotation.
- Corporation or alliance scopes, and role checks.
- Any ESI data fetching. The ESI base URL is only configured here, and `market-orders` uses it.
- Public hosting concerns: public certificates (ACME), domains, and certificate hot-reload. Security controls are **not** relaxed for local use: the app speaks HTTPS only, everywhere, including `localhost`. On Linux, any local user or process can reach loopback ports, and Docker port publishing defaults to `0.0.0.0` and bypasses host firewall rules, so "local" isn't a security boundary.
- Packaging the app with Nix. The flake remains the dev shell only.

## Decisions

### Workspace layout
A Cargo workspace with two crates:
- `crates/sekondo`: a library holding all logic, plus a thin `main.rs`. The library structure lets integration tests build the router directly.
- `crates/eve-stub`: a small axum binary that imitates EVE SSO now, and the ESI endpoints later.

Playwright tests live in `e2e/` with their own `package.json`.
*Alternative:* the stub as a Rust test-only module. Rejected because Playwright needs a running HTTP server outside the Rust test harness.

### Web stack
- axum serves the app.
- Templates use **askama**, which compiles them and type-checks them at build time.
- htmx and **Pico.css** are vendored under `crates/sekondo/static/`, served with `tower-http::ServeDir`, with no CDN and no Node build step.
- State changes (unlink, sign out) are `POST`s sent through htmx. Unlink uses `hx-confirm`.

*Alternatives:* maud (Rust-macro HTML, harder to read for page markup) and minijinja (templates checked only at runtime).

### Configuration
Settings come from environment variables, with `.env` loaded in dev through `dotenvy`. They're parsed once at startup into a typed `Config`, and startup fails fast with the name of any missing or invalid variable:
- `DATABASE_URL`
- `BIND_ADDR` (default `0.0.0.0:8443`, the listener inside the container)
- `TLS_CERT_PATH`, `TLS_KEY_PATH` (PEM, required)
- `TLS_EXTRA_CA_PATH` (optional PEM): an extra root CA trusted for **outbound** HTTPS (SSO and ESI), added to the system roots. Used only to trust the local dev or test CA for `eve-stub`. It adds trust and never disables verification.
- `PUBLIC_BASE_URL`, used to build the callback URL `{PUBLIC_BASE_URL}/auth/callback`
- `EVE_CLIENT_ID`, `EVE_CLIENT_SECRET`
- `EVE_SSO_BASE_URL` (default `https://login.eveonline.com`)
- `ESI_BASE_URL` (default `https://esi.evetech.net`)
- `TOKEN_ENCRYPTION_KEY` (base64, 32 bytes)

There's deliberately no switch to turn off security controls (cookie flags, CSRF checks, security headers, TLS, certificate verification). `PUBLIC_BASE_URL`, `EVE_SSO_BASE_URL` and `ESI_BASE_URL` must all be `https://`, with no localhost exception. Otherwise startup fails.

### TLS termination
The app terminates TLS itself with **rustls**, through `axum-server`'s `tls-rustls` feature. It has no plain-HTTP listener and no HTTP-to-HTTPS redirect. A plain-HTTP client hitting the port just gets a failed TLS handshake.

- TLS 1.2 and 1.3 only, with rustls' default (safe) cipher suites.
- `Strict-Transport-Security: max-age=31536000` is added to the security headers. `includeSubDomains` is deliberately left out so other dev services on `localhost` aren't pinned to HTTPS.
- **Dev certificates:** the flake adds `mkcert`. A documented one-time `mkcert -install` and `mkcert -cert-file certs/dev.pem -key-file certs/dev-key.pem localhost 127.0.0.1 ::1` produces a certificate the browser trusts. `certs/` is gitignored.
- **Outbound:** `reqwest` with rustls verifies every certificate. `danger_accept_invalid_certs` is never used anywhere, including in tests.

### Database TLS
Postgres accepts **only** TLS connections, and the app verifies the server fully:

- **Server:** the `db` service starts with `-c ssl=on -c ssl_cert_file=... -c ssl_key_file=... -c ssl_min_protocol_version=TLSv1.2 -c hba_file=/etc/sekondo/pg_hba.conf`. The custom `pg_hba.conf` has `local all all peer` for the in-container healthcheck, `hostssl all all all scram-sha-256`, and **no** `host` lines, so plain TCP is refused.
- **Certificate:** mkcert issues `certs/db.pem` and `certs/db-key.pem` for `db localhost 127.0.0.1`. Postgres refuses a key that isn't owned by the postgres user, or by root with mode `0640` or tighter. A small entrypoint wrapper therefore copies the read-only mounted key into the container with `chown postgres` and `chmod 600` before running the image's `docker-entrypoint.sh`.
- **Client:** sqlx uses its rustls TLS feature. `Config` parses `DATABASE_URL` and refuses to start unless `sslmode=verify-full` and `sslrootcert` are set. In Compose, the app's `DATABASE_URL` points at `db:5432` with the mounted mkcert root CA. On the host, dev tools use `localhost:5432` with `$(mkcert -CAROOT)/rootCA.pem`. Both sides run `verify-full`.
- **Tests:** `#[sqlx::test]` and `cargo sqlx prepare` read the same verify-full `DATABASE_URL`. The Playwright global setup also connects with verify-full to recreate `sekondo_e2e`.

*Alternative:* plain TCP inside the Compose network. Rejected because other containers and local processes on Linux can reach it.

*Alternative (TLS termination):* a Caddy reverse proxy with `tls internal` in Compose. Rejected because the hop from proxy to app would be plain HTTP, and dev runs without Compose would need a second TLS setup.

A committed `.env.example` documents them. `.env` is gitignored.

### OAuth flow and token validation
- The authorize URL, PKCE (S256) and the code exchange are a few small functions in `oauth.rs` and `sso.rs`, on the app's own `reqwest` client (rustls, with the optional extra CA). The `oauth2` crate was skipped: it ships its own `reqwest` and would need a second TLS configuration for `TLS_EXTRA_CA_PATH`.
- The pending flow is stored in the session before redirecting: `state`, the PKCE verifier, and the **intent**, which is either `Login` or `AddCharacter`. The callback consumes it, which makes `state` single-use.
- `jsonwebtoken` validates the access token against the JWKS fetched from `{EVE_SSO_BASE_URL}/oauth/jwks`. The JWKS is cached in memory and fetched again when an unknown `kid` appears. Issuer validation accepts the configured SSO host both with and without the `https://` prefix, because EVE has issued both forms.
- `scp` is parsed whether it arrives as a string or as an array.

Callback outcomes:

```
                     +-- intent=Login --------+-- char unknown         -> new account, sign in
callback -> verify --+                        +-- char known, owner ok -> resume account, sign in
                     |                        +-- owner hash changed   -> delete char, then "unknown"
                     +-- intent=AddCharacter -+-- char unknown         -> link to session account
                                              +-- char in same account -> refresh tokens
                                              +-- char in other account (same owner) -> reject w/ message
                                              +-- owner hash changed   -> delete char, then link
```

### Data model
The migrations are `sqlx` migrations in `crates/sekondo/migrations/`, applied at startup with `sqlx::migrate!`.

- `accounts(id uuid pk, created_at)`
- `characters(character_id bigint pk, account_id uuid fk -> accounts ON DELETE CASCADE, name, owner_hash, scopes text[], needs_relogin bool, created_at, updated_at)`
- `character_tokens(character_id bigint pk fk -> characters ON DELETE CASCADE, refresh_token bytea, access_token bytea, access_expires_at timestamptz, updated_at)`
- The session table is created by `tower-sessions-sqlx-store`'s own migration.

**Convention for later changes:** every per-character table references `characters(character_id) ON DELETE CASCADE`. This is how "unlink deletes all data" keeps working without changing the unlink code. Unlinking the last character deletes the account in the same transaction.

### Token encryption
`aes-gcm` (AES-256-GCM) is used with a fresh random 96-bit nonce for every encryption, stored as `nonce || ciphertext` in `bytea`. The character ID is passed as associated data, so a ciphertext copied onto another character's row fails to decrypt. Tokens never pass through `Debug` or `Display`; they're wrapped in a `secrecy::SecretString`.
*Alternative:* `pgcrypto`. Rejected because the key would then be sent to the database in queries.

### Token service
`TokenService::access_token(character_id)` works as follows:
1. If the character is marked `needs_relogin`, it returns `Err(NeedsRelogin)` without contacting SSO.
2. If the cached access token is valid for at least another 60 seconds, it's returned.
3. Otherwise the token is refreshed. A new refresh token is saved when SSO returns one.
4. HTTP 400/401 responses (including `invalid_grant`) mark the character `needs_relogin`. Network errors and 5xx responses return `Err(Transient)` and leave the character unmarked.

A per-character async mutex stops two simultaneous refreshes from racing, which matters once the poller exists. The scope check runs on every token validation, so a refresh that returns reduced scopes marks the character as well.

### Sessions
`tower-sessions` with a small Postgres store of our own (`src/session.rs`, table `sessions`). The ready-made `tower-sessions-sqlx-store` 0.15 depends on sqlx 0.8 and would pull in a second sqlx version next to our 0.9. The same settings apply in every environment:

- **Cookie:** named `__Host-sekondo`, which makes browsers enforce `Secure`, `Path=/` and no `Domain`. It's `Secure`, `HttpOnly` and `SameSite=Lax`. Lax is needed so the cookie is sent on the SSO callback, which is a top-level cross-site GET. Strict would drop it there and break login. Because the app is HTTPS-only, `Secure` and `__Host-` work the same in dev, in Playwright and in Compose.
- **Lifetime:** 7 days idle timeout and a 30-day absolute limit, stored in the session and checked on every request. After that the user signs in again with SSO. The SSO session usually makes this a single click.
- **Fixation:** the session ID is rotated when someone signs in, and the account in the session changes only at that point.
- **Logout:** deletes the session record on the server, not just the cookie.
- An extractor called `CurrentAccount` returns either the account or a redirect to `/`.

*Alternative:* a 30-day idle timeout only. Rejected because a stolen cookie would stay valid indefinitely as long as it kept being used.

### CSRF protection
Two independent layers run on every state-changing request (any method other than GET, HEAD or OPTIONS). SameSite=Lax alone isn't relied on, because it doesn't protect against same-site attackers (other services on `localhost` ports, or sibling subdomains) and its behaviour varies by browser.

1. **Synchronizer token.** Each session gets a random 256-bit CSRF token. The base layout renders it as `<meta name="csrf-token">`, and `<body hx-headers='{"X-CSRF-Token": "..."}'>` makes htmx send it with every request. A middleware compares it in constant time and returns 403 when it's missing or doesn't match. Plain `<form>` posts, if ever needed, send it as a hidden field instead.
2. **Origin check.** The same middleware rejects state-changing requests whose `Origin` (or `Referer` when there's no `Origin`) doesn't match `PUBLIC_BASE_URL`, and also rejects requests with `Sec-Fetch-Site: cross-site`.

The OAuth callback is a GET and is protected by the single-use `state` and PKCE. `GET /auth/login` and `GET /auth/add` only start a flow; they change no account data.

*Alternative:* double-submit cookie. Rejected because a server-side session already exists, so the synchronizer pattern is simpler and doesn't depend on cookie integrity.

### Security headers
A `tower-http` `SetResponseHeader` layer adds these headers to every response:

- `Content-Security-Policy: default-src 'self'; img-src 'self' https://images.evetech.net; script-src 'self'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self' <EVE_SSO_BASE_URL>`
- `X-Content-Type-Options: nosniff`
- `Referrer-Policy: same-origin`
- `X-Frame-Options: DENY`, for older browsers

So that htmx works with `style-src 'self'`, `htmx.config.includeIndicatorStyles = false`, set in a vendored `static/app.js` rather than inline, and the indicator styles live in the static CSS. Templates contain no inline scripts or event-handler attributes.

### Routes
| Route | Purpose |
|---|---|
| `GET /` | Landing page with a login button. Redirects to `/characters` when signed in. |
| `GET /auth/login` | Starts the SSO flow with intent `Login` |
| `GET /auth/add` | Starts the SSO flow with intent `AddCharacter` (requires sign-in) |
| `GET /auth/callback` | Handles the SSO redirect |
| `POST /auth/logout` | Ends the session |
| `GET /characters` | Characters page |
| `POST /characters/{id}/unlink` | Unlinks a character |
| `GET /healthz` | Liveness check (over HTTPS, like everything else) |

Portraits load from `https://images.evetech.net/characters/{id}/portrait?size=64`.

### EVE stub
`eve-stub` generates an RSA key at startup and serves:
- `/oauth/jwks`
- `/v2/oauth/authorize`: an HTML character picker built from a fixture of fake characters, so Playwright clicks a character the way a player would on EVE's page. It then redirects to the callback with a code.
- `/v2/oauth/token`: handles both the code and refresh grants and issues signed JWTs.

Control endpoints under `/_stub/` let tests change behaviour: revoke a character's refresh token, change its owner hash, reduce its granted scopes, or make the next token call return 503. Rust integration tests start it in-process on a random port. Playwright starts it as a process.

The stub also speaks **HTTPS only**, so the app's "https only" startup check applies in tests too. Rust tests generate a throwaway CA and server certificate per test run with `rcgen`, give the stub and the app those certificates, and pass the CA to the app through `TLS_EXTRA_CA_PATH` and to the test HTTP client. Playwright uses the mkcert certificate, and Chromium is launched with `--ignore-certificate-errors-spki-list=<SPKI hash of that exact cert>` when the mkcert CA isn't in the browser profile. That pins one known certificate; `ignoreHTTPSErrors` stays off.

### Testing
- Rust unit tests cover crypto, claim parsing and callback decision logic.
- `#[sqlx::test]` integration tests run against Postgres, with the router and stub in-process.
- Playwright covers every UI task.
- In development, Postgres comes from `docker compose up db`.
- On NixOS, Playwright's downloaded browsers don't run. The flake adds `playwright-driver.browsers` and sets `PLAYWRIGHT_BROWSERS_PATH` and `PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS`. `@playwright/test` in `e2e/package.json` is pinned to the same version as nixpkgs' `playwright-driver`. The flake also gains `sqlx-cli`.

### Containers
- `Dockerfile`: multi-stage build, from `rust:<pinned>` (cargo build --release with `SQLX_OFFLINE=true` and the committed `.sqlx/`) to `debian:bookworm-slim` with CA certificates. Only the `sekondo` binary goes in the image.
- `docker-compose.yml` has two services:
  - `app`, built from the Dockerfile, using `.env` and depending on `db` being healthy. It publishes `127.0.0.1:8443:8443`, with an explicit loopback bind and never the default `0.0.0.0`, and mounts `./certs` read-only. The healthcheck is a `sekondo healthcheck` subcommand that calls `/healthz` over HTTPS with verification against the mounted certificate, so no curl is needed in the slim image.
  - `db`, using a **pinned** `timescale/timescaledb:<2.x.y>-pg17` tag and a `pg_isready` healthcheck. The port is published only as `127.0.0.1:5432:5432`, for host-side dev tools (`cargo sqlx prepare`, `#[sqlx::test]`).

**Persistent database.** The `db` service mounts the named volume `sekondo-db-data` at `/var/lib/postgresql/data`, the image's `PGDATA`. All database state survives `docker compose down`/`up`, `docker compose restart`, `--build`, and container recreation, so rerunning compose picks up from the last state. Only `docker compose down -v`, or explicitly removing the volume, wipes it, and the README calls that out.

- The image tag is pinned, never `latest`, because a stored data directory only starts with the PostgreSQL major version that created it. An unpinned tag could silently move from PG 17 to PG 18 and leave the volume unbootable. Changing the major version is a deliberate step using `pg_upgrade` or dump and restore.
- Migrations are re-run on every app start. `sqlx` tracks applied migrations in `_sqlx_migrations`, so this is a no-op against an existing database.
- Init settings (`POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB`) only take effect the first time an empty volume is initialized. Changing them in `.env` later doesn't change the existing database, and the README notes this.

*Alternative:* a bind mount such as `./data/postgres`. Rejected because the container's postgres user then owns files inside the repository, which causes permission friction under rootless Docker and Podman and risks committing database files. A named volume is just as persistent.

**Tests never touch the persistent database.** `#[sqlx::test]` creates and drops a throwaway database for each test. The Playwright global setup recreates a dedicated `sekondo_e2e` database on the same server for each run. Neither uses the app's database.

## Risks / Trade-offs

- [EVE SSO details differ from the stub, such as `iss` format, `scp` shape or refresh-token rotation] → The stub mirrors the documented formats, the parsing tolerates both forms, and a manual smoke test against real SSO is the last task.
- [The Playwright version drifts from nixpkgs `playwright-driver`] → Pin both, and document the version coupling in `e2e/README`.
- [Losing `TOKEN_ENCRYPTION_KEY` makes every stored token unreadable] → Treat an undecryptable token like a rejected refresh: mark the character needs re-login. Re-login fixes it. Nothing else is lost.
- [`docker compose down -v` or `docker volume prune` deletes the persistent database] → The README warns about it next to the compose commands and documents `pg_dump` backups.
- [`sqlx` offline data (`.sqlx/`) goes stale, so the Docker build fails] → Add a `cargo sqlx prepare --check` task to the documented workflow.
- [HTTPS-only makes first-time setup harder (mkcert install, certificates in `certs/`)] → Accepted. The README has a copy-paste setup section, and startup errors name the missing certificate setting.
- [The certificate SPKI pin in Playwright goes stale when the dev certificate is regenerated] → The Playwright config computes the hash from `certs/dev.pem` at startup instead of hard-coding it.
- [Postgres rejects a key file with the wrong owner or mode, so `db` won't start] → The entrypoint wrapper fixes ownership and mode at every start. Task 1.3 verifies a fresh `up` with a freshly generated certificate.
- [Developers have to keep the verify-full `DATABASE_URL` (with an `sslrootcert` path) in `.env`] → `.env.example` contains the exact host-side URL, and a startup error explains what's missing.
- [The strict CSP breaks a future feature that needs inline scripts or styles] → Add static files or nonces instead of loosening the policy. A Playwright check fails on CSP violations reported in the console.
- [A 7-day idle timeout means signing in again more often] → One click through SSO. Accepted in favour of shorter-lived sessions.

## Migration Plan

This is a greenfield deploy with no migration. Rollback means stopping the containers. Data lives in the `sekondo-db-data` volume and survives rollback. Backups are done with `docker compose exec db pg_dump`, which the README documents.
