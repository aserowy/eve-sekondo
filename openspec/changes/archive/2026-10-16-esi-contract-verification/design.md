# Design

## Context

The app reaches EVE through `oauth.rs` (authorization URL with PKCE, `state`, `REQUIRED_SCOPES`),
`sso.rs` (`POST v2/oauth/token` for code exchange and refresh, `GET oauth/jwks`, JWT validation) and
`esi.rs` (`characters/{id}/orders`, `universe/names`, `universe/structures/{id}`, all with the app's
`User-Agent` and `X-Compatibility-Date: COMPATIBILITY_DATE`). `EsiGate` reads `Expires`, `Date`,
`X-ESI-Error-Limit-Remain/Reset`, `X-Ratelimit-Group/Remaining` and `Retry-After`. Every test reaches
"EVE" through `eve-stub`, started in-process by `crates/sekondo/tests/common` and
`crates/eve-stub/tests/common`, or as a binary by `e2e/global-setup.ts`. There is no CI yet.

## Goals / Non-Goals

**Goals:**

- One contract definition used by the stub conformance tests and the real-EVE verification, so the
  two cannot drift apart.
- Tests stay fast, offline and credential-free as long as the stub is unchanged.
- No EVE secret is ever stored by the verification.

**Non-Goals:**

- Checking ESI routes the app does not call, or full OpenAPI validation of ESI.
- Provoking 420, 429 or 5xx on real ESI. Their variants list only the headers the app reads; those
  headers' names and formats are verified on the success answers that carry them.
- Automating the person at the EVE login page.
- Detecting that real ESI changed while the stub did not. Between stub changes, the app's own poller
  logs decode failures (as with `min_volume`); the fix then changes the stub and triggers a
  verification.
- Setting up CI.

## Decisions

### Contracts as Rust data in `crates/esi-contract`

A contract is a `Call` (method, path pattern, name) with `Variant`s (status, `HeaderSpec { name,
format }` with formats like `HttpDate`, `Integer`, `Token`, and a `BodySpec` tree of `Field { name,
ty, presence: Always | Optional | Absent | Nullable }`). `check(variant, status, &HeaderMap,
Option<&serde_json::Value>) -> Vec<Violation>`; a violation names call, variant and header or JSON
path, never values. Orders have sell and buy body variants keyed by `is_buy_order`, which is the case
the stub got wrong. The crate depends on neither `sekondo` nor `eve-stub`, so both can use it without
cycles.

*Alternatives:* JSON Schema files (a validator crate, and header rules would still live elsewhere);
ESI's OpenAPI document (ESI only, describes promises rather than what ESI sends); recorded real
answers as fixtures (real character data to scrub and commit).

### Verifier: `crates/esi-verify`, one command, browser login every time

Depends on `esi-contract` and `sekondo`. `cargo run -p esi-verify` does, in order:

1. Refuses to start unless the stub conformance check passes for the current fingerprint (it runs
   `cargo test -p eve-stub --test contract` and reads its exit code), so a recorded fingerprint always
   means "stub conforms and real EVE conforms".
2. Builds the authorization request with the app's `oauth.rs` flow, listens on the registered callback
   (`PUBLIC_BASE_URL` + `/auth/callback`, `127.0.0.1:8443`, dev certificate; the app container must be
   stopped, as for Playwright), opens the URL with `xdg-open` (`open` on macOS) and prints it. On
   Linux without `DISPLAY`/`WAYLAND_DISPLAY` it fails at once.
3. Checks the callback (`state`), the code exchange (authorization-code variant), the JWT via
   `SsoClient::validate` (JWKS and claims variants), then one refresh (refresh-grant variant).
4. Calls with raw `reqwest` and the app's headers (`user_agent()`, `COMPATIBILITY_DATE`): the orders
   route; `universe/names` with the IDs from the orders plus fixed known IDs; `universe/names` with one
   invalid ID (404 variant); `universe/structures/{ESI_VERIFY_STRUCTURE_ID}` (200) and
   `universe/structures/{ESI_VERIFY_FORBIDDEN_STRUCTURE_ID}` (403). Each answer goes through `check`
   and then into the app's type (`Vec<EsiOrder>`, `Vec<EsiName>`, `EsiStructure`, `TokenSet`). It
   fails unless there is at least one sell and one buy order, and stops at the first 420/429.
5. On full success writes the record.

Tokens live in `SecretString`s in memory and are dropped at exit. No credentials file, so no
encryption key handling and no refresh-token rotation to manage; the grant is the verifier's own and
never shared with the app.

Raw requests instead of `EsiClient`, because the contract needs status and headers before decoding
and `EsiClient` would wrap a decoding failure in its own error. The headers come from the same
functions, so they cannot differ from the app's.

*Alternative:* a stored, encrypted refresh token so the ESI part could run without a person. Not
needed: verification only runs when someone changed the stub and is therefore at the keyboard.

### Stub fingerprint and record

The fingerprint is a SHA-256 over, in a fixed order, every file under `crates/eve-stub/src` (path and
contents) and the canonical JSON of all contracts. It is computed from the working tree, so an
uncommitted stub edit already counts. `crates/esi-contract` exposes `fingerprint(workspace_root)`; the
verifier, the gate and the Playwright setup all use it (Playwright through a tiny
`cargo run -p esi-contract --bin stub-fingerprint`, so there is one implementation).

The record is `crates/esi-contract/verified-stub.json` `{ fingerprint, verified_at, variants: [...] }`,
committed, so the proof travels with the stub it proves. A stub change shows up in review together
with a new record, or tests fail.

Hashing whole files is conservative: a comment change in the stub asks for a verification. Accepted,
because a finer rule would be easy to get wrong in the direction that matters.

### Gate

`esi_contract::gate::require_verified_stub()` computes the fingerprint, compares it with the record
and panics with "the stub changed since it was last verified against real EVE: run `cargo run -p
esi-verify`" unless they match. It is called at the top of every harness function that starts a
stub, before the stub starts (`AppBuilder::spawn` with stub, the stub tests' common start).
`e2e/global-setup.ts` does the same comparison before building. The stub binary itself has no gate,
so `cargo run -p eve-stub` for local development keeps working, and the conformance test
(`tests/contract.rs`) is exempt, because the verifier needs it to run on an unverified stub.

The gate keeps honest runs honest; it is not tamper-proof (the record could be edited by hand).

### Coverage test

`eve-stub` exports its EVE routes (`ROUTES`, without `/_stub/` controls). A test asserts that every
route has a contract and every contract a route. The browser-facing `authorize`/`select` pages map to
the authorization request contract. Because the app's tests reach EVE only through the stub, a new
outbound call cannot be tested without a new stub route, which needs a contract and changes the
fingerprint.

## Risks / Trade-offs

- [Real ESI changes without a stub change] → Out of scope (see Non-Goals); the app's own decode error
  logging catches it, and fixing the stub triggers verification.
- [Changing the stub needs an EVE account, a browser and the right orders and structures] → Accepted;
  the verifier names the missing precondition (buy order, sell order, structure access).
- [Comment-only stub edits require a verification] → Accepted for safety, see fingerprint.
- [Two branches change the stub and both update the record] → A merge conflict in the record means
  "verify again after merging", which is correct.
- [Port 8443 is busy] → The verifier fails with the address and says to stop the app.
- [Real character data in output] → Output and record name calls, variants, JSON paths and types
  only.

## Migration Plan

1. Land the contract crate and fix stub fixtures until conformance passes.
2. Land the verifier; the developer sets the two structure IDs, runs `cargo run -p esi-verify` and
   commits the first record.
3. Turn on the gate in the harnesses and `global-setup.ts`.

Rollback: revert the gate calls; contracts and verifier can stay.

## Open Questions

- Which `X-Ratelimit-*` headers real ESI sends on the three routes, and whether `universe/structures`
  is in a rate-limit group at all. The first verification answers it; contracts and stub are adjusted
  in the same task.
