# Proposal

## Why

Sekondo ("second" / corner man) is a personal helper for EVE Online. Every feature depends on reading private character data from ESI, so the first step is a way to sign in with EVE SSO and link several characters to one account, plus the runnable and testable application foundation everything else builds on.

## What Changes

- Application foundation: axum + htmx skeleton, configuration via environment / `.env`, build-free static CSS.
- Docker Compose setup with the app (multi-stage Rust Dockerfile) and a local TimescaleDB container; `sqlx` migrations run on startup. The Nix flake stays the dev environment only.
- Configurable ESI and SSO base URLs, a stub ESI/SSO server for tests, and a Playwright test harness.
- EVE SSO login: the first login creates an internal account and session; logging in with any linked character resumes that account.
- "Add character" while logged in runs SSO again and links the character to the current account.
- Requested scopes are exactly `esi-markets.read_character_orders.v1` and `esi-universe.read_structures.v1`.
- Refresh tokens are stored encrypted at rest (AES-GCM, key from env).
- If a token refresh fails, the character is flagged "needs re-login", keeps its last known data, and is skipped by background work until the user logs in with it again.
- "Unlink character" removes the character and all of its data (tokens, orders, history).
- Cookie-based sessions stored in Postgres, so they survive restarts.

## Capabilities

### New Capabilities
- `character-auth`: EVE SSO login, account creation, linking and unlinking characters, encrypted token storage and refresh, the needs-re-login state, and sessions.

### Modified Capabilities

## Impact

- New Rust crate: axum, sqlx (Postgres), tower-sessions, an OAuth2 client, AES-GCM.
- New infrastructure: `docker-compose.yml`, `Dockerfile`, TimescaleDB container, migrations directory.
- External dependency: EVE SSO (a registered third-party application with a client ID/secret and an `https://localhost:8443/auth/callback` callback URL).
- The app is HTTPS-only (rustls), including on localhost. Local certificates come from `mkcert`.
- Test infrastructure (the ESI/SSO stub and the Playwright harness) is reused by `market-orders` and `orders-dashboard`.
