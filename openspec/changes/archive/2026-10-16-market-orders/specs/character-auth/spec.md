# Spec Delta

## MODIFIED Requirements

### Requirement: First login creates an account
When a character that is not linked to any account completes login and no one is signed in, the system SHALL create a new account, link the character to it, and start a signed-in session for that account.

#### Scenario: Brand-new character
- **WHEN** an unauthenticated user completes SSO login with a character Sekondo has never seen
- **THEN** a new account exists containing exactly that character, the user is signed in, and they land on the dashboard

### Requirement: Login resumes an existing account
When a character that is already linked to an account completes login and no one is signed in, the system SHALL start a session for that existing account, replace the character's stored tokens with the new ones, and clear its needs-re-login state.

#### Scenario: Returning character
- **WHEN** an unauthenticated user completes SSO login with a character linked to an account that has three characters
- **THEN** the user is signed in to that account, lands on the dashboard, and the characters page lists all three characters

#### Scenario: Login with any linked character
- **WHEN** the user signs out and then logs in with a different character linked to the same account
- **THEN** the user is signed in to the same account
