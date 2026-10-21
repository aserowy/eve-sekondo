## MODIFIED Requirements

### Requirement: Table rows and columns
Every table SHALL list every item that passes its rules and the filters, paginated as described by the ui-layout capability. Each row SHALL show the item name, B, S, net margin per unit and m, and ISK/day. It SHALL also show the 7-day and 30-day daily volume, σ, T_max, the fill time, the minimum daily volume, and the orders within 1% per side, plus the table's own columns. No table SHALL suggest a buy price, except the dip rows of the recurring spread expansion table, whose B is the predicted buy price. A row SHALL be marked "own buy" or "own sell" when a selected character has a current order of that side for the item at the station. An item SHALL be shown under its name, or as "Type <id>" until its name is known.

Column headers SHALL be short, with these labels:

| Column | Header |
|---|---|
| item | Item |
| buy price | B |
| sell price | S |
| net margin per unit | Net |
| net margin | m |
| ISK/day | ISK/d |
| daily volume, 7 and 30 days | V7, V30 |
| volatility | σ |
| allowed holding time, fill time | T_max, T_fill |
| minimum daily volume | V_min |
| orders within 1% per side | n b/s |
| window, direction, window volume, next start in EVE time | Window, Dir, V_win, Next (UTC) |
| %B, RSI 14 | %B, RSI |
| SMA 5, SMA 20, day of the crossing | SMA5, SMA20, Cross |

Every short header except "Item" SHALL be marked as an abbreviation whose full name is shown on hover and given to assistive technology. The full name of B SHALL say that it is the current best buy and not a suggested price, except in a dip row of the recurring spread expansion table, where it is the predicted buy price. Headers SHALL stay sortable as before.

#### Scenario: Own sell order
- **WHEN** a selected character has a sell order for an item at Jita 4-4 that the classic table lists
- **THEN** the row is marked "own sell"

#### Scenario: More than 50 matches
- **WHEN** 80 items pass the classic table's rules and the filters, and the account never chose a page size
- **THEN** the first page shows the 25 with the highest ISK/day, the pagination reads "Page 1 of 4", and the fourth page shows the other 5

#### Scenario: Short headers
- **WHEN** a signed-in user opens `/opportunities/trend`
- **THEN** the headers read Item, B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, SMA5, SMA20 and Cross

#### Scenario: Full name on hover
- **WHEN** a user hovers the header "B"
- **THEN** its full name says it is the current best buy and not a suggested price, except in a dip row of the recurring spread expansion table, and activating the header still sorts by it

#### Scenario: Expansion headers
- **WHEN** a signed-in user opens `/opportunities/expansion`
- **THEN** the headers read Item, B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, Window, Dir, V_win and Next (UTC)

#### Scenario: Dip row
- **WHEN** the expansion table lists an item with a recurring dip window
- **THEN** its row shows Dir "Dip", B the predicted buy price, and S one tick below the current best sell

### Requirement: Opportunity filters
Every opportunity table page SHALL show one set of opportunity filters, shared by all tables, in its filter section, with these settings:
- minimum net margin m (default 5%)
- maximum net margin m (default 80%)
- minimum ISK/day (default 5,000,000)
- an item name search (case-insensitive substring, default empty)
- capital per item (default 100,000,000 ISK)

Changing a setting SHALL save and apply it without a separate apply control, and SHALL show page 1 of the current table. The settings SHALL be stored per account and kept across sessions. Changing them SHALL re-compute every table, because the order size and the exclusion depend on the capital. A row SHALL be listed only when its m is at least the minimum and at most the maximum net margin. The fields SHALL be labelled "Minimum margin (%)", "Maximum margin (%)", "Minimum ISK/day", "Item name" and "Capital per item (ISK)", in this order. A value that is negative, not a number, or a capital of 0 SHALL be rejected, and so SHALL an empty maximum margin and a maximum margin below the minimum margin. Each is rejected with a message in the filter section, and the stored settings SHALL be kept. Saving SHALL need the account's CSRF token.

#### Scenario: Defaults
- **WHEN** an account opens an opportunity table page for the first time
- **THEN** the filters show 5%, 80%, 5,000,000 ISK/day, an empty search and 100,000,000 ISK capital

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

#### Scenario: Maximum margin hides outliers
- **WHEN** the classic table would list an item with m = 120% and one with m = 12%, and the maximum margin is 80%
- **THEN** the table lists the item with 12% and not the one with 120%, and raising the maximum margin to 150% lists both

#### Scenario: Maximum margin below the minimum
- **WHEN** a user with a minimum margin of 10% enters a maximum margin of 5%
- **THEN** the "Maximum margin (%)" input is marked invalid, a message below it says the maximum margin must be at least the minimum margin, and the stored settings are unchanged

#### Scenario: Empty maximum margin
- **WHEN** a user clears the maximum margin
- **THEN** a message below the "Maximum margin (%)" input says it must be a number of at least 0 percent and the stored settings are unchanged

#### Scenario: Detail page names the maximum margin
- **WHEN** a user opens the classic detail page of an item that the classic table does not list only because its m of 120% is above the maximum margin of 80%
- **THEN** the page says that the item is not listed because its net margin is above your maximum margin

## ADDED Requirements

### Requirement: Stored page size
The opportunity table pages SHALL store the chosen page size per account. It is one size shared by all four tables and kept across sessions. When an opportunity table page or its live refresh is opened without `per_page`, it SHALL use the stored size, or 25 when the account never chose one. A `per_page` in the URL SHALL take precedence over the stored size and SHALL NOT change it.

Choosing a page size in the pagination of an opportunity table page SHALL save it and show page 1 of the current table at that size, keeping the character selection and every table's sorting. The URL SHALL be updated without a full page reload. Saving SHALL need the account's CSRF token. A size that is not one of the offered page sizes SHALL be rejected without changing the stored size. The stored size of one account SHALL NOT affect another account or the other table pages.

#### Scenario: Back from a detail page
- **WHEN** a user chooses a page size of 10 on the trend page, opens a row's detail page and activates "← Trend"
- **THEN** the trend page shows 10 rows per page and its page-size control shows 10

#### Scenario: Shared by the tables
- **WHEN** a user chooses a page size of 50 on the classic page and then activates "Mean reversion" in the sub-menu
- **THEN** the mean reversion table shows up to 50 rows per page

#### Scenario: Kept across sessions
- **WHEN** a user chooses a page size of 100, signs out and signs in again, and opens `/opportunities/classic`
- **THEN** the classic table shows up to 100 rows per page

#### Scenario: URL takes precedence
- **WHEN** a user with a stored page size of 100 opens `/opportunities/classic?per_page=10`
- **THEN** the table shows 10 rows per page, and opening `/opportunities/classic` afterwards shows 100 rows per page

#### Scenario: Live refresh keeps the stored size
- **WHEN** a user with a stored page size of 10 has the trend page open and an `opportunities-updated` signal arrives
- **THEN** the table still shows 10 rows per page

#### Scenario: Saving needs the CSRF token
- **WHEN** a page size is submitted without the account's CSRF token
- **THEN** the request is rejected and the stored page size is unchanged

#### Scenario: Other pages unaffected
- **WHEN** a user with a stored opportunity page size of 100 opens the sell orders page without `per_page`
- **THEN** the sell orders table shows 25 orders per page

#### Scenario: Page size stays within the account
- **WHEN** account A chooses a page size of 10
- **THEN** the opportunity tables of account B keep their page size
