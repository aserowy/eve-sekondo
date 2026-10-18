## ADDED Requirements

### Requirement: Page types
Every signed-in page SHALL be one of two page types:
- A **window page** fills the browser window below its bars and SHALL NOT scroll vertically as a whole at 1280px width or more. Its panels share the window's height and scroll inside themselves. The orders page and the wealth page are window pages.
- A **scrolling page** stacks full-width rows of panels from top to bottom and MAY scroll vertically. The dashboard, the characters page, the opportunities page and the opportunity detail page are scrolling pages.

#### Scenario: Window page does not scroll
- **WHEN** a user with 200 sell orders opens the orders page at 1920×1080
- **THEN** the page itself cannot be scrolled vertically

#### Scenario: Scrolling page scrolls
- **WHEN** a user opens the opportunities page at 1920×1080 with 20 rows in every table
- **THEN** the page scrolls vertically and the four table panels are stacked, each as wide as the content area

### Requirement: Content blocks are panels
Every block of content on a signed-in page SHALL be a panel with a title, apart from the top bar, the section sub-menu and the filter box. Pages SHALL NOT show a page heading or loose text outside a panel or a bar. The navigation's current-page marker identifies the page.

#### Scenario: No page heading
- **WHEN** a signed-in user opens the dashboard, the orders page, the wealth page, the characters page, the opportunities page or an opportunity detail page
- **THEN** the page has no `h1` heading and no heading or paragraph outside a panel or a bar

### Requirement: Section sub-menu
Every scrolling page with more than one row SHALL show a section sub-menu directly below the top bar, at most 24px high at 1280px width or more. It SHALL list one link per row in the order of the rows. Activating a link SHALL scroll to that row and put the row's fragment into the URL. Opening a URL with a row's fragment SHALL scroll to that row. A row that the sub-menu scrolled to SHALL have its title fully visible below the bars, at the top of the content area when the page is long enough. The sub-menu SHALL mark one link as current: right after a jump, the link of the row jumped to; while the user scrolls, the link of the row at the top of the content area below the bars, or the last row's link when the page is scrolled to its end. Scrolling SHALL NOT change the URL. Live refreshes SHALL keep the marker, the URL's fragment and the scroll position. The characters page has a single row and SHALL show no sub-menu. Window pages SHALL show no sub-menu.

#### Scenario: Dashboard sub-menu
- **WHEN** a signed-in user opens the dashboard
- **THEN** a sub-menu below the top bar lists "Totals", "Characters", "Split & top items", "Value", "Orders" and "Wealth"

#### Scenario: Jump to a row
- **WHEN** a user activates "Wealth" in the dashboard's sub-menu
- **THEN** the page scrolls until the wealth over-time panel's title is visible below the bars, the URL ends in the wealth row's fragment, and the sub-menu marks "Wealth" as current

#### Scenario: Open with a fragment
- **WHEN** a user opens the opportunities page with the trend table's fragment
- **THEN** the page opens scrolled to the trend panel, with its title visible below the bars

#### Scenario: Marker follows scrolling
- **WHEN** a user scrolls the dashboard by hand from the top until the value over-time panel reaches the top of the content area
- **THEN** the sub-menu marks "Value" as current and the URL is unchanged

#### Scenario: End of the page
- **WHEN** a user scrolls the dashboard by hand to its end
- **THEN** the sub-menu marks "Wealth" as current

#### Scenario: No sub-menu on single-row and window pages
- **WHEN** a signed-in user opens the characters page and then the orders page
- **THEN** neither page shows a sub-menu

### Requirement: Filter box
A filter that affects more than one content block of a page SHALL be placed in the page's filter box. A filter that affects a single content block SHALL be placed in that block's panel title. The filter box SHALL be a full-width strip with a thin border and no title, between the bars above it and the content. It SHALL hold the page's filters, and on its right end, dimmed text that describes the whole page when the page has such text. It SHALL be at most 28px high while its content fits on one line. When its content does not fit, it SHALL wrap instead of scrolling sideways. A filter value the page rejects SHALL be reported inside the filter box. A page without such filters or text SHALL show no filter box. The filter boxes are:
- dashboard: the character filter and the time range
- orders page: the character filter
- wealth page: the character filter
- opportunities page: the character filter, the opportunity filters, and as text the station name and the fees used
- opportunity detail page: the character filter, and as text the station name and the fees used

The wealth page's time range affects only its graph and SHALL stay in the graph panel's title.

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

### Requirement: Sticky bars
On scrolling pages, the top bar, the section sub-menu and the filter box SHALL stay visible at the top of the window while the page scrolls.

#### Scenario: Bars stay while scrolling
- **WHEN** a user scrolls the dashboard to its end at 1920×1080
- **THEN** the top bar, the sub-menu and the filter box are still at the top of the window

### Requirement: Table rows on scrolling pages
On scrolling pages, a table SHALL show at most 20 body rows. Beyond 20 rows it SHALL scroll inside its panel. On window pages, a table SHALL fill its panel and scroll inside it. While a table scrolls, its header row SHALL stay visible at the top of its panel.

#### Scenario: Capped opportunities table
- **WHEN** the classic spread trade table has 50 rows and the user opens the opportunities page at 1920×1080
- **THEN** the table's panel shows 20 rows and scrolls inside itself to show the others, with its header row staying visible

#### Scenario: Short table
- **WHEN** the trend table has 4 rows
- **THEN** its panel is just tall enough for 4 rows and has no inner scrolling

### Requirement: Wide tables
A table whose columns do not fit its panel SHALL scroll sideways inside its panel. Table cells SHALL NOT wrap their text. The page SHALL NOT scroll sideways because of a table.

#### Scenario: Too many columns
- **WHEN** a signed-in user opens the opportunities page in a 1280px wide window
- **THEN** each table that does not fit scrolls sideways inside its panel, no cell wraps, and the page does not scroll horizontally

### Requirement: Shared table style
Every data table SHALL use one table style: the same header, row height, alternating row background, hover background, and right-aligned tabular figures for numbers. A table whose rows open a page SHALL show a pointer cursor over its rows.

#### Scenario: Same striping everywhere
- **WHEN** a signed-in user compares the second body row of the dashboard breakdown, the wealth items table and an opportunities table
- **THEN** all three have the same alternating background

### Requirement: Details on their own page
Details about an entry SHALL be shown on a page of their own that the entry links to, and SHALL NOT be shown in a floating window, a dialog or an overlay.

#### Scenario: Opportunity details
- **WHEN** a user clicks a row of the trend table
- **THEN** the browser navigates to that item's trend detail page, and no floating window opens

## MODIFIED Requirements

### Requirement: Top bar
Every signed-in page SHALL show a single top bar, at most 36px high at 1280px width or more, holding the brand, the navigation, the characters link and sign out, and nothing else. Filters SHALL NOT be in the top bar.

#### Scenario: Dashboard bar
- **WHEN** a signed-in user opens the dashboard at 1920×1080
- **THEN** the top bar is a single row at most 36px high containing the navigation, and neither the character filter nor the time range control is in it

#### Scenario: Characters page bar
- **WHEN** a signed-in user opens the characters page
- **THEN** the top bar shows the navigation and no character filter

### Requirement: Dashboard rows
The dashboard SHALL be laid out in rows, top to bottom: the tiles; the per-character breakdown across the full width; the per-character split graph, the top items by sell value graph and the top items by buy escrow graph side by side; then the value over time graph, the order count over time graph and the wealth over time graph, each alone on a row across the full width. At a window of 1920×1080 the bars, the tiles, the breakdown and the split and top-items row SHALL be inside the window without scrolling. The breakdown SHALL grow with its rows up to the 20 rows of a table on a scrolling page and SHALL scroll inside itself beyond it. Each over-time graph SHALL be at least 16rem high. The sub-menu's "Totals" link leads to the tiles, "Characters" to the breakdown, "Split & top items" to the split and top-items row, and "Value", "Orders" and "Wealth" to the value, order count and wealth over-time graphs.

#### Scenario: First screen
- **WHEN** a signed-in user with three characters opens the dashboard at 1920×1080
- **THEN** the tiles, the breakdown, the split graph and both top-item graphs are inside the window, and the split and both top-item graphs share one row below the breakdown

#### Scenario: Breakdown spans the width
- **WHEN** a signed-in user opens the dashboard at 1920×1080
- **THEN** the breakdown is as wide as the tile row

#### Scenario: Over-time graphs on their own rows
- **WHEN** a signed-in user with orders and wealth history opens the dashboard at 1920×1080
- **THEN** the value, the order count and the wealth over-time graphs are each as wide as the tile row, each below the previous one, below the split and top-items row

#### Scenario: Many characters
- **WHEN** a user with thirty characters opens the dashboard at 1920×1080
- **THEN** the breakdown shows 20 rows and scrolls inside itself, and the split and top-items row is still inside the window

### Requirement: Scrolling panels with sticky headers
The orders page's tables and the wealth page's items table SHALL fill their panel and scroll inside it, as tables on a window page. The dashboard breakdown and the opportunities tables SHALL follow the row limit of tables on a scrolling page. While any of these tables scrolls, its header row SHALL stay visible at the top of its panel.

#### Scenario: Scroll the sell table
- **WHEN** a user with 200 sell orders scrolls the sell table to its end at 1920×1080
- **THEN** the sell table's header row stays visible, the buy table stays in place, and the page itself has not scrolled

### Requirement: Freshness side column
Status and freshness information that belongs to a whole page SHALL be shown in a freshness side column on the right of the content at 1280px width or more, scrolling inside itself when it does not fit. The orders page, the wealth page and the opportunities page SHALL have one. On the orders page it also holds the fees per location. On a window page the column is as tall as the content. On a scrolling page the column SHALL stay in place while the page scrolls and SHALL be as tall as the window below the bars. The order tables SHALL be stacked, the sell table above the buy table, each using the full width left of the side column or of its collapsed strip. The opportunities tables SHALL use the full width left of the side column or of its collapsed strip.

#### Scenario: Orders page columns
- **WHEN** a signed-in user opens the orders page at 1920×1080 and unfolds the freshness
- **THEN** the freshness and fees are right of the order tables, and the buy table is below the sell table

#### Scenario: Wealth page columns
- **WHEN** a signed-in user opens the wealth page at 1920×1080 and unfolds the freshness
- **THEN** the freshness is right of the graph and the items table, and the items table is below the graph

#### Scenario: Opportunities column stays while scrolling
- **WHEN** a signed-in user opens the opportunities page at 1920×1080, unfolds the freshness and scrolls to the trend table
- **THEN** the freshness is still right of the tables, fully inside the window below the bars

### Requirement: Collapsible freshness
On every page with a freshness side column, the column SHALL start collapsed when the page loads: a narrow strip with an icon-only toggle. Activating the toggle SHALL unfold the column and activating it again SHALL fold it. The toggle SHALL expose its state to assistive technology, SHALL have the accessible name "Freshness", and SHALL show "Show freshness" or "Hide freshness" on hover. While collapsed, the strip SHALL show a warning count in the error style, and SHALL show no number when the count is zero. The warning count is:
- orders page: the number of selected characters that are stale or need re-login
- wealth page: the number of selected characters whose wallet or assets are stale, or who need re-login
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

### Requirement: Dense theme
Every page SHALL use one dark theme served by the application itself, without any third-party stylesheet. Body text SHALL be at most 13px. Table body rows with a single line of text SHALL be at most 22px high. Numeric table cells SHALL be right-aligned with tabular figures. Content areas SHALL be framed as panels with a thin border and a small uppercase title instead of cards with shadows. The theme SHALL keep a success style and an error style for status markers. Pages SHALL render without any Content Security Policy violation.

#### Scenario: Compact text and rows
- **WHEN** a signed-in user opens the orders page with at least one sell order
- **THEN** the computed body font size is at most 13px and a sell table row is at most 22px high

#### Scenario: No third-party stylesheet
- **WHEN** any page is loaded
- **THEN** the only stylesheet it loads is the application's own

#### Scenario: No policy violations
- **WHEN** a signed-in user visits the dashboard, the orders page, the wealth page, the characters page, the opportunities page and an opportunity detail page, and uses a sub-menu link on each page that has one
- **THEN** the browser reports no Content Security Policy violations
