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
(with the `X-Pages` header and each order's remaining volume), the character wallet balance, the
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

## ADDED Requirements

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
