# Proposal

## Why

Every test that talks to "EVE" talks to `eve-stub`, and the stub only knows what we believed ESI and SSO
send. The first poll against real ESI failed because sell orders carry no `min_volume`, while the stub
always sent one and every test passed. A green test run is only worth something if the stub behaves
like the real services, so every change to the stub has to be proven against real ESI and SSO before
tests may rely on it.

## What Changes

- One machine-readable **contract per outbound call** the app makes to ESI and EVE SSO: the status, the
  response headers the app reads, and the JSON body's field names, types and which fields may be missing
  or null. Covered calls: the SSO authorization request and callback, `POST /v2/oauth/token`
  (authorization code and refresh grants), `GET /oauth/jwks` and the access token's claims,
  `GET /characters/{id}/orders`, `POST /universe/names` (success and the "one unknown ID fails the whole
  request" 404) and `GET /universe/structures/{id}` (success and 403).
- The stub's answers are checked against the same contracts in the regular test suite, for every
  variant the stub can produce.
- A **real-EVE verification** (`esi-verify`): it opens the real EVE login in a browser, a person logs in
  with a dedicated verification character, and it then makes every contracted call against real SSO and
  ESI with the app's own request code and headers. Each real answer must satisfy its contract and decode
  with the app's own types. Tokens exist only in memory for the duration of the run.
- A **stub fingerprint** over the stub's source and the contracts. A successful verification records
  the fingerprint in a committed file.
- **Mandatory gate:** no stub-backed test (Rust integration tests in both crates and the Playwright
  suite) runs while the recorded fingerprint differs from the current one. The failure says the stub
  changed and names the command to run. When the stub is unchanged, tests run as today, offline and
  without EVE credentials.
- `market-orders` gets a note in its proposal that verifying the stub against real ESI is mandatory and
  is delivered by this change; its manual task 10.2 stays as is.

## Capabilities

### New Capabilities

- `esi-contract`: the per-call contracts for ESI and SSO answers, the stub's conformance to them, the
  real-EVE verification, the stub fingerprint and the gate that blocks stub-backed tests while the stub
  is unverified.

### Modified Capabilities

None. The app's runtime behaviour does not change.

## Impact

- New workspace crates for the contracts and the verifier (dev/test only, not part of the app binary or
  image).
- `crates/eve-stub`: conformance tests; fixtures fixed wherever they violate a contract.
- `crates/sekondo/tests/common`, `crates/eve-stub/tests/common`, `e2e/global-setup.ts`: the gate.
- New committed file with the verified fingerprint; `.env.example` (two structure IDs for the
  verification); `README.md`.
- Changing the stub now needs a person with an EVE account and a browser: the verification character
  needs at least one sell order, one buy order, one structure it can open and one it cannot.
- Depends on `market-orders` (orders, names and structure calls and the stub routes it adds).
