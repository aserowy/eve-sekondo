# wealth-overview Specification

## Purpose

Shows a trader how their wealth splits into wallet ISK, sell orders, buy escrow and inventory, how it developed over time, and which items currently bind capital in their inventory.

## Requirements

### Requirement: Wealth page
A signed-in user SHALL be able to open a wealth page at `/wealth` that shows the wealth over time of their selected characters and the list of their current inventory items. The page SHALL show only data of characters linked to the user's account. Unauthenticated requests SHALL be redirected to the landing page.

#### Scenario: Open the wealth page
- **WHEN** a signed-in user follows the "Wealth" navigation link
- **THEN** the wealth page shows the wealth graph and the inventory list

#### Scenario: Only own data
- **WHEN** two accounts both have characters with assets, and a user of the first account opens the wealth page
- **THEN** no asset or figure of the second account's characters is shown or counted

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests the wealth page
- **THEN** they are redirected to the landing page

### Requirement: Wealth over time graph
The wealth page SHALL show a stacked area graph over the selected time range with four series: wallet, sell orders, buy escrow and inventory, as defined by the wealth-sync capability. The top edge of the stack SHALL be the total wealth. At every point in time, each series SHALL be the sum, over the selected characters, of each character's most recent recorded figure of that series at or before that time. A character with no recorded figure of a series at or before a point SHALL contribute nothing to that series there. A stale character, or one that needs re-login, SHALL keep contributing its last recorded figures. The last point of each series SHALL equal the corresponding current figure. Hovering a point SHALL show its time in EVE time, each series' exact value and the total.

#### Scenario: Components stack up
- **WHEN** at the end of the range the selected characters have 100m ISK in wallets, 30m in personal sell orders, 20m in personal escrow and 50m in inventory
- **THEN** the last point of the graph stacks to 200m and hovering it shows the four values and a total of "200,000,000.00 ISK"

#### Scenario: Components recorded at different times
- **WHEN** a character's wallet was recorded 10 minutes ago and its inventory 50 minutes ago
- **THEN** every later point includes both its latest wallet and its latest inventory figure

#### Scenario: No history in range
- **WHEN** none of the selected characters has any recorded wealth figure at or before the end of the range
- **THEN** the graph says that there is no history yet

### Requirement: Wealth page time range
The wealth graph SHALL cover the time range given by the wealth page's `range` query parameter: `24h`, `7d`, `30d` or `90d`. A missing or unknown value SHALL select `7d`. The page SHALL show a control to switch the range in its filter section. Switching SHALL update the URL and the graph without a full page reload, and SHALL keep the character selection. The range SHALL affect only the graph: the KPI cards, the inventory list and the freshness always show current data. The range SHALL NOT be carried to or from other pages.

#### Scenario: Switch range
- **WHEN** a user with `?chars=<id>` on the wealth page selects 30 days
- **THEN** the URL carries `chars=<id>` and `range=30d`, the graph covers the last 30 days, and the KPI cards and the inventory list are unchanged

#### Scenario: Range is not carried
- **WHEN** a user on `/dashboard?range=30d` follows the "Wealth" menu entry
- **THEN** the wealth graph covers the default 7 days

### Requirement: Inventory list
The wealth page SHALL list the selected characters' current assets with one row per character, item and top-level location, summing the quantities of that item at that location. Each row SHALL show the character, the item, the quantity, the unit price, the value and the location. Unpriced rows SHALL show "—" for unit price and value. The list SHALL be paginated as described by the ui-layout capability, after sorting. An item whose name is not resolved SHALL be labelled with its placeholder (`#<type id>`). An empty list SHALL say that there are no items.

#### Scenario: Same item in two containers of one station
- **WHEN** a character has 100 units of an item in a container and 50 units loose in the same station hangar
- **THEN** the list shows one row for that character, item and station with quantity 150

#### Scenario: Same item at two characters
- **WHEN** two selected characters each hold the same item at the same station
- **THEN** the list shows one row per character

#### Scenario: Unpriced row
- **WHEN** a character holds a blueprint copy
- **THEN** its row shows "—" as unit price and value, and the inventory card's unpriced count includes it

#### Scenario: Many items
- **WHEN** the selected characters hold 300 item rows and the user opens the wealth page
- **THEN** the list shows the first 50 rows in the current sorting and the pagination reads "Page 1 of 6"

### Requirement: Inventory list sorting
The inventory list SHALL be sortable by any of its columns in ascending and descending order by activating the column header. The sort column and direction SHALL be kept in the URL. By default the list SHALL be sorted by value descending. Unpriced rows SHALL come last in both directions when sorting by unit price or value.

#### Scenario: Default sorting
- **WHEN** a user opens the wealth page without sorting parameters
- **THEN** the list is sorted by value, highest first, with unpriced rows last

#### Scenario: Sort by item
- **WHEN** a user activates the item header
- **THEN** the list is sorted by item name ascending, the URL reflects the sorting, and activating it again reverses the order

### Requirement: ISK amounts on the wealth page
Every ISK amount on the wealth page SHALL follow the shortening rule of the orders overview, and graph axes SHALL label ISK amounts in the shortened form. The graph SHALL render without any Content Security Policy violation.

#### Scenario: Shortened value
- **WHEN** a row's value is 1,234,567,890.12 ISK
- **THEN** it reads "1.235b" and hovering it shows "1,234,567,890.12 ISK"

#### Scenario: Graph under the policy
- **WHEN** a user opens the wealth page, switches the range, sorts the list and narrows the selection
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Wealth freshness
The wealth page SHALL show, for each selected character, when its wallet and its assets were last updated and when they are next scheduled, in EVE time (UTC). A character's wallet SHALL be shown as stale when the character needs re-login, when the last successful wallet poll is older than twice the order refresh interval, or when the wallet has never been polled successfully. Its assets SHALL be shown as stale under the same rule with twice one hour. Stale data SHALL still count as last known. A character that needs re-login SHALL link to logging in again.

#### Scenario: Assets not updated for three hours
- **WHEN** a character's last successful assets poll is three hours old
- **THEN** its assets are shown with a stale badge and its last known assets are still listed and counted

#### Scenario: Character needs re-login
- **WHEN** a selected character is marked "needs re-login"
- **THEN** its wallet and assets are shown as stale with a "Log in again" link

### Requirement: Character filter on the wealth page
The wealth page SHALL accept the `chars` parameter and show the filter control as described by the character filter capability. The graph, the inventory list, the totals and the freshness SHALL cover only the selected characters. Changing the selection SHALL keep the user on the wealth page and keep the selected range and sorting.

#### Scenario: Narrow the selection
- **WHEN** a user with two characters on `/wealth?range=30d` deselects the second
- **THEN** the URL is the wealth page with `chars` holding the first character's ID and `range=30d`, and the graph and the list cover only the first character

### Requirement: Live wealth page
The wealth page SHALL re-fetch its KPI cards, graph, inventory list and freshness when a `wealth-updated` or `orders-updated` signal arrives and after the event stream reconnects, keeping the selection, range, sorting, page number and page size, without a full page reload. It SHALL NOT re-fetch because of `market-updated`.

#### Scenario: Wallet changes
- **WHEN** a user has the wealth page open and a wallet poll records a higher balance
- **THEN** the wallet card and the last point of the wallet series show the new balance without the user reloading

#### Scenario: Selection, range and sorting kept
- **WHEN** the wealth page is filtered to one character, set to 24 hours, sorted by item and on page 2, and a `wealth-updated` signal arrives
- **THEN** the refreshed graph and list still cover only that character, the graph covers 24 hours, and the list is sorted by item and still on page 2

### Requirement: Wealth KPIs
The wealth page SHALL show one KPI card for each value of its graph, with the current figure of the selected characters as defined by the wealth-sync capability:
- "Wealth": the total wealth
- "Wallet"
- "Sell orders"
- "Buy escrow"
- "Inventory", whose detail line states how many inventory rows are unpriced when there are any

Each figure SHALL equal the last point of the corresponding graph series, and the total SHALL equal the dashboard's wealth card for the same selection. The figures SHALL NOT depend on the range.

#### Scenario: Cards follow the graph
- **WHEN** the selected characters hold 50,000,000.00 ISK in their wallets, 100,000,000.00 ISK in sell orders, 20,000,000.00 ISK in buy escrow and 30,000,000.00 ISK of priced inventory with 2 unpriced rows
- **THEN** the cards read wealth "200m", wallet "50m", sell orders "100m", buy escrow "20m" and inventory "30m" with 2 unpriced rows

#### Scenario: Range does not change the cards
- **WHEN** a user switches the wealth page from 7 days to 90 days
- **THEN** the KPI cards are unchanged
