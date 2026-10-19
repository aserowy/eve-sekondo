# station-opportunities Specification

## Purpose

Shows a signed-in trader what is worth buying for station trading at the built-in trading station, through a set of established trading analyses. Each analysis has its own table, all tables share the same fee, liquidity and holding-time rules, and a detail window explains each result.

## Requirements

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

### Requirement: Fees used for the analyses
The analyses SHALL use the broker fee b and sales tax t of one selected character at the station: the one with the lowest minimum margin (1 + b) / (1 − b − t) − 1 there. A selected character whose fees at the station are not known yet SHALL count with the base rates b = 3% and t = 7.5%. The page SHALL say which character's fees are used, or that base rates are used.

#### Scenario: Two characters
- **WHEN** the selection holds a character with a minimum margin of 6.5% at the station and one with 9.1%
- **THEN** every analysis uses the fees of the first one, and the page names it

#### Scenario: No fee data yet
- **WHEN** the only selected character's skills have never been polled
- **THEN** the analyses use a 3% broker fee and 7.5% sales tax, and the page says that base rates are used

### Requirement: Valid prices
Every price the analyses suggest SHALL be valid in EVE: it has at most four significant digits and at most two decimals. "One tick below x" SHALL mean the highest valid price below x. "Rounded down to a valid price" SHALL mean the highest valid price that is not above x.

#### Scenario: One tick below
- **WHEN** a target is one tick below 1,234,000.00
- **THEN** the suggested price is 1,233,000.00

#### Scenario: Small prices
- **WHEN** a target is one tick below 9.99
- **THEN** the suggested price is 9.98

### Requirement: Shared figures per row
For each item in a table, with B the current best buy at the station and S the table's sell price, the system SHALL compute:
- **Cost** per unit: c = B × (1 + b). **Revenue** per unit: r = S × (1 − b − t).
- **Net margin**: r − c per unit, and m = (r − c) / c.
- **Order size**: U = ⌊capital per item / c⌋.
- **Market share** per side: 1 / (n + 1), with n the number of orders of that side within 1% of its best price in the latest snapshot.
- **Daily volume** V: the average daily traded volume of the last 7 days from history. The page also shows the 30-day average.
- **Buy side ratio** ρ: of the units taken from either side over the last 30 days of snapshots, the fraction taken from buy orders. 0.5 when no units were taken.
- **Daily capture**: q_buy = share_buy × V × ρ and q_sell = share_sell × V × (1 − ρ).
- **Fill time**: T_fill = U / q_buy + U / q_sell days.
- **Volatility** σ: the sample standard deviation of the daily log returns of the history average price over the last 30 days.
- **Allowed holding time**: T_max = min(3, (m / (2σ))²) days.
- **Minimum daily volume**: the V at which T_fill equals T_max.
- **ISK/day**: (r − c) × U / T_fill.

#### Scenario: Worked example
- **WHEN** b = 1.41%, t = 3.375%, B = 100.00, S = 115.00, the capital per item is 1,000,000, V = 20,000, ρ = 0.5, there are 3 buy and 1 sell orders within 1% of the best prices, and σ = 2%
- **THEN** c = 101.41, r ≈ 109.50, m ≈ 7.97%, U = 9,860, q_buy = 2,500, q_sell = 5,000, T_fill ≈ 5.92 days, T_max = 3 days ((0.0797 / 0.04)² ≈ 3.97 capped at 3), and the item is excluded because T_fill > T_max

### Requirement: Hard exclusion
An item SHALL NOT appear in any table when its net margin is not positive or U is below 1. It SHALL NOT appear when there are fewer than 10 daily returns for σ, or when V is 0. It SHALL also not appear when T_fill exceeds T_max: in other words, when its daily volume is below the minimum daily volume its volatility and margin require. This SHALL apply to every table before the filters and sorting.

#### Scenario: Volatile and thin
- **WHEN** an item has a net margin of 8% but its fill time is 4 days and its T_max is 1.2 days
- **THEN** it appears in no table

#### Scenario: Not enough history
- **WHEN** an item has history for only 6 days
- **THEN** it appears in no table

### Requirement: Recurring spread expansion table
The system SHALL find recurring spread expansions per item. M is the median spread of the item over the last 30 days. A window SHALL count as recurring when:
- **hour of day**: on at least 60% of the days among the last 14 that have data for that hour (and at least 7 such days), the maximum spread in that hour was at least 1.5 × M
- **weekday × hour**: that held in at least 3 of the last 4 weeks
- **weekday** (only while fewer than 7 days of own snapshots exist): the daily spread figure was at least 1.5 × M in at least 3 of the last 4 weeks, where M is the median of the daily spread figures

The peaks of a window SHALL be the maximum best sell of each past occurrence in which the expansion happened. For a weekday window that is the daily high. The sell price S SHALL be one tick below the 25th percentile of the peaks. When an item has several recurring windows, the row SHALL use the one with the highest S. The row SHALL show the window and the start of its next occurrence in EVE time (UTC). While fewer than 7 days of own snapshots exist, the table SHALL say that hourly patterns are still being collected, with the number of days so far.

#### Scenario: Daily evening expansion
- **WHEN** for 10 of the last 14 days an item's spread reached at least 1.5 × its median between 18:00 and 19:00 UTC, with best sell peaks of 120, 122, 125, 126, 128, 130, 131, 133, 135 and 140
- **THEN** it is listed with the window "daily 18:00", S one tick below the 25th percentile of those peaks, and the next 18:00 UTC as the next start

#### Scenario: Too irregular
- **WHEN** an item's spread reached 1.5 × its median at 18:00 on only 5 of the last 14 days
- **THEN** the 18:00 window is not recurring

#### Scenario: First days after the release
- **WHEN** own snapshots cover 3 days
- **THEN** the table lists only weekday windows and says "Collecting hourly data (3/7 days)"

### Requirement: Classic spread trade table
The classic spread trade table SHALL use S = one tick below the current best sell at the station.

#### Scenario: Classic row
- **WHEN** the best buy of an item is 100.00 and the best sell is 120.00
- **THEN** the classic table uses S = 119.90 and the current best buy 100.00

### Requirement: Mean reversion table
The mean reversion table SHALL list an item when, on the latest history day, the average price is below the lower Bollinger band (SMA 20 − 2 standard deviations of the last 20 averages), or RSI 14 is below 30. RSI 14 uses Wilder's smoothing over the daily averages. S SHALL be SMA 20 rounded down to a valid price, and an item whose S is not above the current best buy SHALL not be listed. Items with fewer than 20 history days SHALL not be listed. Each row SHALL show %B and RSI 14.

#### Scenario: Oversold
- **WHEN** an item's latest average is below its lower Bollinger band and its SMA 20 is 150.37
- **THEN** it is listed with S = 150.30

#### Scenario: Within the band
- **WHEN** an item's latest average lies within its Bollinger bands and its RSI 14 is 45
- **THEN** it is not listed in the mean reversion table

### Requirement: Trend table
The trend table SHALL list an item when its SMA 5 of the daily averages is above its SMA 20 on the latest history day and was not above it on one of the three days before. The average daily volume of the last 5 days SHALL also be above that of the last 20 days. S SHALL be one tick below the current best sell. Each row SHALL show SMA 5, SMA 20 and the day of the crossing.

#### Scenario: Fresh crossing with rising volume
- **WHEN** an item's SMA 5 crossed above its SMA 20 two days ago, and its 5-day volume average is above its 20-day average
- **THEN** it is listed in the trend table

#### Scenario: Crossing without volume
- **WHEN** an item's SMA 5 crossed above its SMA 20 yesterday, but its 5-day volume average is below its 20-day average
- **THEN** it is not listed in the trend table

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
