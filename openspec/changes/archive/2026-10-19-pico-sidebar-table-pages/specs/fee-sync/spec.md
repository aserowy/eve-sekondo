## ADDED Requirements

### Requirement: Fees at the fee location
Every character whose skills and standings are known SHALL have a sales tax, broker fee and minimum margin at its fee location, as chosen by the character-location capability, whether or not it has an order there. When the fee location is an NPC station whose owner is not known yet, the system SHALL look the owner up the same way as for stations with orders.

#### Scenario: Docked where it has no orders
- **WHEN** a character with known skills and standings is docked at an NPC station where it has no orders
- **THEN** it has a broker fee and minimum margin at that station once the station's owner is known

#### Scenario: Owner looked up for the docked station
- **WHEN** a location poll reports a character docked at an NPC station whose owner is not known
- **THEN** the station's owner is requested from ESI

### Requirement: Fee location changes raise the fee signal
When a location poll changes a character's fee location, so that its sales tax, broker fee or minimum margin at its fee location changes, the system SHALL deliver `fees-updated` to the character's account as described by the live-updates capability.

#### Scenario: Character moves
- **WHEN** a character docked in Jita 4-4 is next reported docked in an Amarr station with a different broker fee
- **THEN** its account receives a `fees-updated` signal

#### Scenario: Same station
- **WHEN** a location poll reports the character at the same station as before
- **THEN** no `fees-updated` signal is delivered for it
