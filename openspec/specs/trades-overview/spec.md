# trades-overview Specification

## Purpose

Shows a trader what their characters actually bought and sold on the market, as recorded in their wallet transactions, with the quantity, the value and what each sale earned after costs and fees.

## Requirements

### Requirement: Trades page
The system SHALL provide a trades page at `/trades` for signed-in users. It SHALL show one table listing the personal wallet transactions of the selected characters that the trade-margin capability stores, purchases and sales alike. Corporation transactions SHALL NOT be listed. Each transaction SHALL be one row, newest first, with the transaction ID breaking ties, newest ID first. Unauthenticated requests SHALL be redirected to the landing page.

#### Scenario: Buys and sells listed
- **WHEN** a character bought 10 units of Tritanium on day 1 and sold 4 of them on day 2
- **THEN** the trades table shows the sale in the first row and the purchase in the second row

#### Scenario: Corporation purchase not listed
- **WHEN** a poll returned a purchase made for the corporation
- **THEN** the trades table does not list it

#### Scenario: Nothing traded
- **WHEN** none of the selected characters has a stored transaction in the selected range
- **THEN** the table panel shows that there are no trades in the range

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests `/trades`
- **THEN** they are redirected to the landing page

### Requirement: Trade columns
Each row SHALL show, in this order:
- the date and time in EVE time (UTC)
- the character
- the item
- the location
- "Buy" or "Sell"
- the quantity, as a whole number with thousands separators
- the unit price
- the value
- the margin

The value SHALL be the quantity multiplied by the unit price, before fees. The margin SHALL be the margin of the sale as described by the trade-margin capability. A purchase SHALL show an empty margin. A sale without a margin SHALL read "—". When the reason is that the purchase cost is not fully known, hovering it SHALL show how many of the sale's units have a known cost. Hovering a margin SHALL show its exact value and the average purchase cost per unit. A negative margin SHALL be styled with the error style. ISK amounts SHALL follow the shortening rule of the orders overview.

#### Scenario: Sale row
- **WHEN** Alice Alpha sold 10 Tritanium at 150.00 ISK in Jita IV - Moon 4 - Caldari Navy Assembly Plant on 2026-10-18 at 12:34 EVE time, and the sale's margin is 428.23 ISK
- **THEN** the row reads "2026-10-18 12:34", "Alice Alpha", "Tritanium", the station name, "Sell", "10", "150.00", "1.5k" and "428.23"

#### Scenario: Purchase row
- **WHEN** a character bought 1,000 units at 2,500,000.00 ISK
- **THEN** the row reads "Buy", "1,000", "2.5m" and "2.5b", and its margin is empty

#### Scenario: Partly known cost
- **WHEN** a sale of 10 units has a known cost for 4 of them
- **THEN** its margin reads "—" and hovering it shows that the cost is known for 4 of 10 units

#### Scenario: Negative margin
- **WHEN** a sale's margin is −120.50 ISK
- **THEN** the margin shows "-120.50" in the error style

### Requirement: Trades time range
The trades table SHALL list only the transactions dated within the time range given by the trades page's `range` query parameter: `24h`, `7d`, `30d` or `90d`, counted back from now. A missing or unknown value SHALL select `7d`. The control to switch the range SHALL be in the table panel's title. Switching SHALL update the URL and the table without a full page reload, and SHALL keep the character selection. The range SHALL NOT be carried to or from other pages. The freshness SHALL NOT depend on the range.

#### Scenario: Default range
- **WHEN** a user opens the trades page without a `range` parameter, and a character sold something 3 days ago and 10 days ago
- **THEN** only the sale of 3 days ago is listed and the range control marks 7 days

#### Scenario: Switch range
- **WHEN** a user with `?chars=<id>` selects 30 days
- **THEN** the URL carries `chars=<id>` and `range=30d`, and the sale of 10 days ago is listed too

#### Scenario: Unknown range
- **WHEN** a user requests the trades page with `?range=1y`
- **THEN** the table covers the last 7 days

### Requirement: Trades totals
Below the rows, the table SHALL show a totals row across the listed transactions with:
- the bought value, the sum of the values of the purchases
- the sold value, the sum of the values of the sales
- the margin, the sum of the margins of the sales that have one

When some listed sales have no margin, hovering the summed margin SHALL show how many sales it leaves out. The totals row SHALL stay visible while the table scrolls.

#### Scenario: Totals
- **WHEN** the range lists a purchase worth 1,000.00 ISK and two sales worth 1,500.00 and 600.00 ISK, with margins of 428.23 ISK and none
- **THEN** the totals read "1k" bought, "2.1k" sold and a margin of "428.23", and hovering the margin shows its exact value and that 1 sale is left out

### Requirement: Character filter on the trades page
The trades page SHALL accept the `chars` parameter and show the filter control as described by the character filter capability. The table, the totals and the freshness SHALL cover only the selected characters. Changing the selection SHALL keep the user on the trades page and keep the selected range.

#### Scenario: Narrow the selection
- **WHEN** a user with two trading characters deselects one of them on the trades page with `range=30d`
- **THEN** the table and totals show only the other character's transactions over 30 days

### Requirement: Trades freshness
The trades page SHALL show, for each selected character, when its wallet transactions were last updated and when they are next scheduled, in EVE time (UTC). A character's transactions SHALL be shown as stale when the character needs re-login, when the last successful transactions poll is older than two hours, or when the transactions have never been polled successfully. The transactions of a stale character SHALL still be listed as last known. A character that needs re-login SHALL link to logging in again.

#### Scenario: Transactions not updated for three hours
- **WHEN** a character's last successful transactions poll is three hours old
- **THEN** its transactions are shown with a stale badge and its stored transactions are still listed

#### Scenario: Character needs re-login
- **WHEN** a selected character is marked "needs re-login"
- **THEN** it is shown as stale with a "Log in again" link

### Requirement: Live trades page
The trades page SHALL re-fetch its table and its freshness when a `trades-updated` signal arrives, and its table when a `fees-updated` signal arrives, keeping the character selection, the range and the table's scroll position.

#### Scenario: New sale appears
- **WHEN** a user has the trades page open and a transactions poll stores a new sale of one of their characters
- **THEN** the sale appears in the table without the user reloading

#### Scenario: Margin follows the fees
- **WHEN** a user has the trades page open with a sale that has a margin, and a skills poll of its character reports a higher Accounting level
- **THEN** the sale's margin shows the value with the lower sales tax without the user reloading
