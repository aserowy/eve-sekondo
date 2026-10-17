# trade-margin Specification

## Purpose

Tells a trader what each sell order earns. It keeps every character's purchases from ESI's wallet transactions, matches them first in, first out to the sell orders that resell them, and works out the margin after fees.

## Requirements

### Requirement: Poll wallet transactions
The system SHALL poll the wallet transactions of every linked character that does not need re-login. It SHALL poll each character at most once per hour and never before the cache expiry time ESI reported for its previous answer. Polls of different characters SHALL be spread across the hour. Each poll SHALL read further back from the newest transaction until it reaches a transaction it already stored or ESI returns no older ones. It SHALL store every transaction that is a personal purchase (`is_buy` and `is_personal`) as a purchase lot: the item, the quantity, the unit price, the date and the location. It SHALL store no other transactions. A transaction SHALL be stored once, however often it is returned. Stored purchases SHALL be kept after ESI stops returning them. A failed poll SHALL keep the stored purchases and SHALL be retried with the same backoff and ESI etiquette as the order poll. It SHALL NOT affect the order, wealth or fee polls of that character.

#### Scenario: First poll of a character
- **WHEN** a newly linked character's transactions are polled for the first time and ESI returns 3,000 transactions over several requests
- **THEN** every personal purchase among them is stored

#### Scenario: Later poll stops at known transactions
- **WHEN** a character's transactions are polled again and the newest answer already contains a stored transaction
- **THEN** no older transactions are requested

#### Scenario: Sales and corporation purchases are not lots
- **WHEN** a poll returns a sale, and a purchase with `is_personal` false
- **THEN** neither is stored as a purchase lot

#### Scenario: Purchases outlive ESI's window
- **WHEN** a purchase was stored 60 days ago and ESI no longer returns it
- **THEN** it is still a purchase lot of the character

#### Scenario: Character needing re-login is skipped
- **WHEN** a character is marked "needs re-login"
- **THEN** its transactions are not polled and its stored purchases are kept

#### Scenario: Failed poll
- **WHEN** a transactions poll fails because of an ESI server error
- **THEN** the stored purchases are kept, the failure is recorded, and the poll is retried later

### Requirement: Remembered sell orders
The system SHALL remember every personal sell order that an order poll sees, with its character, item, total volume and the time it was issued when first seen. It SHALL keep the order after it closes. A later change of the order's price or issue time SHALL NOT change what is remembered. Corporation sell orders SHALL NOT be remembered. Sell orders that the stored order history already shows when this capability is introduced SHALL be remembered the same way, with the issue time they had when first seen.

#### Scenario: Repriced order
- **WHEN** a sell order first seen as issued at 10:00 is repriced in game, and ESI then reports it as issued at 14:00
- **THEN** it is still remembered as issued at 10:00

#### Scenario: Closed order is kept
- **WHEN** a remembered sell order is no longer returned by ESI
- **THEN** it is still remembered and keeps the purchase lots it took

### Requirement: First in, first out
For each character and item, the remembered sell orders SHALL take units from that character's purchase lots of that item. They SHALL take them in the order they were first issued, the order ID breaking ties. Each order SHALL take up to its total volume, at any location. It SHALL take units from the lots bought at or before its first issue time that earlier orders have not taken, oldest lot first, the transaction ID breaking ties. A lot MAY be split between orders. The units an order took and their total purchase price SHALL be its known cost. The allocation SHALL be redone from all stored lots whenever lots or remembered orders change, so that a purchase stored late still feeds the orders issued after it. Sales into another trader's buy order SHALL NOT take units.

#### Scenario: One order from two purchases
- **WHEN** a character bought 10 units at 100.00 ISK on day 1 and 10 units at 120.00 ISK on day 2, and issues a sell order of 15 units on day 3
- **THEN** the order takes 10 units at 100.00 ISK and 5 units at 120.00 ISK, a known cost of 1,600.00 ISK for 15 units

#### Scenario: Later order gets the rest
- **WHEN** after that order the character issues a second sell order of 10 units of the same item on day 4, without buying more
- **THEN** the second order takes the remaining 5 units at 120.00 ISK, and its cost is known for 5 of 10 units

#### Scenario: Purchases after the order do not count
- **WHEN** a sell order is first issued on day 3 and the character buys more of the item on day 5
- **THEN** the day 5 purchase is not taken by that order

#### Scenario: Other characters' purchases do not count
- **WHEN** one character bought the item and another character of the same account sells it
- **THEN** the selling character's order takes no units from the first character's purchases

#### Scenario: Purchase stored late
- **WHEN** a sell order was remembered before the transactions poll stored the purchase made just before it
- **THEN** after that poll the order takes units from that purchase

### Requirement: Margin of a sell order
For a personal sell order whose known cost covers its whole total volume, and whose character's sales tax t and broker fee b at the order's location are known (as defined by the fee-sync capability), the margin SHALL be V × P × (1 − t − b) − V × C / N. Here V is the remaining volume, P is the price, C is the known cost and N is the total volume. The margin SHALL be rounded half up to the cent and MAY be negative. Otherwise the order SHALL have no margin. When the cost does not cover the whole volume, the units it does cover SHALL be known for display.

#### Scenario: Margin after fees
- **WHEN** the sell order of 15 units from the scenario above has 15 remaining at 150.00 ISK, with a sales tax of 3.375% and a broker fee of 1.41%
- **THEN** its margin is 542.34 ISK

#### Scenario: Partly sold order
- **WHEN** that order has 5 units remaining
- **THEN** its margin is 180.78 ISK

#### Scenario: Cost not fully known
- **WHEN** a sell order's known cost covers 5 of 10 units
- **THEN** it has no margin, and 5 of 10 covered units are known

#### Scenario: Fees not known
- **WHEN** a sell order with fully known cost is placed in a player-owned structure, or its character's skills have not been polled yet
- **THEN** it has no margin

#### Scenario: Corporation order
- **WHEN** a sell order is placed on behalf of the corporation
- **THEN** it has no margin

### Requirement: Trade data is removed with the character
The purchase lots, the remembered sell orders and the transactions sync state of a character SHALL be deleted when the character is unlinked or changes hands.

#### Scenario: Unlink
- **WHEN** a character is unlinked
- **THEN** no purchase lot, remembered sell order or transactions sync row of it remains
