## MODIFIED Requirements

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

## ADDED Requirements

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

## REMOVED Requirements

### Requirement: Market fees per character
**Reason**: Fees no longer live in the freshness; they become fee KPI cards at the character's current station.
**Migration**: See "Fee KPIs"; the fee location is chosen by the character-location capability.
