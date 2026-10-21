## MODIFIED Requirements

### Requirement: Filter box
Every filter of a page SHALL be placed in the page's filter section, including time ranges. The filter section SHALL be shown below the page title and above the KPI cards and the content, without a border or card of its own. It SHALL hold the page's filters and no other text. When its content does not fit on one line, it SHALL wrap instead of scrolling sideways.

The filter section's groups are the character filter, the time range, the opportunity filters and the fee character. Neighbouring groups SHALL be 3rem apart horizontally, so the character filter stands apart from the filters beside it, and lines of a wrapped filter section SHALL be 0.75rem apart. Groups SHALL be set apart by spacing only, without a divider line or border. A page without such filters SHALL show no filter section.

Every group SHALL start at the top of its line of the filter section. A group that is taller than its neighbours, because its fields wrap onto further rows or because it shows helper text or an error message, SHALL NOT move its neighbours up or down.

The filters SHALL be structured as form fields:
- **Character filter**: a group with the visible legend "Characters" above the characters' checkboxes. The checkboxes are laid out side by side, each with its character's name as its label.
- **Text and number fields**: each field's label text is shown above its input. The fields of one form are laid out side by side in columns of equal width that wrap onto further rows when the width does not fit them. Every such field, including a field that searches by name, SHALL be a plain text input with the same shape, size and padding as the other fields, without a search icon, rounded search shape or built-in clear button. Every such input SHALL be 2rem high, border included, and SHALL keep the body text size: only its vertical padding is reduced. Wrapped rows of fields SHALL be 0.75rem apart, from the bottom of one row's inputs to the top of the next row's labels. Neighbouring columns stay 1rem apart.
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
- **THEN** the "Minimum margin (%)", "Maximum margin (%)", "Minimum ISK/day", "Item name" and "Capital per item (ISK)" labels are each above their input, and the five inputs are equally wide

#### Scenario: Low inputs at body text size
- **WHEN** a signed-in user opens the trend page at 1920×1080
- **THEN** every opportunity filter input is 2rem high, border included, and its computed font size is the body text size

#### Scenario: Wrapped rows apart
- **WHEN** a signed-in user opens the trend page in a window so narrow that the opportunity filters wrap onto two rows
- **THEN** the top edge of each label in the second row is 0.75rem below the bottom edge of the input above it

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

### Requirement: Pagination
Every table on a table page SHALL be paginated on the server. The page number SHALL be given by the `page` query parameter, starting at 1, and the page size by the `per_page` query parameter, one of 10, 25, 50, 100 or 200. An unknown page size SHALL select 25. A missing page size SHALL select the page's stored page size where the page has one, as the opportunity table pages do, and 25 otherwise. A missing, malformed or non-positive page number SHALL select page 1, and a page number past the last page SHALL select the last page. The page SHALL show a control to go to the previous and the next page, the current page and the number of pages as "Page x of y", and a control to choose the page size.

The pagination SHALL be the last block of the main content, below the table, except for an explainer card, which follows it. It SHALL NOT be part of the page footer. When the page is shorter than the window, the pagination SHALL sit at the bottom of the window, directly above the explainer card if the page has one and otherwise directly above the page footer, with the spare height between the table and the pagination. When the page is taller than the window, the pagination SHALL follow the table and SHALL only come into view when the page is scrolled to near its end. It SHALL NOT be fixed over the content. A live refresh or a change of page or page size SHALL keep this placement.

Going to another page or choosing another page size SHALL update the URL and the table without a full page reload, and SHALL keep the character selection, the filters, the range and the sorting. Changing the character selection, a filter, the range, the sorting or the page size SHALL show page 1. Pagination SHALL be applied after sorting. A table without rows SHALL show no pagination controls.

#### Scenario: Default page size
- **WHEN** a user with 120 sell orders opens the sell orders page
- **THEN** the table shows the first 25 orders and the pagination reads "Page 1 of 5"

#### Scenario: Page sizes
- **WHEN** a signed-in user opens the page-size control of any table page
- **THEN** it offers 10, 25, 50, 100 and 200, in this order

#### Scenario: Smallest page size
- **WHEN** a user with 120 sell orders chooses a page size of 10
- **THEN** the URL carries `per_page=10`, the table shows 10 orders and the pagination reads "Page 1 of 12"

#### Scenario: Next page
- **WHEN** that user activates the next page control
- **THEN** the URL carries `page=2`, the table shows orders 26 to 50, and the sorting and the character selection are unchanged

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
- **THEN** the table shows 25 orders and the order count card reads 120
