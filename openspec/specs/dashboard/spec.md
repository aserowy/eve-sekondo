# dashboard Specification

## Purpose

Shows a trader at a glance how much ISK their selected characters have listed in sell orders and locked in buy-order escrow, in total and per character, together with how current those figures are.

## Requirements

### Requirement: Dashboard totals
The dashboard SHALL show, for the selected characters' current orders, the total sell order value (the sum over sell orders of price multiplied by remaining volume), the total buy order escrow (the sum of the escrow of buy orders, counting a missing escrow as zero), the number of sell orders and the number of buy orders. The figures SHALL include only orders of characters linked to the user's account. Unauthenticated requests SHALL be redirected to the landing page.

#### Scenario: Totals across characters
- **WHEN** a user's first character has a sell order of 10 remaining units at 1,000,000.00 ISK and a buy order with 2,000,000.00 ISK escrow, and their second character has a sell order of 5 remaining units at 4,000,000.00 ISK
- **THEN** the dashboard shows a sell value of 30,000,000.00 ISK, an escrow of 2,000,000.00 ISK, 2 sell orders and 1 buy order

#### Scenario: No orders
- **WHEN** a user whose selected characters have no current orders opens the dashboard
- **THEN** the dashboard shows a sell value of 0, an escrow of 0, 0 sell orders and 0 buy orders

#### Scenario: Only own orders
- **WHEN** two accounts both have characters with orders, and a user of the first account opens the dashboard
- **THEN** no order of the second account's characters counts toward any figure

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests the dashboard
- **THEN** they are redirected to the landing page

### Requirement: Corporation share
Orders placed on behalf of the corporation SHALL count toward the totals and the per-character figures. The sell value and escrow totals SHALL each also show the part of the amount that comes from corporation orders. When there are no corporation orders, the share SHALL NOT be shown.

#### Scenario: Corporation sell order
- **WHEN** the selected characters have a personal sell order worth 700,000,000.00 ISK and a corporation sell order worth 300,000,000.00 ISK
- **THEN** the sell value total reads 1,000,000,000.00 ISK and states that 300,000,000.00 ISK of it is from corporation orders

#### Scenario: No corporation orders
- **WHEN** none of the selected characters' orders is a corporation order
- **THEN** neither total shows a corporation share

### Requirement: Per-character breakdown
The dashboard SHALL show a table with one row for each selected character, including characters without orders. Each row SHALL show the character's sell value, escrow, number of sell orders and number of buy orders. Rows SHALL be listed in the order the characters were linked to the account.

#### Scenario: Breakdown rows
- **WHEN** a user with two characters, of which only the first has orders, opens the dashboard
- **THEN** the breakdown lists both characters, the first with its figures and the second with zero for every figure

#### Scenario: Row figures add up to the totals
- **WHEN** the dashboard shows several selected characters
- **THEN** the sums of the rows' sell values, escrows and order counts equal the totals

### Requirement: Freshness in the breakdown
Each breakdown row SHALL show when the character's orders were last updated and when the next update is scheduled, in EVE time (UTC). A character SHALL be shown as stale under the same rule as on the orders page: when it needs re-login, when its last successful poll is older than twice the refresh interval, or when it has never been polled successfully. A stale character's last known orders SHALL still count toward its row and the totals. A character that needs re-login SHALL link to logging in again. A character that has never been polled successfully SHALL be shown as waiting for its first update.

#### Scenario: Fresh character
- **WHEN** a character was polled successfully 5 minutes ago with a 20-minute interval
- **THEN** its row shows its last and next update time without a stale badge

#### Scenario: Character needs re-login
- **WHEN** a selected character with last known orders is marked "needs re-login"
- **THEN** its row shows a stale badge and a "Log in again" link, and its last known orders still count toward its row and the totals

#### Scenario: Polling keeps failing
- **WHEN** a character's last successful poll is 45 minutes old with a 20-minute interval
- **THEN** its row shows a stale badge

#### Scenario: Newly linked character
- **WHEN** a character was linked and has not been polled successfully yet
- **THEN** its row shows it as waiting for its first update

### Requirement: ISK amounts on the dashboard
Every ISK amount on the dashboard SHALL follow the shortening rule of the orders overview: amounts of 1,000 or more are shortened to at most four significant digits with the suffix k, m, b or t, and show the exact amount with thousands separators and two decimals on hover.

#### Scenario: Shortened total
- **WHEN** the sell value total is 1,234,567,890.12 ISK
- **THEN** it reads "1.235b" and hovering it shows "1,234,567,890.12 ISK"

### Requirement: Value and order count over time
The dashboard SHALL show two graphs over the selected time range. The first shows the total sell value and the total buy escrow as two lines. The second shows the number of sell orders and buy orders as two lines. At every point in time, each line SHALL be the sum, over the selected characters, of that character's figures as recorded by its most recent successful poll at or before that time. A character with no successful poll at or before a point SHALL contribute nothing to it. A character that is stale or needs re-login SHALL keep contributing its last recorded figures. The last point of each line SHALL equal the corresponding current total shown in the tiles.

#### Scenario: Value develops over polls
- **WHEN** a character's sell orders were worth 10,000,000.00 ISK at a poll two hours ago and 15,000,000.00 ISK at a poll one hour ago, and the range is 24 hours
- **THEN** the sell value line shows 10,000,000.00 ISK between those polls and 15,000,000.00 ISK from the later poll to now

#### Scenario: All orders close
- **WHEN** a character's last poll returned no orders after earlier polls returned orders
- **THEN** both lines for that character's contribution drop to zero from that poll on

#### Scenario: Characters polled at different times
- **WHEN** two selected characters are polled at different times within the range
- **THEN** at every point both characters contribute their most recent figures, so the line never dips because one character's poll fell into a different moment

#### Scenario: Stale character is carried forward
- **WHEN** a selected character needs re-login and its last successful poll was 10 hours ago with 5,000,000.00 ISK in sell orders
- **THEN** the sell value line includes 5,000,000.00 ISK for it up to now

#### Scenario: No history in range
- **WHEN** none of the selected characters has a successful poll at or before the end of the range
- **THEN** each over-time graph says that there is no history yet

### Requirement: Time range
The over-time graphs SHALL cover the time range given by the `range` query parameter: `24h`, `7d`, `30d` or `90d`. A missing or unknown value SHALL select `7d`. The over-time graphs are the value graph, the order count graph and the wealth graph. The dashboard SHALL show one control to switch the range, which applies to all of them at once. Switching SHALL update the URL and all over-time graphs without a full page reload, and SHALL keep the character selection. The range SHALL NOT affect the tiles, the breakdown, the per-character split or the top items, which always show current data. The range SHALL NOT be carried to other pages.

#### Scenario: Default range
- **WHEN** a user opens the dashboard without a `range` parameter
- **THEN** the over-time graphs cover the last 7 days and the range control marks 7 days

#### Scenario: Switch range
- **WHEN** a user with `?chars=<id>` selects 30 days in the range control
- **THEN** the URL carries `chars=<id>` and `range=30d`, and the value, order count and wealth graphs all cover the last 30 days

#### Scenario: Unknown range
- **WHEN** a user requests the dashboard with `?range=1y`
- **THEN** the over-time graphs cover the last 7 days

### Requirement: Per-character split graph
The dashboard SHALL show a bar graph with, for each selected character in the breakdown's order, one bar for its current sell value and one for its current escrow. The values SHALL be the same as in the breakdown.

#### Scenario: Two characters
- **WHEN** the first selected character has 30,000,000.00 ISK in sell orders and none in escrow, and the second has 2,000,000.00 ISK in escrow
- **THEN** the graph shows the first character with a 30m sell bar and a zero escrow bar, and the second with a zero sell bar and a 2m escrow bar

### Requirement: Top items graph
The dashboard SHALL show the items with the most ISK in the selected characters' current orders, as two bar graphs. One shows the ten items with the highest total sell value. The other shows the ten items with the highest total escrow. Each item's figure SHALL be summed over all selected characters and locations. Items SHALL be ordered by figure descending, with ties ordered by item name. Fewer than ten bars SHALL be shown when fewer items have orders. An item whose name is not resolved SHALL be labelled with its placeholder (`#<type id>`).

#### Scenario: Item listed by two characters
- **WHEN** two selected characters each have sell orders for the same item worth 4,000,000.00 ISK and 6,000,000.00 ISK
- **THEN** the sell top-items graph shows that item once with 10,000,000.00 ISK

#### Scenario: More than ten items
- **WHEN** the selected characters have sell orders for twelve different items
- **THEN** the sell top-items graph shows the ten with the highest value, highest first

#### Scenario: No buy orders
- **WHEN** the selected characters have no buy orders
- **THEN** the escrow top-items graph says that there are no buy orders

### Requirement: Graph values on hover
Graph axes SHALL label ISK amounts with the shortened form of the ISK amount rule. Hovering a point or bar SHALL show its label (time in EVE time, character or item), the series name, and the exact value. Exact values are ISK with thousands separators and two decimals, or the whole number for order counts. The graphs SHALL render without any Content Security Policy violation.

#### Scenario: Hover a point
- **WHEN** a user hovers the sell value line at a point where the value is 1,234,567,890.12 ISK
- **THEN** a tooltip shows the point's time in EVE time, "Sell value" and "1,234,567,890.12 ISK"

#### Scenario: Graphs under the policy
- **WHEN** a user opens the dashboard, switches the range and narrows the selection
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Character filter on the dashboard
The dashboard SHALL accept the `chars` parameter and show the filter control as described by the character filter capability. Every figure, row, freshness indicator and graph SHALL cover only the selected characters. Changing the selection SHALL keep the user on the dashboard and keep the selected time range.

#### Scenario: Narrow the selection
- **WHEN** a user with two characters on `/dashboard?range=30d` deselects the second in the dashboard's filter control
- **THEN** the URL is the dashboard with `chars` holding the first character's ID and `range=30d`, the breakdown lists only the first character, and the totals and graphs cover only its orders

#### Scenario: Selection carried to the orders page
- **WHEN** a user with a narrowed selection on the dashboard follows the "Orders" menu entry
- **THEN** the sell orders page opens with the same selection

### Requirement: Live dashboard
The dashboard SHALL re-fetch its totals, breakdown and graphs, including the wealth tile and the wealth graph, when an `orders-updated` or `wealth-updated` signal arrives and after the event stream reconnects. It SHALL keep the current selection and time range, and SHALL NOT do a full page reload.

#### Scenario: Totals refresh after a poll
- **WHEN** a user has the dashboard open and a poll raises the price of one of their sell orders
- **THEN** the sell value total, the character's row, the per-character split, the last point of the sell value line and the wealth tile show the new value without the user reloading

#### Scenario: Wealth refreshes after a wallet poll
- **WHEN** a user has the dashboard open and a wallet poll records a higher balance
- **THEN** the wealth tile and the last point of the wealth graph show the new total without the user reloading

#### Scenario: Selection and range kept by a live refresh
- **WHEN** the dashboard is filtered to one character with the 24-hour range and an `orders-updated` signal arrives
- **THEN** the refreshed totals, breakdown and graphs still cover only that character, and the over-time graphs still cover 24 hours

### Requirement: Wealth tile
The dashboard SHALL show a "Wealth" KPI card with the current total wealth of the selected characters, as defined by the wealth-sync capability, and SHALL link it to the wealth page with the current character selection. The amount SHALL follow the ISK amount rule of the dashboard. Every dashboard tile, including the sell value, buy escrow, sell order and buy order tiles, SHALL be a KPI card as described by the ui-layout capability.

#### Scenario: Wealth across characters
- **WHEN** the first selected character's wealth is 200,000,000.00 ISK and the second's is 50,000,000.00 ISK
- **THEN** the wealth card reads "250m" and hovering it shows "250,000,000.00 ISK"

#### Scenario: Tile links to the wealth page
- **WHEN** a user with `?chars=<id>` activates the wealth card
- **THEN** the wealth page opens with `chars=<id>`

#### Scenario: Tiles are cards
- **WHEN** a signed-in user opens the dashboard
- **THEN** the sell value, buy escrow, wealth, sell order and buy order figures are each a KPI card of the same width, each wider than it is high

### Requirement: Wealth over time on the dashboard
The dashboard SHALL show a graph of total wealth over the selected time range as a single line. At every point, the line SHALL equal the top edge of the wealth page's graph for the same selection and range. Its last point SHALL equal the wealth tile. When there is no wealth history in the range, the graph SHALL say that there is no history yet.

#### Scenario: Wealth develops over time
- **WHEN** a character's wallet was 10,000,000.00 ISK at a poll two hours ago and 15,000,000.00 ISK at a poll one hour ago, with nothing else recorded, and the range is 24 hours
- **THEN** the wealth line shows 10,000,000.00 ISK between those polls and 15,000,000.00 ISK from the later poll to now
