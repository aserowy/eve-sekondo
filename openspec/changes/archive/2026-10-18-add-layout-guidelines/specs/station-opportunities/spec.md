## ADDED Requirements

### Requirement: Opportunity detail page
Every table row SHALL link to a detail page for that item and table at `/opportunities/<table>/<type id>`, where `<table>` is `expansion`, `classic`, `reversion` or `trend`. Clicking a row SHALL navigate to it. The detail page SHALL be for signed-in users only. Unauthenticated requests SHALL be redirected to the landing page, and an unknown table SHALL answer "not found". The detail page accepts the character filter and every table's sorting, carries them in its links, and computes its figures with the fees chosen for the selection and with the account's stored capital per item. The navigation SHALL mark the opportunities page as current on it.

The detail page is a scrolling page:
- Its filter box holds the character filter, and on its right end the station name and which character's fees are used, or that base rates are used.
- Its sub-menu starts with a link "← Opportunities" back to the opportunities page, keeping the character filter and every table's sorting. It then lists the page's rows.
- Its first row is a panel titled with the item name and the table, holding the fees used, B, S, m, σ, T_max, the fill time, and for each figure how it was computed with this item's numbers.
- After that, each chart of its table is a panel alone on a full-width row, at least 16rem high. For a recurring spread expansion, the next occurrences of the chosen window in EVE time follow as a panel of their own.

The charts per table are:
- **Recurring spread expansion**: the spread by hour of day and by weekday over 30 days, and the past peaks of the chosen window with S marked
- **Classic spread trade**: best buy, best sell and spread over 30 days, and the orders and units within 1% per side
- **Mean reversion**: the daily average with SMA 20 and Bollinger bands, and RSI 14
- **Trend**: the daily average with SMA 5 and SMA 20, and the daily volume

When the table does not list the item right now, the page SHALL say so and why, and SHALL still show what is known of the item. An item the station no longer trades SHALL be shown under its name, or as "Type <id>", with that reason.

#### Scenario: Open from a row
- **WHEN** a user with `?chars=<id>` and the trend table sorted by σ clicks a row of the mean reversion table
- **THEN** the browser navigates to `/opportunities/reversion/<type id>` carrying `chars=<id>` and the trend sorting, the page shows the item's figures, its Bollinger chart and its RSI chart each on its own row, and the navigation marks "Opportunities" as current

#### Scenario: Sub-menu of an expansion page
- **WHEN** a user opens the detail page of a row in the recurring spread expansion table
- **THEN** the sub-menu lists "← Opportunities", "Figures", one entry per chart and "Next occurrences"

#### Scenario: Back to the tables
- **WHEN** a user on a detail page with `?chars=<id>` and the trend table sorted by σ activates "← Opportunities"
- **THEN** the opportunities page opens with `chars=<id>` and the trend table sorted by σ

#### Scenario: Fees follow the selection
- **WHEN** a user on a detail page deselects the character whose fees are used
- **THEN** the URL keeps the detail page, the filter box names the fees of the remaining selection, and the figures are recomputed with them

#### Scenario: Item dropped out
- **WHEN** a user opens the classic detail page of an item that the classic table no longer lists because its margin is not positive
- **THEN** the page says that the item is no longer listed in this table and why, and still shows its figures

#### Scenario: Unknown table
- **WHEN** a signed-in user requests `/opportunities/arbitrage/34`
- **THEN** the answer is "not found"

#### Scenario: Signed out
- **WHEN** an unauthenticated user requests `/opportunities/trend/34`
- **THEN** they are redirected to the landing page

## MODIFIED Requirements

### Requirement: Opportunities page
The system SHALL provide a page `/opportunities` for signed-in users. It accepts the character filter like the other character-scoped views. Unauthenticated requests SHALL be redirected to the landing page. The page is a scrolling page:
- Its filter box holds the character filter and the opportunity filters, and on its right end the name of the station it analyses and which character's fees are used, or that base rates are used.
- Its sub-menu lists the four tables, each with the number of rows its table shows.
- Its rows are one table panel each for recurring spread expansion, classic spread trade, mean reversion and trend, in this order.
- Its freshness side column holds the station book's freshness, the market history's freshness and the number of days of own snapshots.

The row counts in the sub-menu SHALL follow the tables when the filters, the character selection or a live refresh change them.

#### Scenario: Open the page
- **WHEN** a signed-in user opens `/opportunities`
- **THEN** they see the filter box naming Jita IV - Moon 4 - Caldari Navy Assembly Plant, the collapsed freshness strip, and the four table panels titled "Recurring spread expansion", "Classic spread trade", "Mean reversion" and "Trend", stacked in this order

#### Scenario: Sub-menu counts
- **WHEN** the classic table shows 50 rows and the trend table 3
- **THEN** the sub-menu's classic entry shows 50 and its trend entry shows 3

#### Scenario: Counts follow the filters
- **WHEN** a user raises the minimum margin until the trend table shows no rows
- **THEN** the sub-menu's trend entry shows 0

#### Scenario: Signed out
- **WHEN** an unauthenticated user requests `/opportunities`
- **THEN** they are redirected to the landing page

### Requirement: Opportunity filters
The page SHALL have one set of opportunity filters for all tables, in its filter box, with these settings:
- minimum net margin m (default 5%)
- minimum ISK/day (default 5,000,000)
- an item name search (case-insensitive substring, default empty)
- capital per item (default 100,000,000 ISK)

Changing a setting SHALL save and apply it without a separate apply control. The settings SHALL be stored per account and kept across sessions. Changing them SHALL re-compute every table, because the order size and the exclusion depend on the capital. A value that is negative, not a number, or a capital of 0 SHALL be rejected with a message in the filter box, and the stored settings SHALL be kept. Saving SHALL need the account's CSRF token.

#### Scenario: Defaults
- **WHEN** an account opens the page for the first time
- **THEN** the filters show 5%, 5,000,000 ISK/day, an empty search and 100,000,000 ISK capital

#### Scenario: No apply button
- **WHEN** a signed-in user opens the opportunities page
- **THEN** the filter box has no apply button, and changing the minimum margin re-computes the tables

#### Scenario: Higher capital
- **WHEN** a user raises the capital per item from 100M to 2B
- **THEN** every table is recomputed with the larger order size, items that can no longer fill in time disappear, and the setting is still 2B after signing in again

#### Scenario: Invalid capital
- **WHEN** a user enters a capital of 0
- **THEN** a message in the filter box says the capital must be positive and the stored settings are unchanged

#### Scenario: Settings stay within the account
- **WHEN** account A changes its minimum margin
- **THEN** the filters of account B are unchanged

### Requirement: Live opportunities
The opportunities page SHALL re-fetch its freshness, its tables and its sub-menu counts when an `opportunities-updated` signal arrives or the stream reconnects. It SHALL keep the character filter, the filters, every table's sorting and the scroll position. A detail page SHALL refresh its figures and charts in place on the same signals, keeping its scroll position. If its item no longer appears in its table, the detail page SHALL say so and keep showing the item.

#### Scenario: Book refresh while reading
- **WHEN** a user has the opportunities page open with the trend table sorted by σ and scrolled to it, and the station book is refreshed
- **THEN** the tables show the new figures, still sorted by σ and at the same scroll position

#### Scenario: Detail page refresh
- **WHEN** a user has a trend detail page open, scrolled to the volume chart, and the station book is refreshed
- **THEN** the figures show the new values, and the page is still scrolled to the volume chart

## REMOVED Requirements

### Requirement: Floating detail window
**Reason**: The layout guidelines show details on a page of their own, never in a floating window. The window held tables and charts, which need the room of a page.
**Migration**: Clicking a row now navigates to the opportunity detail page at `/opportunities/<table>/<type id>`, which shows the same figures and charts.
