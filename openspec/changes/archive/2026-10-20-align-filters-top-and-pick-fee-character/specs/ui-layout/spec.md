# Spec Delta

## MODIFIED Requirements

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

