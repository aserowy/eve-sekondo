# Spec Delta

## MODIFIED Requirements

### Requirement: Navigation
Every signed-in page SHALL show a navigation with links to the dashboard, the orders page, the wealth page, the trades page, the opportunities page and the characters page, mark the current page, and offer sign out. The dashboard, orders, wealth, trades and opportunities links SHALL be shown as text. The characters link and sign out SHALL be shown as icons only; each SHALL keep "Characters" and "Sign out" as its accessible name and show it on hover. Links to views that accept a character filter SHALL carry the current filter. Links SHALL NOT carry a time range.

#### Scenario: Navigate between pages
- **WHEN** a signed-in user on the dashboard follows the "Orders" link, then the "Wealth" link, then the "Trades" link, then the "Opportunities" link and then the "Characters" link
- **THEN** they reach the orders page, the wealth page, the trades page, the opportunities page and then the characters page, and the navigation marks each as current

#### Scenario: Selection carried to the wealth page
- **WHEN** a user with a narrowed selection on the orders page follows the "Wealth" link
- **THEN** the wealth page opens with the same selection

#### Scenario: Selection carried to the trades page
- **WHEN** a user with a narrowed selection on the orders page follows the "Trades" link
- **THEN** the trades page opens with the same selection

#### Scenario: Range not carried to the trades page
- **WHEN** a user on `/dashboard?range=30d` follows the "Trades" link
- **THEN** the trades page opens with its default range of 7 days

#### Scenario: Selection carried to the opportunities page
- **WHEN** a user with a narrowed selection on the orders page follows the "Opportunities" link
- **THEN** the opportunities page opens with the same selection

#### Scenario: Sign out from any page
- **WHEN** a signed-in user clicks "Sign out" in the navigation on the orders page
- **THEN** they are signed out and land on the landing page

#### Scenario: Icon-only controls keep their names
- **WHEN** a signed-in user hovers the characters link and the sign-out control in the navigation
- **THEN** each shows only an icon, reveals "Characters" and "Sign out" respectively on hover, and is found by assistive technology under that name

#### Scenario: Not shown when signed out
- **WHEN** an unauthenticated user opens the landing page
- **THEN** no signed-in navigation is shown
