## ADDED Requirements

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

## MODIFIED Requirements

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
Every signed-in page SHALL be laid out right of the sidebar as a page column. From top to bottom, the column holds the page header, the page's main content and, on pages with a freshness section, a page footer. The page header consists of, from top to bottom, the page title as the page's only first-level heading, the sub-menu if the page has one, and the filter section if the page has one. The main content and the page footer SHALL be exposed to assistive technology as the page's main and content-info landmarks. The page titles are:
- dashboard: "Dashboard"
- characters page: "Characters"
- wealth page: "Wealth"
- trades page: "Trades"
- sell orders page and buy orders page: "Sell orders" and "Buy orders"
- opportunity table pages: the table's name, e.g. "Trend"
- opportunity detail page: the item's name

Section titles inside a page SHALL be second-level headings and SHALL be rendered smaller than the page title. Consecutive blocks of the main content, such as the KPI cards, a graph card and a table, SHALL be separated by one and the same vertical spacing. The pagination is placed as the pagination requirement describes and is not bound to that spacing.

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
- **WHEN** a signed-in user with wealth history opens the wealth page
- **THEN** the vertical gap between the KPI cards and the graph card equals the gap between the graph card and the items table

#### Scenario: No footer without freshness
- **WHEN** a signed-in user opens the dashboard or the characters page
- **THEN** the page shows no page footer

#### Scenario: Landmarks
- **WHEN** a signed-in user opens the trades page
- **THEN** assistive technology finds one main landmark holding the trades table and one content-info landmark holding the freshness

### Requirement: Section sub-menu
A page with sibling pages, and a page with more than one row of sections, SHALL show a sub-menu in its page header directly below the page title, with one text-only link per entry and no icons or counts. The sub-menu SHALL be shown as a tab bar: its entries in one row above a full-width divider line, with the current entry underlined in the primary colour and the other entries dimmed. The entries SHALL start at the left edge of the bar and follow each other with one fixed gap; they SHALL NOT be spread over the bar's width, and the space right of the last entry SHALL stay empty. The sub-menu SHALL stay visible at the top of the window while the page scrolls; it SHALL be the only part of the page header that does.

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

#### Scenario: Entries packed at the left
- **WHEN** a signed-in user opens the sell orders page in a 1920px wide window
- **THEN** the "Sell" entry starts at the sub-menu's left edge, the gap between "Sell" and "Buy" equals the gap between the dashboard sub-menu's "Totals" and "Characters", and the "Buy" entry ends in the left half of the sub-menu

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

### Requirement: Filter box
Every filter of a page SHALL be placed in the page's filter section, including time ranges. The filter section SHALL be shown below the sub-menu and above the KPI cards and the content, without a border or card of its own. It SHALL hold the page's filters, and on its right end, dimmed text that describes the whole page when the page has such text. When its content does not fit on one line, it SHALL wrap instead of scrolling sideways. A page without such filters or text SHALL show no filter section.

The filters SHALL be structured as form fields:
- **Character filter**: a group with the visible legend "Characters" above the characters' checkboxes. The checkboxes are laid out side by side, each with its character's name as its label.
- **Text, number and search fields**: each field's label text is shown above its input. The fields of one form are laid out side by side in columns of equal width that wrap onto further rows when the width does not fit them.
- **Time range**: a group of joined buttons, one per range, with the visible label "Range" above it. The current range is shown as the highlighted button and is exposed to assistive technology as current.

The filters' field heights SHALL be aligned so that the inputs and the time range's buttons of one row share their bottom edge.

A filter value the page rejects SHALL be reported at the field that holds it. The field's input SHALL be marked invalid, and the message SHALL be shown as helper text directly below that input. Any other field SHALL show no error. Once the values are accepted, no field SHALL be marked invalid and no message SHALL be shown.

The filter sections are:
- dashboard: the character filter and the time range
- sell orders page and buy orders page: the character filter
- wealth page: the character filter and the time range of the graph
- trades page: the character filter and the time range
- opportunity table pages: the character filter, the opportunity filters, and as text the station name and the fees used
- opportunity detail page: the character filter, and as text the station name and the fees used

#### Scenario: Orders filter box
- **WHEN** a signed-in user with three characters opens the sell orders page at 1280×800
- **THEN** the character filter is in the filter section between the sub-menu and the KPI cards

#### Scenario: Characters legend
- **WHEN** a signed-in user with three characters opens the sell orders page
- **THEN** the character filter is a group named "Characters" with a visible legend, holding three checkboxes side by side, each labelled with its character's name

#### Scenario: Labels above inputs
- **WHEN** a signed-in user opens the trend page at 1920×1080
- **THEN** the "Minimum margin (%)", "Minimum ISK/day", "Item name" and "Capital per item (ISK)" labels are each above their input, and the four inputs are equally wide

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
- **THEN** the filter section wraps onto more lines and the page does not scroll horizontally

### Requirement: Shared table style
Every data table SHALL use one compact table style: text at 0.875rem, cells padded by 0.3rem at the top and bottom and 0.75rem at the left and right, a single-line body row at most 1.75rem high, the same header, alternating row background and hover background, and right-aligned tabular figures for numbers. Table cells SHALL NOT wrap their text. A table whose rows open a page SHALL show a pointer cursor over its rows.

#### Scenario: Compact rows
- **WHEN** a signed-in user opens the sell orders page with at least one sell order
- **THEN** the table's computed font size is 0.875rem and a single-line body row is at most 1.75rem high

#### Scenario: Roomier cells
- **WHEN** a signed-in user opens the trades page with at least one trade
- **THEN** every body cell has a computed top and bottom padding of 0.3rem and a left and right padding of 0.75rem

#### Scenario: Same striping everywhere
- **WHEN** a signed-in user compares the second body row of the dashboard breakdown, the wealth items table and an opportunity table
- **THEN** all three have the same alternating background

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
