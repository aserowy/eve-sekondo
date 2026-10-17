# fee-sync Specification

## Purpose

Keeps a current copy of every linked character's trade skills and NPC standings, and works out from them what the market charges the character at each station or structure where it trades: the sales tax, the broker fee and the minimum margin a station trade needs to break even.

## Requirements

### Requirement: Poll skills and standings
The system SHALL poll the skills and the standings of every linked character that does not need re-login, each at most once per hour and never before the cache expiry time ESI reported for its previous answer. It SHALL use the active skill level, not the trained level. A character SHALL keep its last known skills and standings when a poll fails. A failed poll SHALL be retried with the same backoff and ESI etiquette as the order poll, and SHALL NOT affect the order or wealth polls of that character.

#### Scenario: Skills after a poll
- **WHEN** a skills poll reports Accounting with active level 4 and trained level 5
- **THEN** the character's Accounting level counts as 4

#### Scenario: Polls wait for the hour
- **WHEN** a character's standings were polled successfully 20 minutes ago
- **THEN** they are not polled again until an hour has passed since that poll

#### Scenario: Character needing re-login is skipped
- **WHEN** a character is marked "needs re-login"
- **THEN** its skills and standings are not polled and its last known values are kept

#### Scenario: Failed poll
- **WHEN** a standings poll fails because of an ESI server error
- **THEN** the last known standings are kept and the poll is retried later

### Requirement: Station owner and faction
For every NPC station where a current order of any linked character is placed, the system SHALL know the corporation that owns the station, as ESI reports it, and that corporation's faction, from a table taken from CCP's static data export that is kept with the application. A station's owner SHALL be looked up once and then reused for every account. A corporation that the table does not list SHALL count as having no faction.

#### Scenario: Station shared by two accounts
- **WHEN** characters of two accounts have orders at the same NPC station
- **THEN** its owner is requested from ESI once

#### Scenario: Corporation without a faction
- **WHEN** a station's owner corporation is not in the faction table
- **THEN** the faction standing counts as 0 for that station

### Requirement: Sales tax
The sales tax rate of a character SHALL be 7.5% × (1 − 0.11 × L), with L the character's Accounting level, or 0 when the character has not trained Accounting. It SHALL be the same at every station and structure.

#### Scenario: Accounting 5
- **WHEN** a character has Accounting at level 5
- **THEN** its sales tax is 3.375%

#### Scenario: No Accounting
- **WHEN** a character has not trained Accounting
- **THEN** its sales tax is 7.5%

### Requirement: Broker fee at an NPC station
The broker fee rate of a character at an NPC station SHALL be 3% − 0.3% × B − 0.03% × F − 0.02% × C, with B the character's Broker Relations level, F the character's standing towards the faction of the station's owner, and C its standing towards the owner corporation. B, F and C SHALL be 0 when the skill is not trained, the station owner has no faction, or ESI reports no standing. Standings SHALL be used as ESI reports them, and negative standings SHALL raise the fee.

#### Scenario: Skills and standings lower the fee
- **WHEN** a character with Broker Relations 5 has a standing of 2.0 towards the station owner's faction and 1.5 towards the owner corporation
- **THEN** its broker fee at that station is 1.41%

#### Scenario: Negative standing
- **WHEN** a character without Broker Relations has a standing of −5.0 towards the station owner's faction and none towards the corporation
- **THEN** its broker fee at that station is 3.15%

### Requirement: Broker fee in a player structure
In a player-owned structure, the system SHALL treat the broker fee as 0.5% plus a fee set by the structure owner that is not known. Skills and standings SHALL NOT change it.

#### Scenario: Order in a structure
- **WHEN** a character has an order in a player-owned structure
- **THEN** its broker fee there is 0.5% plus an unknown owner fee

### Requirement: Minimum margin
For a location where the broker fee is known, the system SHALL determine the minimum margin of a station trade: an item bought through a buy order and sold through a sell order at that location, with the broker fee b paid on both orders and the sales tax t paid on the sale. It SHALL be the markup on the buy price at which the sale exactly pays back the purchase, (1 + b) / (1 − b − t) − 1. Where the broker fee is not known, there SHALL be no minimum margin.

#### Scenario: Station trade at a known fee
- **WHEN** a character's broker fee at a station is 1.41% and its sales tax is 3.375%
- **THEN** its minimum margin there is 6.506% (rounded)

#### Scenario: Structure
- **WHEN** a character's location is a player-owned structure
- **THEN** there is no minimum margin for it

### Requirement: Fee data not known yet
A character SHALL have no sales tax, broker fee or minimum margin while its skills have never been polled successfully, and no NPC station broker fee or minimum margin while its standings have never been polled successfully or the station's owner is not known yet. Skills and standings that are older than an hour SHALL still be used as last known.

#### Scenario: Newly linked character
- **WHEN** a character was linked and its skills have not been polled yet
- **THEN** it has no sales tax, broker fee or minimum margin

#### Scenario: Station owner not known yet
- **WHEN** a character places its first order at an NPC station whose owner has not been looked up yet
- **THEN** it has a sales tax but no broker fee or minimum margin at that station until the owner is known

### Requirement: Fee data is removed with the character
Skills, standings and fee sync state of a character SHALL be deleted when the character is unlinked or changes hands. Station owners are public and SHALL NOT belong to any account.

#### Scenario: Unlink
- **WHEN** a character is unlinked
- **THEN** no skill, standing or fee sync row of it remains
