# Spec Delta

## MODIFIED Requirements

### Requirement: Opportunities page
The system SHALL provide one page per opportunity table for signed-in users, at `/opportunities/expansion` (recurring spread expansion), `/opportunities/classic` (classic spread trade), `/opportunities/reversion` (mean reversion) and `/opportunities/trend` (trend). Each accepts the character filter like the other character-scoped views. Unauthenticated requests SHALL be redirected to the landing page. Each table page is laid out as a table page:
- Its sub-menu lists the four tables as text, in the order recurring spread expansion, classic spread trade, mean reversion and trend, without row counts, and marks the current one.
- Its page title is followed by the name of the station it analyses as subtitle.
- Its filter section holds the character filter, the fee character and the opportunity filters.
- It shows the one table titled after it, with its pagination. The recurring spread expansion page shows, above its table, how many days of hourly data have been collected while fewer than needed are.
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

### Requirement: Table rows and columns
Every table SHALL list every item that passes its rules and the filters, paginated as described by the ui-layout capability. Each row SHALL show the item name, the current best buy, S, net margin per unit and m, and ISK/day. It SHALL also show the 7-day and 30-day daily volume, σ, T_max, the fill time, the minimum daily volume, and the orders within 1% per side, plus the table's own columns. No table SHALL suggest a buy price. A row SHALL be marked "own buy" or "own sell" when a selected character has a current order of that side for the item at the station. An item SHALL be shown under its name, or as "Type <id>" until its name is known.

Column headers SHALL be short, with these labels:

| Column | Header |
|---|---|
| item | Item |
| current best buy | B |
| sell price | S |
| net margin per unit | Net |
| net margin | m |
| ISK/day | ISK/d |
| daily volume, 7 and 30 days | V7, V30 |
| volatility | σ |
| allowed holding time, fill time | T_max, T_fill |
| minimum daily volume | V_min |
| orders within 1% per side | n b/s |
| window, next start in EVE time | Window, Next (UTC) |
| %B, RSI 14 | %B, RSI |
| SMA 5, SMA 20, day of the crossing | SMA5, SMA20, Cross |

Every short header except "Item" SHALL be marked as an abbreviation whose full name is shown on hover and given to assistive technology. The full name of B SHALL say that it is the current best buy and not a suggested price. Headers SHALL stay sortable as before.

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
- **THEN** its full name says it is the current best buy and not a suggested price, and activating the header still sorts by it

### Requirement: Opportunity detail page
Every table row SHALL link to a detail page for that item and table at `/opportunities/<table>/<type id>`, where `<table>` is `expansion`, `classic`, `reversion` or `trend`. Clicking a row SHALL navigate to it. The detail page SHALL be for signed-in users only. Unauthenticated requests SHALL be redirected to the landing page, and an unknown table SHALL answer "not found". The detail page accepts the character filter and every table's sorting, carries them in its links, and computes its figures with the fees of the fee character and with the account's stored capital per item. The main menu SHALL mark "Opportunities" as current on it.

The detail page is laid out from top to bottom:
- Its section sub-menu starts with a link "← <table title>" back to its table's page, keeping the character filter and every table's sorting. It then lists the page's rows.
- Its page title is followed by the station name as subtitle.
- Its filter section holds the character filter and the fee character.
- Its first row is the figures section, titled with the item name and the table.
- Its second row is the legend card.
- After that, each chart of its table is a section alone on a full-width row. For a recurring spread expansion, the next occurrences of the chosen window in EVE time follow as a section of their own.

The figures section SHALL show each figure as a KPI card, in labelled groups in this order:
- **Fees**: b, t
- **Prices**: B, best sell, S
- **Margin**: c, r, Net, m
- **Size & liquidity**: U, V7, V30, ρ, q_buy, q_sell
- **Timing & risk**: σ, T_fill, T_max, V_min
- **Result**: ISK/d

A card's name SHALL be the figure's short label as in the table headers, or "Best sell" for the current best sell, with its full name on hover. Its value SHALL be the figure. A figure that is computed from others SHALL have a detail line with the computation using this item's numbers, for example "10,000.00 × 1.0141" for c. The formula and meaning of every figure SHALL be in the legend card, not in the figures section. Figures that cannot be computed for the item SHALL be left out, as before.

The charts per table are:
- **Recurring spread expansion**: the spread by hour of day and by weekday over 30 days, and the past peaks of the chosen window with S marked
- **Classic spread trade**: best buy, best sell and spread over 30 days, and the orders and units within 1% per side
- **Mean reversion**: the daily average with SMA 20 and Bollinger bands, and RSI 14
- **Trend**: the daily average with SMA 5 and SMA 20, and the daily volume

When the table does not list the item right now, the page SHALL say so and why, and SHALL still show what is known of the item. An item the station no longer trades SHALL be shown under its name, or as "Type <id>", with that reason.

#### Scenario: Open from a row
- **WHEN** a user with `?chars=<id>` and the trend table sorted by σ clicks a row of the mean reversion table
- **THEN** the browser navigates to `/opportunities/reversion/<type id>` carrying `chars=<id>` and the trend sorting, the page shows the item's figures, its legend card, its Bollinger chart and its RSI chart each on its own row, and the main menu marks "Opportunities" as current

#### Scenario: Sub-menu of an expansion page
- **WHEN** a user opens the detail page of a row in the recurring spread expansion table
- **THEN** the sub-menu lists "← Recurring spread expansion", "Figures", "Legend", one entry per chart and "Next occurrences"

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

## ADDED Requirements

### Requirement: Legend
Every short label used by the opportunity tables and the detail page SHALL have one legend entry: the short label, its full name, its meaning and, for a computed figure, its formula. The same label SHALL have the same entry, and the same full name on hover, wherever it is shown. The entries SHALL cover Item (including the "own buy" and "own sell" marks), b, t, B, Best sell, S, c, r, Net, m, U, V7, V30, ρ, q_buy, q_sell, σ, T_fill, T_max, V_min, ISK/d, n b/s, Window, Next (UTC), %B, RSI, SMA5, SMA20 and Cross. The formulas SHALL be those of the shared figures and of the table rules, for example c = B × (1 + b), m = (r − c) / c, T_max = min(3, (m / 2σ)²) days, and V_min as the V at which T_fill equals T_max. The entry of B SHALL say that it is the current best buy and not a suggested price.

The legend of a table lists the table's columns in their order. The legend of a detail page lists its figures in the order of the figures section.

#### Scenario: Same wording everywhere
- **WHEN** a user reads the legend entry of T_max on the trend page and on a trend detail page
- **THEN** both show the same full name, meaning and formula min(3, (m / 2σ)²) days

#### Scenario: Table legend follows the columns
- **WHEN** a user opens the legend on the mean reversion page
- **THEN** it lists Item, B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, %B and RSI, and no entry for Window or SMA5

### Requirement: Table explainer card
Every opportunity table page SHALL show an explainer card for its table below the pagination, or below the table when it has no rows. The card SHALL be a disclosure titled "How to read this table" that is closed when the page loads. Its open state SHALL NOT be stored, and it SHALL stay as the user left it across a live refresh, a change of page or page size, a change of sorting and a change of the opportunity filters.

Every explainer SHALL hold, in this order:
1. **Legend**: the table's legend.
2. **Signal**: when the table lists an item and how its S is derived, explained for that table alone.
3. **Shared figures**: the intermediate figures that are not columns (c, r, U, market share, ρ, q_buy, q_sell) with their formulas, and the hard exclusion. This is the only part worded the same in every explainer.
4. **Worked example**: one illustrative row of that table, continuing its signal into every column of the table, read column by column. It SHALL say that the row passes the default filters and how far V is above V_min.
5. **Order playbook**: how to act on a row of that table.

The worked examples SHALL share b = 1.41%, t = 3.375%, a capital per item of 100,000,000, V7 = 80,000, ρ = 0.5, and 3 buy and 1 sell orders within 1%, which give q_buy = 10,000 and q_sell = 20,000. Everything else is per table:

| | Recurring spread expansion | Classic spread trade | Mean reversion | Trend |
|---|---|---|---|---|
| Signal input | peaks 12,000, 12,200, 12,500, 12,600, 12,800, 13,000, 13,100, 13,300, 13,500, 14,000 of the window "daily 18:00"; 25th percentile 12,525 | best sell 11,500.00 | SMA 20 10,537.40, %B −0.08, RSI 27 | best sell 11,800.00; SMA5 crossed above SMA20 two days ago; 5-day volume 95,000 against 80,000 over 20 days |
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

Every playbook SHALL state the common steps once: place a buy order for U units one tick above B, check that m after outbidding still clears the minimum, and expect both orders to fill within about T_fill, re-checking the row when they have not filled within T_max. Each SHALL then give its table's own steps:
- **Recurring spread expansion**: place the buy order well before the window. Once it has filled, place the sell order at S before the next start shown in the row. When the sell order has not filled by the end of the window, wait for the next occurrence or re-check the row, instead of chasing the price down.
- **Classic spread trade**: place the sell order at S, one tick below the best sell, as soon as the buy order has filled. Watch both sides, because being outbid or undercut stops the fills. Re-price by one tick only while m still clears the minimum.
- **Mean reversion**: buy while %B is below 0 or RSI below 30. Sell at S, the SMA 20 the price is expected to return to. When the price has not recovered within T_max, or keeps falling, drop the trade instead of averaging down.
- **Trend**: buy shortly after the crossing. Sell one tick below the best sell, following it upwards while the trend holds. Exit when SMA5 falls back below SMA20 or the row leaves the table.

The example numbers SHALL be fixed. They SHALL NOT depend on the table's rows, the filters, the fee character or the market data. Only the explainer SHALL give buy-side guidance, and only as these general rules. The table rows SHALL still suggest no buy price.

#### Scenario: Collapsed on load
- **WHEN** a signed-in user opens `/opportunities/classic`
- **THEN** below the pagination there is a closed "How to read this table" card, and opening it shows the legend, the signal, the shared figures, the worked example and the order playbook

#### Scenario: Unique per table
- **WHEN** a signed-in user opens the explainer on `/opportunities/reversion`
- **THEN** its signal explains the Bollinger band, RSI and %B, its worked example shows B = 9,000.00, S = 10,530.00, m ≈ 9.85%, T_max ≈ 2.70 days and ≈ 5.99M ISK/d, and its playbook says to sell at the SMA 20 and not to average down

#### Scenario: Example continues the signal
- **WHEN** a signed-in user opens the explainer on `/opportunities/expansion`
- **THEN** the worked example uses S = 12,520.00 from the signal's peaks, shows the window "daily 18:00" with a next start, and reads ≈ 17.55% and ≈ 11.87M ISK/d

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
