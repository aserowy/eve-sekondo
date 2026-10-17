# navigation Specification

## Purpose

Gives signed-in users a home page and a consistent way to move between Sekondo's pages, so new views can be added without each page linking to every other one.

## Requirements

### Requirement: Dashboard as signed-in home
The system SHALL provide the dashboard page as the signed-in home. A signed-in user requesting `/` SHALL be redirected to the dashboard, and a completed sign-in SHALL land on it. The dashboard SHALL show the figures described by the dashboard capability. Unauthenticated requests to the dashboard SHALL be redirected to the landing page.

#### Scenario: Signed-in user opens the root
- **WHEN** a signed-in user requests `/`
- **THEN** they are redirected to the dashboard

#### Scenario: Dashboard after sign-in
- **WHEN** a user completes sign-in
- **THEN** they land on the dashboard, which shows the totals of their characters' orders

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests the dashboard
- **THEN** they are redirected to the landing page

### Requirement: Navigation
Every signed-in page SHALL show a navigation with links to the dashboard, the orders page, the wealth page and the characters page, mark the current page, and offer sign out. The dashboard, orders and wealth links SHALL be shown as text. The characters link and sign out SHALL be shown as icons only; each SHALL keep "Characters" and "Sign out" as its accessible name and show it on hover. Links to views that accept a character filter SHALL carry the current filter. Links SHALL NOT carry a time range.

#### Scenario: Navigate between pages
- **WHEN** a signed-in user on the dashboard follows the "Orders" link, then the "Wealth" link and then the "Characters" link
- **THEN** they reach the orders page, the wealth page and then the characters page, and the navigation marks each as current

#### Scenario: Selection carried to the wealth page
- **WHEN** a user with a narrowed selection on the orders page follows the "Wealth" link
- **THEN** the wealth page opens with the same selection

#### Scenario: Sign out from any page
- **WHEN** a signed-in user clicks "Sign out" in the navigation on the orders page
- **THEN** they are signed out and land on the landing page

#### Scenario: Icon-only controls keep their names
- **WHEN** a signed-in user hovers the characters link and the sign-out control in the navigation
- **THEN** each shows only an icon, reveals "Characters" and "Sign out" respectively on hover, and is found by assistive technology under that name

#### Scenario: Not shown when signed out
- **WHEN** an unauthenticated user opens the landing page
- **THEN** no signed-in navigation is shown
