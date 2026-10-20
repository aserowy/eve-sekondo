# Spec Delta

## MODIFIED Requirements

### Requirement: Opportunities page
The system SHALL provide one page per opportunity table for signed-in users, at `/opportunities/expansion` (recurring spread expansion), `/opportunities/classic` (classic spread trade), `/opportunities/reversion` (mean reversion) and `/opportunities/trend` (trend). Each accepts the character filter like the other character-scoped views. Unauthenticated requests SHALL be redirected to the landing page. Each table page is laid out as a table page:
- Its sub-menu lists the four tables as text, in the order recurring spread expansion, classic spread trade, mean reversion and trend, without row counts, and marks the current one.
- Its page title is followed by the name of the station it analyses as subtitle.
- Its filter section holds the character filter, the fee character and the opportunity filters.
- It shows the one table titled after it, with its pagination. The recurring spread expansion page shows, above its table, how many days of hourly data have been collected while fewer than needed are.
- Its freshness section holds the station book's freshness, the market history's freshness and the number of days of own snapshots.

#### Scenario: Open the page
- **WHEN** a signed-in user opens `/opportunities/classic`
- **THEN** they see the sub-menu marking "Classic spread trade", the subtitle Jita IV - Moon 4 - Caldari Navy Assembly Plant below the page title, the filter section, the classic spread trade table and, below it, the freshness

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

### Requirement: Opportunity detail page
Every table row SHALL link to a detail page for that item and table at `/opportunities/<table>/<type id>`, where `<table>` is `expansion`, `classic`, `reversion` or `trend`. Clicking a row SHALL navigate to it. The detail page SHALL be for signed-in users only. Unauthenticated requests SHALL be redirected to the landing page, and an unknown table SHALL answer "not found". The detail page accepts the character filter and every table's sorting, carries them in its links, and computes its figures with the fees of the fee character and with the account's stored capital per item. The main menu SHALL mark "Opportunities" as current on it.

The detail page is laid out from top to bottom:
- Its section sub-menu starts with a link "← <table title>" back to its table's page, keeping the character filter and every table's sorting. It then lists the page's rows.
- Its page title is followed by the station name as subtitle.
- Its filter section holds the character filter and the fee character.
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
- **THEN** the URL keeps the detail page, the fee character group checks the fee character of the remaining selection, and the figures are recomputed with its fees

#### Scenario: Choose fees on the detail page
- **WHEN** a user with two characters selected checks the other character in the fee character group of a trend detail page
- **THEN** the URL keeps the detail page, the figures are recomputed with that character's fees, and the trend page then uses them as well

#### Scenario: Item dropped out
- **WHEN** a user opens the classic detail page of an item that the classic table no longer lists because its margin is not positive
- **THEN** the page says that the item is no longer listed in this table and why, and still shows its figures

#### Scenario: Unknown table
- **WHEN** a signed-in user requests `/opportunities/arbitrage/34`
- **THEN** the answer is "not found"

#### Scenario: Signed out
- **WHEN** an unauthenticated user requests `/opportunities/trend/34`
- **THEN** they are redirected to the landing page
