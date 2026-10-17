# Spec Delta

## MODIFIED Requirements

### Requirement: Navigation
Every signed-in page SHALL show a navigation with links to the dashboard, the orders page, the wealth page and the characters page, mark the current page, and offer sign out. Links to views that accept a character filter SHALL carry the current filter. Links SHALL NOT carry a time range.

#### Scenario: Navigate between pages
- **WHEN** a signed-in user on the dashboard follows the "Orders" link, then the "Wealth" link and then the "Characters" link
- **THEN** they reach the orders page, the wealth page and then the characters page, and the navigation marks each as current

#### Scenario: Selection carried to the wealth page
- **WHEN** a user with a narrowed selection on the orders page follows the "Wealth" link
- **THEN** the wealth page opens with the same selection

#### Scenario: Sign out from any page
- **WHEN** a signed-in user clicks "Sign out" in the navigation on the orders page
- **THEN** they are signed out and land on the landing page

#### Scenario: Not shown when signed out
- **WHEN** an unauthenticated user opens the landing page
- **THEN** no signed-in navigation is shown
