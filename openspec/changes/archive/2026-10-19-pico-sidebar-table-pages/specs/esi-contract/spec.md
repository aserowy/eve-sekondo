## MODIFIED Requirements

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
standings, the station lookup and the character location (docked at a station, docked in a
structure, and in space). A variant that cannot be provoked against the
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

#### Scenario: The location call is contracted

- **WHEN** the stub serves the character location route
- **THEN** the contract coverage test finds a contract for it

## ADDED Requirements

### Requirement: The stub serves the location call

The stub SHALL serve the character location, with a control to set per character whether it is
docked at a station, docked in a structure or in space, and every answer SHALL satisfy its contract
variant. The call SHALL require a token with the location scope and SHALL answer 403 otherwise. The
stub's characters SHALL hold the location scope by default, and its per-character scopes control
SHALL be able to withhold it.

#### Scenario: Location fixture drifts from the contract

- **WHEN** a location fixture of the stub omits the solar system the contract marks as always present
- **THEN** the stub conformance test fails and names the fixture and the field

#### Scenario: Token without the location scope

- **WHEN** the stub receives a location request with a token that lacks the location scope
- **THEN** it answers 403
