# Spec Delta

## MODIFIED Requirements

### Requirement: Dense theme
Every page SHALL use one dark theme served by the application itself, without any third-party stylesheet. Body text SHALL be at most 13px. Table body rows with a single line of text SHALL be at most 22px high. Numeric table cells SHALL be right-aligned with tabular figures. Content areas SHALL be framed as panels with a thin border and a small uppercase title instead of cards with shadows. The theme SHALL keep a success style and an error style for status markers. Pages SHALL render without any Content Security Policy violation.

#### Scenario: Compact text and rows
- **WHEN** a signed-in user opens the orders page with at least one sell order
- **THEN** the computed body font size is at most 13px and a sell table row is at most 22px high

#### Scenario: No third-party stylesheet
- **WHEN** any page is loaded
- **THEN** the only stylesheet it loads is the application's own

#### Scenario: No policy violations
- **WHEN** a signed-in user visits the dashboard, the orders page, the wealth page, the trades page, the characters page, the opportunities page and an opportunity detail page, and uses a sub-menu link on each page that has one
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Scrolling panels with sticky headers
The orders page's tables, the wealth page's items table and the trades table SHALL fill their panel and scroll inside it, as tables on a window page. The dashboard breakdown and the opportunities tables SHALL follow the row limit of tables on a scrolling page. While any of these tables scrolls, its header row SHALL stay visible at the top of its panel.

#### Scenario: Scroll the sell table
- **WHEN** a user with 200 sell orders scrolls the sell table to its end at 1920×1080
- **THEN** the sell table's header row stays visible, the buy table stays in place, and the page itself has not scrolled

#### Scenario: Scroll the trades table
- **WHEN** a user with 300 transactions in the range scrolls the trades table to its end at 1920×1080
- **THEN** the trades table's header row and totals row stay visible, and the page itself has not scrolled

### Requirement: Freshness side column
Status and freshness information that belongs to a whole page SHALL be shown in a freshness side column on the right of the content at 1280px width or more, scrolling inside itself when it does not fit. The orders page, the wealth page, the trades page and the opportunities page SHALL have one. On the orders page it also holds the fees per location. On a window page the column is as tall as the content. On a scrolling page the column SHALL stay in place while the page scrolls and SHALL be as tall as the window below the bars. The order tables SHALL be stacked, the sell table above the buy table, each using the full width left of the side column or of its collapsed strip. The trades table and the opportunities tables SHALL use the full width left of the side column or of its collapsed strip.

#### Scenario: Orders page columns
- **WHEN** a signed-in user opens the orders page at 1920×1080 and unfolds the freshness
- **THEN** the freshness and fees are right of the order tables, and the buy table is below the sell table

#### Scenario: Wealth page columns
- **WHEN** a signed-in user opens the wealth page at 1920×1080 and unfolds the freshness
- **THEN** the freshness is right of the graph and the items table, and the items table is below the graph

#### Scenario: Opportunities column stays while scrolling
- **WHEN** a signed-in user opens the opportunities page at 1920×1080, unfolds the freshness and scrolls to the trend table
- **THEN** the freshness is still right of the tables, fully inside the window below the bars

#### Scenario: Trades page columns
- **WHEN** a signed-in user opens the trades page at 1920×1080 and unfolds the freshness
- **THEN** the freshness is right of the trades table

### Requirement: Collapsible freshness
On every page with a freshness side column, the column SHALL start collapsed when the page loads: a narrow strip with an icon-only toggle. Activating the toggle SHALL unfold the column and activating it again SHALL fold it. The toggle SHALL expose its state to assistive technology, SHALL have the accessible name "Freshness", and SHALL show "Show freshness" or "Hide freshness" on hover. While collapsed, the strip SHALL show a warning count in the error style, and SHALL show no number when the count is zero. The warning count is:
- orders page: the number of selected characters that are stale or need re-login
- wealth page: the number of selected characters whose wallet or assets are stale, or who need re-login
- trades page: the number of selected characters whose transactions are stale, or who need re-login
- opportunities page: one each for a stale station book, a failed last book refresh and a failed last history run

The state SHALL stay as the user left it across live refreshes and changes of filter, sorting and range on the same page, and SHALL return to collapsed when the page is loaded again. While collapsed, the content SHALL use the width the column gave up.

#### Scenario: Starts collapsed
- **WHEN** a signed-in user opens the orders page at 1920×1080
- **THEN** the freshness and fees are not shown, a narrow strip with the toggle is right of the tables, and the sell table reaches to that strip

#### Scenario: Unfold and fold
- **WHEN** the user activates the toggle on the orders page, and then activates it again
- **THEN** the freshness and fees are shown right of the tables after the first activation and hidden after the second

#### Scenario: Warning count
- **WHEN** one of two selected characters needs re-login and the user opens the orders page
- **THEN** the collapsed strip shows "1" in the error style

#### Scenario: No warnings
- **WHEN** every selected character is fresh and the user opens the wealth page
- **THEN** the collapsed strip shows no number

#### Scenario: Opportunities warning count
- **WHEN** the station book is stale and its last refresh failed, and the last history run succeeded
- **THEN** the opportunities page's collapsed strip shows "2" in the error style

#### Scenario: Kept across a live refresh
- **WHEN** the user has unfolded the freshness on the orders page and an `orders-updated` signal arrives
- **THEN** the refreshed freshness is still unfolded

#### Scenario: Kept across a filter change
- **WHEN** the user has unfolded the freshness on the wealth page and deselects a character
- **THEN** the freshness is still unfolded and shows only the selected characters

#### Scenario: Reload starts collapsed
- **WHEN** the user has unfolded the freshness and reloads the page
- **THEN** the freshness is collapsed

#### Scenario: Trades warning count
- **WHEN** one of two selected characters' last successful transactions poll is three hours old and the user opens the trades page
- **THEN** the collapsed strip shows "1" in the error style

### Requirement: Page types
Every signed-in page SHALL be one of two page types:
- A **window page** fills the browser window below its bars and SHALL NOT scroll vertically as a whole at 1280px width or more. Its panels share the window's height and scroll inside themselves. The orders page, the wealth page and the trades page are window pages.
- A **scrolling page** stacks full-width rows of panels from top to bottom and MAY scroll vertically. The dashboard, the characters page, the opportunities page and the opportunity detail page are scrolling pages.

#### Scenario: Window page does not scroll
- **WHEN** a user with 200 sell orders opens the orders page at 1920×1080
- **THEN** the page itself cannot be scrolled vertically

#### Scenario: Scrolling page scrolls
- **WHEN** a user opens the opportunities page at 1920×1080 with 20 rows in every table
- **THEN** the page scrolls vertically and the four table panels are stacked, each as wide as the content area

#### Scenario: Trades page does not scroll
- **WHEN** a user with 300 transactions in the range opens the trades page at 1920×1080
- **THEN** the page itself cannot be scrolled vertically

### Requirement: Content blocks are panels
Every block of content on a signed-in page SHALL be a panel with a title, apart from the top bar, the section sub-menu and the filter box. Pages SHALL NOT show a page heading or loose text outside a panel or a bar. The navigation's current-page marker identifies the page.

#### Scenario: No page heading
- **WHEN** a signed-in user opens the dashboard, the orders page, the wealth page, the trades page, the characters page, the opportunities page or an opportunity detail page
- **THEN** the page has no `h1` heading and no heading or paragraph outside a panel or a bar

### Requirement: Filter box
A filter that affects more than one content block of a page SHALL be placed in the page's filter box. A filter that affects a single content block SHALL be placed in that block's panel title. The filter box SHALL be a full-width strip with a thin border and no title, between the bars above it and the content. It SHALL hold the page's filters, and on its right end, dimmed text that describes the whole page when the page has such text. It SHALL be at most 28px high while its content fits on one line. When its content does not fit, it SHALL wrap instead of scrolling sideways. A filter value the page rejects SHALL be reported inside the filter box. A page without such filters or text SHALL show no filter box. The filter boxes are:
- dashboard: the character filter and the time range
- orders page: the character filter
- wealth page: the character filter
- trades page: the character filter
- opportunities page: the character filter, the opportunity filters, and as text the station name and the fees used
- opportunity detail page: the character filter, and as text the station name and the fees used

The wealth page's time range affects only its graph and SHALL stay in the graph panel's title. The trades page's time range affects only its table and SHALL stay in the table panel's title.

#### Scenario: Orders filter box
- **WHEN** a signed-in user with three characters opens the orders page at 1280×800
- **THEN** the character filter is in a single-line strip at most 28px high between the top bar and the order tables, and not in the top bar

#### Scenario: Dashboard filter box
- **WHEN** a signed-in user opens the dashboard
- **THEN** the character filter and the time range control are in the filter box

#### Scenario: Wealth range stays with its graph
- **WHEN** a signed-in user opens the wealth page
- **THEN** the character filter is in the filter box and the time range control is in the graph panel's title

#### Scenario: No filter box on the characters page
- **WHEN** a signed-in user opens the characters page
- **THEN** the page shows no filter box

#### Scenario: Narrow window wraps
- **WHEN** a signed-in user with five characters opens the opportunities page in a 1024px wide window
- **THEN** the filter box wraps onto more lines and the page does not scroll horizontally

#### Scenario: Trades range stays with its table
- **WHEN** a signed-in user opens the trades page
- **THEN** the character filter is in the filter box and the time range control is in the trades table panel's title
