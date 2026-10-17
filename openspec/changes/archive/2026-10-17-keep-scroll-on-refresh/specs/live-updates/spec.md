# Spec Delta

## ADDED Requirements

### Requirement: Refreshes keep the scroll position
When a page re-fetches a part of itself because a signal arrived, or because the stream reconnected, every panel in that part that scrolls on its own SHALL keep its vertical and horizontal scroll position. If the refreshed content is shorter than the kept position, the panel SHALL show its end. Changes the user makes to the view (the character filter, a table's sorting, the time range, or navigating to another page) SHALL show the resulting view scrolled to the top. Keeping the position SHALL NOT change which part is re-fetched, the character filter, the sorting or the URL.

#### Scenario: Sell table keeps its place after a poll
- **WHEN** a user with 200 sell orders has scrolled the sell table halfway down, and a poll of their character delivers an `orders-updated` signal
- **THEN** the refreshed sell table is at the same scroll position, and the buy table is at its own previous position

#### Scenario: Market signal keeps the place
- **WHEN** a user has scrolled the sell table down, and a `market-updated` signal arrives
- **THEN** the refreshed sell table is at the same scroll position

#### Scenario: Fewer rows after a refresh
- **WHEN** a user has scrolled the sell table to its end, and a poll removes most of the orders so the table is shorter than the old position
- **THEN** the refreshed sell table shows its last row

#### Scenario: Sorting starts at the top
- **WHEN** a user has scrolled the sell table down and clicks the Price header
- **THEN** the sorted sell table is scrolled to the top

#### Scenario: Filter starts at the top
- **WHEN** a user has scrolled the sell table down and deselects a character in the filter
- **THEN** the filtered sell table is scrolled to the top

#### Scenario: Dashboard breakdown keeps its place
- **WHEN** a user with twenty characters has scrolled the dashboard breakdown down, and an `orders-updated` signal arrives
- **THEN** the refreshed breakdown is at the same scroll position

#### Scenario: Wealth items keep their place
- **WHEN** a user has scrolled the wealth items table down, and a `wealth-updated` signal arrives
- **THEN** the refreshed items table is at the same scroll position

#### Scenario: Freshness column keeps its place
- **WHEN** a user with twenty characters has scrolled the orders page's freshness column down, and a `fees-updated` signal arrives
- **THEN** the refreshed freshness column is at the same scroll position
