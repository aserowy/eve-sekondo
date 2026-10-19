# orders-overview Specification

## Purpose

Shows a trader every active market order of their selected characters in one place, split into sell and buy orders, with the figures and freshness information needed to act on them.

## Requirements

### Requirement: Orders page
A signed-in user SHALL be able to open a sell orders page at `/orders/sell` and a buy orders page at `/orders/buy`. The sell orders page SHALL show the current sell orders of their selected characters in one table, and the buy orders page their current buy orders in one table. The pages SHALL show only orders of characters linked to the user's account. Both pages SHALL keep each other's sorting in their sub-menu links. Unauthenticated requests SHALL be redirected to the landing page.

#### Scenario: Sell and buy orders are separated
- **WHEN** a signed-in user whose characters have two sell orders and one buy order opens the sell orders page and then follows "Buy" in the sub-menu
- **THEN** the sell orders page lists the two sell orders and the buy orders page lists the one buy order

#### Scenario: No orders
- **WHEN** a signed-in user whose characters have no current orders opens the sell orders page
- **THEN** the table says that there are no sell orders and no pagination is shown

#### Scenario: Only own orders
- **WHEN** two accounts both have characters with orders, and a user of the first account opens the sell and the buy orders page
- **THEN** no order of the second account's characters is shown or counted

#### Scenario: Sorting kept between the pages
- **WHEN** a user sorts the buy table by price, switches to "Sell" and back to "Buy"
- **THEN** the buy table is still sorted by price

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests the sell or the buy orders page
- **THEN** they are redirected to the landing page

### Requirement: Order columns
Each order SHALL be shown on two lines. The first line SHALL show, under the table's column headers, the item, the price, the standing, the gap to the best price ("Behind by"), the undercut volume, the remaining and total volume, and the order amount: for sell orders the order value (price multiplied by remaining volume) followed by the margin, for buy orders the escrow; and finally the time left until the order expires. The second line SHALL be a single dimmed line spanning the whole table width, without a header, showing the date issued, the character, a "Corp" marker for orders placed on behalf of the corporation, the location and its region, in that order. Hovering either line SHALL highlight both lines of the order. Dates and times SHALL be shown in EVE time (UTC). The standing SHALL read "Top" for a top order, styled with the page's success style, and "Undercut N" for an undercut order, with N the number of undercutting orders, styled with its error style. The gap SHALL be empty for a top order and otherwise show the per-unit price difference to the best price followed by its percentage in brackets, with the difference shortened as described below and its exact value on hover. The undercut volume SHALL be empty for a top order and otherwise show the undercut volume, as described by the market-position capability, as a whole number with thousands separators. The gap and the undercut volume SHALL read "—" while the order has no standing, as described by the market-position capability. The margin SHALL show the margin of the sell order, as described by the trade-margin capability, shortened as described below. Hovering it SHALL show its exact value and the average purchase cost per unit. A negative margin SHALL be styled with the error style. The margin SHALL read "—" while the order has no margin. When the reason is that the purchase cost is not fully known, hovering it SHALL show how many of the order's units have a known cost.

#### Scenario: Sell row
- **WHEN** a sell order of 10 remaining out of 20 units at 1,500,000.00 ISK is shown
- **THEN** its first line shows volume "10 / 20" and value 15,000,000.00 ISK (shortened as described below)

#### Scenario: Buy row
- **WHEN** a buy order with 2,500,000.00 ISK in escrow is shown
- **THEN** its first line shows the escrow of 2,500,000.00 ISK (shortened as described below)

#### Scenario: Time left
- **WHEN** an order issued 80 days and 2 hours ago with a 90-day duration is shown
- **THEN** its time left reads 9 days 22 hours (rounded down to whole hours)

#### Scenario: Column headers
- **WHEN** the sell table is shown
- **THEN** its headers are Item, Price, Standing, Behind by, Undercut volume, Volume, Value, Margin and Time left, in that order

#### Scenario: Buy column headers
- **WHEN** the buy table is shown
- **THEN** its headers are Item, Price, Standing, Behind by, Undercut volume, Volume, Escrow and Time left, in that order

#### Scenario: Location and region come last
- **WHEN** a sell order of Alice Alpha issued on 2026-10-01 12:00 at "Jita IV - Moon 4 - Caldari Navy Assembly Plant" in The Forge is shown
- **THEN** its second line reads the issue date and time, "Alice Alpha", "Jita IV - Moon 4 - Caldari Navy Assembly Plant" and "The Forge", in that order, with location and region last

#### Scenario: Corporation order
- **WHEN** an order placed on behalf of the corporation is shown
- **THEN** its second line carries a "Corp" marker next to the character

#### Scenario: Top order
- **WHEN** a sell order that is the lowest price at its station is shown
- **THEN** its standing reads "Top" in the success style, and its gap and undercut volume are empty

#### Scenario: Undercut order
- **WHEN** a sell order of 1,012,500.00 ISK is undercut by three orders with 1,250 units remaining between them, and the best price is 1,000,000.00 ISK
- **THEN** its standing reads "Undercut 3" in the error style, its gap reads "12.5k (1.3%)" and shows "12,500.00 ISK" on hover, and its undercut volume reads "1,250"

#### Scenario: Undercut buy order
- **WHEN** a buy order undercut by two reaching orders with 300 and 1,000 units remaining is shown
- **THEN** its undercut volume reads "1,300"

#### Scenario: No market data
- **WHEN** an order whose market data has not been fetched yet is shown
- **THEN** its standing, its gap and its undercut volume read "—"

#### Scenario: Margin shown
- **WHEN** a sell order with a margin of 2,142,337.50 ISK and a purchase cost of 106.67 ISK per unit is shown
- **THEN** its margin reads "2.142m", and hovering it shows "2,142,337.50 ISK" and the cost per unit

#### Scenario: Negative margin
- **WHEN** a sell order with a margin of −1,500.00 ISK is shown
- **THEN** its margin reads "-1.5k" in the error style

#### Scenario: Cost not fully known
- **WHEN** a sell order whose purchase cost is known for 5 of 10 units is shown
- **THEN** its margin reads "—", and hovering it shows "cost known for 5 of 10 units"

#### Scenario: Buy table has no margin
- **WHEN** the buy table is shown
- **THEN** it has no margin column

### Requirement: Sorting
Each table SHALL be sortable by any of its column headers in ascending and descending order by activating the header. The values of the details line (issued, character, location, region) SHALL NOT be sortable. The sort column and direction SHALL be kept in the URL. By default the tables SHALL be sorted by item name ascending. A URL that names an unknown sort column, including one of the details line's values, SHALL sort by the default. The standing column SHALL sort by its text, with the number of an "Undercut N" read as a number: "Top" first, then "Undercut 1", "Undercut 2" and so on up, in ascending order. The gap column SHALL sort by its price difference, a top order counting as no difference. The undercut volume column SHALL sort by its number, a top order counting as 0. Orders without a standing SHALL come last in both directions of the standing, gap and undercut volume columns. The margin column SHALL sort by the margin, and orders without a margin SHALL come last in both directions.

#### Scenario: Sort by price
- **WHEN** a user activates the price header of the sell table
- **THEN** the sell table is sorted by price, the URL reflects the sorting, and activating the header again reverses the order

#### Scenario: Default sorting
- **WHEN** a user opens the orders page without sorting parameters
- **THEN** both tables are sorted by item name ascending

#### Scenario: Former sort column in the URL
- **WHEN** a user opens the orders page with `sort=location&dir=desc`
- **THEN** the sell table is sorted by item name ascending

#### Scenario: Sort by standing
- **WHEN** a table holds orders that are top, undercut 3 times, undercut 12 times, undercut once, and without market data, and its standing header is activated
- **THEN** the order is top, undercut 1, undercut 3, undercut 12, and the one without market data last

#### Scenario: Sort by standing descending
- **WHEN** the standing header is activated again
- **THEN** the order is undercut 12, undercut 3, undercut 1, top, and the one without market data still last

#### Scenario: Sort by gap
- **WHEN** a table holds a top order, an order 5.00 ISK behind, an order 80.00 ISK behind and one without market data, and its gap header is activated
- **THEN** the order is the top one, 5.00 ISK, 80.00 ISK, and the one without market data last

#### Scenario: Sort by undercut volume
- **WHEN** a table holds a top order, an order with 300 units in front, one with 20 units in front and one without market data, and its undercut volume header is activated
- **THEN** the order is the top one, 20, 300, and the one without market data last

#### Scenario: Sort by margin descending
- **WHEN** the sell table holds orders with margins of 500.00 ISK, −20.00 ISK and 9,000.00 ISK, and one without margin, and its margin header is activated twice
- **THEN** the order is 9,000.00 ISK, 500.00 ISK, −20.00 ISK, and the one without margin last

### Requirement: ISK amounts
ISK amounts of 1,000 or more SHALL be shortened to at most four significant digits with the suffix k (thousand), m (million), b (billion) or t (trillion), and SHALL show the exact amount with thousands separators and two decimals on hover.

#### Scenario: Shortened amount
- **WHEN** an order value of 1,234,567,890.12 ISK is shown
- **THEN** it reads "1.235b" and hovering it shows "1,234,567,890.12 ISK"

#### Scenario: Small amount
- **WHEN** a price of 950.50 ISK is shown
- **THEN** it reads "950.50"

### Requirement: Freshness
The sell and the buy orders page SHALL each show in their freshness section, for each selected character, when its orders were last updated and when the next update is scheduled. A character SHALL be shown as stale when it needs re-login, when its last successful poll is older than twice the refresh interval, or when it has never been polled successfully. The orders of a stale character SHALL still be shown as last known. A character that needs re-login SHALL link to logging in again. The freshness SHALL NOT list fees.

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

### Requirement: Order KPIs
The sell orders page SHALL show these KPI cards, over all sell orders of the selected characters:
- "Sell orders": the number of sell orders
- "Sell value": the sum of the order values, with the corporation share as its detail line when there is one
- "Net margin": the sum of the margins of the sell orders that have one, with its detail line showing that sum as a percentage of the known cost of those orders, rounded half up to two decimals

The buy orders page SHALL show:
- "Buy orders": the number of buy orders
- "Buy escrow": the sum of the escrows, with the corporation share as its detail line when there is one

Hovering the net margin SHALL show its exact value and, when some sell orders have no margin, how many it leaves out. A negative net margin SHALL be styled with the error style. When no sell order has a margin, the net margin SHALL read "—". The figures SHALL equal the corresponding dashboard totals for the same selection.

#### Scenario: Sell KPIs
- **WHEN** the selected characters have three sell orders worth 10,000,000.00 ISK in total, two of them with margins of 1,500,000.00 ISK and 500,000.00 ISK on a known cost of 8,000,000.00 ISK, and one without a margin
- **THEN** the cards read 3 sell orders, a sell value of "10m" and a net margin of "2m" with "25.00%", and hovering the net margin shows "2,000,000.00 ISK" and that 1 order is left out

#### Scenario: Negative net margin
- **WHEN** the margins of the selected characters' sell orders sum to −1,500.00 ISK
- **THEN** the net margin card reads "-1.5k" in the error style

#### Scenario: Buy KPIs
- **WHEN** the selected characters have two buy orders with 2,000,000.00 ISK escrow in total
- **THEN** the buy orders page's cards read 2 buy orders and a buy escrow of "2m"

### Requirement: Fee KPIs
The sell and the buy orders page SHALL each show, after the order KPIs and for every selected character in the order the characters were linked, three KPI cards: "Sales tax", "Broker fee" and "Min. margin". Each card's detail line SHALL name the character and the fee location chosen by the character-location capability. The values SHALL be the character's fees at that location, as defined by the fee-sync capability. Rates SHALL be shown as percentages rounded half up to two decimals. Hovering a rate SHALL show it rounded half up to four decimals together with its inputs: the Accounting level for the sales tax, and the Broker Relations level, the faction standing and the corporation standing for an NPC station broker fee. In a player-owned structure the broker fee SHALL read "0.5% + owner fee" and the minimum margin "—". A figure that is not known yet SHALL read "—". When the fee location falls back to Jita 4-4 because the character has not granted the location scope, the cards SHALL show a "Log in again" link that lets the user grant it.

#### Scenario: NPC station
- **WHEN** a character with Accounting 5 and Broker Relations 5 is docked at an NPC station where its faction standing is 2.0 and its corporation standing is 1.5
- **THEN** its cards read sales tax "3.38%", broker fee "1.41%" and min. margin "6.51%", each naming the character and that station

#### Scenario: Inputs on hover
- **WHEN** a user hovers that broker fee
- **THEN** it shows "1.4100%", Broker Relations 5, faction standing 2.00 and corporation standing 1.50

#### Scenario: Player structure
- **WHEN** a character is docked in a player-owned structure
- **THEN** its broker fee card reads "0.5% + owner fee" and its min. margin card "—"

#### Scenario: Skills not polled yet
- **WHEN** a newly linked character has not had its skills polled yet
- **THEN** its three cards read "—"

#### Scenario: Location scope missing
- **WHEN** a character linked before the location scope was introduced is selected
- **THEN** its cards name Jita IV - Moon 4 - Caldari Navy Assembly Plant and show a "Log in again" link

#### Scenario: Only selected characters
- **WHEN** the selection is narrowed to one of two characters
- **THEN** only that character's three fee cards are shown

### Requirement: Order pagination
The sell table and the buy table SHALL be paginated as described by the ui-layout capability. Each page SHALL keep its own page number and page size; the sub-menu links SHALL NOT carry them.

#### Scenario: Second page of sell orders
- **WHEN** a user with 70 sell orders sorted by price opens page 2 of the sell orders page
- **THEN** the table shows the 51st to 70th order by price
