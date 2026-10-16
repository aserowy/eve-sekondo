# character-filter Specification

## Purpose

Lets a trader look at all of their characters or a chosen subset, with the selection kept in the URL so it can be bookmarked and applies the same way to every view.

## Requirements

### Requirement: Character selection in the URL
Every signed-in view that shows character data SHALL accept a `chars` query parameter holding a comma-separated list of character IDs, and SHALL show data only for the selected characters. A missing or empty `chars` parameter SHALL select all characters of the account.

#### Scenario: All characters by default
- **WHEN** a user with three characters opens the orders page without a `chars` parameter
- **THEN** orders of all three characters are shown

#### Scenario: Subset
- **WHEN** a user with three characters opens the orders page with `?chars=<id of the first>,<id of the second>`
- **THEN** only orders of those two characters are shown

#### Scenario: Bookmark
- **WHEN** a user opens a bookmarked URL with a `chars` parameter in a new browser session after signing in
- **THEN** the same characters are selected

### Requirement: Selection is limited to the account
The system SHALL ignore any ID in `chars` that is not a character of the signed-in account, including malformed values, and SHALL NOT reveal whether such a character exists. When no selected ID remains after this, all characters of the account SHALL be selected.

#### Scenario: Foreign character ID
- **WHEN** a user requests the orders page with `?chars=<id of a character of another account>`
- **THEN** no data of that character is shown, and the page shows the user's own characters as if no filter were set

#### Scenario: Mixed IDs
- **WHEN** a user requests `?chars=<own id>,<foreign id>,abc`
- **THEN** only data of the user's own selected character is shown

### Requirement: Filter control
Signed-in views that accept `chars` SHALL show a control listing the account's characters, through which the user can select all characters or any subset. Changing the selection SHALL update the URL and the shown data without a full page reload, and every fragment the page loads afterwards SHALL carry the selection.

#### Scenario: Narrow the selection
- **WHEN** a user on the orders page deselects one of three characters in the filter control
- **THEN** the URL's `chars` parameter lists the two remaining characters and the tables show only their orders

#### Scenario: Selection survives a live refresh
- **WHEN** the selection is narrowed to one character and a live update refreshes the tables
- **THEN** the refreshed tables still show only that character's orders

#### Scenario: Selection carries across pages
- **WHEN** a user with a narrowed selection follows the navigation to another view that accepts `chars`
- **THEN** that view opens with the same selection
