# Spec Delta

## ADDED Requirements

### Requirement: Poll totals history
Every successful poll SHALL record one history entry for the character, holding the poll time and the totals of the orders it returned: sell value, buy escrow, the corporation share of each, and the number of sell and buy orders. It SHALL be recorded together with the current orders and the snapshot rows, so that either all of them or none of them are stored. A poll that returns no orders SHALL record an entry with every figure zero. A failed poll SHALL record no entry. Entries SHALL be kept without a retention limit and SHALL be deleted when the character is unlinked. When this history is introduced, it SHALL be filled from the existing snapshots, one entry per character and poll time that has snapshot rows.

#### Scenario: Entry after a poll with orders
- **WHEN** a poll returns a sell order of 10 remaining units at 1,000,000.00 ISK and a corporation buy order with 2,000,000.00 ISK escrow
- **THEN** one entry for that poll time holds a sell value of 10,000,000.00 ISK, an escrow of 2,000,000.00 ISK, an escrow corporation share of 2,000,000.00 ISK, one sell order and one buy order

#### Scenario: Entry after a poll without orders
- **WHEN** a poll returns no orders
- **THEN** an entry for that poll time exists with every figure zero

#### Scenario: Failed poll
- **WHEN** a poll fails because of an ESI server error
- **THEN** no entry is recorded for it

#### Scenario: Entries are removed with the character
- **WHEN** a character with history entries is unlinked
- **THEN** no entry for that character remains

#### Scenario: Backfill from snapshots
- **WHEN** the history is introduced on a database with snapshots of two polls of one character
- **THEN** two entries exist for that character, each matching the totals of its poll's snapshot rows
