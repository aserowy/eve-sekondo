# station-opportunities Specification

## Purpose

Shows a signed-in trader what is worth buying for station trading at the built-in trading station, through a set of established trading analyses. Each analysis has its own table, all tables share the same fee, liquidity and holding-time rules, and a detail window explains each result.

## Requirements

### Requirement: Opportunities page
The system SHALL provide one page per opportunity table for signed-in users, at `/opportunities/expansion` (recurring spread expansion), `/opportunities/classic` (classic spread trade), `/opportunities/reversion` (mean reversion) and `/opportunities/trend` (trend). Each accepts the character filter like the other character-scoped views. Unauthenticated requests SHALL be redirected to the landing page. Each table page is laid out as a table page:
- Its sub-menu lists the four tables as text, in the order recurring spread expansion, classic spread trade, mean reversion and trend, without row counts, and marks the current one.
- Its page title is followed by the name of the station it analyses as subtitle.
- Its filter section holds the character filter, the fee character and the opportunity filters.
- It shows the one table titled after it, with its pagination, and nothing between the filter section and the table. No page shows a note about collecting hourly data.
- Below the pagination, it shows the table's explainer card.
- Its freshness section holds the station book's freshness, the market history's freshness and the number of days of own snapshots.

#### Scenario: Open the page
- **WHEN** a signed-in user opens `/opportunities/classic`
- **THEN** they see the sub-menu marking "Classic spread trade", the subtitle Jita IV - Moon 4 - Caldari Navy Assembly Plant below the page title, the filter section, the classic spread trade table, its explainer card and, below it, the freshness

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

#### Scenario: Few days of own data
- **WHEN** own snapshots cover 3 days and a user opens `/opportunities/expansion`
- **THEN** no "Collecting hourly data" note is shown above the table, and the freshness section reads "3 days of own snapshots"

### Requirement: Fees used for the analyses
The analyses SHALL use the broker fee b and sales tax t at the station of one selected character, the fee character. The opportunity table pages and the opportunity detail page SHALL let the user choose the fee character among the selected characters in the fee character group of their filter section, and SHALL show the chosen one as checked.

The fee character SHALL be the character the account chose, as long as it is selected. Otherwise, and when the account has not chosen one, it SHALL be the selected character with the lowest minimum margin (1 + b) / (1 − b − t) − 1 at the station; this does not change the account's choice. A selected character whose fees at the station are not known yet SHALL count with the base rates b = 3% and t = 7.5%, both when its minimum margin is compared and when it is the fee character. With no character selected, the analyses SHALL use the base rates.

The helper text of the fee character group SHALL show the broker fee and sales tax in use, or, when the base rates are used, say so and show them.

Choosing a fee character SHALL save it and apply it without a separate apply control. On a table page it SHALL show page 1 of the current table; on a detail page it SHALL keep the detail page and recompute its figures. The choice SHALL be stored per account, kept across sessions, and shared by every table page and every detail page. Saving SHALL need the account's CSRF token. A character that is not linked to the account SHALL be rejected without changing the stored choice.

#### Scenario: Two characters
- **WHEN** an account that never chose a fee character selects a character with a minimum margin of 6.5% at the station and one with 9.1%
- **THEN** the first one is checked in the fee character group, and every analysis uses its fees

#### Scenario: Choose another character
- **WHEN** that user checks the character with 9.1% on the trend page
- **THEN** the trend table shows page 1 recomputed with that character's fees, the helper text shows its broker fee and sales tax, and after signing in again the classic page still uses its fees

#### Scenario: Choice applies on the detail page
- **WHEN** the account chose the character with 9.1% and the user opens a trend detail page with both characters selected
- **THEN** that character is checked, and the figures are computed with its fees

#### Scenario: Chosen character deselected
- **WHEN** the account chose a character and the user deselects it in the character filter
- **THEN** the remaining selected character with the lowest minimum margin is checked and used, and selecting the chosen character again checks and uses it again

#### Scenario: No fee data yet
- **WHEN** the only selected character's skills have never been polled
- **THEN** that character is checked, the analyses use a 3% broker fee and 7.5% sales tax, and the helper text says that base rates are used

#### Scenario: Nobody selected
- **WHEN** a user deselects every character on the trend page
- **THEN** the fee character group shows no radio button, its helper text says that base rates are used, and the analyses use them

#### Scenario: Foreign character rejected
- **WHEN** a request saves a fee character that is not linked to the account
- **THEN** the stored choice is unchanged

#### Scenario: Choice stays within the account
- **WHEN** account A chooses a fee character
- **THEN** the fee character of account B is unchanged

### Requirement: Valid prices
Every price the analyses suggest SHALL be valid in EVE: it has at most four significant digits and at most two decimals. "One tick below x" SHALL mean the highest valid price below x. "One tick above x" SHALL mean the lowest valid price above x. "Rounded down to a valid price" SHALL mean the highest valid price that is not above x.

#### Scenario: One tick below
- **WHEN** a target is one tick below 1,234,000.00
- **THEN** the suggested price is 1,233,000.00

#### Scenario: Small prices
- **WHEN** a target is one tick below 9.99
- **THEN** the suggested price is 9.98

#### Scenario: One tick above
- **WHEN** a target is one tick above 1,234,500.00
- **THEN** the suggested price is 1,235,000.00

### Requirement: Shared figures per row
For each item in a table, with B the table's buy price and S the table's sell price, the system SHALL compute the figures below. B SHALL be the current best buy at the station, except for a dip row of the recurring spread expansion table, where it is the predicted buy price.
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

#### Scenario: Dip row uses the predicted buy price
- **WHEN** the current best buy of an item is 100.00 and its recurring spread expansion row is a dip with B = 82.10
- **THEN** c = 82.10 × (1 + b), and every figure derived from c uses it

### Requirement: Hard exclusion
An item SHALL NOT appear in any table when its net margin is not positive or U is below 1. It SHALL NOT appear when there are fewer than 10 daily returns for σ, or when V is 0. It SHALL also not appear when T_fill exceeds T_max: in other words, when its daily volume is below the minimum daily volume its volatility and margin require. This SHALL apply to every table before the filters and sorting.

#### Scenario: Volatile and thin
- **WHEN** an item has a net margin of 8% but its fill time is 4 days and its T_max is 1.2 days
- **THEN** it appears in no table

#### Scenario: Not enough history
- **WHEN** an item has history for only 6 days
- **THEN** it appears in no table

### Requirement: Recurring spread expansion table
The system SHALL find recurring spread expansions per item from the hourly rollups of its own snapshots. Only **covered days** count: UTC days with rollups in at least 20 of their 24 hours. Only complete days before today count.

**Expanded hour.** An hour of a covered day SHALL count as expanded when its maximum spread is at least 2 × the median of the hourly maximum spreads of that day. Its **direction** SHALL be:
- **dip** when the drop of its minimum best buy below the day's median hourly average best buy is larger, in percent, than the rise of its maximum best sell above the day's median hourly average best sell
- **spike** otherwise

The **trough** of a dip SHALL be its minimum best buy divided by the day's median hourly average best buy. The **peak** of a spike SHALL be its maximum best sell divided by the day's median hourly average best sell.

**Quiet items only.** An item SHALL have no recurring window when more than 10% of its hours on the covered days of the last 30 days expanded.

**Windows.** A window is a direction together with one of these periods. It SHALL count as recurring when:
- **hour of day** (e.g. "daily 18:00"): among the last 28 days, that hour expanded in that direction on at least 60% of the covered days, and on at least 7 days
- **weekday × hour** (e.g. "weekly Tue 18:00"): among the last 52 weeks, that hour of that weekday expanded in that direction in at least 60% of the weeks whose day is covered, and in at least 3 weeks. It also expanded in that direction in at least 2 of the last 4 weeks.

An occurrence of a window is one past day (hour of day) or week (weekday × hour) in which it expanded in its direction. No window SHALL be found from market history or from daily figures. A window SHALL count as soon as it has enough occurrences, however few days of own snapshots exist.

**Prices.** Let the 7-day median best sell and best buy be the medians of the hourly average best sell and best buy over the last 7 days.
- **Spike window:** B SHALL be the current best buy. S SHALL be one tick below (25th percentile of its occurrences' peaks × the 7-day median best sell).
- **Dip window:** B SHALL be one tick above (75th percentile of its occurrences' troughs × the 7-day median best buy). S SHALL be one tick below the current best sell.

Percentiles interpolate linearly between ranks.

**Window volume** V_win SHALL be the average, over a window's occurrences, of the units taken in that hour from the side that moved: from buy orders for a dip, from sell orders for a spike.

**Row.** When an item has several recurring windows, its row SHALL use the one with the highest m. On equal m, the one that starts next wins. The row SHALL show the window, its direction as "Dip" or "Spike", V_win, and the start of its next occurrence in EVE time (UTC).

#### Scenario: Daily evening expansion
- **WHEN** on 20 of the last 28 covered days an item's maximum spread between 18:00 and 19:00 UTC reached at least 2 × that day's median hourly maximum spread, each time because the best sell rose, with peaks of 1.20, 1.22, 1.25, 1.26, 1.28, 1.30, 1.31, 1.33, 1.35 and 1.40 on ten of them and higher on the rest, and its 7-day median best sell is 10,000.00
- **THEN** it is listed with the window "daily 18:00", direction Spike, B the current best buy, S one tick below the 25th percentile of the 20 peaks × 10,000.00, and the next 18:00 UTC as the next start

#### Scenario: Weekly dip
- **WHEN** over the last 52 weeks an item's buy side dropped at Tuesday 11:00 with the spread at least 2 × the day's median in 14 of the 20 weeks whose Tuesday is covered, including 3 of the last 4, and its 7-day median best buy is 1,000.00 with the 75th percentile of the troughs at 0.82
- **THEN** it is listed with the window "weekly Tue 11:00", direction Dip, B = 820.10 (one tick above 820.00), S one tick below the current best sell, and the next Tuesday 11:00 UTC as the next start

#### Scenario: Constant spread
- **WHEN** an item's spread stays at about 10% in every hour of the last 30 days
- **THEN** none of its hours expands, and it is not listed

#### Scenario: Jumpy all day
- **WHEN** 15% of an item's hours on the covered days of the last 30 days expanded, including 18:00 on 20 of the last 28 days
- **THEN** it is not listed

#### Scenario: Too irregular
- **WHEN** an item's 18:00 hour expanded as a spike on only 10 of the last 28 covered days
- **THEN** the "daily 18:00" spike window is not recurring

#### Scenario: Died out
- **WHEN** an item's Tuesday 11:00 hour dipped in 12 of the 20 covered weeks of the last 52, but in none of the last 4
- **THEN** the "weekly Tue 11:00" dip window is not recurring

#### Scenario: Directions are separate
- **WHEN** an item's 18:00 hour expanded on 20 of the last 28 covered days, 10 times as a dip and 10 times as a spike
- **THEN** neither the dip nor the spike "daily 18:00" window is recurring

#### Scenario: Old peaks follow today's price
- **WHEN** a weekly spike window's peaks were recorded when the item's median best sell was 8,000.00, and its 7-day median best sell is now 10,000.00
- **THEN** S is derived from the peak ratios × 10,000.00, not from the ISK peaks of back then

#### Scenario: Highest margin wins
- **WHEN** an item has a recurring spike window giving m = 12% and a recurring dip window giving m = 18%
- **THEN** its row shows the dip window

#### Scenario: First days after the release
- **WHEN** own snapshots cover 5 days
- **THEN** no hour-of-day window can recur yet, and the table shows no note about collecting data

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
- **WHEN** 80 items pass the classic table's rules and the filters
- **THEN** the first page shows the 50 with the highest ISK/day and the second page the other 30

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
Every table row SHALL link to a detail page for that item and table at `/opportunities/<table>/<type id>`, where `<table>` is `expansion`, `classic`, `reversion` or `trend`. Clicking a row SHALL navigate to it. The detail page SHALL be for signed-in users only. Unauthenticated requests SHALL be redirected to the landing page, and an unknown table SHALL answer "not found". The detail page accepts the character filter and every table's sorting, carries them in its links, and computes its figures with the fees of the fee character and with the account's stored capital per item. The main menu SHALL mark "Opportunities" as current on it.

The detail page is laid out from top to bottom:
- Its section sub-menu starts with a link "← <table title>" back to its table's page, keeping the character filter and every table's sorting. It then lists the page's rows.
- Its page title is followed by the station name as subtitle.
- Its filter section holds the character filter and the fee character.
- Its first row is the figures section, titled with the item name and the table.
- Its second row is the legend card.
- After that, each chart of its table is a section alone on a full-width row. For a recurring spread expansion, a section "Recurring windows" follows. It lists every recurring window of the item with its direction, its predicted price (S for a spike, B for a dip), its number of occurrences, V_win and its next three starts in EVE time, and marks the window the row uses.

The figures section SHALL show each figure as a KPI card, in labelled groups in this order:
- **Fees**: b, t
- **Prices**: B, best buy (only when B is not the current best buy), best sell, S
- **Margin**: c, r, Net, m
- **Size & liquidity**: U, V7, V30, ρ, q_buy, q_sell
- **Timing & risk**: σ, T_fill, T_max, V_min
- **Result**: ISK/d

A card's name SHALL be the figure's short label as in the table headers, or "Best buy" and "Best sell" for the current best buy and best sell, with its full name on hover. Its value SHALL be the figure. A figure that is computed from others SHALL have a detail line with the computation using this item's numbers, for example "10,000.00 × 1.0141" for c. The formula and meaning of every figure SHALL be in the legend card, not in the figures section. Figures that cannot be computed for the item SHALL be left out, as before.

The charts per table are:
- **Recurring spread expansion**: the spread by hour of day and by weekday over 30 days, and the past occurrences of the chosen window in ISK: the maximum best sell of each occurrence with S marked for a spike, or the minimum best buy of each occurrence with B marked for a dip
- **Classic spread trade**: best buy, best sell and spread over 30 days, and the orders and units within 1% per side
- **Mean reversion**: the daily average with SMA 20 and Bollinger bands, and RSI 14
- **Trend**: the daily average with SMA 5 and SMA 20, and the daily volume

When the table does not list the item right now, the page SHALL say so and why, and SHALL still show what is known of the item. An item the station no longer trades SHALL be shown under its name, or as "Type <id>", with that reason.

#### Scenario: Open from a row
- **WHEN** a user with `?chars=<id>` and the trend table sorted by σ clicks a row of the mean reversion table
- **THEN** the browser navigates to `/opportunities/reversion/<type id>` carrying `chars=<id>` and the trend sorting, the page shows the item's figures, its legend card, its Bollinger chart and its RSI chart each on its own row, and the main menu marks "Opportunities" as current

#### Scenario: Sub-menu of an expansion page
- **WHEN** a user opens the detail page of a row in the recurring spread expansion table
- **THEN** the sub-menu lists "← Recurring spread expansion", "Figures", "Legend", one entry per chart and "Recurring windows"

#### Scenario: All windows of an item
- **WHEN** a user opens the expansion detail page of an item with a recurring "daily 18:00" spike and a recurring "weekly Tue 11:00" dip, whose row uses the dip
- **THEN** the "Recurring windows" section lists both with their direction, predicted price, occurrences, V_win and next three starts, marks the dip, and the chart shows the dip's past troughs in ISK with B marked

#### Scenario: Figures as grouped cards
- **WHEN** a user opens the classic detail page of an item with B = 100.00 and a 1.41% broker fee
- **THEN** the figures show the groups Fees, Prices, Margin, Size & liquidity, Timing & risk and Result in this order, the c card is named "c", shows 101.41 and has the detail line "100.00 × 1.0141", and no card shows the formula "B × (1 + b)"

#### Scenario: Figures without a computation
- **WHEN** a user opens a detail page
- **THEN** the cards of b, t, B, best sell, V7, V30, ρ and σ have no detail line

#### Scenario: Back to the tables
- **WHEN** a user on a mean reversion detail page with `?chars=<id>` and the trend table sorted by σ activates "← Mean reversion"
- **THEN** `/opportunities/reversion` opens with `chars=<id>` and the trend table's sorting kept

#### Scenario: Fees follow the selection
- **WHEN** a user on a detail page deselects the character whose fees are used
- **THEN** the URL keeps the detail page, the fee character group checks the fee character of the remaining selection, and the figures are recomputed with its fees

#### Scenario: Choose fees on the detail page
- **WHEN** a user with two characters selected checks the other character in the fee character group of a trend detail page
- **THEN** the URL keeps the detail page, the figures are recomputed with that character's fees, the b and t cards show them, and the trend page then uses them as well

#### Scenario: Item dropped out
- **WHEN** a user opens the classic detail page of an item that the classic table no longer lists because its margin is not positive
- **THEN** the page says that the item is no longer listed in this table and why, and still shows its figures

#### Scenario: Unknown table
- **WHEN** a signed-in user requests `/opportunities/arbitrage/34`
- **THEN** the answer is "not found"

#### Scenario: Signed out
- **WHEN** an unauthenticated user requests `/opportunities/trend/34`
- **THEN** they are redirected to the landing page

### Requirement: Legend
Every short label used by the opportunity tables and the detail page SHALL have one legend entry: the short label, its full name, its meaning and, for a computed figure, its formula. The same label SHALL have the same entry, and the same full name on hover, wherever it is shown. The entries SHALL cover Item (including the "own buy" and "own sell" marks), b, t, B, Best buy, Best sell, S, c, r, Net, m, U, V7, V30, ρ, q_buy, q_sell, σ, T_fill, T_max, V_min, ISK/d, n b/s, Window, Dir, V_win, Next (UTC), %B, RSI, SMA5, SMA20 and Cross. The formulas SHALL be those of the shared figures and of the table rules, for example c = B × (1 + b), m = (r − c) / c, T_max = min(3, (m / 2σ)²) days, and V_min as the V at which T_fill equals T_max. The entry of B SHALL say that it is the current best buy and not a suggested price, except in a dip row of the recurring spread expansion table, where it is the predicted buy price. The entries of Window and Dir SHALL explain the expanded hour, the dip and spike directions and the recurrence rules, and the entry of V_win SHALL say which side's taken units it averages.

The legend of a table lists the table's columns in their order. The legend of a detail page lists its figures in the order of the figures section.

A legend SHALL be shown as a table with the header row Label, Name, Meaning and Formula, and one row per entry. The label SHALL be the row's header. The name SHALL be plain text. The Formula cell SHALL hold only the right-hand side of the formula, in code style, and SHALL be empty for an entry without a formula. A legend table SHALL look like the worked example's table and SHALL NOT be the page's data table: it is neither sortable nor a link to a detail page. In a narrow window the meaning SHALL wrap within its cell, the label and the formula SHALL stay on one line, and the page SHALL NOT scroll sideways.

#### Scenario: Same wording everywhere
- **WHEN** a user reads the legend entry of T_max on the trend page and on a trend detail page
- **THEN** both show the same full name, meaning and formula min(3, (m / 2σ)²) days

#### Scenario: Table legend follows the columns
- **WHEN** a user opens the legend on the mean reversion page
- **THEN** it lists Item, B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, %B and RSI, and no entry for Window or SMA5

#### Scenario: Legend as a table
- **WHEN** a user opens the Legend card on a classic detail page
- **THEN** it shows a table headed Label, Name, Meaning and Formula, whose row c reads c, Cost per unit, its meaning and B × (1 + b), and whose row B has an empty Formula cell

#### Scenario: Legend in a narrow window
- **WHEN** a user opens the explainer on the trend page in a window 600 pixels wide
- **THEN** the meanings wrap within their cells, the labels and formulas stay on one line each, and the page does not scroll sideways

### Requirement: Table explainer card
Every opportunity table page SHALL show an explainer card for its table below the pagination, or below the table when it has no rows. The card SHALL be a disclosure titled "How to read this table" that is closed when the page loads. Its open state SHALL NOT be stored, and it SHALL stay as the user left it across a live refresh, a change of page or page size, a change of sorting and a change of the opportunity filters.

Every explainer SHALL hold, in this order:
1. **Legend**: the table's legend.
2. **Signal**: when the table lists an item and how its S is derived, explained for that table alone. For the recurring spread expansion table it SHALL also explain the quiet-item rule, both directions, and how B is derived for a dip.
3. **Shared figures**: the intermediate figures that are not columns (c, r, U, market share, ρ, q_buy, q_sell) with their formulas, and the hard exclusion. This is the only part worded the same in every explainer.
4. **Worked example**: one illustrative row of that table, continuing its signal into every column of the table, read column by column. It SHALL say that the row passes the default filters and how far V is above V_min.
5. **Order playbook**: how to act on a row of that table.

The worked examples SHALL share b = 1.41%, t = 3.375%, a capital per item of 100,000,000, V7 = 80,000, ρ = 0.5, and 3 buy and 1 sell orders within 1%, which give q_buy = 10,000 and q_sell = 20,000. Everything else is per table:

| | Recurring spread expansion | Classic spread trade | Mean reversion | Trend |
|---|---|---|---|---|
| Signal input | spike peaks 1.20, 1.22, 1.25, 1.26, 1.28, 1.30, 1.31, 1.33, 1.35, 1.40 of the window "daily 18:00"; 25th percentile 1.2525 × 7-day median best sell 10,000.00 = 12,525 | best sell 11,500.00 | SMA 20 10,537.40, %B −0.08, RSI 27 | best sell 11,800.00; SMA5 crossed above SMA20 two days ago; 5-day volume 95,000 against 80,000 over 20 days |
| B | 10,000.00 | 10,000.00 | 9,000.00 | 10,000.00 |
| S | 12,520.00 | 11,490.00 | 10,530.00 | 11,790.00 |
| σ | 2% | 2% | 3% | 2% |
| c | 10,141.00 | 10,141.00 | 9,126.90 | 10,141.00 |
| r | ≈ 11,920.92 | ≈ 10,940.20 | ≈ 10,026.14 | ≈ 11,225.85 |
| Net | ≈ 1,779.92 | ≈ 799.20 | ≈ 899.24 | ≈ 1,084.85 |
| m | ≈ 17.55% | ≈ 7.88% | ≈ 9.85% | ≈ 10.70% |
| U | 9,860 | 9,860 | 10,956 | 9,860 |
| T_fill | ≈ 1.48 days | ≈ 1.48 days | ≈ 1.64 days | ≈ 1.48 days |
| T_max | 3 days (≈ 19.25 capped) | 3 days (≈ 3.88 capped) | ≈ 2.70 days | 3 days (≈ 7.15 capped) |
| V_min | 39,440 | 39,440 | ≈ 48,756 | 39,440 |
| ISK/d | ≈ 11.87M | ≈ 5.33M | ≈ 5.99M | ≈ 7.23M |
| Outbid buy price | 10,010.00 | 10,010.00 | 9,001.00 | 10,010.00 |
| c and m after outbidding | ≈ 10,151.14, ≈ 17.43% | ≈ 10,151.14, ≈ 7.77% | ≈ 9,127.91, ≈ 9.84% | ≈ 10,151.14, ≈ 10.59% |

Every playbook SHALL state the common steps once: place a buy order for U units one tick above B (unless the table's own steps say otherwise), check that m after outbidding still clears the minimum, and expect both orders to fill within about T_fill, re-checking the row when they have not filled within T_max. Each SHALL then give its table's own steps:
- **Recurring spread expansion**: for a spike row, place the buy order well before the window. Once it has filled, place the sell order at S before the next start shown in the row. When the sell order has not filled by the end of the window, wait for the next occurrence or re-check the row, instead of chasing the price down. For a dip row, place the buy order at B itself, not one tick above, shortly before the next start, and cancel it when the window ends unfilled. Once it has filled, sell at S, one tick below the best sell, as in a classic spread trade.
- **Classic spread trade**: place the sell order at S, one tick below the best sell, as soon as the buy order has filled. Watch both sides, because being outbid or undercut stops the fills. Re-price by one tick only while m still clears the minimum.
- **Mean reversion**: buy while %B is below 0 or RSI below 30. Sell at S, the SMA 20 the price is expected to return to. When the price has not recovered within T_max, or keeps falling, drop the trade instead of averaging down.
- **Trend**: buy shortly after the crossing. Sell one tick below the best sell, following it upwards while the trend holds. Exit when SMA5 falls back below SMA20 or the row leaves the table.

The example numbers SHALL be fixed. They SHALL NOT depend on the table's rows, the filters, the fee character or the market data. Only the explainer SHALL give buy-side guidance, and only as these general rules. Apart from the B of a dip row, the table rows SHALL still suggest no buy price.

#### Scenario: Collapsed on load
- **WHEN** a signed-in user opens `/opportunities/classic`
- **THEN** below the pagination there is a closed "How to read this table" card, and opening it shows the legend, the signal, the shared figures, the worked example and the order playbook

#### Scenario: Unique per table
- **WHEN** a signed-in user opens the explainer on `/opportunities/reversion`
- **THEN** its signal explains the Bollinger band, RSI and %B, its worked example shows B = 9,000.00, S = 10,530.00, m ≈ 9.85%, T_max ≈ 2.70 days and ≈ 5.99M ISK/d, and its playbook says to sell at the SMA 20 and not to average down

#### Scenario: Example continues the signal
- **WHEN** a signed-in user opens the explainer on `/opportunities/expansion`
- **THEN** the worked example uses S = 12,520.00 from the signal's peak ratios and 7-day median best sell, shows the window "daily 18:00" as a spike with a next start, and reads ≈ 17.55% and ≈ 11.87M ISK/d

#### Scenario: Both directions explained
- **WHEN** a signed-in user opens the explainer on `/opportunities/expansion`
- **THEN** its signal explains the 2 × daily-median expansion, the 10% quiet rule, dips and spikes, and how S and B are predicted, and its playbook gives the steps for a spike row and for a dip row

#### Scenario: Shared figures are the same
- **WHEN** a user compares the explainers of the classic and the trend page
- **THEN** their shared figures parts are worded the same, while their worked examples show S = 11,490.00 and S = 11,790.00 and their playbooks differ

#### Scenario: Static numbers
- **WHEN** a user changes the fee character or the capital per item
- **THEN** the explainer's worked example still shows b = 1.41%, t = 3.375% and U = 9,860

#### Scenario: Empty table
- **WHEN** the trend table has no rows
- **THEN** the page shows "No opportunities." and below it the trend explainer

#### Scenario: Stays open across a refresh
- **WHEN** a user has opened the explainer on the trend page and an `opportunities-updated` signal arrives, or they go to page 2
- **THEN** the explainer is still open

### Requirement: Detail legend card
Every opportunity detail page SHALL show a legend card directly below the figures section and above the charts. It SHALL be a disclosure titled "Legend" that is closed when the page loads, and SHALL hold only the detail page's legend, without a signal, worked example or order playbook. The section sub-menu SHALL list it as "Legend" right after "Figures". Its open state SHALL NOT be stored, and it SHALL stay as the user left it across a live refresh, a change of the character filter and a change of the fee character.

#### Scenario: Closed legend below the figures
- **WHEN** a signed-in user opens `/opportunities/trend/34`
- **THEN** a closed "Legend" card sits between the figures and the first chart, and opening it lists b, t, B, Best sell, S, c, r, Net, m, U, V7, V30, ρ, q_buy, q_sell, σ, T_fill, T_max, V_min and ISK/d with their full names, meanings and formulas

#### Scenario: Jump to the legend
- **WHEN** a user activates "Legend" in the detail page's sub-menu
- **THEN** the page scrolls to the legend card and its title is visible below the sub-menu

#### Scenario: Legend stays open
- **WHEN** a user has opened the legend on a detail page and an `opportunities-updated` signal arrives or they choose another fee character
- **THEN** the legend is still open and the figures show the new values
