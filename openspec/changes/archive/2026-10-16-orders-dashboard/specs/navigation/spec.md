# Spec Delta

## ADDED Requirements

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

## REMOVED Requirements

### Requirement: Signed-in home
**Reason**: It described a placeholder dashboard that says the dashboard is coming; the dashboard now has content.
**Migration**: Replaced by "Dashboard as signed-in home", which keeps the redirect from `/` and the redirect of unauthenticated requests.
