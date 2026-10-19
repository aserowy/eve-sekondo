## MODIFIED Requirements

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
- **THEN** the sell value, buy escrow, wealth, sell order and buy order figures are each a KPI card of the same width, each wider than it is high
