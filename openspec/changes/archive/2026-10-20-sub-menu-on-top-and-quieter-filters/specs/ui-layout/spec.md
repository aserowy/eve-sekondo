## MODIFIED Requirements

### Requirement: Page structure
Every signed-in page SHALL be laid out right of the sidebar as a page column. From top to bottom, the column holds the page header, the page's main content and, on pages with a freshness section, a page footer. The page header consists of, from top to bottom, the sub-menu if the page has one, the page title as the page's only first-level heading, and the filter section if the page has one. On a page with a sub-menu, the sub-menu SHALL be the first element of the page column; on a page without one, the page title SHALL be. The main content and the page footer SHALL be exposed to assistive technology as the page's main and content-info landmarks. The page titles are:
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
- **THEN** the sub-menu is above the page title "Buy orders", and the page title is above the filter section

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
Every filter of a page SHALL be placed in the page's filter section, including time ranges. The filter section SHALL be shown below the page title and above the KPI cards and the content, without a border or card of its own. It SHALL hold the page's filters, and on its right end, dimmed text that describes the whole page when the page has such text. When its content does not fit on one line, it SHALL wrap instead of scrolling sideways.

The filter section's groups are the character filter, the time range, the opportunity filters and the page's descriptive text. Neighbouring groups SHALL be 3rem apart horizontally, so the character filter stands apart from the filters beside it, and lines of a wrapped filter section SHALL be 0.75rem apart. Groups SHALL be set apart by spacing only, without a divider line or border. A page without such filters or text SHALL show no filter section.

The filters SHALL be structured as form fields:
- **Character filter**: a group with the visible legend "Characters" above the characters' checkboxes. The checkboxes are laid out side by side, each with its character's name as its label.
- **Text and number fields**: each field's label text is shown above its input. The fields of one form are laid out side by side in columns of equal width that wrap onto further rows when the width does not fit them. Every such field, including a field that searches by name, SHALL be a plain text input with the same shape, size and padding as the other fields, without a search icon, rounded search shape or built-in clear button.
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
