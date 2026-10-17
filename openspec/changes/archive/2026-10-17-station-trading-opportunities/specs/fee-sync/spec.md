# Spec Delta

## ADDED Requirements

### Requirement: Fees at the built-in trading station
The system SHALL know the owner corporation and faction of the built-in trading station (Jita IV - Moon 4 - Caldari Navy Assembly Plant) even when no linked character has an order there. It SHALL look the owner up the same way as for other NPC stations, once, and reuse it for every account. Every character whose skills and standings are known SHALL have a sales tax, broker fee and minimum margin at that station, whether or not it has an order there. These fees SHALL NOT be listed among a character's order locations on the orders page unless it has an order there.

#### Scenario: Character without orders in Jita
- **WHEN** a character with known skills and standings has orders only in Amarr
- **THEN** it still has a broker fee and minimum margin at Jita 4-4, and the orders page lists only Amarr for it

#### Scenario: Owner looked up once
- **WHEN** the app starts with no station owners known and no character trading in Jita
- **THEN** the owner of Jita 4-4 is requested from ESI once and then reused
