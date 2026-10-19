## MODIFIED Requirements

### Requirement: Trades time range
The trades table SHALL list only the transactions dated within the time range given by the trades page's `range` query parameter: `24h`, `7d`, `30d` or `90d`, counted back from now. A missing or unknown value SHALL select `7d`. The control to switch the range SHALL be in the page's filter section. Switching SHALL update the URL, the KPI cards and the table without a full page reload, SHALL keep the character selection, and SHALL show page 1. The range SHALL NOT be carried to or from other pages. The freshness SHALL NOT depend on the range.

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
Between the filter section and the table, the trades page SHALL show KPI cards across all transactions in the range, not only those of the current table page:
- "Bought": the sum of the values of the purchases
- "Sold": the sum of the values of the sales
- "Margin": the sum of the margins of the sales that have one

When some sales in the range have no margin, hovering the margin card SHALL show how many sales it leaves out. A negative margin SHALL be styled with the error style. The table SHALL have no totals row.

#### Scenario: Totals
- **WHEN** the range lists a purchase worth 1,000.00 ISK and two sales worth 1,500.00 and 600.00 ISK, with margins of 428.23 ISK and none
- **THEN** the cards read "1k" bought, "2.1k" sold and a margin of "428.23", and hovering the margin shows its exact value and that 1 sale is left out

#### Scenario: Totals span every table page
- **WHEN** the range holds 80 transactions and the table shows the first 50
- **THEN** the bought and sold cards sum all 80 transactions

### Requirement: Live trades page
The trades page SHALL re-fetch its KPI cards, its table and its freshness when a `trades-updated` signal arrives, and its KPI cards and table when a `fees-updated` signal arrives, keeping the character selection, the range, the page number, the page size and the scroll position.

#### Scenario: New sale appears
- **WHEN** a user has the trades page open on page 1 and a transactions poll stores a new sale of one of their characters
- **THEN** the sale appears in the table and in the sold card without the user reloading

#### Scenario: Page kept
- **WHEN** a user is on page 2 of the trades table and a `trades-updated` signal arrives
- **THEN** the refreshed table is still on page 2

#### Scenario: Margin follows the fees
- **WHEN** a user has the trades page open with a sale that has a margin, and a skills poll of its character reports a higher Accounting level
- **THEN** the sale's margin shows the value with the lower sales tax without the user reloading

## ADDED Requirements

### Requirement: Trades pagination
The trades table SHALL be paginated as described by the ui-layout capability, newest transaction first.

#### Scenario: Older trades on the next page
- **WHEN** the range holds 120 transactions and the user goes to page 3
- **THEN** the table shows the 101st to 120th newest transaction
