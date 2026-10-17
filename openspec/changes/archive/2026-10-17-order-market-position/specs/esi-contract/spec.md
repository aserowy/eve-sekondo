# Spec Delta

## MODIFIED Requirements

### Requirement: Every outbound EVE call has a contract

Each request the app sends to ESI or EVE SSO SHALL have exactly one contract, with one variant per
answer the app handles differently. A variant SHALL state the HTTP status, every response header the
app reads (by name, with its format), and for a JSON body every field the app reads with its type and
whether it may be missing or null. The covered calls are: the authorization request and its callback,
the token request (authorization code grant and refresh grant), the JWKS document, the claims of the
access token JWT, the character orders route, the names lookup (success, and the 404 that one unknown
ID causes), the structure lookup (success and 403), the public market orders of a region for one item
(with the `X-Pages` header). A variant that cannot be provoked against the
real services without harm (420, 429, 5xx) SHALL be marked as not verifiable against EVE, with a
reason.

#### Scenario: A stub route without a contract

- **WHEN** the stub serves an EVE route (not a stub control route) that has no contract
- **THEN** the contract coverage test fails and names the route

#### Scenario: A header the app reads is part of the contract

- **WHEN** the app reads a response header of a call (for example `Expires`, `Date`,
  `X-ESI-Error-Limit-Remain`, `X-Ratelimit-Group`)
- **THEN** the contract variants of that call that carry it list the header and its format

### Requirement: Real EVE is verified against the contracts

A verification command SHALL open the real EVE login in a browser with the authorization request the
app builds, wait for a person to log in with a verification character, and then send every
verifiable variant's request to real SSO and ESI with the same `User-Agent` and
`X-Compatibility-Date` the app sends. Each real answer SHALL satisfy its contract variant and SHALL
decode with the app's own response handling. The character's orders SHALL contain at least one sell
order and at least one buy order, otherwise the verification fails and says which side is missing.
The market orders call SHALL be verified for an item and region taken from the character's own orders,
so that sell and buy orders of the real market are checked.

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

#### Scenario: Real market answer lacks a field the contract requires

- **WHEN** a real market orders answer lacks a field (such as `system_id`) that its contract variant
  marks as always present
- **THEN** the verification fails and names the call, the variant and the field

## ADDED Requirements

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
