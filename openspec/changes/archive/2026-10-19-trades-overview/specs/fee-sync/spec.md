# Spec Delta

## MODIFIED Requirements

### Requirement: Station owner and faction
For every NPC station where a current order of any linked character is placed, or where a stored sale of any linked character was made, the system SHALL know the corporation that owns the station, as ESI reports it, and that corporation's faction, from a table taken from CCP's static data export that is kept with the application. A station's owner SHALL be looked up once and then reused for every account. A corporation that the table does not list SHALL count as having no faction.

#### Scenario: Station shared by two accounts
- **WHEN** characters of two accounts have orders at the same NPC station
- **THEN** its owner is requested from ESI once

#### Scenario: Station of a sale
- **WHEN** a transactions poll stores a sale at an NPC station where no linked character has an order and whose owner has not been looked up yet
- **THEN** the station's owner is looked up, and the sale's margin is known once it is

#### Scenario: Corporation without a faction
- **WHEN** a station's owner corporation is not in the faction table
- **THEN** the faction standing counts as 0 for that station
