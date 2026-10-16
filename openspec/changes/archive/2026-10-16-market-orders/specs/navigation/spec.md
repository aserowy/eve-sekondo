# Spec Delta

## Purpose

Gives signed-in users a home page and a consistent way to move between Sekondo's pages, so new views can be added without each page linking to every other one.

## ADDED Requirements

### Requirement: Signed-in home
The system SHALL provide a dashboard page as the signed-in home. A signed-in user requesting `/` SHALL be redirected to the dashboard. Until the dashboard gains content, it SHALL state that the dashboard is coming and link to the orders page. Unauthenticated requests to the dashboard SHALL be redirected to the landing page.

#### Scenario: Signed-in user opens the root
- **WHEN** a signed-in user requests `/`
- **THEN** they are redirected to the dashboard

#### Scenario: Stub dashboard
- **WHEN** a signed-in user opens the dashboard
- **THEN** the page says the dashboard is coming and links to the orders page

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated user requests the dashboard
- **THEN** they are redirected to the landing page

### Requirement: Navigation
Every signed-in page SHALL show a navigation with links to the dashboard, the orders page and the characters page, mark the current page, and offer sign out. Links to views that accept a character filter SHALL carry the current filter.

#### Scenario: Navigate between pages
- **WHEN** a signed-in user on the dashboard follows the "Orders" link and then the "Characters" link
- **THEN** they reach the orders page and then the characters page, and the navigation marks each as current

#### Scenario: Sign out from any page
- **WHEN** a signed-in user clicks "Sign out" in the navigation on the orders page
- **THEN** they are signed out and land on the landing page

#### Scenario: Not shown when signed out
- **WHEN** an unauthenticated user opens the landing page
- **THEN** no signed-in navigation is shown
