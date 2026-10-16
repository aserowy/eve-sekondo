# orders-overview Specification

## Purpose

Shows a trader every active market order of their selected characters in one place, split into sell and buy orders, with the figures and freshness information needed to act on them.

## Requirements

### Requirement: Orders page
A signed-in user SHALL be able to open an orders page that shows the current orders of their selected characters in two separate tables: one for sell orders and one for buy orders. The page SHALL show only orders of characters linked to the user's account. Unauthenticated requests SHALL be redirected to the landing page.

#### Scenario: Sell and buy orders are separated
- **WHEN** a signed-in user whose characters have two sell orders and one buy order opens the orders page
- **THEN** the sell table lists the two sell orders and the buy table lists the one buy order

#### Scenario: No orders
- **WHEN** a signed-in user whose characters have no current orders opens the orders page
- **THEN** both tables say that there are no orders

#### Scenario: Only own orders
- **WHEN** two accounts both have characters with orders, and a user of the first account opens the orders page
- **THEN** no order of the second account's characters is shown

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests the orders page
- **THEN** they are redirected to the landing page

### Requirement: Order columns
Each row SHALL show the character, the item, the location and its region, the price, the remaining and total volume, the date issued and the time left until the order expires. Sell rows SHALL show the order value (price multiplied by remaining volume). Buy rows SHALL show the escrow. Orders placed on behalf of the corporation SHALL carry a "Corp" marker. Dates and times SHALL be shown in EVE time (UTC).

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

### Requirement: Sorting
Each table SHALL be sortable by any of its columns in ascending and descending order by activating the column header. The sort column and direction SHALL be kept in the URL. By default the tables SHALL be sorted by item name ascending.

#### Scenario: Sort by price
- **WHEN** a user activates the price header of the sell table
- **THEN** the sell table is sorted by price, the URL reflects the sorting, and activating the header again reverses the order

#### Scenario: Default sorting
- **WHEN** a user opens the orders page without sorting parameters
- **THEN** both tables are sorted by item name ascending

### Requirement: ISK amounts
ISK amounts of 1,000 or more SHALL be shortened to at most four significant digits with the suffix k (thousand), m (million), b (billion) or t (trillion), and SHALL show the exact amount with thousands separators and two decimals on hover.

#### Scenario: Shortened amount
- **WHEN** an order value of 1,234,567,890.12 ISK is shown
- **THEN** it reads "1.235b" and hovering it shows "1,234,567,890.12 ISK"

#### Scenario: Small amount
- **WHEN** a price of 950.50 ISK is shown
- **THEN** it reads "950.50"

### Requirement: Freshness
The orders page SHALL show, for each selected character, when its orders were last updated and when the next update is scheduled. A character SHALL be shown as stale when it needs re-login, when its last successful poll is older than twice the refresh interval, or when it has never been polled successfully. The orders of a stale character SHALL still be shown as last known. A character that needs re-login SHALL link to logging in again.

#### Scenario: Fresh character
- **WHEN** a character was polled successfully 5 minutes ago with a 20-minute interval
- **THEN** the page shows its last update time and next update time, without a stale badge

#### Scenario: Character needs re-login
- **WHEN** a selected character is marked "needs re-login"
- **THEN** the page shows a stale badge with a "Log in again" link for it, and its last known orders are still listed

#### Scenario: Polling keeps failing
- **WHEN** a character's last successful poll is 45 minutes old with a 20-minute interval
- **THEN** the page shows a stale badge for it

#### Scenario: Newly linked character
- **WHEN** a character was linked and has not been polled successfully yet
- **THEN** the page shows it as waiting for its first update
