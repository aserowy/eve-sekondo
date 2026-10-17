# character-auth Specification

## Purpose

Lets a player sign in to Sekondo with EVE Online SSO, link several characters to one account, and keep the ESI credentials needed to read their private data valid over time.

## Requirements

### Requirement: Sign in with EVE SSO
The system SHALL let an unauthenticated user start a login that redirects to EVE SSO. The request SHALL ask for exactly the scopes `esi-markets.read_character_orders.v1`, `esi-universe.read_structures.v1`, `esi-wallet.read_character_wallet.v1` and `esi-assets.read_assets.v1`, and SHALL be protected by a single-use `state` value and PKCE. All four scopes SHALL be required: a character whose granted scopes lack any of them, including a character linked before the wallet and assets scopes were introduced, SHALL be marked "needs re-login" under the needs re-login rule.

#### Scenario: Start login
- **WHEN** an unauthenticated user clicks "Log in with EVE Online"
- **THEN** the browser is redirected to the configured SSO authorize URL with the client ID, the callback URL, exactly the four required scopes, a `state` value and a PKCE code challenge

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
- **WHEN** a character linked with only the orders and structures scopes has its token refreshed
- **THEN** it is marked "needs re-login", and logging in again with all four scopes clears the mark

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

### Requirement: Character ownership change
The system SHALL compare the character owner hash reported by SSO with the one stored for the character. If they differ, the character has changed hands. The system SHALL then remove the character and all of its data from its previous account, and treat the login as a login with a brand-new character.

#### Scenario: Character was transferred to another player
- **WHEN** a character linked to account A logs in with an owner hash that differs from the stored one
- **THEN** the character and its data are removed from account A, and the login proceeds as for a character Sekondo has never seen

### Requirement: Add character to the current account
A signed-in user SHALL be able to start another SSO login that links the resulting character to their current account without ending their session.

#### Scenario: Add a second character
- **WHEN** a signed-in user with one character clicks "Add character" and completes SSO with another character
- **THEN** the account has both characters and the user is still signed in to the same account

#### Scenario: Add a character that is already in this account
- **WHEN** a signed-in user completes "Add character" with a character already linked to their account
- **THEN** no duplicate is created, the character's tokens are replaced, and its needs-re-login state is cleared

#### Scenario: Add a character linked to another account
- **WHEN** a signed-in user completes "Add character" with a character linked to a different account with the same owner hash
- **THEN** the character is not moved, the user stays signed in to their account, and they see a message saying the character belongs to another account and must be unlinked there first

### Requirement: Credential storage
The system SHALL store ESI refresh and access tokens encrypted at rest with a key supplied through configuration, and SHALL NOT write tokens in plain text to the database or logs. The system SHALL refuse to start if the encryption key is missing or invalid.

#### Scenario: Tokens are not readable in the database
- **WHEN** a character has been linked
- **THEN** no column in the database contains the refresh token or access token in plain text

#### Scenario: Missing encryption key
- **WHEN** the application starts without a valid token encryption key configured
- **THEN** it exits with an error naming the missing or invalid setting

### Requirement: Access token refresh
The system SHALL provide a valid ESI access token for a linked character on demand. It SHALL refresh the token through SSO when the token has expired or is about to expire, and SHALL store the refresh token SSO returns in place of the old one.

#### Scenario: Expired access token
- **WHEN** an access token is requested for a character whose stored access token has expired
- **THEN** the system refreshes it through SSO, stores the new access and refresh tokens, and returns the new access token

#### Scenario: Valid access token
- **WHEN** an access token is requested for a character whose stored access token is still valid
- **THEN** the stored token is returned without contacting SSO

### Requirement: Needs re-login state
The system SHALL mark a character "needs re-login" when SSO rejects its refresh token, or when its granted scopes do not include every required scope. While marked, the system SHALL not request tokens for the character, SHALL keep its existing data, and SHALL show the state to the user. Temporary failures, such as network errors or SSO server errors, SHALL NOT mark the character.

#### Scenario: Refresh token revoked
- **WHEN** a token refresh for a character is rejected by SSO as invalid
- **THEN** the character is marked "needs re-login" and the characters page shows a badge with a "Log in again" action for it

#### Scenario: Missing scope
- **WHEN** a character's granted scopes do not include every required scope
- **THEN** the character is marked "needs re-login"

#### Scenario: SSO temporarily unavailable
- **WHEN** a token refresh fails because of a network error or an SSO server error
- **THEN** the character is not marked "needs re-login" and the refresh can be retried later

#### Scenario: Re-login clears the state
- **WHEN** the user logs in again with a character marked "needs re-login" and grants all required scopes
- **THEN** the mark is cleared and the new tokens are stored

### Requirement: Characters page
A signed-in user SHALL be able to see every character linked to their account, with its portrait, its name, and whether it needs re-login, plus actions to add a character, unlink a character and sign out.

#### Scenario: List linked characters
- **WHEN** a signed-in user opens the characters page
- **THEN** every character linked to their account is listed with portrait, name and status, and no character from another account is shown

### Requirement: Unlink character
A signed-in user SHALL be able to unlink a character from their account after confirming. Unlinking SHALL delete the character's tokens and all data stored for it. Unlinking the last character SHALL delete the account and sign the user out.

#### Scenario: Unlink one of several characters
- **WHEN** a signed-in user with two characters confirms unlinking one of them
- **THEN** that character and all of its data are deleted, and the characters page lists only the other character

#### Scenario: Unlink the last character
- **WHEN** a signed-in user confirms unlinking their only character
- **THEN** the character and the account are deleted and the user is signed out to the landing page

#### Scenario: Unlink a character of another account
- **WHEN** an unlink request names a character that is not linked to the signed-in account
- **THEN** nothing is deleted and the request is rejected

### Requirement: Sessions
The system SHALL keep signed-in state in a session whose data is stored server-side and survives application restarts. The session cookie SHALL be `Secure`, `HttpOnly`, `SameSite=Lax` and host-only (`__Host-` prefix) in every environment, with no configuration that weakens these attributes. Sessions SHALL expire after 7 days of inactivity and at the latest 30 days after sign-in. The session identifier SHALL be replaced when a user signs in. Pages that need an account SHALL redirect unauthenticated users to the landing page. Signing out SHALL delete the session on the server.

#### Scenario: Session survives restart
- **WHEN** a signed-in user reloads a page after the application has restarted
- **THEN** the user is still signed in

#### Scenario: Cookie attributes
- **WHEN** a user signs in
- **THEN** the session cookie is named with the `__Host-` prefix and is `Secure`, `HttpOnly`, `SameSite=Lax` and `Path=/` with no `Domain`

#### Scenario: Session identifier rotates on sign-in
- **WHEN** a browser that already has a session cookie completes login
- **THEN** the session cookie value after login differs from the one before login, and the old value no longer authenticates

#### Scenario: Idle timeout
- **WHEN** a signed-in user makes no request for more than 7 days
- **THEN** their next request to a protected page redirects to the landing page

#### Scenario: Absolute lifetime
- **WHEN** a session is older than 30 days, even if it was used every day
- **THEN** the next request to a protected page redirects to the landing page

#### Scenario: Unauthenticated access to a protected page
- **WHEN** an unauthenticated user requests the characters page
- **THEN** they are redirected to the landing page

#### Scenario: Sign out
- **WHEN** a signed-in user clicks "Sign out"
- **THEN** the session is deleted on the server, replaying the old cookie value does not authenticate, and requesting the characters page redirects to the landing page

### Requirement: Cross-site request forgery protection
The system SHALL reject every state-changing request (any method other than GET, HEAD or OPTIONS) unless it carries the session's CSRF token and comes from the application's own origin. It SHALL reject such requests with HTTP 403 and SHALL NOT perform the action. GET requests SHALL NOT change account or character data.

#### Scenario: Missing CSRF token
- **WHEN** a signed-in user's browser sends a request to unlink a character or sign out without a CSRF token
- **THEN** the response is 403 and nothing changes

#### Scenario: Wrong CSRF token
- **WHEN** a state-changing request carries a CSRF token from a different session
- **THEN** the response is 403 and nothing changes

#### Scenario: Cross-origin request
- **WHEN** a state-changing request carries a valid CSRF token but its `Origin` differs from the application's public origin
- **THEN** the response is 403 and nothing changes

#### Scenario: Legitimate request from the app
- **WHEN** the user clicks "Unlink" or "Sign out" in the application
- **THEN** the request carries the token automatically and succeeds

### Requirement: HTTPS only
The system SHALL serve its user interface only over TLS, and SHALL NOT open any plain-HTTP listener. There SHALL be no exception for `localhost` or loopback addresses. It SHALL refuse to start when its public base URL, the EVE SSO base URL or the ESI base URL is not `https://`, or when no valid TLS certificate and private key are configured. It SHALL connect to the database only over TLS with full certificate and hostname verification.

#### Scenario: Plain http on localhost
- **WHEN** the application starts with a public base URL of `http://localhost:8443`
- **THEN** it exits with an error explaining that HTTPS is required

#### Scenario: Plain http EVE endpoint
- **WHEN** the application starts with an SSO or ESI base URL that uses `http://`
- **THEN** it exits with an error explaining that HTTPS is required

#### Scenario: Missing certificate
- **WHEN** the application starts without a readable TLS certificate and private key
- **THEN** it exits with an error naming the missing setting

#### Scenario: Plain-HTTP request
- **WHEN** a client opens a plain-HTTP connection to the application's port
- **THEN** no page or session cookie is served

#### Scenario: Database connection without verified TLS
- **WHEN** the application starts with a database connection string whose TLS mode is weaker than full certificate and hostname verification
- **THEN** it exits with an error explaining that a verified TLS database connection is required

#### Scenario: HTTPS on localhost
- **WHEN** the application starts with a public base URL of `https://localhost:8443` and a valid certificate for `localhost`
- **THEN** it serves the landing page over TLS

### Requirement: Browser security headers
Every response SHALL carry a Content Security Policy that allows scripts and styles only from the application itself and images only from the application and the EVE image server, forbids framing, and allows form submissions only to the application and EVE SSO. Every response SHALL also set `Strict-Transport-Security` with a max-age of at least one year, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` and `X-Frame-Options: DENY`.

#### Scenario: Headers on a page
- **WHEN** any page or fragment is requested
- **THEN** the response includes the Content Security Policy and the other security headers

#### Scenario: Application works under the policy
- **WHEN** a user logs in, adds a character, unlinks a character and signs out
- **THEN** the browser reports no Content Security Policy violations

### Requirement: Configurable EVE endpoints
The system SHALL read the EVE SSO base URL and the ESI base URL from configuration. They SHALL default to the official EVE endpoints, so the application can run against a stand-in server.

#### Scenario: Run against a stub server
- **WHEN** the SSO base URL is configured to point to a stub server
- **THEN** login, token validation and token refresh all use the stub server and never contact the official EVE endpoints
