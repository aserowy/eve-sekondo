# ui-layout Specification

## Purpose

Defines how Sekondo's pages use the screen: a dense, dark layout built on Pico CSS, with one page structure (title, sub-menu, filters, content and a quiet freshness footer) that spends pixels on information and stays consistent across all pages.

## Requirements

### Requirement: Dense theme
Every page SHALL be styled with the Pico CSS framework (version 2) in its dark theme, served by the application itself, plus the application's own stylesheet. No other stylesheet SHALL be loaded, and no stylesheet SHALL be loaded from another origin. The base font size SHALL be Pico's unscaled base of 100% at every window width; Pico's growth of the font size with the window width SHALL NOT apply. The application's own stylesheet SHALL only set:
- that base font size;
- the table style;
- the layout of the sidebar, the page header, the sub-menu, the filter section, the KPI cards, the graph cards, the pagination and the page footer;
- the heading sizes.

Everything else SHALL keep Pico's defaults, including the look and size of form fields, buttons, button groups, badges and cards, and chart sizes. The theme SHALL keep a success style and an error style for status markers. Pages SHALL render without any Content Security Policy violation.

#### Scenario: No third-party stylesheet
- **WHEN** any page is loaded
- **THEN** it loads exactly the application's own copy of Pico and the application's own stylesheet, both from the application's origin

#### Scenario: Compact text and rows
- **WHEN** a signed-in user opens the sell orders page with at least one sell order
- **THEN** the sell table's computed font size is 0.875rem and a single-line row is at most 1.75rem high, while the body text is 1rem

#### Scenario: Pico defaults outside tables
- **WHEN** a signed-in user opens the characters page in a 1024px, a 1280px and a 1920px wide window
- **THEN** the computed root and body font sizes are 16px in each window

#### Scenario: Form fields keep Pico's size
- **WHEN** a signed-in user opens the trend page
- **THEN** the "Minimum margin (%)" input has the same computed height and padding as an unstyled Pico text input

#### Scenario: No policy violations
- **WHEN** a signed-in user visits the dashboard, the characters page, the wealth page, the trades page, the sell and buy orders pages, every opportunity table page and an opportunity detail page, and uses a sub-menu link on each page that has one
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Page structure
Every signed-in page SHALL be laid out right of the sidebar as a page column. From top to bottom, the column holds the page header, the page's main content and, on pages with a freshness section, a page footer. The page header consists of, from top to bottom, the sub-menu if the page has one, the page title as the page's only first-level heading, the page's subtitle if it has one, and the filter section if the page has one. A subtitle SHALL be shown directly below the page title as dimmed text in the body text size, and SHALL NOT be a heading. On a page with a sub-menu, the sub-menu SHALL be the first element of the page column; on a page without one, the page title SHALL be. The main content and the page footer SHALL be exposed to assistive technology as the page's main and content-info landmarks. The page titles are:
- dashboard: "Dashboard"
- characters page: "Characters"
- wealth page: "Wealth"
- trades page: "Trades"
- sell orders page and buy orders page: "Sell orders" and "Buy orders"
- opportunity table pages: the table's name, e.g. "Trend"
- opportunity detail page: the item's name

The opportunity table pages and the opportunity detail page have the name of the station they analyse as subtitle. No other page has a subtitle.

Section titles inside a page SHALL be second-level headings and SHALL be rendered smaller than the page title. Consecutive blocks of the main content, such as the KPI cards, a graph card and a table, SHALL be separated by one and the same vertical spacing. The pagination is placed as the pagination requirement describes and is not bound to that spacing.

#### Scenario: Title on every page
- **WHEN** a signed-in user opens the dashboard, the characters page, the wealth page, the trades page, the sell orders page, the trend page and a trend detail page
- **THEN** each page has exactly one first-level heading, reading "Dashboard", "Characters", "Wealth", "Trades", "Sell orders", "Trend" and the item's name respectively

#### Scenario: Header order
- **WHEN** a signed-in user opens the buy orders page
- **THEN** the sub-menu is above the page title "Buy orders", and the page title is above the filter section

#### Scenario: Station as subtitle
- **WHEN** a signed-in user opens the trend page and a trend detail page
- **THEN** on each page, directly below the page title, a dimmed line reads "Jita IV - Moon 4 - Caldari Navy Assembly Plant", its computed font size is the body text size, and the page still has exactly one first-level heading and no other heading above the filter section

#### Scenario: No subtitle elsewhere
- **WHEN** a signed-in user opens the sell orders page
- **THEN** the filter section follows the page title "Sell orders" without a subtitle between them

#### Scenario: Sub-menu at the top of the page
- **WHEN** a signed-in user opens the dashboard, the sell orders page, the trend page and a trend detail page, each scrolled to the top
- **THEN** on each page the sub-menu is the topmost element of the page column, and nothing of the page column is shown above it

#### Scenario: Title at the top without a sub-menu
- **WHEN** a signed-in user opens the wealth page scrolled to the top
- **THEN** the page title "Wealth" is the topmost element of the page column

#### Scenario: Section titles are smaller
- **WHEN** a signed-in user opens the dashboard
- **THEN** the computed font size of the "Per character" title is smaller than that of the "Dashboard" title

#### Scenario: Even spacing
- **WHEN** a signed-in user with wealth history opens the wealth page
- **THEN** the vertical gap between the KPI cards and the graph card equals the gap between the graph card and the items table

#### Scenario: No footer without freshness
- **WHEN** a signed-in user opens the dashboard or the characters page
- **THEN** the page shows no page footer

#### Scenario: Landmarks
- **WHEN** a signed-in user opens the trades page
- **THEN** assistive technology finds one main landmark holding the trades table and one content-info landmark holding the freshness

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
A page with sibling pages, and a page with more than one row of sections, SHALL show a sub-menu at the top of its page header, directly above the page title, with one text-only link per entry and no icons or counts. The sub-menu SHALL be shown as a tab bar: its entries in one row above a full-width divider line, with the current entry underlined in the primary colour and the other entries dimmed. The entries SHALL start at the left edge of the bar and follow each other with one fixed gap; they SHALL NOT be spread over the bar's width, and the space right of the last entry SHALL stay empty. The sub-menu SHALL stay visible at the top of the window while the page scrolls; it SHALL be the only part of the page header that does.

A **page sub-menu** links to sibling pages and marks the current page:
- sell orders page and buy orders page: "Sell" and "Buy"
- opportunity table pages: "Recurring spread expansion", "Classic spread trade", "Mean reversion" and "Trend"

Page sub-menu links SHALL carry the character selection and, between opportunity table pages, every table's sorting. They SHALL NOT carry the page number.

A **section sub-menu** lists one link per row of the page in the order of the rows. The dashboard and the opportunity detail page have one. Activating a link SHALL scroll to that row and put the row's fragment into the URL. Opening a URL with a row's fragment SHALL scroll to that row. A row that the sub-menu scrolled to SHALL have its title fully visible below the sub-menu, at the top of the window when the page is long enough. The sub-menu SHALL mark one link as current: right after a jump, the link of the row jumped to; while the user scrolls, the link of the row at the top of the window below the sub-menu, or the last row's link when the page is scrolled to its end. Scrolling SHALL NOT change the URL. Live refreshes SHALL keep the marker, the URL's fragment and the scroll position.

The characters page, the wealth page and the trades page SHALL show no sub-menu.

#### Scenario: Orders sub-menu
- **WHEN** a signed-in user opens the sell orders page
- **THEN** a sub-menu above the page title lists "Sell" and "Buy" as text, and marks "Sell" as current

#### Scenario: Current tab is underlined
- **WHEN** a signed-in user opens the buy orders page
- **THEN** the "Buy" entry has a bottom border in the primary colour, and the "Sell" entry has none and is dimmed

#### Scenario: Entries packed at the left
- **WHEN** a signed-in user opens the sell orders page in a 1920px wide window
- **THEN** the "Sell" entry starts at the sub-menu's left edge, the gap between "Sell" and "Buy" equals the gap between the dashboard sub-menu's "Totals" and "Characters", and the "Buy" entry ends in the left half of the sub-menu

#### Scenario: Opportunities sub-menu without counts
- **WHEN** a signed-in user opens the trend page
- **THEN** the sub-menu lists the four tables as text without any number, and marks "Trend" as current

#### Scenario: Dashboard sub-menu
- **WHEN** a signed-in user opens the dashboard
- **THEN** a sub-menu above the page title lists "Totals", "Characters", "Split & top items", "Value", "Orders" and "Wealth"

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

### Requirement: Filter box
Every filter of a page SHALL be placed in the page's filter section, including time ranges. The filter section SHALL be shown below the page title and above the KPI cards and the content, without a border or card of its own. It SHALL hold the page's filters and no other text. When its content does not fit on one line, it SHALL wrap instead of scrolling sideways.

The filter section's groups are the character filter, the time range, the opportunity filters and the fee character. Neighbouring groups SHALL be 3rem apart horizontally, so the character filter stands apart from the filters beside it, and lines of a wrapped filter section SHALL be 0.75rem apart. Groups SHALL be set apart by spacing only, without a divider line or border. A page without such filters SHALL show no filter section.

Every group SHALL start at the top of its line of the filter section. A group that is taller than its neighbours, because its fields wrap onto further rows or because it shows helper text or an error message, SHALL NOT move its neighbours up or down.

The filters SHALL be structured as form fields:
- **Character filter**: a group with the visible legend "Characters" above the characters' checkboxes. The checkboxes are laid out side by side, each with its character's name as its label.
- **Text and number fields**: each field's label text is shown above its input. The fields of one form are laid out side by side in columns of equal width that wrap onto further rows when the width does not fit them. Every such field, including a field that searches by name, SHALL be a plain text input with the same shape, size and padding as the other fields, without a search icon, rounded search shape or built-in clear button.
- **Time range**: a group of joined buttons, one per range, with the visible label "Range" above it. The current range is shown as the highlighted button and is exposed to assistive technology as current.
- **Fee character**: a group with the visible legend "Fees" above one radio button per selected character, laid out side by side, each with its character's name as its label. Exactly one is checked. Helper text below the radio buttons tells the rates in use. With no character selected, the group shows the helper text only.

The legends and labels of the groups of one line SHALL share their top edge, so that the inputs of the first row of fields and the time range's buttons share their top edge.

A filter value the page rejects SHALL be reported at the field that holds it. The field's input SHALL be marked invalid, and the message SHALL be shown as helper text directly below that input. Any other field SHALL show no error. Once the values are accepted, no field SHALL be marked invalid and no message SHALL be shown.

The filter sections are:
- dashboard: the character filter and the time range
- sell orders page and buy orders page: the character filter
- wealth page: the character filter and the time range of the graph
- trades page: the character filter and the time range
- opportunity table pages: the character filter, the fee character and the opportunity filters
- opportunity detail page: the character filter and the fee character

#### Scenario: Orders filter box
- **WHEN** a signed-in user with three characters opens the sell orders page at 1280×800
- **THEN** the character filter is in the filter section between the page title and the KPI cards

#### Scenario: Characters legend
- **WHEN** a signed-in user with three characters opens the sell orders page
- **THEN** the character filter is a group named "Characters" with a visible legend, holding three checkboxes side by side, each labelled with its character's name

#### Scenario: Labels above inputs
- **WHEN** a signed-in user opens the trend page at 1920×1080
- **THEN** the "Minimum margin (%)", "Minimum ISK/day", "Item name" and "Capital per item (ISK)" labels are each above their input, and the four inputs are equally wide

#### Scenario: Item name is a plain input
- **WHEN** a signed-in user opens the trend page
- **THEN** the "Item name" input is a text input with the same computed height, padding, border radius and background image as the "Minimum margin (%)" input

#### Scenario: Character filter stands apart
- **WHEN** a signed-in user with two characters opens the trades page at 1920×1080
- **THEN** the horizontal gap between the character filter and the time range is 3rem, wider than the gap between two of the character checkboxes

#### Scenario: Character filter apart from the opportunity filters
- **WHEN** a signed-in user with two characters opens the trend page at 1920×1080
- **THEN** the horizontal gap between the character filter and the "Minimum margin (%)" field is 3rem, wider than the gap between the "Minimum margin (%)" and the "Minimum ISK/day" fields

#### Scenario: Groups aligned at the top
- **WHEN** a signed-in user with two characters opens the trend page at 1920×1080
- **THEN** the top edges of the "Characters" legend, the "Fees" legend and the "Minimum margin (%)" label are equal

#### Scenario: Character legend level with the range label
- **WHEN** a signed-in user with two characters opens the trades page at 1920×1080
- **THEN** the "Characters" legend and the "Range" label share their top edge

#### Scenario: Wrapped fields do not move the character filter
- **WHEN** a signed-in user with two characters opens the trend page in a window so narrow that the opportunity filters wrap onto two rows on the same line as the character filter
- **THEN** the "Characters" legend's top edge equals the "Minimum margin (%)" label's top edge

#### Scenario: Error does not move neighbours
- **WHEN** a user on the trend page at 1920×1080 enters "-1" as the minimum margin
- **THEN** the top edges of the character filter and the fee character are where they were before the error was shown

#### Scenario: No divider between groups
- **WHEN** a signed-in user opens the dashboard
- **THEN** the character filter and the time range have no border

#### Scenario: Dashboard filter box
- **WHEN** a signed-in user opens the dashboard
- **THEN** the character filter and the time range control are in the filter section

#### Scenario: Range as a button group
- **WHEN** a signed-in user opens the trades page with the 30-day range
- **THEN** the time range is a group of joined buttons labelled "Range", the 30-day button is highlighted and marked as current, and the other buttons are not

#### Scenario: Wealth range stays with its graph
- **WHEN** a signed-in user opens the wealth page
- **THEN** the character filter and the time range control are both in the filter section, and the graph has no range control of its own

#### Scenario: Trades range stays with its table
- **WHEN** a signed-in user opens the trades page
- **THEN** the character filter and the time range control are both in the filter section above the KPI cards

#### Scenario: Error at its field
- **WHEN** a user enters "-1" as the minimum margin on the trend page
- **THEN** the "Minimum margin (%)" input is marked invalid, the message is shown directly below it, and no other input is marked invalid

#### Scenario: Error cleared
- **WHEN** that user then enters "5" as the minimum margin
- **THEN** no input is marked invalid and no error message is shown

#### Scenario: No filter box on the characters page
- **WHEN** a signed-in user opens the characters page
- **THEN** the page shows no filter section

#### Scenario: Narrow window wraps
- **WHEN** a signed-in user with five characters opens the classic spread trade page in a 1024px wide window
- **THEN** the filter section wraps onto more lines, its lines are 0.75rem apart, and the page does not scroll horizontally

### Requirement: Wide tables
A table whose columns do not fit its panel SHALL scroll sideways inside its panel. Table cells SHALL NOT wrap their text. The page SHALL NOT scroll sideways because of a table.

#### Scenario: Too many columns
- **WHEN** a signed-in user opens the opportunities page in a 1280px wide window
- **THEN** each table that does not fit scrolls sideways inside its panel, no cell wraps, and the page does not scroll horizontally

### Requirement: Shared table style
Every data table SHALL use one compact table style: text at 0.875rem, cells padded by 0.3rem at the top and bottom and 0.75rem at the left and right, a single-line body row at most 1.75rem high, the same header, alternating row background and hover background, and right-aligned tabular figures for numbers. The header's text, including the links that sort the table, SHALL NOT be underlined, neither at rest nor on hover or keyboard focus. The header row SHALL keep its bold text and its line below it, which is thicker than the line below a body row. Table cells SHALL NOT wrap their text. A table whose rows open a page SHALL show a pointer cursor over its rows.

#### Scenario: Compact rows
- **WHEN** a signed-in user opens the sell orders page with at least one sell order
- **THEN** the table's computed font size is 0.875rem and a single-line body row is at most 1.75rem high

#### Scenario: Roomier cells
- **WHEN** a signed-in user opens the trades page with at least one trade
- **THEN** every body cell has a computed top and bottom padding of 0.3rem and a left and right padding of 0.75rem

#### Scenario: Same striping everywhere
- **WHEN** a signed-in user compares the second body row of the dashboard breakdown, the wealth items table and an opportunity table
- **THEN** all three have the same alternating background

#### Scenario: Header text not underlined
- **WHEN** a signed-in user opens the sell orders page and hovers the "Item" column's sort link
- **THEN** no header cell's text and no sort link is underlined, before and while hovering

#### Scenario: Header line kept
- **WHEN** a signed-in user opens the trades page
- **THEN** the header cells' bottom border is wider than a body cell's bottom border

### Requirement: Details on their own page
Details about an entry SHALL be shown on a page of their own that the entry links to, and SHALL NOT be shown in a floating window, a dialog or an overlay.

#### Scenario: Opportunity details
- **WHEN** a user clicks a row of the trend table
- **THEN** the browser navigates to that item's trend detail page, and no floating window opens

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

### Requirement: Pagination
Every table on a table page SHALL be paginated on the server. The page number SHALL be given by the `page` query parameter, starting at 1, and the page size by the `per_page` query parameter, one of 25, 50, 100 or 200. A missing or unknown page size SHALL select 50. A missing, malformed or non-positive page number SHALL select page 1, and a page number past the last page SHALL select the last page. The page SHALL show a control to go to the previous and the next page, the current page and the number of pages as "Page x of y", and a control to choose the page size.

The pagination SHALL be the last block of the main content, below the table, and SHALL NOT be part of the page footer. When the page is shorter than the window, the pagination SHALL sit at the bottom of the window directly above the page footer, with the spare height between the table and the pagination. When the page is taller than the window, the pagination SHALL follow the table and SHALL only come into view when the page is scrolled to near its end. It SHALL NOT be fixed over the content. A live refresh or a change of page or page size SHALL keep this placement.

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

#### Scenario: Pagination scrolls in on a long page
- **WHEN** a user with 200 sell orders opens the sell orders page with a page size of 200 at 1280×800
- **THEN** the pagination is not in view, and after scrolling the page to its end it is in view directly below the table and above the freshness footer

#### Scenario: Placement kept after a page change
- **WHEN** a user with 60 sell orders on the sell orders page at 1920×1080 with a page size of 50 goes to page 2
- **THEN** the pagination is still directly above the freshness footer at the bottom of the window

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

### Requirement: Graph cards
Every graph SHALL be shown inside a card of its own. The card SHALL hold the graph's section title, the graph and, when there is nothing to draw, the graph's empty-state text. A graph card SHALL have no separate header band. A row that the section sub-menu jumps to and that holds a single graph SHALL be that graph's card, so a jump shows the card's title below the sub-menu. Graphs that share a row SHALL each have their own card, of equal height within the row. This applies to the graphs of the dashboard, the wealth page and the opportunity detail page.

#### Scenario: Dashboard graphs in cards
- **WHEN** a signed-in user with orders and wealth history opens the dashboard
- **THEN** the per-character split, both top-item graphs and the value, order count and wealth over-time graphs are each inside their own card, together with their title

#### Scenario: Wealth graph in a card
- **WHEN** a signed-in user opens the wealth page
- **THEN** the "Over time" title and the graph are inside one card, and the card has no header band

#### Scenario: Empty graph keeps its card
- **WHEN** a signed-in user without sell orders opens the dashboard
- **THEN** the "Top items by sell value" card shows its title and its empty-state text

#### Scenario: Shared row
- **WHEN** a signed-in user opens the dashboard at 1920×1080
- **THEN** the split card and both top-item cards are side by side and are equally high

#### Scenario: Jump lands on the card
- **WHEN** a user activates "Wealth" in the dashboard's sub-menu
- **THEN** the wealth over-time card's title is visible below the sub-menu

### Requirement: Main menu alignment
In the main menu, the icons of all entries, including the menu toggle and "Sign out", SHALL start at one and the same left edge in the collapsed sidebar. In the expanded sidebar, the icons of the page entries and of "Sign out" SHALL start at one and the same left edge, and their texts SHALL start at one and the same left edge. In the expanded sidebar, the menu toggle's icon SHALL sit at the right edge of the sidebar.

#### Scenario: Expanded menu lines up
- **WHEN** a signed-in user opens the dashboard with the sidebar expanded
- **THEN** the icons of "Dashboard", "Characters", "Wealth", "Trades", "Orders", "Opportunities" and "Sign out" have the same left edge, and so do their texts

#### Scenario: Toggle on the right
- **WHEN** a signed-in user opens the dashboard with the sidebar expanded
- **THEN** the menu toggle's icon is right of every entry's icon and ends at the sidebar's right edge, apart from the sidebar's padding

#### Scenario: Collapsed menu lines up
- **WHEN** a signed-in user collapses the sidebar
- **THEN** the menu toggle's icon and the icons of every entry, including "Sign out", have the same left edge
