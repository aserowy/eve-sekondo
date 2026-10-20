# Spec Delta

## MODIFIED Requirements

### Requirement: Table page layout
Every page whose main content is a data table SHALL show, from top to bottom: the page header with the page title, the sub-menu if the page has one and the filter section; the KPI cards if the page has KPIs; the table and its pagination; the explainer card if the page has one; and the page footer with the freshness. The table pages are the sell orders page, the buy orders page, the wealth page, the trades page and each opportunity table page. On the wealth page, the graph SHALL sit between the KPI cards and the items table. Each page SHALL show exactly one data table.

#### Scenario: Sell page order
- **WHEN** a signed-in user with sell orders opens the sell orders page
- **THEN** the page shows, from top to bottom, the page title, the sub-menu, the filter section, the KPI cards, the sell table, its pagination and the freshness footer

#### Scenario: Wealth page order
- **WHEN** a signed-in user opens the wealth page
- **THEN** the page shows, from top to bottom, the page title, the filter section, the KPI cards, the graph, the items table, its pagination and the freshness footer

#### Scenario: Opportunity page order
- **WHEN** a signed-in user opens the classic spread trade page
- **THEN** the page shows, from top to bottom, the page title, the sub-menu, the filter section, the classic spread trade table, its pagination, its explainer card and the freshness footer

#### Scenario: One table per page
- **WHEN** a signed-in user opens the classic spread trade page
- **THEN** it shows the classic spread trade table and no other opportunity table

### Requirement: Pagination
Every table on a table page SHALL be paginated on the server. The page number SHALL be given by the `page` query parameter, starting at 1, and the page size by the `per_page` query parameter, one of 25, 50, 100 or 200. A missing or unknown page size SHALL select 50. A missing, malformed or non-positive page number SHALL select page 1, and a page number past the last page SHALL select the last page. The page SHALL show a control to go to the previous and the next page, the current page and the number of pages as "Page x of y", and a control to choose the page size.

The pagination SHALL be the last block of the main content, below the table, except for an explainer card, which follows it. It SHALL NOT be part of the page footer. When the page is shorter than the window, the pagination SHALL sit at the bottom of the window, directly above the explainer card if the page has one and otherwise directly above the page footer, with the spare height between the table and the pagination. When the page is taller than the window, the pagination SHALL follow the table and SHALL only come into view when the page is scrolled to near its end. It SHALL NOT be fixed over the content. A live refresh or a change of page or page size SHALL keep this placement.

Going to another page or choosing another page size SHALL update the URL and the table without a full page reload, and SHALL keep the character selection, the filters, the range and the sorting. Changing the character selection, a filter, the range, the sorting or the page size SHALL show page 1. Pagination SHALL be applied after sorting. A table without rows SHALL show no pagination controls.

#### Scenario: Default page size
- **WHEN** a user with 120 sell orders opens the sell orders page
- **THEN** the table shows the first 50 orders and the pagination reads "Page 1 of 3"

#### Scenario: Next page
- **WHEN** that user activates the next page control
- **THEN** the URL carries `page=2`, the table shows orders 51 to 100, and the sorting and the character selection are unchanged

#### Scenario: Larger page size
- **WHEN** that user chooses a page size of 200 while on page 2
- **THEN** the URL carries `per_page=200` and `page=1`, and the table shows all 120 orders

#### Scenario: Page past the end
- **WHEN** a user with 120 sell orders opens `/orders/sell?page=9`
- **THEN** the table shows page 3

#### Scenario: Sorting resets the page
- **WHEN** a user on page 2 of the trades table changes the sorting or the range
- **THEN** the table shows page 1

#### Scenario: Pagination at the bottom of a short page
- **WHEN** a signed-in user with two sell orders opens the sell orders page at 1920×1080
- **THEN** the pagination is directly above the freshness footer at the bottom of the window, with free space between the table and the pagination, and the pagination is inside the main landmark

#### Scenario: Pagination above the explainer on a short page
- **WHEN** a signed-in user opens the classic spread trade page with two rows at 1920×1080
- **THEN** the pagination is directly above the closed explainer card, the explainer card is directly above the freshness footer at the bottom of the window, and there is free space between the table and the pagination

#### Scenario: Pagination scrolls in on a long page
- **WHEN** a user with 200 sell orders opens the sell orders page with a page size of 200 at 1280×800
- **THEN** the pagination is not in view, and after scrolling the page to its end it is in view directly below the table and above the freshness footer

#### Scenario: Placement kept after a page change
- **WHEN** a user with 60 sell orders on the sell orders page at 1920×1080 with a page size of 50 goes to page 2
- **THEN** the pagination is still directly above the freshness footer at the bottom of the window
