# Tasks

## 1. Workspace and dev environment

- [x] 1.1 Create the Cargo workspace with `crates/sekondo` (lib + bin) and `crates/eve-stub` (bin), and verify `cargo build --workspace` succeeds
- [x] 1.2 Add `sqlx-cli`, `mkcert`, `playwright-driver.browsers` and the `PLAYWRIGHT_BROWSERS_PATH` / `PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS` variables to `flake.nix`, add `certs/` to `.gitignore`, and add a `scripts/dev-certs.sh` that runs `mkcert` for the app (`localhost 127.0.0.1 ::1`) and the database (`db localhost 127.0.0.1`). Verify `sqlx --version` and `mkcert -version` work in `nix develop`, and that the script produces all four PEM files
- [x] 1.3 Add `docker-compose.yml` with the `db` service: a pinned `timescale/timescaledb:<2.x.y>-pg17` tag, named volume `sekondo-db-data` at `/var/lib/postgresql/data`, port `127.0.0.1:5432:5432` only, TLS settings (`ssl=on`, cert and key, `ssl_min_protocol_version=TLSv1.2`), a custom `pg_hba.conf` with `hostssl` only, the entrypoint wrapper that fixes the key's owner and mode, and a `pg_isready` healthcheck. Verify that:
  - `docker compose up db` reports healthy with freshly generated certificates
  - `psql "sslmode=verify-full sslrootcert=$(mkcert -CAROOT)/rootCA.pem host=localhost ..."` connects
  - `psql "sslmode=disable ..."` is rejected
  - a table created via `psql` still exists after `docker compose down` followed by `docker compose up db`
- [x] 1.4 Add `.env.example` with every setting from the design, including the host-side verify-full `DATABASE_URL` and the TLS paths, add `.env` to `.gitignore`, and verify the example matches the `Config` fields (task 2.1)

## 2. Application foundation

- [x] 2.1 Implement typed `Config` loading from env/`.env` with defaults and fail-fast errors that name the bad variable. Require `https://` for `PUBLIC_BASE_URL`, `EVE_SSO_BASE_URL` and `ESI_BASE_URL` with no localhost exception, require readable `TLS_CERT_PATH` and `TLS_KEY_PATH`, and require `sslmode=verify-full` plus `sslrootcert` in `DATABASE_URL`. Verify unit tests cover:
  - a missing setting and an invalid encryption key length
  - defaulted URLs
  - `http://localhost` and `http://` SSO and ESI URLs rejected
  - a missing certificate rejected
  - `sslmode=require` and a missing `sslrootcert` rejected
  - a valid all-HTTPS, verify-full configuration accepted
- [x] 2.2 Wire up the HTTPS-only axum server (rustls through `axum-server`, TLS 1.2 or later, no plain-HTTP listener) with an outbound `reqwest` client that adds `TLS_EXTRA_CA_PATH` to the system roots, run migrations at startup over the verify-full database connection, add `GET /healthz`, and serve vendored htmx, Pico.css and `app.js` (which sets `htmx.config.includeIndicatorStyles = false`) from `static/`. Verify with integration tests (using per-run `rcgen` certificates) that `/healthz` returns 200 over TLS, `/static/htmx.min.js` is served, and a plain-HTTP request to the port gets no response body
- [x] 2.3 Add the security-header layer (CSP as designed, HSTS with `max-age=31536000`, `nosniff`, `Referrer-Policy`, `X-Frame-Options`), and verify with an integration test that every header is present on `/healthz`, a page and a static file
- [x] 2.4 Add an askama base layout and the landing page `GET /` with a "Log in with EVE Online" button. Scaffold `e2e/` with:
  - a pinned `@playwright/test`
  - a global setup that connects with verify-full to recreate a dedicated `sekondo_e2e` database on the Compose server (never the app's persistent database) and starts `eve-stub` and `sekondo` over HTTPS with the mkcert certificates
  - Chromium pinned to the dev certificate's SPKI hash, computed from `certs/dev.pem` (no `ignoreHTTPSErrors`)
  - a shared fixture that fails any test when the console reports a CSP violation

  Verify with a Playwright test that `https://localhost:8443/` renders the button with no CSP violations
- [x] 2.5 Set up `tower-sessions` with the Postgres store and a `__Host-sekondo` cookie (`Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`), a 7-day idle timeout and a 30-day absolute expiry. Verify integration tests for the cookie attributes, a session value surviving a router rebuild, idle expiry and absolute expiry (using a controllable clock)
- [x] 2.6 Implement the CSRF middleware (per-session 256-bit token, constant-time comparison of `X-CSRF-Token` or a form field, an `Origin`/`Referer` match against `PUBLIC_BASE_URL`, rejecting `Sec-Fetch-Site: cross-site`) and render the token through `<meta>` and `<body hx-headers>` in the base layout. Verify integration tests against a test POST route for a missing token, a token from another session, a foreign `Origin` and a valid request

## 3. EVE SSO stub

- [x] 3.1 Implement `eve-stub` as an HTTPS-only server (cert and key from arguments), with RSA key generation, `GET /oauth/jwks`, and token issuing for a character fixture (`sub`, `name`, `owner`, `scp`, `iss`, `aud`, `exp`). Verify with a test that decodes an issued token against the JWKS served over TLS
- [x] 3.2 Implement `GET /v2/oauth/authorize` (HTML character picker that redirects to `redirect_uri` with `code` and `state`) and `POST /v2/oauth/token` for the `authorization_code` (with PKCE check) and `refresh_token` grants, rotating refresh tokens. Verify with stub integration tests for both grants and a wrong PKCE verifier
- [x] 3.3 Implement the `/_stub/` control endpoints (revoke refresh token, change owner hash, reduce scopes, fail the next token call with 503) and the ability to run in-process on a random port with per-run `rcgen` certificates, and verify each control with a stub integration test

## 4. Accounts, characters and credential storage

- [x] 4.1 Write migrations for `accounts`, `characters` and `character_tokens` with `ON DELETE CASCADE` as designed, and verify with an `#[sqlx::test]` that deleting an account removes its characters and tokens
- [x] 4.2 Implement AES-256-GCM token encryption (random nonce, character ID as associated data, `secrecy` wrappers), and verify unit tests for round trips, a tampered ciphertext, a wrong character ID and a wrong key
- [x] 4.3 Implement the account/character repository (create account, upsert character with tokens, find by character, list by account, delete character and delete the account when it was the last one), and verify `#[sqlx::test]` tests for each operation, including a check that stored token bytes never contain the plain token

## 5. SSO login flow

- [x] 5.1 Implement `GET /auth/login` and `GET /auth/add` to build the authorize URL with the exact two scopes, `state` and PKCE, storing the pending flow and intent in the session. Verify with an integration test of the redirect URL's parameters, and check that `/auth/add` without a session redirects to `/`
- [x] 5.2 Implement access-token validation (JWKS cache with refetch on unknown `kid`, both `iss` forms, `aud`, `exp`, and `scp` as a string or an array), and verify unit tests against stub-signed tokens, including a bad signature, wrong audience and expired token
- [x] 5.3 Implement the callback decision logic as a pure function (intent × known/unknown × same/other account × owner hash), and verify unit tests that cover every branch of the design diagram
- [x] 5.4 Implement `GET /auth/callback` (consume the single-use state, exchange the code, validate, apply the decision, rotate the session ID, redirect to `/characters`) plus error pages for state mismatch and SSO errors. Verify with in-process integration tests for each spec scenario under "Sign in with EVE SSO", "First login creates an account", "Login resumes an existing account", "Character ownership change" and "Add character to the current account", and verify that the session cookie value changes on login and the old value no longer authenticates

## 6. Token service and needs-re-login

- [x] 6.1 Implement `TokenService::access_token` (reuse a valid token, refresh within 60 seconds of expiry, store the rotated refresh token, per-character mutex), and verify integration tests against the stub for the valid-token and expired-token scenarios
- [x] 6.2 Implement needs-re-login handling: 400/401 on refresh, missing scopes and undecryptable tokens mark the character, while network errors and 5xx return a temporary error. Verify integration tests that use the stub controls for revoke, reduced scopes and 503, and check that a marked character never contacts SSO

## 7. Characters page

- [x] 7.1 Add the `CurrentAccount` extractor and the `GET /characters` page with portraits, names, needs-re-login badges with a "Log in again" link, an "Add character" button and a sign-out button. Verify with a Playwright test that logs in through the stub picker, sees the character, then adds a second character and sees both
- [x] 7.2 Implement `POST /characters/{id}/unlink` (htmx with `hx-confirm`, account-scoped, removing the last character deletes the account and signs out). Verify with a Playwright test that unlinks one of two characters and then the last one, plus integration tests that unlinking another account's character is rejected and that an unlink without a CSRF token returns 403 and deletes nothing
- [x] 7.3 Implement `POST /auth/logout` (deletes the server-side session). Verify with a Playwright test that after signing out, `/characters` redirects to the landing page and logging in again with either linked character returns to the same account, plus an integration test that replaying the pre-logout cookie does not authenticate
- [x] 7.4 Show the needs-re-login badge after a revoked refresh and clear it on re-login. Verify with a Playwright test that uses the stub revoke control, triggers a refresh through a test-only hook, sees the badge, logs in again and sees the badge gone

## 8. Container image and docs

- [x] 8.1 Add the multi-stage `Dockerfile` (`SQLX_OFFLINE=true` with the committed `.sqlx/`), a `sekondo healthcheck` subcommand (calls `/healthz` over HTTPS with certificate verification), and the `app` service in `docker-compose.yml`:
  - depends on a healthy `db`
  - publishes only `127.0.0.1:8443:8443`
  - mounts `./certs` read-only
  - uses a `DATABASE_URL` pointing at `db:5432` with `sslmode=verify-full` and the mounted CA
  - uses the healthcheck subcommand

  Verify that:
  - `docker compose up --build` reports `app` healthy
  - `https://localhost:8443/healthz` responds
  - the port isn't reachable on non-loopback interfaces
  - `cargo sqlx prepare --check` passes
  - data persists: log in through the stack, run `docker compose down` and then `docker compose up --build`, and the account and character still exist and migrations run without error
- [x] 8.2 Update `README.md`, then verify the documented commands work from a clean checkout. Cover:
  - EVE application registration (callback `https://localhost:8443/auth/callback`, the two scopes)
  - `mkcert -install` and `scripts/dev-certs.sh`
  - `.env` setup with the verify-full `DATABASE_URL`, and generating `TOKEN_ENCRYPTION_KEY`
  - running with Compose: data persists in `sekondo-db-data`, `down -v` wipes it, `POSTGRES_*` settings apply only when the volume is first created, `pg_dump` backups over TLS
  - running Rust and Playwright tests

## 9. Integration check

- [x] 9.1 Run a manual smoke test against real EVE SSO with a registered application: log in, add a second character, sign out and log back in, unlink. Verify that real token claims (`iss`, `scp`, refresh rotation) are accepted, and record any differences from the stub as stub fixes
