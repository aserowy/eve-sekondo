# market-position Specification

## Purpose

Tells a station trader whether each of their orders is the best price at its station and, when it is not, how many orders undercut it and by how much, from the public market data of ESI.

## Requirements

### Requirement: Market data poll
The system SHALL keep a copy of the public market orders for every (region, item) pair that the current orders of any linked character use, and SHALL refresh each copy every 5 minutes by default. It SHALL NOT refresh a pair before the cache expiry time ESI reported for its previous answer, and it SHALL read every page of an answer. Refreshes of different pairs SHALL be spread across the interval rather than issued together. A pair that no current order uses any more SHALL stop being refreshed and its copy SHALL be removed. The copy SHALL contain public data only and SHALL NOT depend on any account or token. A failed refresh SHALL keep the previous copy and SHALL be retried with the same backoff and ESI etiquette as the order poll.

#### Scenario: A pair is tracked after an order poll
- **WHEN** a poll stores a character's first order for an item in a region that no other order uses
- **THEN** that (region, item) pair gets its first market refresh within one refresh interval

#### Scenario: Two accounts share a pair
- **WHEN** two accounts have orders for the same item in the same region
- **THEN** the market orders of that pair are requested from ESI once per refresh, not once per account

#### Scenario: Refresh waits for the cache
- **WHEN** a pair was refreshed and ESI's `Expires` lies 8 minutes ahead
- **THEN** the pair is not refreshed again until that time has passed

#### Scenario: A pair stops being used
- **WHEN** the last current order for an item in a region closes
- **THEN** that pair is no longer refreshed and its market orders are removed

#### Scenario: Failed refresh
- **WHEN** a refresh fails because of an ESI server error
- **THEN** the previous market orders of that pair are kept and the refresh is retried later

### Requirement: Standing of a sell order
A sell order SHALL be compared to all other sell orders for the same item at the same station or structure, regardless of the character or account that placed them. The order is **top** when no compared order has a strictly lower price, and is **undercut** otherwise. The number of undercutting orders SHALL be the number of compared orders with a strictly lower price. An order with the same price as the best compared order SHALL count as top.

#### Scenario: Lowest price
- **WHEN** a sell order of 100 ISK is the lowest sell price of its item at its station
- **THEN** its standing is top

#### Scenario: Undercut
- **WHEN** a sell order of 100 ISK has two other sell orders for the same item at the same station priced at 99 ISK and 95 ISK, and one at 98 ISK at another station
- **THEN** its standing is undercut by 2 orders and its best price is 95 ISK

#### Scenario: Tie for the lowest price
- **WHEN** two sell orders at one station have the same lowest price
- **THEN** both are top

#### Scenario: Own orders of other characters count
- **WHEN** another linked character has a sell order for the same item at the same station at a lower price
- **THEN** it counts as an undercutting order

### Requirement: Standing of a buy order
A buy order SHALL be compared at its own station or structure, whatever its own range, to the other buy orders for the same item in the same region that reach that station. A competing order reaches it when it is at the same station or structure (whatever its range), when its range is `region`, or when its range is `solarsystem` or a number of jumps and it is in the same solar system. A competing order with a range of a number of jumps in another solar system SHALL NOT be counted, even if it would reach the station in game. The order is **top** when no reaching order has a strictly higher price, and is **undercut** otherwise. The number of undercutting orders SHALL be the number of reaching orders with a strictly higher price. An order with the same price as the best reaching order SHALL count as top. Minimum volume and remaining volume SHALL NOT matter.

#### Scenario: Highest reaching price
- **WHEN** a buy order of 50 ISK has station-range buy orders for the same item at other stations at 60 ISK, and region-range orders only at 40 ISK
- **THEN** its standing is top

#### Scenario: Region range reaches everywhere
- **WHEN** a buy order of 50 ISK has a region-range buy order for the same item at another station in the region at 55 ISK
- **THEN** its standing is undercut by 1 order

#### Scenario: Any range at the same station
- **WHEN** a buy order of 50 ISK has a station-range buy order and a 3-jump buy order at the same station at 52 ISK and 53 ISK
- **THEN** its standing is undercut by 2 orders and its best reaching price is 53 ISK

#### Scenario: Solar system range
- **WHEN** a buy order of 50 ISK has a solar-system-range buy order at 55 ISK at another station in the same system, and another at 56 ISK in a neighbouring system
- **THEN** only the first one undercuts it

#### Scenario: Jump range in the same system
- **WHEN** a buy order of 50 ISK has a 5-jump buy order at 55 ISK at another station in the same system
- **THEN** its standing is undercut by 1 order

#### Scenario: Jump range from another system is not counted
- **WHEN** a buy order of 50 ISK has a 5-jump buy order at 55 ISK in a system one jump away
- **THEN** it does not undercut the order

#### Scenario: Station range at another station
- **WHEN** a station-range buy order at 70 ISK is placed at another station in the same system
- **THEN** it does not undercut the order

#### Scenario: Own range is ignored
- **WHEN** a buy order of 50 ISK has a region range and a station-range buy order at 60 ISK sits at another station
- **THEN** its standing is top, as it is compared at its own station only

### Requirement: Gap to the best price
For an order that is not top, the system SHALL determine the gap as the per-unit price difference to the best price among the compared orders: for a sell order its price minus the best price, for a buy order the best price minus its price. It SHALL also determine the gap as a percentage of the best price, rounded half up to one decimal. A top order has no gap.

#### Scenario: Sell order behind
- **WHEN** a sell order of 1,012,500.00 ISK is compared to a best price of 1,000,000.00 ISK
- **THEN** the gap is 12,500.00 ISK, 1.3 % of the best price

#### Scenario: Buy order behind
- **WHEN** a buy order of 95.00 ISK is compared to a best reaching price of 100.00 ISK
- **THEN** the gap is 5.00 ISK, 5.0 % of the best price

### Requirement: No market data
An order SHALL have no standing and no gap while its (region, item) pair has never been refreshed successfully, for a buy order while the solar system of the order is not known from the market data, and always for an order placed in a player-owned structure, because ESI's region market data does not list the orders of structures (apart from ranged buy orders). Market data that is older than its refresh interval SHALL still be used as last known.

#### Scenario: Pair not fetched yet
- **WHEN** an order was just stored and its pair has not been refreshed yet
- **THEN** it has no standing and no gap

#### Scenario: Buy order not in the market data yet
- **WHEN** a buy order was placed after the last refresh of its pair, so the market data does not list it
- **THEN** it has no standing and no gap until the next refresh lists it

#### Scenario: Order in a player structure
- **WHEN** an order placed in a player-owned structure is shown
- **THEN** it has no standing and no gap, however fresh the market data of its pair is

#### Scenario: Stale market data
- **WHEN** the last successful refresh of a pair is an hour old
- **THEN** the orders of that pair are compared with the last known market orders

### Requirement: Undercut volume
For an order that is undercut, the system SHALL determine the undercut volume. This is the sum of the remaining volumes, as last refreshed, of exactly the orders counted as undercutting it. For a buy order, minimum volumes SHALL NOT matter. A top order SHALL have no undercut volume. An order without a standing SHALL have no undercut volume. The copy of the public market orders SHALL keep each order's remaining volume for this.

#### Scenario: Sell order behind two orders
- **WHEN** a sell order of 100 ISK is undercut by orders at 99 ISK with 5 units remaining and at 95 ISK with 20 units remaining at the same station, and an order at 90 ISK with 50 units sits at another station
- **THEN** its undercut volume is 25

#### Scenario: Buy order behind reaching orders
- **WHEN** a buy order of 50 ISK is undercut by a region-range order at 55 ISK with 1,000 units remaining and a station-range order at its station at 52 ISK with 300 units remaining, and a 5-jump order at 60 ISK in another system has 700 units
- **THEN** its undercut volume is 1,300

#### Scenario: Top order
- **WHEN** an order is top
- **THEN** it has no undercut volume

#### Scenario: Volumes follow the refresh
- **WHEN** an undercutting order's remaining volume drops from 20 to 8 and the pair is refreshed
- **THEN** the undercut volume uses 8
