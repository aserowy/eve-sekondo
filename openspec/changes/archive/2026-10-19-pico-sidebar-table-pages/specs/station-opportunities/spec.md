## MODIFIED Requirements

### Requirement: Opportunities page
The system SHALL provide one page per opportunity table for signed-in users, at `/opportunities/expansion` (recurring spread expansion), `/opportunities/classic` (classic spread trade), `/opportunities/reversion` (mean reversion) and `/opportunities/trend` (trend). Each accepts the character filter like the other character-scoped views. Unauthenticated requests SHALL be redirected to the landing page. Each table page is laid out as a table page:
- Its sub-menu lists the four tables as text, in the order recurring spread expansion, classic spread trade, mean reversion and trend, without row counts, and marks the current one.
- Its filter section holds the character filter and the opportunity filters, and on its right end the name of the station it analyses and which character's fees are used, or that base rates are used.
- It shows the one table titled after it, with its pagination. The recurring spread expansion page shows, above its table, how many days of hourly data have been collected while fewer than needed are.
- Its freshness section holds the station book's freshness, the market history's freshness and the number of days of own snapshots.

#### Scenario: Open the page
- **WHEN** a signed-in user opens `/opportunities/classic`
- **THEN** they see the sub-menu marking "Classic spread trade", the filter section naming Jita IV - Moon 4 - Caldari Navy Assembly Plant, the classic spread trade table and, below it, the freshness

#### Scenario: Sub-menu counts
- **WHEN** the classic table has 50 rows and the trend table 3
- **THEN** the sub-menu's classic and trend entries show their titles only, without a number

#### Scenario: Switch table
- **WHEN** a user with `?chars=<id>` on the classic page activates "Trend" in the sub-menu
- **THEN** `/opportunities/trend` opens with `chars=<id>`, every table's sorting kept, and page 1 of the trend table

#### Scenario: Counts follow the filters
- **WHEN** a user raises the minimum margin until the trend table shows no rows
- **THEN** the sub-menu's "Trend" entry shows no number

#### Scenario: Signed out
- **WHEN** an unauthenticated user requests `/opportunities/trend`
- **THEN** they are redirected to the landing page

### Requirement: Table rows and columns
Every table SHALL list every item that passes its rules and the filters, paginated as described by the ui-layout capability. Each row SHALL show the item name, the current best buy (labelled as current best buy, not as a suggestion), S, net margin per unit and m, and ISK/day. It SHALL also show the 7-day and 30-day daily volume, σ, T_max, the fill time, the minimum daily volume, and the orders within 1% per side, plus the table's own columns. No table SHALL suggest a buy price. A row SHALL be marked "own buy" or "own sell" when a selected character has a current order of that side for the item at the station. An item SHALL be shown under its name, or as "Type <id>" until its name is known.

#### Scenario: Own sell order
- **WHEN** a selected character has a sell order for an item at Jita 4-4 that the classic table lists
- **THEN** the row is marked "own sell"

#### Scenario: More than 50 matches
- **WHEN** 80 items pass the classic table's rules and the filters
- **THEN** the first page shows the 50 with the highest ISK/day and the second page the other 30

### Requirement: Sorting
Every table SHALL be sorted by ISK/day, highest first, by default. Every column SHALL be sortable on its own, per table, and the sorting of every table SHALL be part of each table page's URL. Pagination SHALL be applied after sorting.

#### Scenario: Sort one table
- **WHEN** a user sorts the trend table by σ and then switches to the classic table in the sub-menu
- **THEN** the classic table keeps its own sorting, the URL still holds the trend sorting, and switching back shows the trend table sorted by σ

### Requirement: Opportunity filters
Every opportunity table page SHALL show one set of opportunity filters, shared by all tables, in its filter section, with these settings:
- minimum net margin m (default 5%)
- minimum ISK/day (default 5,000,000)
- an item name search (case-insensitive substring, default empty)
- capital per item (default 100,000,000 ISK)

Changing a setting SHALL save and apply it without a separate apply control, and SHALL show page 1 of the current table. The settings SHALL be stored per account and kept across sessions. Changing them SHALL re-compute every table, because the order size and the exclusion depend on the capital. A value that is negative, not a number, or a capital of 0 SHALL be rejected with a message in the filter section, and the stored settings SHALL be kept. Saving SHALL need the account's CSRF token.

#### Scenario: Defaults
- **WHEN** an account opens an opportunity table page for the first time
- **THEN** the filters show 5%, 5,000,000 ISK/day, an empty search and 100,000,000 ISK capital

#### Scenario: No apply button
- **WHEN** a signed-in user opens an opportunity table page
- **THEN** the filter section has no apply button, and changing the minimum margin re-computes the table

#### Scenario: Shared across tables
- **WHEN** a user sets the minimum margin to 10% on the classic page and then opens the trend page
- **THEN** the trend page's filters show 10% and its table applies it

#### Scenario: Higher capital
- **WHEN** a user raises the capital per item from 100M to 2B
- **THEN** every table is recomputed with the larger order size, items that can no longer fill in time disappear, and the setting is still 2B after signing in again

#### Scenario: Invalid capital
- **WHEN** a user enters a capital of 0
- **THEN** a message in the filter section says the capital must be positive and the stored settings are unchanged

#### Scenario: Settings stay within the account
- **WHEN** account A changes its minimum margin
- **THEN** the filters of account B are unchanged

### Requirement: Live opportunities
An opportunity table page SHALL re-fetch its freshness and its table when an `opportunities-updated` signal arrives or the stream reconnects. It SHALL keep the character filter, the filters, every table's sorting, the page number, the page size and the scroll position. A detail page SHALL refresh its figures and charts in place on the same signals, keeping its scroll position. If its item no longer appears in its table, the detail page SHALL say so and keep showing the item.

#### Scenario: Book refresh while reading
- **WHEN** a user has the trend page open sorted by σ, on page 2 and scrolled down, and the station book is refreshed
- **THEN** the table shows the new figures, still sorted by σ, on page 2 and at the same scroll position

#### Scenario: Detail page refresh
- **WHEN** a user has a trend detail page open, scrolled to the volume chart, and the station book is refreshed
- **THEN** the figures show the new values, and the page is still scrolled to the volume chart

### Requirement: Opportunity detail page
Every table row SHALL link to a detail page for that item and table at `/opportunities/<table>/<type id>`, where `<table>` is `expansion`, `classic`, `reversion` or `trend`. Clicking a row SHALL navigate to it. The detail page SHALL be for signed-in users only. Unauthenticated requests SHALL be redirected to the landing page, and an unknown table SHALL answer "not found". The detail page accepts the character filter and every table's sorting, carries them in its links, and computes its figures with the fees chosen for the selection and with the account's stored capital per item. The main menu SHALL mark "Opportunities" as current on it.

The detail page is laid out from top to bottom:
- Its section sub-menu starts with a link "← <table title>" back to its table's page, keeping the character filter and every table's sorting. It then lists the page's rows.
- Its filter section holds the character filter, and on its right end the station name and which character's fees are used, or that base rates are used.
- Its first row is a section titled with the item name and the table, holding the fees used, B, S, m, σ, T_max, the fill time, and for each figure how it was computed with this item's numbers.
- After that, each chart of its table is a section alone on a full-width row. For a recurring spread expansion, the next occurrences of the chosen window in EVE time follow as a section of their own.

The charts per table are:
- **Recurring spread expansion**: the spread by hour of day and by weekday over 30 days, and the past peaks of the chosen window with S marked
- **Classic spread trade**: best buy, best sell and spread over 30 days, and the orders and units within 1% per side
- **Mean reversion**: the daily average with SMA 20 and Bollinger bands, and RSI 14
- **Trend**: the daily average with SMA 5 and SMA 20, and the daily volume

When the table does not list the item right now, the page SHALL say so and why, and SHALL still show what is known of the item. An item the station no longer trades SHALL be shown under its name, or as "Type <id>", with that reason.

#### Scenario: Open from a row
- **WHEN** a user with `?chars=<id>` and the trend table sorted by σ clicks a row of the mean reversion table
- **THEN** the browser navigates to `/opportunities/reversion/<type id>` carrying `chars=<id>` and the trend sorting, the page shows the item's figures, its Bollinger chart and its RSI chart each on its own row, and the main menu marks "Opportunities" as current

#### Scenario: Sub-menu of an expansion page
- **WHEN** a user opens the detail page of a row in the recurring spread expansion table
- **THEN** the sub-menu lists "← Recurring spread expansion", "Figures", one entry per chart and "Next occurrences"

#### Scenario: Back to the tables
- **WHEN** a user on a mean reversion detail page with `?chars=<id>` and the trend table sorted by σ activates "← Mean reversion"
- **THEN** `/opportunities/reversion` opens with `chars=<id>` and the trend table's sorting kept

#### Scenario: Fees follow the selection
- **WHEN** a user on a detail page deselects the character whose fees are used
- **THEN** the URL keeps the detail page, the filter section names the fees of the remaining selection, and the figures are recomputed with them

#### Scenario: Item dropped out
- **WHEN** a user opens the classic detail page of an item that the classic table no longer lists because its margin is not positive
- **THEN** the page says that the item is no longer listed in this table and why, and still shows its figures

#### Scenario: Unknown table
- **WHEN** a signed-in user requests `/opportunities/arbitrage/34`
- **THEN** the answer is "not found"

#### Scenario: Signed out
- **WHEN** an unauthenticated user requests `/opportunities/trend/34`
- **THEN** they are redirected to the landing page
