# Tasks

## 1. Contract crate

- [x] 1.1 Create `crates/esi-contract` (workspace member, no dependency on `sekondo` or `eve-stub`) with `Call`, `Variant`, `HeaderSpec`, `BodySpec`/`Field` (presence `Always | Optional | Absent | Nullable`) and `check(...) -> Vec<Violation>`; verify with unit tests for each presence kind, each header format, nested arrays, and violation messages that carry JSON paths and types but never values
- [x] 1.2 Write the contracts for the authorization request/callback, token (authorization-code and refresh grants), JWKS, access-token claims, orders (sell and buy), names (200 and 404), structures (200 and 403), and the not-verifiable 420/429/5xx variants with their reason; verify with unit tests that feed each variant a hand-written conforming answer and one with a missing or extra field
- [x] 1.3 Add `fingerprint(workspace_root)` (all files under `crates/eve-stub/src` by path and contents, plus the canonical JSON of all contracts) and the `stub-fingerprint` binary; verify with unit tests that a changed stub file, an added stub file and a changed contract each change it, and that file order on disk does not
- [x] 1.4 Add `gate::require_verified_stub()` reading `crates/esi-contract/verified-stub.json`; verify with unit tests for a missing record, a stale fingerprint (message names `cargo run -p esi-verify`) and a match

## 2. Stub conformance

- [x] 2.1 Export the stub's EVE route list from `eve-stub` and add the coverage test that every route has a contract and every contract a route; verify it fails when a route is added without a contract
- [x] 2.2 Add `crates/eve-stub/tests/contract.rs` checking every stub variant (default fixtures, sell/buy/station/structure orders, refused structure, unknown name, `esi-fail-next` statuses with headers, authorize redirect, code exchange, refresh, JWKS, issued JWT claims) against its contract, and fix the stub until it passes; verify with `cargo test -p eve-stub --test contract`

## 3. Real-EVE verifier

- [x] 3.1 Create `crates/esi-verify` with the login step: refuse to start unless `cargo test -p eve-stub --test contract` passes; authorization request from the app's `oauth.rs`; callback listener on `PUBLIC_BASE_URL`'s `/auth/callback` with the dev certificate; opened with `xdg-open`/`open` and printed; fail at once without a display; `state` checked; code exchange, JWT (`SsoClient::validate`) and one refresh checked against their contracts; verify with an integration test against the stub in which the test plays the browser, covering a wrong `state` (no code exchanged), a busy port (address named) and no display
- [x] 3.2 Add the ESI calls (orders, names with known IDs, names 404, structure 200 from `ESI_VERIFY_STRUCTURE_ID`, structure 403 from `ESI_VERIFY_FORBIDDEN_STRUCTURE_ID`) with the app's `User-Agent` and `X-Compatibility-Date`, each checked against its contract and decoded with the app's types; fail without a sell or a buy order; stop at 420/429; write `verified-stub.json` only on full success; verify with integration tests against the stub (pass writes the record, a contract violation, a decode failure and a missing buy order leave it unchanged, no token or order value appears in any output)
- [x] 3.3 Run `cargo run -p esi-verify` against real EVE with a real verification character (needs the user: EVE login in the browser, the two structure IDs in `.env`, a sell and a buy order); record the real `X-Ratelimit-*` headers per route, adjust contracts and stub fixtures to what real ESI sends, re-run until it passes, and commit `verified-stub.json`

## 4. Gate

- [x] 4.1 Call `require_verified_stub()` before any stub starts in `crates/sekondo/tests/common` and `crates/eve-stub/tests/common` (except `tests/contract.rs`); verify that after editing a stub fixture `cargo test --workspace --features sekondo/test-hooks` fails with the message and starts no stub, and that with a matching record it passes without network access to EVE
- [x] 4.2 Add the same check to `e2e/global-setup.ts` (via `stub-fingerprint`) before the build; verify that `npx playwright test` fails with the message after a stub edit and passes with a matching record
- [x] 4.3 Document in `README.md` (tests section) and `.env.example`: why the gate exists, that changing the stub requires `cargo run -p esi-verify` and committing `verified-stub.json`, the verification character's preconditions, the two structure IDs, and that the app container must be stopped; verify the documented commands run as written from a clean shell
