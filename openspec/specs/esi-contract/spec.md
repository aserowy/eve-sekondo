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
ID causes), the structure lookup (success and 403), the public market orders of a region for one item
(with the `X-Pages` header and each order's remaining volume), the public market orders of a region
for all items (with the `X-Pages` header and each order's type, location and remaining volume), the
market history of a region for one item, the character wallet balance, the
character wallet transactions (the newest ones, and older ones from a given transaction ID), the
character assets (with the `X-Pages` header), the market prices, the character skills, the character
standings and the station lookup. A variant that cannot be provoked against the
real services without harm (420, 429, 5xx) SHALL be marked as not verifiable against EVE, with a
reason.

#### Scenario: A stub route without a contract

- **WHEN** the stub serves an EVE route (not a stub control route) that has no contract
- **THEN** the contract coverage test fails and names the route

#### Scenario: A header the app reads is part of the contract

- **WHEN** the app reads a response header of a call (for example `Expires`, `Date`,
  `X-ESI-Error-Limit-Remain`, `X-Ratelimit-Group`)
- **THEN** the contract variants of that call that carry it list the header and its format

#### Scenario: Wealth calls are contracted

- **WHEN** the stub serves the wallet, assets or market prices route
- **THEN** the contract coverage test finds a contract for it

#### Scenario: Fee calls are contracted

- **WHEN** the stub serves the skills, standings or station route
- **THEN** the contract coverage test finds a contract for it

#### Scenario: The transactions call is contracted

- **WHEN** the stub serves the wallet transactions route
- **THEN** the contract coverage test finds a contract for it

#### Scenario: Station market calls are contracted

- **WHEN** the stub serves the region-wide market orders or the market history route
- **THEN** the contract coverage test finds a contract for each

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
The character's assets SHALL contain at least one asset, otherwise the verification fails and says
that an asset is needed. The character's standings SHALL contain at least one NPC corporation or
faction standing, otherwise the verification fails and says that a standing is needed.
The market orders call SHALL be verified for an item and region taken from the character's own orders,
so that sell and buy orders of the real market are checked. The region-wide market orders call SHALL
be verified with the first page of The Forge, and the market history call for The Forge and the item
of the market orders check. The station lookup SHALL be verified for
an NPC station taken from the character's own orders, or for a fixed well-known NPC station when all
of its orders are in structures.

#### Scenario: Real ESI omits a field the contract requires

- **WHEN** a real answer lacks a field its contract variant marks as always present
- **THEN** the verification fails and names the call, the variant and the field

#### Scenario: The contract matches but the app cannot decode

- **WHEN** a real answer satisfies its contract but the app's own decoding rejects it
- **THEN** the verification fails with the decoding error

#### Scenario: Not enough orders to verify both sides

- **WHEN** the verification character has no buy order (or no sell order)
- **THEN** the verification fails and says that a buy (or sell) order is needed

#### Scenario: No asset to verify

- **WHEN** the verification character has no assets
- **THEN** the verification fails and says that an asset is needed

#### Scenario: No standing to verify

- **WHEN** the verification character has no NPC standings
- **THEN** the verification fails and says that a standing is needed

#### Scenario: A login answer with a wrong state

- **WHEN** the login callback arrives with a `state` that does not match the one sent
- **THEN** no code is exchanged and the verification fails

#### Scenario: No browser available

- **WHEN** the verification starts where no browser can be opened
- **THEN** it fails at once and says a browser and a person are needed

#### Scenario: Real market answer lacks a field the contract requires

- **WHEN** a real market orders answer lacks a field (such as `system_id`) that its contract variant
  marks as always present
- **THEN** the verification fails and names the call, the variant and the field

#### Scenario: Real history answer lacks a field the contract requires

- **WHEN** a real market history answer lacks a field (such as `volume`) that its contract variant
  marks as always present
- **THEN** the verification fails and names the call, the variant and the field

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

### Requirement: The stub serves the market call

The stub SHALL serve the public market orders of a region for an item (with the `X-Pages` header and
pages as real ESI does), with a control to set the market orders per region, and every answer SHALL
satisfy its contract variant. The call SHALL need no token.

#### Scenario: Market fixture drifts from the contract

- **WHEN** a market fixture of the stub omits a field the contract marks as always present
- **THEN** the stub conformance test fails and names the fixture and the field

#### Scenario: The market route is contracted

- **WHEN** the stub serves the market orders route
- **THEN** the contract coverage test finds a contract for it

### Requirement: The stub serves the wealth calls

The stub SHALL serve the character wallet balance, the character assets (with the `X-Pages` header and
pages as real ESI does) and the market prices, with controls to set the balance and the assets per
character and the prices, and every answer SHALL satisfy its contract variant. The wallet and assets
calls SHALL require a token with the matching scope and SHALL answer 403 otherwise. The prices call
SHALL need no token.

#### Scenario: Assets fixture drifts from the contract

- **WHEN** an assets fixture of the stub omits a field the contract marks as always present
- **THEN** the stub conformance test fails and names the fixture and the field

#### Scenario: Token without the wallet scope

- **WHEN** the stub receives a wallet request with a token that lacks the wallet scope
- **THEN** it answers 403

### Requirement: The stub serves the fee calls

The stub SHALL serve the character skills, the character standings and the station lookup, with
controls to set the skills and the standings per character and the owner per station, and every
answer SHALL satisfy its contract variant. The skills and standings calls SHALL require a token with
the matching scope and SHALL answer 403 otherwise. The station call SHALL need no token.

#### Scenario: Skills fixture drifts from the contract

- **WHEN** a skills fixture of the stub omits a field the contract marks as always present
- **THEN** the stub conformance test fails and names the fixture and the field

#### Scenario: Token without the standings scope

- **WHEN** the stub receives a standings request with a token that lacks the standings scope
- **THEN** it answers 403

### Requirement: The stub serves the transactions call

The stub SHALL serve the character wallet transactions, newest first, with a limit per answer, and
older ones when given a transaction ID to start from, as real ESI does. It SHALL have a control to
set the transactions per character, and every answer SHALL satisfy its contract variant. The call
SHALL require a token with the wallet scope and SHALL answer 403 otherwise. The verification against
real EVE SHALL check the call with the verification character, and SHALL fail with a message saying
that a purchase is needed when the answer contains no purchase.

#### Scenario: Transactions fixture drifts from the contract

- **WHEN** a transactions fixture of the stub omits a field the contract marks as always present
- **THEN** the stub conformance test fails and names the fixture and the field

#### Scenario: Older transactions

- **WHEN** the stub holds more transactions than one answer carries and is asked for those older
  than the oldest one returned
- **THEN** it returns the next older ones, and an empty list once there are none

#### Scenario: Token without the wallet scope

- **WHEN** the stub receives a transactions request with a token that lacks the wallet scope
- **THEN** it answers 403

#### Scenario: No purchase to verify

- **WHEN** the verification character has no purchase in its wallet transactions
- **THEN** the verification fails and says that a purchase is needed

### Requirement: The stub serves the station market calls

The stub SHALL serve the public market orders of a region for all items (with the `X-Pages` header and
pages as real ESI does) and the market history of a region for one item. It SHALL have controls to set
the region's orders and the history per item, and to make a chosen page or a chosen item's history
fail. Every answer SHALL satisfy its contract variant. Neither call SHALL need a token.

#### Scenario: Region-wide orders over several pages
- **WHEN** the stub holds more orders for a region than fit on one page and the app reads the
  region-wide order book
- **THEN** the stub answers with `X-Pages` and splits the orders over the pages as real ESI does

#### Scenario: History fixture drifts from the contract
- **WHEN** a history fixture of the stub omits a field the contract marks as always present
- **THEN** the stub conformance test fails and names the fixture and the field
