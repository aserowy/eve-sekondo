## MODIFIED Requirements

### Requirement: Character filter on the dashboard
The dashboard SHALL accept the `chars` parameter and show the filter control as described by the character filter capability. Every figure, row, freshness indicator and graph SHALL cover only the selected characters. Changing the selection SHALL keep the user on the dashboard and keep the selected time range.

#### Scenario: Narrow the selection
- **WHEN** a user with two characters on `/dashboard?range=30d` deselects the second in the dashboard's filter control
- **THEN** the URL is the dashboard with `chars` holding the first character's ID and `range=30d`, the breakdown lists only the first character, and the totals and graphs cover only its orders

#### Scenario: Selection carried to the orders page
- **WHEN** a user with a narrowed selection on the dashboard follows the "Orders" menu entry
- **THEN** the sell orders page opens with the same selection

### Requirement: Wealth tile
The dashboard SHALL show a "Wealth" KPI card with the current total wealth of the selected characters, as defined by the wealth-sync capability, and SHALL link it to the wealth page with the current character selection. The amount SHALL follow the ISK amount rule of the dashboard. Every dashboard tile, including the sell value, buy escrow, sell order and buy order tiles, SHALL be a KPI card as described by the ui-layout capability.

#### Scenario: Wealth across characters
- **WHEN** the first selected character's wealth is 200,000,000.00 ISK and the second's is 50,000,000.00 ISK
- **THEN** the wealth card reads "250m" and hovering it shows "250,000,000.00 ISK"

#### Scenario: Tile links to the wealth page
- **WHEN** a user with `?chars=<id>` activates the wealth card
- **THEN** the wealth page opens with `chars=<id>`

#### Scenario: Tiles are cards
- **WHEN** a signed-in user opens the dashboard
- **THEN** the sell value, buy escrow, wealth, sell order and buy order figures are each a square KPI card of the same size
