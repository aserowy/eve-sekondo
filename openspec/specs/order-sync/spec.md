# order-sync Specification

## Purpose

Keeps a current copy of every linked character's active market orders by polling ESI politely, records a snapshot of each poll as history, and resolves the names the views need.

## Requirements

### Requirement: Poll character orders
The system SHALL periodically fetch the active market orders of every linked character that is not marked "needs re-login", using that character's ESI access token. It SHALL NOT request orders or tokens for a character marked "needs re-login". Polling SHALL run inside the application process without any user being signed in.

#### Scenario: Orders are fetched for a linked character
- **WHEN** a character with two active orders in ESI is linked and its poll comes due
- **THEN** the system fetches its orders and both orders become the character's current orders

#### Scenario: Character needing re-login is skipped
- **WHEN** a character's poll comes due while it is marked "needs re-login"
- **THEN** no ESI or SSO request is made for it, and its last known orders are kept

#### Scenario: Token refresh is rejected during a poll
- **WHEN** fetching an access token for a poll fails because SSO rejects the refresh token
- **THEN** the character is marked "needs re-login", no orders request is made, and its last known orders are kept

### Requirement: Poll cadence
The system SHALL poll each character at most once per configured refresh interval, which SHALL default to 20 minutes. It SHALL NOT poll a character before the cache expiry time ESI reported in the `Expires` header of that character's previous orders response. Polls of different characters SHALL be spread across the interval rather than issued together. The system SHALL refuse to start when the configured interval is not a whole number of minutes between 1 and 1440.

#### Scenario: Next poll waits for the interval
- **WHEN** a character was polled successfully and ESI's `Expires` lies 5 minutes ahead, with the default interval
- **THEN** the character is not polled again until 20 minutes after the previous poll

#### Scenario: Next poll waits for ESI's cache expiry
- **WHEN** the interval is configured to 5 minutes and ESI's `Expires` for a character lies 20 minutes ahead
- **THEN** the character is not polled again before that `Expires` time

#### Scenario: Polls are spread out
- **WHEN** three characters are linked at the same moment
- **THEN** their polls do not all fall within the same second; their scheduled times differ

#### Scenario: Invalid interval
- **WHEN** the application starts with a refresh interval of 0 minutes or a value that is not a number
- **THEN** it exits with an error naming the interval setting

### Requirement: ESI etiquette
Every ESI request SHALL carry a `User-Agent` that names Sekondo, its version and the application's public URL, and that includes the configured contact when one is set. The system SHALL stop sending ESI requests when the remaining error limit reported by ESI falls below a safety margin or ESI answers HTTP 420, until the error-limit window resets. It SHALL stop sending requests to a rate-limited route group when ESI answers HTTP 429, until the reported `Retry-After` has passed. Requests SHALL be sent over HTTPS to the configured ESI base URL only.

#### Scenario: User-Agent
- **WHEN** the system sends any ESI request
- **THEN** the `User-Agent` header names Sekondo, its version and the public base URL

#### Scenario: Error limit nearly exhausted
- **WHEN** an ESI response reports fewer remaining errors than the safety margin, with the window resetting in 30 seconds
- **THEN** no further ESI request is sent until those 30 seconds have passed

#### Scenario: Rate limited
- **WHEN** ESI answers an orders request with HTTP 429 and `Retry-After: 60`
- **THEN** no further orders request is sent for 60 seconds, and the affected characters are polled afterwards

#### Scenario: ESI server error
- **WHEN** ESI answers an orders request with a server error or the request times out
- **THEN** the character's current orders are left unchanged, the failure is recorded, the character is not marked "needs re-login", and it is polled again later

### Requirement: Current orders
After a successful poll, the character's current orders SHALL be exactly the orders ESI returned. Orders that are no longer returned (filled, cancelled or expired) SHALL stop being current orders. A failed poll SHALL NOT change the character's current orders. Orders a character placed on behalf of their corporation SHALL be kept and flagged as corporation orders.

#### Scenario: An order closes
- **WHEN** a character had orders A and B, and the next successful poll returns only B
- **THEN** only B is a current order of the character

#### Scenario: An order changes
- **WHEN** a successful poll returns an existing order with a new price and a lower remaining volume
- **THEN** the current order shows the new price and remaining volume

#### Scenario: Corporation order
- **WHEN** a poll returns an order flagged as placed on behalf of the corporation
- **THEN** the order is a current order of the character and is flagged as a corporation order

### Requirement: Order snapshots
Every successful poll SHALL record one snapshot row per returned order, holding the poll time, the character, the order identifier and the order's state as returned (including price and remaining volume). Snapshots SHALL be kept without a retention limit. A poll that returns no orders SHALL record that the poll happened without adding snapshot rows. Unlinking a character SHALL delete its snapshots together with its other data.

#### Scenario: Snapshot per order and poll
- **WHEN** a character with three orders is polled successfully twice
- **THEN** six snapshot rows exist for the character, three per poll time

#### Scenario: Snapshots are removed with the character
- **WHEN** a character with snapshots is unlinked
- **THEN** no snapshot row for that character remains

### Requirement: Sync status
The system SHALL record, per character, the time of the last successful poll, the time of the next scheduled poll, and whether the last attempt failed. This status SHALL be available to the views.

#### Scenario: Status after a successful poll
- **WHEN** a character is polled successfully
- **THEN** its last successful poll time is the poll time and its next scheduled poll time lies in the future

#### Scenario: Status after a failed poll
- **WHEN** a poll fails because of an ESI server error
- **THEN** the last successful poll time is unchanged and the failure is recorded

### Requirement: Name resolution
The system SHALL resolve the names of the items, regions, stations and structures referenced by current orders through ESI, cache them in the database, and reuse cached names instead of asking ESI again. Structure names SHALL be resolved with the access token of a character that has an order there. When a structure cannot be resolved because access is denied or it does not exist, the system SHALL show it as "Unknown structure (<id>)" and SHALL retry at most once a day. When a name cannot be resolved because of a temporary failure, the views SHALL show a placeholder with the identifier, and the system SHALL retry on the next poll.

#### Scenario: Names are resolved once
- **WHEN** two characters have orders for the same item in the same station
- **THEN** the item and station names are requested from ESI at most once and both orders show the names

#### Scenario: Inaccessible structure
- **WHEN** an order is located in a structure that ESI refuses to describe for the character
- **THEN** its location is shown as "Unknown structure (<structure id>)"

#### Scenario: Names are available when orders appear
- **WHEN** a poll returns an order for an item whose name is not cached and ESI's name lookup succeeds
- **THEN** the first time the order is shown, it shows the item's name and not its identifier
