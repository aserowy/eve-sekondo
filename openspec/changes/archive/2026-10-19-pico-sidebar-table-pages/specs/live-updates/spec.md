## MODIFIED Requirements

### Requirement: Fees signal
The system SHALL deliver a `fees-updated` signal over the event stream of an account when a skills or standings poll of one of its characters, the lookup of a station owner where one of its characters has orders or is docked, or a location poll that changes a character's fee location changes a sales tax, broker fee or minimum margin of that character. The signal SHALL name only what changed and SHALL NOT carry skill, standing, location or fee data. No account SHALL receive it for another account's characters. The sell and the buy orders page SHALL re-fetch their KPI cards and their table when it arrives, because the fee KPIs and the margins depend on the fees, keeping the current selection, sorting, page number and page size. The dashboard and the wealth page SHALL NOT re-fetch because of it.

#### Scenario: Skill trained
- **WHEN** a user has the sell orders page open and a skills poll of one of their characters reports a higher Accounting level
- **THEN** their stream delivers a `fees-updated` signal and the character's sales tax card shows the new rate without the user reloading

#### Scenario: Margins follow the fees
- **WHEN** a user has the sell orders page open with a sell order that has a margin, and a skills poll of its character reports a higher Accounting level
- **THEN** the order's margin and the net margin card show the values with the lower sales tax without the user reloading

#### Scenario: Character docks elsewhere
- **WHEN** a user has the buy orders page open and a location poll reports one of their characters docked at a different station with different fees
- **THEN** their stream delivers a `fees-updated` signal and the character's fee cards name the new station and show its fees without the user reloading

#### Scenario: Nothing changed
- **WHEN** a skills poll reports the same levels as before
- **THEN** no `fees-updated` signal is delivered

#### Scenario: Signals stay within the account
- **WHEN** a standings poll changes a fee of a character of account A
- **THEN** a user signed in to account B receives no `fees-updated` signal for it

#### Scenario: Signal carries no data
- **WHEN** a `fees-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

### Requirement: Refreshes keep the scroll position
When a page re-fetches a part of itself because a signal arrived, or because the stream reconnected, the page SHALL keep its vertical scroll position, every table that scrolls sideways SHALL keep its horizontal scroll position, and a paginated table SHALL stay on its page number and page size. If the refreshed table has fewer pages than the kept page number, it SHALL show its last page. If the refreshed content is shorter than the kept position, the page SHALL show its end. Changes the user makes to the view (the character filter, a table's sorting, the time range, the page number, the page size, or navigating to another page) SHALL show the resulting view with the table's top visible. Keeping the position SHALL NOT change which part is re-fetched, the character filter, the sorting or the URL.

#### Scenario: Sell table keeps its place after a poll
- **WHEN** a user with 200 sell orders is on page 2 of the sell orders page, scrolled halfway down, and a poll of their character delivers an `orders-updated` signal
- **THEN** the refreshed sell table is still on page 2 and the page is at the same scroll position

#### Scenario: Market signal keeps the place
- **WHEN** a user has scrolled the sell orders page down, and a `market-updated` signal arrives
- **THEN** the refreshed page is at the same scroll position

#### Scenario: Fewer rows after a refresh
- **WHEN** a user is on page 3 of the sell table, and a poll removes most of the orders so only one page remains
- **THEN** the refreshed sell table shows page 1, its last page

#### Scenario: Sorting starts at the top
- **WHEN** a user has scrolled the sell orders page down and clicks the Price header
- **THEN** the sorted sell table shows page 1 with its top visible

#### Scenario: Filter starts at the top
- **WHEN** a user has scrolled the sell orders page down and deselects a character in the filter
- **THEN** the filtered sell table shows page 1 with its top visible

#### Scenario: Wealth items keep their place
- **WHEN** a user has scrolled the wealth page down to the items table, and a `wealth-updated` signal arrives
- **THEN** the refreshed page is at the same scroll position

#### Scenario: Dashboard breakdown keeps its place
- **WHEN** a user with twenty characters has scrolled the dashboard down to the breakdown, and an `orders-updated` signal arrives
- **THEN** the refreshed dashboard is at the same scroll position

#### Scenario: Freshness column keeps its place
- **WHEN** a user with twenty characters has scrolled the sell orders page down to its freshness section, and a `fees-updated` signal arrives
- **THEN** the refreshed page is at the same scroll position and the freshness section is still in view

#### Scenario: Wide table keeps its sideways place
- **WHEN** a user has scrolled an opportunity table sideways, and an `opportunities-updated` signal arrives
- **THEN** the refreshed table is at the same horizontal scroll position

### Requirement: Opportunities signal
The system SHALL deliver an `opportunities-updated` signal over the event stream of every signed-in account after each successful station book refresh and after each history run that stored new history. The station market is public, so every account receives it. The signal SHALL name only what changed and SHALL NOT carry price, order or item data. An open opportunity table page SHALL re-fetch its freshness and its table when the signal arrives, and an open opportunity detail page SHALL re-fetch its figures and charts. Both SHALL keep the character filter, the filters, the sorting, the page number and the scroll position. The dashboard, orders and wealth pages SHALL NOT re-fetch because of it.

#### Scenario: Signal after a book refresh
- **WHEN** two accounts have an opportunity table page open and the station book is refreshed
- **THEN** both streams deliver an `opportunities-updated` signal and both pages show the new figures without reloading

#### Scenario: Opportunities page follows the signal
- **WHEN** a user has the trend page open sorted by σ, and the station book is refreshed
- **THEN** the freshness and the trend table show the new figures without reloading, the trend table is still sorted by σ, and the character filter and the opportunity filters are unchanged

#### Scenario: Detail page follows the signal
- **WHEN** a user has an opportunity detail page open and the station book is refreshed
- **THEN** the detail page shows the new figures without reloading

#### Scenario: Failed refresh
- **WHEN** a station book refresh fails
- **THEN** no `opportunities-updated` signal is delivered

#### Scenario: Signal carries no data
- **WHEN** an `opportunities-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

#### Scenario: Orders page ignores it
- **WHEN** an `opportunities-updated` signal arrives while the sell orders page is open
- **THEN** the sell orders page does not re-fetch

#### Scenario: Opportunity tables keep their place
- **WHEN** a user has scrolled the trend page down and an `opportunities-updated` signal arrives
- **THEN** the refreshed page is at the same scroll position
