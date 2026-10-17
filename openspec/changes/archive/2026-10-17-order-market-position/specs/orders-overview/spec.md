# Spec Delta

## MODIFIED Requirements

### Requirement: Order columns
Each row SHALL show the character, the item, the price, the standing, the gap to the best price, the remaining and total volume, the date issued, the time left until the order expires, and finally the location and its region. Sell rows SHALL show the order value (price multiplied by remaining volume). Buy rows SHALL show the escrow. Orders placed on behalf of the corporation SHALL carry a "Corp" marker. Dates and times SHALL be shown in EVE time (UTC). The standing SHALL read "Top" for a top order, styled with the success style of the page's CSS framework, and "Undercut N" for an undercut order, with N the number of undercutting orders, styled with its error style. The gap SHALL be empty for a top order and otherwise show the per-unit price difference to the best price followed by its percentage in brackets, with the difference shortened as described below and its exact value on hover. Both SHALL read "—" while the order has no standing, as described by the market-position capability.

#### Scenario: Sell row
- **WHEN** a sell order of 10 remaining out of 20 units at 1,500,000.00 ISK is shown
- **THEN** its row shows volume "10 / 20" and value 15,000,000.00 ISK (shortened as described below)

#### Scenario: Buy row
- **WHEN** a buy order with 2,500,000.00 ISK in escrow is shown
- **THEN** its row shows the escrow of 2,500,000.00 ISK (shortened as described below)

#### Scenario: Time left
- **WHEN** an order issued 80 days and 2 hours ago with a 90-day duration is shown
- **THEN** its time left reads 9 days 22 hours (rounded down to whole hours)

#### Scenario: Corporation order
- **WHEN** an order placed on behalf of the corporation is shown
- **THEN** its row carries a "Corp" marker

#### Scenario: Location and region come last
- **WHEN** either table is shown
- **THEN** its last two columns are the location and the region, after the time left

#### Scenario: Top order
- **WHEN** a sell order that is the lowest price at its station is shown
- **THEN** its standing reads "Top" in the success style and its gap is empty

#### Scenario: Undercut order
- **WHEN** a sell order of 1,012,500.00 ISK is undercut by three orders and the best price is 1,000,000.00 ISK
- **THEN** its standing reads "Undercut 3" in the error style and its gap reads "12.5k (1.3%)", showing "12,500.00 ISK" on hover

#### Scenario: No market data
- **WHEN** an order whose market data has not been fetched yet is shown
- **THEN** its standing and its gap read "—"

### Requirement: Sorting
Each table SHALL be sortable by any of its columns in ascending and descending order by activating the column header. The sort column and direction SHALL be kept in the URL. By default the tables SHALL be sorted by item name ascending. The standing column SHALL sort by its text, with the number of an "Undercut N" read as a number: "Top" first, then "Undercut 1", "Undercut 2" and so on up, in ascending order. The gap column SHALL sort by its price difference, a top order counting as no difference. Orders without a standing SHALL come last in both directions of both columns.

#### Scenario: Sort by price
- **WHEN** a user activates the price header of the sell table
- **THEN** the sell table is sorted by price, the URL reflects the sorting, and activating the header again reverses the order

#### Scenario: Default sorting
- **WHEN** a user opens the orders page without sorting parameters
- **THEN** both tables are sorted by item name ascending

#### Scenario: Sort by standing
- **WHEN** a table holds orders that are top, undercut 3 times, undercut 12 times, undercut once, and without market data, and its standing header is activated
- **THEN** the order is top, undercut 1, undercut 3, undercut 12, and the one without market data last

#### Scenario: Sort by standing descending
- **WHEN** the standing header is activated again
- **THEN** the order is undercut 12, undercut 3, undercut 1, top, and the one without market data still last

#### Scenario: Sort by gap
- **WHEN** a table holds a top order, an order 5.00 ISK behind, an order 80.00 ISK behind and one without market data, and its gap header is activated
- **THEN** the order is the top one, 5.00 ISK, 80.00 ISK, and the one without market data last
