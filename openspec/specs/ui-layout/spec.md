# ui-layout Specification

## Purpose

Defines how Sekondo's pages use the screen: a dense, dark, cockpit-like layout that spends every pixel on information, consistent across all pages and built without a third-party CSS framework.

## Requirements

### Requirement: Dense theme
Every page SHALL use one dark theme served by the application itself, without any third-party stylesheet. Body text SHALL be at most 13px. Table body rows with a single line of text SHALL be at most 22px high. Numeric table cells SHALL be right-aligned with tabular figures. Content areas SHALL be framed as panels with a thin border and a small uppercase title instead of cards with shadows. The theme SHALL keep a success style and an error style for status markers. Pages SHALL render without any Content Security Policy violation.

#### Scenario: Compact text and rows
- **WHEN** a signed-in user opens the orders page with at least one sell order
- **THEN** the computed body font size is at most 13px and a sell table row is at most 22px high

#### Scenario: No third-party stylesheet
- **WHEN** any page is loaded
- **THEN** the only stylesheet it loads is the application's own

#### Scenario: No policy violations
- **WHEN** a signed-in user visits the dashboard, the orders page, the wealth page and the characters page
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Full-width pages
Every page SHALL use the full width of the browser window, apart from a gutter of at most 8px on each side. Layouts SHALL be designed for windows of 1280px width and more; narrower windows SHALL stay usable by wrapping, without horizontal page scrolling.

#### Scenario: Wide window
- **WHEN** a signed-in user opens the orders page in a 2560px wide window
- **THEN** the order tables' panels end at most 8px from both window edges

#### Scenario: Narrow window
- **WHEN** a signed-in user opens the dashboard in a 1024px wide window
- **THEN** the page does not scroll horizontally

### Requirement: Top bar
Every signed-in page SHALL show a single top bar, at most 36px high at 1280px width or more, holding the brand, the navigation, and the page's controls. On pages that accept a character filter, the filter control SHALL be in the top bar. On the dashboard, the time range control SHALL be in the top bar. Pages SHALL NOT show a separate page heading; the navigation's current-page marker identifies the page.

#### Scenario: Dashboard bar
- **WHEN** a signed-in user opens the dashboard at 1920×1080
- **THEN** the top bar is a single row at most 36px high containing the navigation, the character filter and the time range control, and the page has no `h1` heading

#### Scenario: Characters page bar
- **WHEN** a signed-in user opens the characters page
- **THEN** the top bar shows the navigation and no character filter

### Requirement: Dashboard fits one screen
At a window of 1920×1080 the dashboard SHALL show all of its tiles, the breakdown, the per-character split, both top-item graphs and all three over-time graphs without vertical page scrolling. A panel whose content does not fit SHALL scroll inside itself. Graphs SHALL take their height from the panel they are in.

#### Scenario: No page scroll
- **WHEN** a signed-in user with three characters opens the dashboard at 1920×1080
- **THEN** the page's scroll height does not exceed the window height, and every tile and graph is inside the window

#### Scenario: Many characters
- **WHEN** a user with twenty characters opens the dashboard at 1920×1080
- **THEN** the breakdown panel scrolls inside itself and the page still does not scroll vertically

### Requirement: Scrolling panels with sticky headers
The orders page's tables, the dashboard breakdown and the wealth page's items table SHALL fill their panel and scroll inside it. While such a table scrolls, its header row SHALL stay visible at the top of the panel. At 1280px width or more these pages SHALL NOT scroll vertically as a whole.

#### Scenario: Scroll the sell table
- **WHEN** a user with 200 sell orders scrolls the sell table to its end at 1920×1080
- **THEN** the sell table's header row stays visible, the buy table stays in place, and the page itself has not scrolled

### Requirement: Freshness side column
On the orders page and the wealth page, the freshness information (and on the orders page the fees per location) SHALL be shown in a narrow column on the right of the tables at 1280px width or more, scrolling inside itself when it does not fit. The order tables SHALL be stacked, the sell table above the buy table, each using the full width left of the side column.

#### Scenario: Orders page columns
- **WHEN** a signed-in user opens the orders page at 1920×1080
- **THEN** the freshness and fees are right of the order tables, and the buy table is below the sell table

#### Scenario: Wealth page columns
- **WHEN** a signed-in user opens the wealth page at 1920×1080
- **THEN** the freshness is right of the graph and the items table, and the items table is below the graph

### Requirement: Icons
Icons SHALL be used sparingly and come from one consistent line-icon set served by the application itself. Icon-only controls SHALL appear only in the top bar, and each SHALL keep its text as its accessible name and show it on hover. Outside the top bar, an action MAY carry an icon in front of its text, but SHALL keep the text. The "Unlink", "Add character" and "Log in again" actions SHALL carry an icon and their text. Icons SHALL take the colour of the surrounding text.

#### Scenario: Actions keep their text
- **WHEN** a signed-in user opens the characters page
- **THEN** the "Add character" and "Unlink" controls show an icon followed by their text

#### Scenario: Log in again keeps its text
- **WHEN** a character needs re-login and the user opens the orders page
- **THEN** its "Log in again" link shows an icon followed by its text

### Requirement: Chart styling
Graphs SHALL use small, dimmed axis labels, grid lines and legends that match the theme. Their data, colours per series and hover tooltips SHALL be unchanged by the theme.

#### Scenario: Tooltip unchanged
- **WHEN** a user hovers the sell value line at a point where the value is 1,234,567,890.12 ISK
- **THEN** a tooltip shows the point's time in EVE time, "Sell value" and "1,234,567,890.12 ISK"
