# esi-contract Specification

## Purpose

Guarantees that the stand-in for EVE SSO and ESI answers the way the real services do: every change to
the stand-in is verified against real EVE before any test may rely on it.

## Requirements

### Requirement: Every outbound EVE call has a contract

Each request the app sends to ESI or EVE SSO SHALL have exactly one contract, with one variant per
answer the app handles differently. A variant SHALL state the HTTP status, every response header the
app reads (by name, with its format), and for a JSON body every field the app reads with its type and
whether it may be missing or null. The covered calls are: the authorization request and its callback,
the token request (authorization code grant and refresh grant), the JWKS document, the claims of the
access token JWT, the character orders route, the names lookup (success, and the 404 that one unknown
ID causes) and the structure lookup (success and 403). A variant that cannot be provoked against the
real services without harm (420, 429, 5xx) SHALL be marked as not verifiable against EVE, with a
reason.

#### Scenario: A stub route without a contract

- **WHEN** the stub serves an EVE route (not a stub control route) that has no contract
- **THEN** the contract coverage test fails and names the route

#### Scenario: A header the app reads is part of the contract

- **WHEN** the app reads a response header of a call (for example `Expires`, `Date`,
  `X-ESI-Error-Limit-Remain`, `X-Ratelimit-Group`)
- **THEN** the contract variants of that call that carry it list the header and its format

### Requirement: The stub conforms to the contracts

Every answer the stub can give for a contracted call SHALL satisfy the matching contract variant,
including sell and buy orders, orders in stations and structures, refused structures, unknown names,
and injected failures. The check SHALL run in the regular test suite.

#### Scenario: Stub fixture drifts from the contract

- **WHEN** a stub fixture sends a field the contract marks as absent for that variant, or omits one
  marked as always present
- **THEN** the stub conformance test fails and names the fixture and the field

### Requirement: Real EVE is verified against the contracts

A verification command SHALL open the real EVE login in a browser with the authorization request the
app builds, wait for a person to log in with a verification character, and then send every
verifiable variant's request to real SSO and ESI with the same `User-Agent` and
`X-Compatibility-Date` the app sends. Each real answer SHALL satisfy its contract variant and SHALL
decode with the app's own response handling. The character's orders SHALL contain at least one sell
order and at least one buy order, otherwise the verification fails and says which side is missing.

#### Scenario: Real ESI omits a field the contract requires

- **WHEN** a real answer lacks a field its contract variant marks as always present
- **THEN** the verification fails and names the call, the variant and the field

#### Scenario: The contract matches but the app cannot decode

- **WHEN** a real answer satisfies its contract but the app's own decoding rejects it
- **THEN** the verification fails with the decoding error

#### Scenario: Not enough orders to verify both sides

- **WHEN** the verification character has no buy order (or no sell order)
- **THEN** the verification fails and says that a buy (or sell) order is needed

#### Scenario: A login answer with a wrong state

- **WHEN** the login callback arrives with a `state` that does not match the one sent
- **THEN** no code is exchanged and the verification fails

#### Scenario: No browser available

- **WHEN** the verification starts where no browser can be opened
- **THEN** it fails at once and says a browser and a person are needed

### Requirement: The verified stub is recorded by fingerprint

The stub fingerprint SHALL cover all of the stub's source (routes, fixtures, failure injections) and
all contracts, so any change to the stub's behaviour or to a contract changes it. Only a verification
in which every verifiable variant passed against real EVE, and the stub conformance check passed for
the same fingerprint, SHALL record the fingerprint, in a file under version control. The record SHALL
contain the fingerprint, the time and the list of verified variants, and no token or character data.

#### Scenario: Successful verification

- **WHEN** the verification passes for the current stub
- **THEN** the record holds the current fingerprint and the variants that were verified

#### Scenario: Failed verification

- **WHEN** any check of the verification fails
- **THEN** the record is left unchanged

#### Scenario: A new ESI call

- **WHEN** a stub route and a contract are added for a new call
- **THEN** the fingerprint changes and the stub counts as unverified

### Requirement: Stub-backed tests require a verified stub

No test that starts the stub SHALL run while the recorded fingerprint differs from the current one.
The failure SHALL say that the stub changed since its last verification against real EVE and name the
verification command, and SHALL happen before any stub starts. There SHALL be no setting or flag that
lets stub-backed tests run with an unverified stub. When the fingerprints match, stub-backed tests
SHALL run without network access to EVE and without EVE credentials.

#### Scenario: Stub unchanged

- **WHEN** the tests run and the recorded fingerprint matches the current stub
- **THEN** they run without contacting EVE

#### Scenario: Stub changed

- **WHEN** a stub fixture was edited and the tests run before a new verification
- **THEN** every stub-backed test fails with a message naming the verification command, and no stub
  is started

#### Scenario: Playwright with a changed stub

- **WHEN** the Playwright suite starts while the stub is unverified
- **THEN** its setup fails with the same message before building or starting anything

### Requirement: Verification secrets stay in memory

Tokens obtained during the verification SHALL exist only in memory for the duration of the run. They
SHALL never be written to disk, printed, logged or included in the record or in failure messages.
Failure messages SHALL name JSON paths and types, not values from real answers.

#### Scenario: Verification output on failure

- **WHEN** the verification fails on a token or orders call
- **THEN** its output names the call and the problem but contains no token and no order data
