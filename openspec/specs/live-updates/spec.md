# live-updates Specification

## Purpose

Pushes change signals to a signed-in user's open pages so they refresh the affected parts on their own when new data arrives, without the user reloading.

## Requirements

### Requirement: Account event stream
The system SHALL offer a signed-in user a server-sent event stream that delivers change signals for their own account only. Signals SHALL name what changed (such as `orders-updated`) and SHALL NOT carry order data. The stream SHALL reject requests without a signed-in session and SHALL send a keep-alive at least every 30 seconds.

#### Scenario: Signal after a poll
- **WHEN** a signed-in user has the orders page open and a poll of one of their characters completes
- **THEN** their stream delivers an `orders-updated` signal

#### Scenario: Signals stay within the account
- **WHEN** a poll completes for a character of account A
- **THEN** a user signed in to account B receives no signal for it

#### Scenario: Unauthenticated stream request
- **WHEN** a request without a signed-in session opens the event stream
- **THEN** no stream is opened and no signal is delivered

#### Scenario: Signals carry no data
- **WHEN** an `orders-updated` signal is delivered
- **THEN** it contains no order, price or character data beyond the signal name

### Requirement: Stream ends with the session
The system SHALL close an open event stream within 60 seconds after its session is signed out or expires, and SHALL NOT deliver signals over it afterwards.

#### Scenario: Sign out in another tab
- **WHEN** a user with an open orders page signs out in another tab
- **THEN** the open stream is closed within 60 seconds and reconnecting does not open a new one

### Requirement: Pages refresh on signals
Pages that show data affected by a signal SHALL re-fetch the affected parts when the signal arrives, keeping the current character filter and table sorting, without a full page reload. When the stream drops, the page SHALL reconnect and re-fetch the affected parts once it is connected again.

#### Scenario: Table refreshes after a poll
- **WHEN** a user has the orders page open and a poll changes the price of one of their orders
- **THEN** the page shows the new price without the user reloading

#### Scenario: Filter and sorting are kept
- **WHEN** the orders page is filtered to one character and sorted by price, and an `orders-updated` signal arrives
- **THEN** the refreshed tables are still filtered to that character and sorted by price

#### Scenario: Missed signals after a reconnect
- **WHEN** the stream was disconnected while a poll changed an order, and the stream reconnects
- **THEN** the page shows the changed order without the user reloading

### Requirement: Market signal
The system SHALL deliver a `market-updated` signal over the event stream of an account when a market refresh changes the standing, the gap or the undercut volume of at least one current order of that account. The signal SHALL name only what changed and SHALL NOT carry order or price data. Accounts none of whose orders changed standing, gap or undercut volume SHALL NOT receive it, and no account SHALL receive it for another account's orders. The orders page SHALL re-fetch its tables when the signal arrives, keeping the current character filter and table sorting, without a full page reload. The dashboard SHALL NOT re-fetch because of it.

#### Scenario: Standing changes after a market refresh
- **WHEN** a user has the orders page open with a sell order that is top, and a market refresh brings in another sell order at a lower price at the same station
- **THEN** their stream delivers a `market-updated` signal and the order's standing changes to "Undercut 1" without the user reloading

#### Scenario: Only the undercut volume changes
- **WHEN** a user's sell order is undercut by one order, and a market refresh shows that order with fewer units remaining at the same price
- **THEN** their stream delivers a `market-updated` signal and the undercut volume shows the new number without the user reloading

#### Scenario: Nothing changed
- **WHEN** a market refresh brings in orders that change no standing, gap or undercut volume of the user's orders
- **THEN** no `market-updated` signal is delivered to them

#### Scenario: Signals stay within the account
- **WHEN** a refresh changes the standing of an order of account A
- **THEN** a user signed in to account B receives no `market-updated` signal for it

#### Scenario: Filter and sorting are kept
- **WHEN** the orders page is filtered to one character and sorted by standing, and a `market-updated` signal arrives
- **THEN** the refreshed tables are still filtered to that character and sorted by standing

#### Scenario: Dashboard ignores it
- **WHEN** a `market-updated` signal arrives while the dashboard is open
- **THEN** the dashboard does not re-fetch

#### Scenario: Missed signal after a reconnect
- **WHEN** the stream was disconnected while a market refresh changed a standing, and the stream reconnects
- **THEN** the orders page shows the changed standing without the user reloading

### Requirement: Wealth signal
The system SHALL deliver a `wealth-updated` signal over the event stream of an account when a wallet poll or an assets poll of one of its characters succeeds, and when a price refresh succeeds for every account that has at least one character with current assets. The signal SHALL name only what changed and SHALL NOT carry balance, asset or price data. No account SHALL receive it for another account's characters. The wealth page and the wealth parts of the dashboard SHALL re-fetch when it arrives. The orders page SHALL NOT re-fetch because of it.

#### Scenario: Signal after a wallet poll
- **WHEN** a signed-in user has the wealth page open and a wallet poll of one of their characters succeeds
- **THEN** their stream delivers a `wealth-updated` signal

#### Scenario: Signals stay within the account
- **WHEN** an assets poll completes for a character of account A
- **THEN** a user signed in to account B receives no `wealth-updated` signal for it

#### Scenario: Signal carries no data
- **WHEN** a `wealth-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

#### Scenario: Orders page ignores it
- **WHEN** a `wealth-updated` signal arrives while the orders page is open
- **THEN** the orders page does not re-fetch

### Requirement: Fees signal
The system SHALL deliver a `fees-updated` signal over the event stream of an account when a skills or standings poll of one of its characters, or the lookup of a station owner where one of its characters has orders, changes a sales tax, broker fee or minimum margin of that character. The signal SHALL name only what changed and SHALL NOT carry skill, standing or fee data. No account SHALL receive it for another account's characters. The orders page SHALL re-fetch its character entries and its tables when it arrives, because the margins depend on the fees, keeping the current selection and sorting. The dashboard and the wealth page SHALL NOT re-fetch because of it.

#### Scenario: Skill trained
- **WHEN** a user has the orders page open and a skills poll of one of their characters reports a higher Accounting level
- **THEN** their stream delivers a `fees-updated` signal and the character's sales tax shows the new rate without the user reloading

#### Scenario: Margins follow the fees
- **WHEN** a user has the orders page open with a sell order that has a margin, and a skills poll of its character reports a higher Accounting level
- **THEN** the order's margin shows the value with the lower sales tax without the user reloading

#### Scenario: Nothing changed
- **WHEN** a skills poll reports the same levels as before
- **THEN** no `fees-updated` signal is delivered

#### Scenario: Signals stay within the account
- **WHEN** a standings poll changes a fee of a character of account A
- **THEN** a user signed in to account B receives no `fees-updated` signal for it

#### Scenario: Signal carries no data
- **WHEN** a `fees-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

### Requirement: Purchase signal
The system SHALL deliver an `orders-updated` signal over the event stream of an account when a transactions poll of one of its characters stores at least one new purchase lot. A poll that stores no new purchase SHALL NOT deliver it. The orders page SHALL then show the margins with the new purchases.

#### Scenario: New purchase arrives
- **WHEN** a user has the orders page open with a sell order whose margin reads "—" because its cost is not fully known, and a transactions poll stores the missing purchase
- **THEN** their stream delivers an `orders-updated` signal and the order's margin is shown without the user reloading

#### Scenario: Nothing new
- **WHEN** a transactions poll returns only transactions that are already stored
- **THEN** no signal is delivered for it

### Requirement: Refreshes keep the scroll position
When a page re-fetches a part of itself because a signal arrived, or because the stream reconnected, every panel in that part that scrolls on its own SHALL keep its vertical and horizontal scroll position. If the refreshed content is shorter than the kept position, the panel SHALL show its end. Changes the user makes to the view (the character filter, a table's sorting, the time range, or navigating to another page) SHALL show the resulting view scrolled to the top. Keeping the position SHALL NOT change which part is re-fetched, the character filter, the sorting or the URL.

#### Scenario: Sell table keeps its place after a poll
- **WHEN** a user with 200 sell orders has scrolled the sell table halfway down, and a poll of their character delivers an `orders-updated` signal
- **THEN** the refreshed sell table is at the same scroll position, and the buy table is at its own previous position

#### Scenario: Market signal keeps the place
- **WHEN** a user has scrolled the sell table down, and a `market-updated` signal arrives
- **THEN** the refreshed sell table is at the same scroll position

#### Scenario: Fewer rows after a refresh
- **WHEN** a user has scrolled the sell table to its end, and a poll removes most of the orders so the table is shorter than the old position
- **THEN** the refreshed sell table shows its last row

#### Scenario: Sorting starts at the top
- **WHEN** a user has scrolled the sell table down and clicks the Price header
- **THEN** the sorted sell table is scrolled to the top

#### Scenario: Filter starts at the top
- **WHEN** a user has scrolled the sell table down and deselects a character in the filter
- **THEN** the filtered sell table is scrolled to the top

#### Scenario: Dashboard breakdown keeps its place
- **WHEN** a user with twenty characters has scrolled the dashboard breakdown down, and an `orders-updated` signal arrives
- **THEN** the refreshed breakdown is at the same scroll position

#### Scenario: Wealth items keep their place
- **WHEN** a user has scrolled the wealth items table down, and a `wealth-updated` signal arrives
- **THEN** the refreshed items table is at the same scroll position

#### Scenario: Freshness column keeps its place
- **WHEN** a user with twenty characters has scrolled the orders page's freshness column down, and a `fees-updated` signal arrives
- **THEN** the refreshed freshness column is at the same scroll position

### Requirement: Opportunities signal
The system SHALL deliver an `opportunities-updated` signal over the event stream of every signed-in account after each successful station book refresh and after each history run that stored new history. The station market is public, so every account receives it. The signal SHALL name only what changed and SHALL NOT carry price, order or item data. The opportunities page SHALL re-fetch its freshness, its tables and its sub-menu counts when the signal arrives, and an open opportunity detail page SHALL re-fetch its figures and charts. Both SHALL keep the character filter, the filters, the sorting and the scroll position. The dashboard, orders and wealth pages SHALL NOT re-fetch because of it.

#### Scenario: Signal after a book refresh
- **WHEN** two accounts have the opportunities page open and the station book is refreshed
- **THEN** both streams deliver an `opportunities-updated` signal and both pages show the new figures without reloading

#### Scenario: Opportunities page follows the signal
- **WHEN** a user has the opportunities page open with the trend table sorted by σ, and the station book is refreshed
- **THEN** the freshness, the four tables and the sub-menu counts show the new figures without reloading, the trend table is still sorted by σ, and the character filter and the opportunity filters are unchanged

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
- **WHEN** an `opportunities-updated` signal arrives while the orders page is open
- **THEN** the orders page does not re-fetch

#### Scenario: Opportunity tables keep their place
- **WHEN** a user has scrolled the opportunities page down to the trend table and an `opportunities-updated` signal arrives
- **THEN** the refreshed page is at the same scroll position
