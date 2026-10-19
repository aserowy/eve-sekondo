# ui-layout Specification

## Purpose

Defines how Sekondo's pages use the screen: a dense, dark, cockpit-like layout that spends every pixel on information, consistent across all pages and built without a third-party CSS framework.

## Requirements

### Requirement: Dense theme
Every page SHALL be styled with the Pico CSS framework (version 2) in its dark theme, served by the application itself, plus the application's own stylesheet. No other stylesheet SHALL be loaded, and no stylesheet SHALL be loaded from another origin. The application's own stylesheet SHALL only make tables compact and lay out the sidebar, the sub-menu, the KPI cards and the pagination. Everything else, including text, forms, buttons, badges, headings and chart sizes, SHALL keep Pico's defaults. The theme SHALL keep a success style and an error style for status markers. Pages SHALL render without any Content Security Policy violation.

#### Scenario: No third-party stylesheet
- **WHEN** any page is loaded
- **THEN** it loads exactly the application's own copy of Pico and the application's own stylesheet, both from the application's origin

#### Scenario: Compact text and rows
- **WHEN** a signed-in user opens the sell orders page with at least one sell order
- **THEN** the sell table's computed font size is 0.875rem and a single-line row is at most 28px high, while the body text keeps Pico's default size

#### Scenario: Pico defaults outside tables
- **WHEN** a signed-in user opens the characters page
- **THEN** the computed body font size equals Pico's default

#### Scenario: No policy violations
- **WHEN** a signed-in user visits the dashboard, the characters page, the wealth page, the trades page, the sell and buy orders pages, every opportunity table page and an opportunity detail page, and uses a sub-menu link on each page that has one
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Full-width pages
Every page SHALL use the full width of the browser window right of the sidebar, without a maximum content width. Layouts SHALL be designed for windows of 1280px width and more; narrower windows SHALL stay usable by wrapping, without horizontal page scrolling.

#### Scenario: Wide window
- **WHEN** a signed-in user opens the sell orders page in a 2560px wide window
- **THEN** the table's container reaches from the sidebar to the right edge of the window, apart from Pico's fluid container padding

#### Scenario: Narrow window
- **WHEN** a signed-in user opens the dashboard in a 1024px wide window
- **THEN** the page does not scroll horizontally

### Requirement: Dashboard rows
The dashboard SHALL be laid out in rows, top to bottom: the filter section; the tiles as KPI cards; the per-character breakdown across the full width; the per-character split graph, the top items by sell value graph and the top items by buy escrow graph side by side; then the value over time graph, the order count over time graph and the wealth over time graph, each alone on a row across the full width. The breakdown SHALL show one row per selected character and SHALL NOT be paginated. The sub-menu's "Totals" link leads to the tiles, "Characters" to the breakdown, "Split & top items" to the split and top-items row, and "Value", "Orders" and "Wealth" to the value, order count and wealth over-time graphs.

#### Scenario: First screen
- **WHEN** a signed-in user with three characters opens the dashboard at 1920×1080
- **THEN** the tiles are above the breakdown, and the split and both top-item graphs share one row below the breakdown

#### Scenario: Breakdown spans the width
- **WHEN** a signed-in user opens the dashboard at 1920×1080
- **THEN** the breakdown is as wide as the content area

#### Scenario: Over-time graphs on their own rows
- **WHEN** a signed-in user with orders and wealth history opens the dashboard at 1920×1080
- **THEN** the value, the order count and the wealth over-time graphs are each as wide as the content area, each below the previous one, below the split and top-items row

#### Scenario: Many characters
- **WHEN** a user with thirty characters opens the dashboard
- **THEN** the breakdown shows all thirty rows without pagination

### Requirement: Icons
Icons SHALL be used sparingly and come from one consistent line-icon set served by the application itself. Every main menu entry SHALL carry an icon in front of its text. Icon-only controls SHALL appear only in the collapsed main menu and as the menu's collapse toggle. Each icon-only control SHALL keep its text as its accessible name and show it on hover. Outside the main menu, any other action MAY carry an icon in front of its text, but SHALL keep the text. The "Unlink", "Add character" and "Log in again" actions SHALL carry an icon and their text. Icons SHALL take the colour of the surrounding text.

#### Scenario: Actions keep their text
- **WHEN** a signed-in user opens the characters page
- **THEN** the "Add character" and "Unlink" controls show an icon followed by their text

#### Scenario: Log in again keeps its text
- **WHEN** a character needs re-login and the user opens the sell orders page
- **THEN** its "Log in again" link in the freshness shows an icon followed by its text

#### Scenario: Freshness toggle is icon-only
- **WHEN** a signed-in user opens the sell orders page
- **THEN** there is no freshness toggle, the menu toggle is the only icon-only control outside the collapsed menu, and every expanded menu entry shows an icon followed by its text

### Requirement: Chart styling
Graphs SHALL use small, dimmed axis labels, grid lines and legends that match the theme. Their data, colours per series and hover tooltips SHALL be unchanged by the theme.

#### Scenario: Tooltip unchanged
- **WHEN** a user hovers the sell value line at a point where the value is 1,234,567,890.12 ISK
- **THEN** a tooltip shows the point's time in EVE time, "Sell value" and "1,234,567,890.12 ISK"

### Requirement: Section sub-menu
A page with sibling pages, and a page with more than one row of sections, SHALL show a sub-menu at the top of its content, with one text-only link per entry and no icons or counts. The sub-menu SHALL stay visible at the top of the window while the page scrolls.

A **page sub-menu** links to sibling pages and marks the current page:
- sell orders page and buy orders page: "Sell" and "Buy"
- opportunity table pages: "Recurring spread expansion", "Classic spread trade", "Mean reversion" and "Trend"

Page sub-menu links SHALL carry the character selection and, between opportunity table pages, every table's sorting. They SHALL NOT carry the page number.

A **section sub-menu** lists one link per row of the page in the order of the rows. The dashboard and the opportunity detail page have one. Activating a link SHALL scroll to that row and put the row's fragment into the URL. Opening a URL with a row's fragment SHALL scroll to that row. A row that the sub-menu scrolled to SHALL have its title fully visible below the sub-menu, at the top of the window when the page is long enough. The sub-menu SHALL mark one link as current: right after a jump, the link of the row jumped to; while the user scrolls, the link of the row at the top of the window below the sub-menu, or the last row's link when the page is scrolled to its end. Scrolling SHALL NOT change the URL. Live refreshes SHALL keep the marker, the URL's fragment and the scroll position.

The characters page, the wealth page and the trades page SHALL show no sub-menu.

#### Scenario: Orders sub-menu
- **WHEN** a signed-in user opens the sell orders page
- **THEN** a sub-menu at the top lists "Sell" and "Buy" as text, and marks "Sell" as current

#### Scenario: Opportunities sub-menu without counts
- **WHEN** a signed-in user opens the trend page
- **THEN** the sub-menu lists the four tables as text without any number, and marks "Trend" as current

#### Scenario: Dashboard sub-menu
- **WHEN** a signed-in user opens the dashboard
- **THEN** a sub-menu at the top lists "Totals", "Characters", "Split & top items", "Value", "Orders" and "Wealth"

#### Scenario: Jump to a row
- **WHEN** a user activates "Wealth" in the dashboard's sub-menu
- **THEN** the page scrolls until the wealth over-time graph's title is visible below the sub-menu, the URL ends in the wealth row's fragment, and the sub-menu marks "Wealth" as current

#### Scenario: Marker follows scrolling
- **WHEN** a user scrolls the dashboard by hand from the top until the value over-time graph reaches the top of the window below the sub-menu
- **THEN** the sub-menu marks "Value" as current and the URL is unchanged

#### Scenario: End of the page
- **WHEN** a user scrolls the dashboard by hand to its end
- **THEN** the sub-menu marks "Wealth" as current

#### Scenario: Open with a fragment
- **WHEN** a user opens the dashboard with the wealth row's fragment
- **THEN** the page opens scrolled to the wealth over-time graph, with its title visible below the sub-menu

#### Scenario: Sub-menu stays while scrolling
- **WHEN** a user scrolls the sell orders page to its end
- **THEN** the sub-menu is still at the top of the window

#### Scenario: No sub-menu on single-row and window pages
- **WHEN** a signed-in user opens the characters page, the wealth page and the trades page
- **THEN** none of them shows a sub-menu

### Requirement: Filter box
Every filter of a page SHALL be placed in the page's filter section, including time ranges. The filter section SHALL be shown below the sub-menu and above the KPI cards and the content. It SHALL hold the page's filters, and on its right end, dimmed text that describes the whole page when the page has such text. When its content does not fit on one line, it SHALL wrap instead of scrolling sideways. A filter value the page rejects SHALL be reported inside the filter section. A page without such filters or text SHALL show no filter section. The filter sections are:
- dashboard: the character filter and the time range
- sell orders page and buy orders page: the character filter
- wealth page: the character filter and the time range of the graph
- trades page: the character filter and the time range
- opportunity table pages: the character filter, the opportunity filters, and as text the station name and the fees used
- opportunity detail page: the character filter, and as text the station name and the fees used

#### Scenario: Orders filter box
- **WHEN** a signed-in user with three characters opens the sell orders page at 1280×800
- **THEN** the character filter is in the filter section between the sub-menu and the KPI cards

#### Scenario: Dashboard filter box
- **WHEN** a signed-in user opens the dashboard
- **THEN** the character filter and the time range control are in the filter section

#### Scenario: Wealth range stays with its graph
- **WHEN** a signed-in user opens the wealth page
- **THEN** the character filter and the time range control are both in the filter section, and the graph has no range control of its own

#### Scenario: Trades range stays with its table
- **WHEN** a signed-in user opens the trades page
- **THEN** the character filter and the time range control are both in the filter section above the KPI cards

#### Scenario: No filter box on the characters page
- **WHEN** a signed-in user opens the characters page
- **THEN** the page shows no filter section

#### Scenario: Narrow window wraps
- **WHEN** a signed-in user with five characters opens the classic spread trade page in a 1024px wide window
- **THEN** the filter section wraps onto more lines and the page does not scroll horizontally

### Requirement: Wide tables
A table whose columns do not fit its panel SHALL scroll sideways inside its panel. Table cells SHALL NOT wrap their text. The page SHALL NOT scroll sideways because of a table.

#### Scenario: Too many columns
- **WHEN** a signed-in user opens the opportunities page in a 1280px wide window
- **THEN** each table that does not fit scrolls sideways inside its panel, no cell wraps, and the page does not scroll horizontally

### Requirement: Shared table style
Every data table SHALL use one compact table style: text at 0.875rem, a single-line body row at most 28px high, the same header, alternating row background and hover background, and right-aligned tabular figures for numbers. Table cells SHALL NOT wrap their text. A table whose rows open a page SHALL show a pointer cursor over its rows.

#### Scenario: Compact rows
- **WHEN** a signed-in user opens the sell orders page with at least one sell order
- **THEN** the table's computed font size is 0.875rem and a single-line body row is at most 28px high

#### Scenario: Same striping everywhere
- **WHEN** a signed-in user compares the second body row of the dashboard breakdown, the wealth items table and an opportunity table
- **THEN** all three have the same alternating background

### Requirement: Details on their own page
Details about an entry SHALL be shown on a page of their own that the entry links to, and SHALL NOT be shown in a floating window, a dialog or an overlay.

#### Scenario: Opportunity details
- **WHEN** a user clicks a row of the trend table
- **THEN** the browser navigates to that item's trend detail page, and no floating window opens

### Requirement: Table page layout
Every page whose main content is a data table SHALL show, from top to bottom: the sub-menu if the page has one, the filter section, the KPI cards if the page has KPIs, the table and its pagination, and the freshness. The table pages are the sell orders page, the buy orders page, the wealth page, the trades page and each opportunity table page. On the wealth page, the graph SHALL sit between the KPI cards and the items table. Each page SHALL show exactly one data table.

#### Scenario: Sell page order
- **WHEN** a signed-in user with sell orders opens the sell orders page
- **THEN** the page shows, from top to bottom, the sub-menu, the filter section, the KPI cards, the sell table, its pagination and the freshness

#### Scenario: Wealth page order
- **WHEN** a signed-in user opens the wealth page
- **THEN** the page shows, from top to bottom, the filter section, the KPI cards, the graph, the items table, its pagination and the freshness

#### Scenario: One table per page
- **WHEN** a signed-in user opens the classic spread trade page
- **THEN** it shows the classic spread trade table and no other opportunity table

### Requirement: KPI cards
Every KPI SHALL be shown as its own square card holding the KPI's name, its value and, where a KPI has one, a dimmed detail line. All KPI cards of a page SHALL have the same size. Cards SHALL be laid out in rows from left to right and SHALL wrap onto further rows when the width does not fit all of them, without horizontal page scrolling. A KPI SHALL cover all rows of its table that pass the page's filters, not only the rows of the current table page. ISK values on cards SHALL follow the shortening rule of the orders overview, with the exact value on hover.

#### Scenario: Cards are square
- **WHEN** a signed-in user opens the buy orders page
- **THEN** every KPI card is as high as it is wide, and all cards have the same size

#### Scenario: Cards wrap
- **WHEN** a user with five selected characters opens the sell orders page in a 1280px wide window
- **THEN** the KPI cards continue on further rows and the page does not scroll horizontally

#### Scenario: KPIs cover every table page
- **WHEN** a user with 120 sell orders opens the sell orders page with the default page size
- **THEN** the table shows 50 orders and the order count card reads 120

### Requirement: Pagination
Every table on a table page SHALL be paginated on the server. The page number SHALL be given by the `page` query parameter, starting at 1, and the page size by the `per_page` query parameter, one of 25, 50, 100 or 200. A missing or unknown page size SHALL select 50. A missing, malformed or non-positive page number SHALL select page 1, and a page number past the last page SHALL select the last page. Below the table, the page SHALL show a control to go to the previous and the next page, the current page and the number of pages as "Page x of y", and a control to choose the page size. Going to another page or choosing another page size SHALL update the URL and the table without a full page reload, and SHALL keep the character selection, the filters, the range and the sorting. Changing the character selection, a filter, the range, the sorting or the page size SHALL show page 1. Pagination SHALL be applied after sorting. A table without rows SHALL show no pagination controls.

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

### Requirement: Freshness at the end of the page
Status and freshness information that belongs to a whole page SHALL be shown in a freshness section after the table and its pagination, always unfolded. The sell orders page, the buy orders page, the wealth page, the trades page and every opportunity table page SHALL have one. It SHALL NOT be collapsible and SHALL NOT show a warning count.

#### Scenario: Freshness below the table
- **WHEN** a signed-in user opens the trades page
- **THEN** the freshness section is shown below the trades table's pagination, with every selected character's update times visible without any toggle

#### Scenario: No toggle
- **WHEN** a signed-in user opens the sell orders page
- **THEN** the page has no freshness toggle
