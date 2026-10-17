# Spec Delta

## ADDED Requirements

### Requirement: Market fees per character
Behind each character's freshness entry, the orders page SHALL list every station or structure where the character currently has orders, ordered by location name, each with the character's sales tax, broker fee and minimum margin there, as defined by the fee-sync capability. Rates SHALL be shown as percentages rounded half up to two decimals. Hovering a rate SHALL show it rounded half up to four decimals together with its inputs: the Accounting level for the sales tax, and the Broker Relations level, the faction standing and the corporation standing for an NPC station broker fee. In a player-owned structure the broker fee SHALL read "0.5% + owner fee" and the minimum margin "—". A figure that is not known yet SHALL read "—". A character without orders SHALL list no locations.

#### Scenario: NPC station
- **WHEN** a character with Accounting 5 and Broker Relations 5 has orders at an NPC station where its faction standing is 2.0 and its corporation standing is 1.5
- **THEN** its entry lists that station with sales tax "3.38%", broker fee "1.41%" and minimum margin "6.51%"

#### Scenario: Inputs on hover
- **WHEN** a user hovers that broker fee
- **THEN** it shows "1.4100%", Broker Relations 5, faction standing 2.00 and corporation standing 1.50

#### Scenario: Minimum margin on hover
- **WHEN** a user hovers that minimum margin
- **THEN** it shows "6.5063%"

#### Scenario: Two locations
- **WHEN** a character has orders at an NPC station and in a player-owned structure
- **THEN** its entry lists both, and the structure's broker fee reads "0.5% + owner fee" and its minimum margin "—"

#### Scenario: Skills not polled yet
- **WHEN** a newly linked character with orders has not had its skills polled yet
- **THEN** each of its locations reads "—" for every figure

#### Scenario: Only selected characters
- **WHEN** the selection is narrowed to one of two characters
- **THEN** only that character's locations and fees are shown
