# Spec Delta

## ADDED Requirements

### Requirement: Dashboard rows
The dashboard SHALL be laid out in rows, top to bottom: the tiles; the per-character breakdown across the full width; the per-character split graph, the top items by sell value graph and the top items by buy escrow graph side by side; then the value over time graph, the order count over time graph and the wealth over time graph, each alone on a row across the full width. The page MAY scroll vertically. At a window of 1920×1080 the tiles, the breakdown and the split and top-items row SHALL be inside the window without scrolling. The breakdown SHALL grow with its rows up to a limit and SHALL scroll inside itself beyond it. Each over-time graph SHALL be at least 16rem high.

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
- **WHEN** a user with twenty characters opens the dashboard at 1920×1080
- **THEN** the breakdown scrolls inside itself and the split and top-items row is still inside the window

### Requirement: Collapsible freshness
On the orders page and the wealth page, the freshness side column SHALL start collapsed when the page loads: a narrow strip with an icon-only toggle. Activating the toggle SHALL unfold the column and activating it again SHALL fold it. The toggle SHALL expose its state to assistive technology, SHALL have the accessible name "Freshness", and SHALL show "Show freshness" or "Hide freshness" on hover. While collapsed, the strip SHALL show the number of selected characters that are stale or need re-login (on the wealth page: whose wallet or assets are stale, or who need re-login) in the error style, and SHALL show no number when there are none. The state SHALL stay as the user left it across live refreshes and changes of filter, sorting and range on the same page, and SHALL return to collapsed when the page is loaded again. While collapsed, the tables and the graph SHALL use the width the column gave up.

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

#### Scenario: Kept across a live refresh
- **WHEN** the user has unfolded the freshness on the orders page and an `orders-updated` signal arrives
- **THEN** the refreshed freshness is still unfolded

#### Scenario: Kept across a filter change
- **WHEN** the user has unfolded the freshness on the wealth page and deselects a character
- **THEN** the freshness is still unfolded and shows only the selected characters

#### Scenario: Reload starts collapsed
- **WHEN** the user has unfolded the freshness and reloads the page
- **THEN** the freshness is collapsed

## MODIFIED Requirements

### Requirement: Scrolling panels with sticky headers
The orders page's tables, the dashboard breakdown and the wealth page's items table SHALL fill their panel and scroll inside it. While such a table scrolls, its header row SHALL stay visible at the top of the panel. At 1280px width or more the orders page and the wealth page SHALL NOT scroll vertically as a whole.

#### Scenario: Scroll the sell table
- **WHEN** a user with 200 sell orders scrolls the sell table to its end at 1920×1080
- **THEN** the sell table's header row stays visible, the buy table stays in place, and the page itself has not scrolled

### Requirement: Freshness side column
On the orders page and the wealth page, the freshness information (and on the orders page the fees per location) SHALL be shown, when unfolded, in a narrow column on the right of the tables at 1280px width or more, scrolling inside itself when it does not fit. The order tables SHALL be stacked, the sell table above the buy table, each using the full width left of the side column or of its collapsed strip.

#### Scenario: Orders page columns
- **WHEN** a signed-in user opens the orders page at 1920×1080 and unfolds the freshness
- **THEN** the freshness and fees are right of the order tables, and the buy table is below the sell table

#### Scenario: Wealth page columns
- **WHEN** a signed-in user opens the wealth page at 1920×1080 and unfolds the freshness
- **THEN** the freshness is right of the graph and the items table, and the items table is below the graph

### Requirement: Icons
Icons SHALL be used sparingly and come from one consistent line-icon set served by the application itself. Icon-only controls SHALL appear only in the top bar, with one exception: the freshness toggle. Each icon-only control SHALL keep its text as its accessible name and show it on hover. Outside the top bar, any other action MAY carry an icon in front of its text, but SHALL keep the text. The "Unlink", "Add character" and "Log in again" actions SHALL carry an icon and their text. Icons SHALL take the colour of the surrounding text.

#### Scenario: Actions keep their text
- **WHEN** a signed-in user opens the characters page
- **THEN** the "Add character" and "Unlink" controls show an icon followed by their text

#### Scenario: Log in again keeps its text
- **WHEN** a character needs re-login and the user opens the orders page and unfolds the freshness
- **THEN** its "Log in again" link shows an icon followed by its text

#### Scenario: Freshness toggle is icon-only
- **WHEN** a signed-in user opens the orders page
- **THEN** the freshness toggle shows only an icon (and a warning count when there is one), has the accessible name "Freshness", and shows "Show freshness" on hover

## REMOVED Requirements

### Requirement: Dashboard fits one screen
**Reason**: The user wants the breakdown across the full width and each over-time graph on its own full-width row, which no longer fits one 1920×1080 screen.
**Migration**: Replaced by "Dashboard rows": the dashboard scrolls vertically; tiles, breakdown and the split and top-items row stay on the first screen.
