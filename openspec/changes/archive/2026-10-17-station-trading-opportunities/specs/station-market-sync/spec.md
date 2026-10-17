# Spec Delta

## Purpose

Keeps a public, account-independent record of the market at the built-in trading station: the current order book, a top-of-book snapshot per refresh with hourly and daily rollups, and ESI's daily market history, all over a rolling 30-day window, as the data the opportunity analyses read.

## ADDED Requirements

### Requirement: Built-in trading station
The system SHALL know exactly one trading station: Jita IV - Moon 4 - Caldari Navy Assembly Plant (station 60003760) in The Forge (region 10000002). All station market data SHALL be stored by region and station, so another station can be added without changing data already stored. There SHALL be no way to choose another station.

#### Scenario: Only Jita 4-4
- **WHEN** the station market is synced
- **THEN** only the order book of The Forge is read, and only orders placed at station 60003760 are kept

### Requirement: Station order book refresh
The system SHALL read every page of the public order book of the station's region, for all items and both sides. It SHALL do this again once ESI's cache expiry for the previous answer has passed, and at most every 5 minutes. From each complete answer it SHALL replace the current station book with the orders placed at the station. A refresh where any page fails SHALL keep the previous book, record nothing, and be retried with the same backoff and ESI etiquette as the other polls. The book SHALL be public: it belongs to no account and is read once for all accounts.

#### Scenario: Successful refresh
- **WHEN** a refresh reads all pages of The Forge's order book
- **THEN** the station book holds exactly the orders of that answer whose location is the station

#### Scenario: Orders in other stations are dropped
- **WHEN** the order book contains a buy order at another station in Jita with region range
- **THEN** that order is not part of the station book

#### Scenario: One page fails
- **WHEN** page 3 of the order book answers with a server error
- **THEN** the previous station book is kept, no snapshot is recorded, and the refresh is retried later

#### Scenario: Cache not expired
- **WHEN** the previous answer's cache expiry lies 4 minutes in the future
- **THEN** no new refresh starts before it has passed

### Requirement: Tradable items at the station
An item SHALL count as tradable at the station when the current station book holds at least one buy order and at least one sell order for it. Only tradable items SHALL get snapshots, history and analyses.

#### Scenario: Sell orders only
- **WHEN** the station book has sell orders but no buy order for an item
- **THEN** the item is not tradable, and no snapshot or history is recorded for it

### Requirement: Top-of-book snapshot
For every tradable item, each successful refresh SHALL record one snapshot with the refresh time. Each of these SHALL be its own value, and no value SHALL be derivable only from another snapshot:
- the best buy price and the best sell price
- the spread: (best sell − best buy) / best buy
- per side, the units and the number of orders priced within 1% of that side's best price
- per side, the total units in the station book
- per side, the units taken since the previous refresh: the drop in remaining volume of orders present in both refreshes, ignoring orders that appeared or disappeared

#### Scenario: Snapshot content
- **WHEN** the station book holds, for an item, buy orders at 100.00 (500 units) and 99.50 (300 units), and sell orders at 110.00 (200 units) and 120.00 (1000 units)
- **THEN** its snapshot has best buy 100.00, best sell 110.00, spread 10%, 2 buy orders with 800 units within 1%, 1 sell order with 200 units within 1%, 800 buy units and 1200 sell units in total

#### Scenario: Units taken
- **WHEN** a sell order had 200 units remaining at the previous refresh and has 150 now, and another sell order disappeared
- **THEN** the snapshot counts 50 units taken from the sell side

### Requirement: Hourly and daily rollups
The system SHALL keep hourly and UTC-daily rollups of the snapshots of each item. A rollup SHALL keep the minimum, maximum and average of best buy, best sell and spread in separate values. It SHALL keep the averages of the units and orders within 1% and of the total units, and the sums of the units taken, per side and in separate values. It SHALL also keep the number of snapshots it covers. Rollups SHALL be complete for a period once the period has ended.

#### Scenario: Hourly maximum and average kept apart
- **WHEN** an hour holds snapshots of an item with best sell 110, 130 and 120
- **THEN** that hour's rollup has a minimum best sell of 110, a maximum of 130 and an average of 120

### Requirement: Daily market history
Once a day, after ESI has published the previous day, the system SHALL read ESI's market history of the station's region for every tradable item. It SHALL keep, per item and day, the average, highest and lowest price, the traded volume and the order count. An item that becomes tradable SHALL get its history on the next run. A failed request SHALL keep what is stored for the item and SHALL be retried later the same day. The requests SHALL be spread out so they follow the existing ESI rate limits.

#### Scenario: New tradable item
- **WHEN** an item first becomes tradable at the station during the day
- **THEN** its last 30 days of history are stored after the next history run

#### Scenario: History request fails
- **WHEN** the history request for one item fails
- **THEN** that item keeps its stored history, the other items are updated, and the item is retried later

### Requirement: Daily figures from own data first
The daily figures the analyses read SHALL be made per item and UTC day as follows:
- **Low and high**: the minimum best buy and maximum best sell from that day's rollup, when the day is covered, meaning it has snapshots in at least 20 of its 24 hours. Otherwise the history's lowest and highest.
- **Spread**: the maximum spread of a covered day. Otherwise (highest − lowest) / lowest from history.
- **Average price and traded volume**: always from the history.

Each daily figure SHALL record whether it came from own data or from history.

#### Scenario: Covered day
- **WHEN** a day has snapshots of an item in all 24 hours and history also reports that day
- **THEN** that day's low and high come from the snapshots and the volume comes from the history

#### Scenario: Gap in own data
- **WHEN** the app was not running for most of a day
- **THEN** that day's low, high and spread come from the history and are marked as such

### Requirement: Thirty-day window
The system SHALL keep station snapshots, rollups and history for 30 days and SHALL delete anything older. Snapshots older than one day SHALL be stored compressed without losing any value.

#### Scenario: Old data is dropped
- **WHEN** a snapshot is older than 30 days
- **THEN** it is no longer stored, and the analyses do not read it

#### Scenario: Compression loses nothing
- **WHEN** a snapshot from three days ago is read
- **THEN** every value it was recorded with is returned unchanged

### Requirement: Station sync freshness
The system SHALL remember when the station book was last refreshed successfully, when the last history run finished, and the last error of each. It SHALL also remember how many days of own snapshots exist. The Opportunities page SHALL show these.

#### Scenario: Stale book
- **WHEN** the last successful book refresh was 40 minutes ago
- **THEN** the Opportunities page shows the time of the last refresh and marks it as stale
