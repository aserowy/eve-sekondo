## MODIFIED Requirements

### Requirement: Opportunities signal
The system SHALL deliver an `opportunities-updated` signal over the event stream of every signed-in account after each successful station book refresh and after each history run that stored new history. The station market is public, so every account receives it. The signal SHALL name only what changed and SHALL NOT carry price, order or item data. The opportunities page SHALL re-fetch its freshness, its tables and its sub-menu counts when the signal arrives, and an open opportunity detail page SHALL re-fetch its figures and charts. Both SHALL keep the character filter, the filters, the sorting and the scroll position. The dashboard, orders and wealth pages SHALL NOT re-fetch because of it.

#### Scenario: Signal after a book refresh
- **WHEN** two accounts have the opportunities page open and the station book is refreshed
- **THEN** both streams deliver an `opportunities-updated` signal and both pages show the new figures without reloading

#### Scenario: Opportunities page follows the signal
- **WHEN** a user has the opportunities page open with the trend table sorted by σ, and the station book is refreshed
- **THEN** the freshness, the four tables and the sub-menu counts show the new figures without reloading, the trend table is still sorted by σ, and the character filter and the opportunity filters are unchanged

#### Scenario: Detail page follows the signal
- **WHEN** a user has an opportunity detail page open and the station book is refreshed
- **THEN** the detail page shows the new figures without reloading

#### Scenario: Failed refresh
- **WHEN** a station book refresh fails
- **THEN** no `opportunities-updated` signal is delivered

#### Scenario: Signal carries no data
- **WHEN** an `opportunities-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

#### Scenario: Orders page ignores it
- **WHEN** an `opportunities-updated` signal arrives while the orders page is open
- **THEN** the orders page does not re-fetch

#### Scenario: Opportunity tables keep their place
- **WHEN** a user has scrolled the opportunities page down to the trend table and an `opportunities-updated` signal arrives
- **THEN** the refreshed page is at the same scroll position
