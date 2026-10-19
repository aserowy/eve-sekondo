## ADDED Requirements

### Requirement: Page structure
Every signed-in page SHALL be laid out right of the sidebar as a page column. From top to bottom, the column holds the page header, the page's main content and, on pages with a freshness section, a page footer. The page header consists of, from top to bottom, the page title as the page's only first-level heading, the sub-menu if the page has one, and the filter section if the page has one. The main content and the page footer SHALL be exposed to assistive technology as the page's main and content-info landmarks. The page titles are:
- dashboard: "Dashboard"
- characters page: "Characters"
- wealth page: "Wealth"
- trades page: "Trades"
- sell orders page and buy orders page: "Sell orders" and "Buy orders"
- opportunity table pages: the table's name, e.g. "Trend"
- opportunity detail page: the item's name

Section titles inside a page SHALL be second-level headings and SHALL be rendered smaller than the page title. Consecutive blocks of the main content, such as the KPI cards, a graph, a table and its pagination, SHALL be separated by one and the same vertical spacing.

#### Scenario: Title on every page
- **WHEN** a signed-in user opens the dashboard, the characters page, the wealth page, the trades page, the sell orders page, the trend page and a trend detail page
- **THEN** each page has exactly one first-level heading, reading "Dashboard", "Characters", "Wealth", "Trades", "Sell orders", "Trend" and the item's name respectively

#### Scenario: Header order
- **WHEN** a signed-in user opens the buy orders page
- **THEN** the page title "Buy orders" is above the sub-menu, and the sub-menu is above the filter section

#### Scenario: Section titles are smaller
- **WHEN** a signed-in user opens the dashboard
- **THEN** the computed font size of the "Per character" title is smaller than that of the "Dashboard" title

#### Scenario: Even spacing
- **WHEN** a signed-in user with sell orders opens the sell orders page
- **THEN** the vertical gap between the KPI cards and the table equals the gap between the table and the pagination

#### Scenario: No footer without freshness
- **WHEN** a signed-in user opens the dashboard or the characters page
- **THEN** the page shows no page footer

#### Scenario: Landmarks
- **WHEN** a signed-in user opens the trades page
- **THEN** assistive technology finds one main landmark holding the trades table and one content-info landmark holding the freshness

### Requirement: KPI stat cards
Every KPI SHALL be shown as its own card holding, from top to bottom, the KPI's name in small dimmed text, its value in large text and, where a KPI has one, a dimmed detail line. A card SHALL have no separate header band. Cards SHALL be as high as their content needs, SHALL NOT be stretched to a square, and SHALL be at least 10rem wide. All KPI cards of a page SHALL have the same width and the cards of one row the same height. Cards SHALL be laid out in rows from left to right and SHALL wrap onto further rows when the width does not fit all of them, without horizontal page scrolling. A KPI SHALL cover all rows of its table that pass the page's filters, not only the rows of the current table page. ISK values on cards SHALL follow the shortening rule of the orders overview, with the exact value on hover.

#### Scenario: Cards fit their content
- **WHEN** a signed-in user opens the buy orders page in a 1920px wide window
- **THEN** every KPI card is wider than it is high, all cards have the same width, and no card has a header band

#### Scenario: Cards wrap
- **WHEN** a user with five selected characters opens the sell orders page in a 1280px wide window
- **THEN** the KPI cards continue on further rows and the page does not scroll horizontally

#### Scenario: KPIs cover every table page
- **WHEN** a user with 120 sell orders opens the sell orders page with the default page size
- **THEN** the table shows 50 orders and the order count card reads 120

## MODIFIED Requirements

### Requirement: Dense theme
Every page SHALL be styled with the Pico CSS framework (version 2) in its dark theme, served by the application itself, plus the application's own stylesheet. No other stylesheet SHALL be loaded, and no stylesheet SHALL be loaded from another origin. The base font size SHALL be Pico's unscaled base of 100% at every window width; Pico's growth of the font size with the window width SHALL NOT apply. The application's own stylesheet SHALL only set that base font size, make tables compact, and lay out the sidebar, the page header, the sub-menu, the KPI cards, the pagination, the page footer and the heading sizes. Everything else, including forms, buttons, badges and chart sizes, SHALL keep Pico's defaults. The theme SHALL keep a success style and an error style for status markers. Pages SHALL render without any Content Security Policy violation.

#### Scenario: No third-party stylesheet
- **WHEN** any page is loaded
- **THEN** it loads exactly the application's own copy of Pico and the application's own stylesheet, both from the application's origin

#### Scenario: Compact text and rows
- **WHEN** a signed-in user opens the sell orders page with at least one sell order
- **THEN** the sell table's computed font size is 0.875rem and a single-line row is at most 28px high, while the body text is 16px

#### Scenario: Pico defaults outside tables
- **WHEN** a signed-in user opens the characters page in a 1024px, a 1280px and a 1920px wide window
- **THEN** the computed root and body font sizes are 16px in each window

#### Scenario: No policy violations
- **WHEN** a signed-in user visits the dashboard, the characters page, the wealth page, the trades page, the sell and buy orders pages, every opportunity table page and an opportunity detail page, and uses a sub-menu link on each page that has one
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Section sub-menu
A page with sibling pages, and a page with more than one row of sections, SHALL show a sub-menu in its page header directly below the page title, with one text-only link per entry and no icons or counts. The sub-menu SHALL be shown as a tab bar: its entries in one row above a full-width divider line, with the current entry underlined in the primary colour and the other entries dimmed. The sub-menu SHALL stay visible at the top of the window while the page scrolls; it SHALL be the only part of the page header that does.

A **page sub-menu** links to sibling pages and marks the current page:
- sell orders page and buy orders page: "Sell" and "Buy"
- opportunity table pages: "Recurring spread expansion", "Classic spread trade", "Mean reversion" and "Trend"

Page sub-menu links SHALL carry the character selection and, between opportunity table pages, every table's sorting. They SHALL NOT carry the page number.

A **section sub-menu** lists one link per row of the page in the order of the rows. The dashboard and the opportunity detail page have one. Activating a link SHALL scroll to that row and put the row's fragment into the URL. Opening a URL with a row's fragment SHALL scroll to that row. A row that the sub-menu scrolled to SHALL have its title fully visible below the sub-menu, at the top of the window when the page is long enough. The sub-menu SHALL mark one link as current: right after a jump, the link of the row jumped to; while the user scrolls, the link of the row at the top of the window below the sub-menu, or the last row's link when the page is scrolled to its end. Scrolling SHALL NOT change the URL. Live refreshes SHALL keep the marker, the URL's fragment and the scroll position.

The characters page, the wealth page and the trades page SHALL show no sub-menu.

#### Scenario: Orders sub-menu
- **WHEN** a signed-in user opens the sell orders page
- **THEN** a sub-menu below the page title lists "Sell" and "Buy" as text, and marks "Sell" as current

#### Scenario: Current tab is underlined
- **WHEN** a signed-in user opens the buy orders page
- **THEN** the "Buy" entry has a bottom border in the primary colour, and the "Sell" entry has none and is dimmed

#### Scenario: Opportunities sub-menu without counts
- **WHEN** a signed-in user opens the trend page
- **THEN** the sub-menu lists the four tables as text without any number, and marks "Trend" as current

#### Scenario: Dashboard sub-menu
- **WHEN** a signed-in user opens the dashboard
- **THEN** a sub-menu below the page title lists "Totals", "Characters", "Split & top items", "Value", "Orders" and "Wealth"

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
- **THEN** the sub-menu is still at the top of the window, and the page title has scrolled out of view

#### Scenario: No sub-menu on single-row and window pages
- **WHEN** a signed-in user opens the characters page, the wealth page and the trades page
- **THEN** none of them shows a sub-menu

### Requirement: Table page layout
Every page whose main content is a data table SHALL show, from top to bottom: the page header with the page title, the sub-menu if the page has one and the filter section; the KPI cards if the page has KPIs; the table and its pagination; and the page footer with the freshness. The table pages are the sell orders page, the buy orders page, the wealth page, the trades page and each opportunity table page. On the wealth page, the graph SHALL sit between the KPI cards and the items table. Each page SHALL show exactly one data table.

#### Scenario: Sell page order
- **WHEN** a signed-in user with sell orders opens the sell orders page
- **THEN** the page shows, from top to bottom, the page title, the sub-menu, the filter section, the KPI cards, the sell table, its pagination and the freshness footer

#### Scenario: Wealth page order
- **WHEN** a signed-in user opens the wealth page
- **THEN** the page shows, from top to bottom, the page title, the filter section, the KPI cards, the graph, the items table, its pagination and the freshness footer

#### Scenario: One table per page
- **WHEN** a signed-in user opens the classic spread trade page
- **THEN** it shows the classic spread trade table and no other opportunity table

### Requirement: Freshness at the end of the page
Status and freshness information that belongs to a whole page SHALL be shown in the page footer, after the main content. The sell orders page, the buy orders page, the wealth page, the trades page and every opportunity table page SHALL have one. When the page is shorter than the window, the footer SHALL sit at the bottom edge of the window. When the page is taller than the window, the footer SHALL follow the content and SHALL only come into view when the page is scrolled to its end; it SHALL NOT be fixed over the content. The footer SHALL be set apart from the content by a divider line, SHALL use small dimmed text, and SHALL NOT have a heading. Each freshness entry, such as a character's update times, SHALL be shown on a single line when the window is wide enough, with its badges and its "Log in again" link inline. The footer SHALL always be unfolded, SHALL NOT be collapsible and SHALL NOT show a warning count. A live refresh of the freshness SHALL keep the footer's placement.

#### Scenario: Footer at the bottom of a short page
- **WHEN** a signed-in user with two sell orders opens the sell orders page at 1920×1080
- **THEN** the freshness footer's bottom edge is at the bottom edge of the window

#### Scenario: Footer scrolls in on a long page
- **WHEN** a user with 200 sell orders opens the sell orders page with a page size of 200 at 1280×800
- **THEN** the freshness footer is not in view, and after scrolling the page to its end it is in view below the pagination

#### Scenario: Compact entries
- **WHEN** a signed-in user with two characters opens the trades page at 1920×1080
- **THEN** the footer has no heading, and each character's name, last update and next update are on one line

#### Scenario: Freshness below the table
- **WHEN** a signed-in user opens the trades page
- **THEN** the freshness footer is shown below the trades table's pagination, with every selected character's update times visible without any toggle

#### Scenario: No toggle
- **WHEN** a signed-in user opens the sell orders page
- **THEN** the page has no freshness toggle

## REMOVED Requirements

### Requirement: KPI cards
**Reason**: Square cards of one size left large empty areas on wide windows; replaced by compact stat cards of natural height.
**Migration**: See "KPI stat cards". Coverage, wrapping and ISK shortening carry over unchanged.
