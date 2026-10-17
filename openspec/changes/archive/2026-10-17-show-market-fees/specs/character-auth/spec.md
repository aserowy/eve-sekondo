# Spec Delta

## MODIFIED Requirements

### Requirement: Sign in with EVE SSO
The system SHALL let an unauthenticated user start a login that redirects to EVE SSO. The request SHALL ask for exactly the scopes `esi-markets.read_character_orders.v1`, `esi-universe.read_structures.v1`, `esi-wallet.read_character_wallet.v1`, `esi-assets.read_assets.v1`, `esi-skills.read_skills.v1` and `esi-characters.read_standings.v1`, and SHALL be protected by a single-use `state` value and PKCE. All six scopes SHALL be required: a character whose granted scopes lack any of them, including a character linked before the skills and standings scopes were introduced, SHALL be marked "needs re-login" under the needs re-login rule.

#### Scenario: Start login
- **WHEN** an unauthenticated user clicks "Log in with EVE Online"
- **THEN** the browser is redirected to the configured SSO authorize URL with the client ID, the callback URL, exactly the six required scopes, a `state` value and a PKCE code challenge

#### Scenario: Callback with mismatched state
- **WHEN** the SSO callback arrives with a `state` value that does not match the one issued for this browser session
- **THEN** the login is rejected, no character is linked, no account session is established, and an error page offers to try again

#### Scenario: SSO reports an error or the user cancels
- **WHEN** the SSO callback arrives with an error instead of an authorization code
- **THEN** no character is linked and the user sees an error page that offers to try again

#### Scenario: Invalid access token
- **WHEN** the token returned by SSO fails signature, issuer, audience or expiry validation
- **THEN** the login is rejected and no character data is stored

#### Scenario: Character linked before the new scopes
- **WHEN** a character linked with only the orders, structures, wallet and assets scopes has its token refreshed
- **THEN** it is marked "needs re-login", and logging in again with all six scopes clears the mark
