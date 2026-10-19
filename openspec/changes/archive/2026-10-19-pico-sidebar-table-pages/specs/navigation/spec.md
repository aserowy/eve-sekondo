## MODIFIED Requirements

### Requirement: Navigation
Every signed-in page SHALL show the main menu as a sidebar on the left of the page. From top to bottom it SHALL list "Dashboard", "Characters", "Wealth", "Trades", "Orders" and "Opportunities", and SHALL show "Sign out" pinned at the bottom of the sidebar. Every entry SHALL show an icon followed by its text. "Orders" SHALL lead to the sell orders page and "Opportunities" to the first opportunity table page. The menu SHALL mark the entry of the current page as current; the sell and buy orders pages mark "Orders", and the opportunity table pages and detail pages mark "Opportunities". Links to views that accept a character filter SHALL carry the current filter. Links SHALL NOT carry a time range, a page number or a page size.

#### Scenario: Entry order
- **WHEN** a signed-in user opens the dashboard
- **THEN** the sidebar lists "Dashboard", "Characters", "Wealth", "Trades", "Orders" and "Opportunities" from top to bottom, each with an icon and its text, and "Sign out" at the bottom of the sidebar

#### Scenario: Navigate between pages
- **WHEN** a signed-in user on the dashboard follows "Characters", then "Wealth", then "Trades", then "Orders" and then "Opportunities"
- **THEN** they reach the characters page, the wealth page, the trades page, the sell orders page and then the recurring spread expansion page, and the menu marks each entry as current

#### Scenario: Orders marked on the buy page
- **WHEN** a signed-in user opens the buy orders page
- **THEN** the menu marks "Orders" as current

#### Scenario: Selection carried to the wealth page
- **WHEN** a user with a narrowed selection on the sell orders page follows "Wealth"
- **THEN** the wealth page opens with the same selection

#### Scenario: Selection carried to the trades page
- **WHEN** a user with a narrowed selection on the sell orders page follows "Trades"
- **THEN** the trades page opens with the same selection

#### Scenario: Range not carried to the trades page
- **WHEN** a user on `/dashboard?range=30d` follows "Trades"
- **THEN** the trades page opens with its default range of 7 days

#### Scenario: Page not carried
- **WHEN** a user on page 2 of the trades table follows "Orders"
- **THEN** the sell orders page opens on page 1 with the default page size

#### Scenario: Selection carried to the opportunities page
- **WHEN** a user with a narrowed selection on the sell orders page follows "Opportunities"
- **THEN** the first opportunity table page opens with the same selection

#### Scenario: Icon-only controls keep their names
- **WHEN** a signed-in user collapses the sidebar and hovers the characters entry and the sign-out entry
- **THEN** each shows only an icon, reveals "Characters" and "Sign out" respectively on hover, and is found by assistive technology under that name

#### Scenario: Sign out from any page
- **WHEN** a signed-in user activates "Sign out" in the sidebar on the sell orders page
- **THEN** they are signed out and land on the landing page

#### Scenario: Not shown when signed out
- **WHEN** an unauthenticated user opens the landing page
- **THEN** no sidebar is shown

## ADDED Requirements

### Requirement: Collapsible main menu
The sidebar SHALL have a toggle at its top that collapses and expands it. Collapsed, the sidebar SHALL show only the entries' icons, and the content SHALL use the width the sidebar gave up. Each collapsed entry SHALL keep its text as its accessible name and show it on hover. The toggle SHALL expose its state to assistive technology and SHALL have the accessible name "Menu". The sidebar SHALL start expanded in a browser that has never collapsed it. The browser SHALL remember the state across page loads, live refreshes and sign-ins.

#### Scenario: Collapse
- **WHEN** a signed-in user activates the menu toggle on the dashboard
- **THEN** the sidebar shows only icons, the content becomes wider, and hovering the wealth icon shows "Wealth"

#### Scenario: Names kept while collapsed
- **WHEN** the sidebar is collapsed
- **THEN** assistive technology finds every entry, including "Sign out", under its text

#### Scenario: Remembered across pages
- **WHEN** a user collapses the sidebar on the dashboard and then follows the "Trades" icon
- **THEN** the trades page opens with the sidebar collapsed

#### Scenario: Starts expanded
- **WHEN** a user signs in for the first time in a new browser
- **THEN** the sidebar is expanded

### Requirement: Orders and opportunities entry pages
A signed-in request for `/orders` SHALL be redirected to the sell orders page `/orders/sell`, and a request for `/opportunities` to the first opportunity table page `/opportunities/expansion`. Both redirects SHALL keep the query parameters of the request.

#### Scenario: Old orders bookmark
- **WHEN** a signed-in user opens a bookmarked `/orders?chars=<id>`
- **THEN** they are redirected to `/orders/sell?chars=<id>`

#### Scenario: Old opportunities bookmark
- **WHEN** a signed-in user opens `/opportunities` with the trend table sorted by σ
- **THEN** they are redirected to `/opportunities/expansion` with the trend table still sorted by σ
