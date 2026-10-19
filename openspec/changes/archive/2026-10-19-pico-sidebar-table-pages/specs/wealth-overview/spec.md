## MODIFIED Requirements

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

### Requirement: Live wealth page
The wealth page SHALL re-fetch its KPI cards, graph, inventory list and freshness when a `wealth-updated` or `orders-updated` signal arrives and after the event stream reconnects, keeping the selection, range, sorting, page number and page size, without a full page reload. It SHALL NOT re-fetch because of `market-updated`.

#### Scenario: Wallet changes
- **WHEN** a user has the wealth page open and a wallet poll records a higher balance
- **THEN** the wallet card and the last point of the wallet series show the new balance without the user reloading

#### Scenario: Selection, range and sorting kept
- **WHEN** the wealth page is filtered to one character, set to 24 hours, sorted by item and on page 2, and a `wealth-updated` signal arrives
- **THEN** the refreshed graph and list still cover only that character, the graph covers 24 hours, and the list is sorted by item and still on page 2

## ADDED Requirements

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
