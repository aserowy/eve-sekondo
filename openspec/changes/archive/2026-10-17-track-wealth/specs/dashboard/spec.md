# Spec Delta

## ADDED Requirements

### Requirement: Wealth tile
The dashboard SHALL show a "Wealth" tile with the current total wealth of the selected characters, as defined by the wealth-sync capability, and SHALL link it to the wealth page with the current character selection. The amount SHALL follow the ISK amount rule of the dashboard.

#### Scenario: Wealth across characters
- **WHEN** the first selected character's wealth is 200,000,000.00 ISK and the second's is 50,000,000.00 ISK
- **THEN** the wealth tile reads "250m" and hovering it shows "250,000,000.00 ISK"

#### Scenario: Tile links to the wealth page
- **WHEN** a user with `?chars=<id>` activates the wealth tile
- **THEN** the wealth page opens with `chars=<id>`

### Requirement: Wealth over time on the dashboard
The dashboard SHALL show a graph of total wealth over the selected time range as a single line. At every point, the line SHALL equal the top edge of the wealth page's graph for the same selection and range. Its last point SHALL equal the wealth tile. When there is no wealth history in the range, the graph SHALL say that there is no history yet.

#### Scenario: Wealth develops over time
- **WHEN** a character's wallet was 10,000,000.00 ISK at a poll two hours ago and 15,000,000.00 ISK at a poll one hour ago, with nothing else recorded, and the range is 24 hours
- **THEN** the wealth line shows 10,000,000.00 ISK between those polls and 15,000,000.00 ISK from the later poll to now

## MODIFIED Requirements

### Requirement: Time range
The over-time graphs SHALL cover the time range given by the `range` query parameter: `24h`, `7d`, `30d` or `90d`. A missing or unknown value SHALL select `7d`. The over-time graphs are the value graph, the order count graph and the wealth graph. The dashboard SHALL show one control to switch the range, which applies to all of them at once. Switching SHALL update the URL and all over-time graphs without a full page reload, and SHALL keep the character selection. The range SHALL NOT affect the tiles, the breakdown, the per-character split or the top items, which always show current data. The range SHALL NOT be carried to other pages.

#### Scenario: Default range
- **WHEN** a user opens the dashboard without a `range` parameter
- **THEN** the over-time graphs cover the last 7 days and the range control marks 7 days

#### Scenario: Switch range
- **WHEN** a user with `?chars=<id>` selects 30 days in the range control
- **THEN** the URL carries `chars=<id>` and `range=30d`, and the value, order count and wealth graphs all cover the last 30 days

#### Scenario: Unknown range
- **WHEN** a user requests the dashboard with `?range=1y`
- **THEN** the over-time graphs cover the last 7 days

### Requirement: Live dashboard
The dashboard SHALL re-fetch its totals, breakdown and graphs, including the wealth tile and the wealth graph, when an `orders-updated` or `wealth-updated` signal arrives and after the event stream reconnects. It SHALL keep the current selection and time range, and SHALL NOT do a full page reload.

#### Scenario: Totals refresh after a poll
- **WHEN** a user has the dashboard open and a poll raises the price of one of their sell orders
- **THEN** the sell value total, the character's row, the per-character split, the last point of the sell value line and the wealth tile show the new value without the user reloading

#### Scenario: Wealth refreshes after a wallet poll
- **WHEN** a user has the dashboard open and a wallet poll records a higher balance
- **THEN** the wealth tile and the last point of the wealth graph show the new total without the user reloading

#### Scenario: Selection and range kept by a live refresh
- **WHEN** the dashboard is filtered to one character with the 24-hour range and an `orders-updated` signal arrives
- **THEN** the refreshed totals, breakdown and graphs still cover only that character, and the over-time graphs still cover 24 hours
