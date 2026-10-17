# live-updates Specification

## Purpose

Pushes change signals to a signed-in user's open pages so they refresh the affected parts on their own when new data arrives, without the user reloading.

## Requirements

### Requirement: Account event stream
The system SHALL offer a signed-in user a server-sent event stream that delivers change signals for their own account only. Signals SHALL name what changed (such as `orders-updated`) and SHALL NOT carry order data. The stream SHALL reject requests without a signed-in session and SHALL send a keep-alive at least every 30 seconds.

#### Scenario: Signal after a poll
- **WHEN** a signed-in user has the orders page open and a poll of one of their characters completes
- **THEN** their stream delivers an `orders-updated` signal

#### Scenario: Signals stay within the account
- **WHEN** a poll completes for a character of account A
- **THEN** a user signed in to account B receives no signal for it

#### Scenario: Unauthenticated stream request
- **WHEN** a request without a signed-in session opens the event stream
- **THEN** no stream is opened and no signal is delivered

#### Scenario: Signals carry no data
- **WHEN** an `orders-updated` signal is delivered
- **THEN** it contains no order, price or character data beyond the signal name

### Requirement: Stream ends with the session
The system SHALL close an open event stream within 60 seconds after its session is signed out or expires, and SHALL NOT deliver signals over it afterwards.

#### Scenario: Sign out in another tab
- **WHEN** a user with an open orders page signs out in another tab
- **THEN** the open stream is closed within 60 seconds and reconnecting does not open a new one

### Requirement: Pages refresh on signals
Pages that show data affected by a signal SHALL re-fetch the affected parts when the signal arrives, keeping the current character filter and table sorting, without a full page reload. When the stream drops, the page SHALL reconnect and re-fetch the affected parts once it is connected again.

#### Scenario: Table refreshes after a poll
- **WHEN** a user has the orders page open and a poll changes the price of one of their orders
- **THEN** the page shows the new price without the user reloading

#### Scenario: Filter and sorting are kept
- **WHEN** the orders page is filtered to one character and sorted by price, and an `orders-updated` signal arrives
- **THEN** the refreshed tables are still filtered to that character and sorted by price

#### Scenario: Missed signals after a reconnect
- **WHEN** the stream was disconnected while a poll changed an order, and the stream reconnects
- **THEN** the page shows the changed order without the user reloading

### Requirement: Market signal
The system SHALL deliver a `market-updated` signal over the event stream of an account when a market refresh changes the standing or the gap of at least one current order of that account. The signal SHALL name only what changed and SHALL NOT carry order or price data. Accounts none of whose orders changed standing SHALL NOT receive it, and no account SHALL receive it for another account's orders. The orders page SHALL re-fetch its tables when the signal arrives, keeping the current character filter and table sorting, without a full page reload. The dashboard SHALL NOT re-fetch because of it.

#### Scenario: Standing changes after a market refresh
- **WHEN** a user has the orders page open with a sell order that is top, and a market refresh brings in another sell order at a lower price at the same station
- **THEN** their stream delivers a `market-updated` signal and the order's standing changes to "Undercut 1" without the user reloading

#### Scenario: Nothing changed
- **WHEN** a market refresh brings in orders that change no standing or gap of the user's orders
- **THEN** no `market-updated` signal is delivered to them

#### Scenario: Signals stay within the account
- **WHEN** a refresh changes the standing of an order of account A
- **THEN** a user signed in to account B receives no `market-updated` signal for it

#### Scenario: Filter and sorting are kept
- **WHEN** the orders page is filtered to one character and sorted by standing, and a `market-updated` signal arrives
- **THEN** the refreshed tables are still filtered to that character and sorted by standing

#### Scenario: Dashboard ignores it
- **WHEN** a `market-updated` signal arrives while the dashboard is open
- **THEN** the dashboard does not re-fetch

#### Scenario: Missed signal after a reconnect
- **WHEN** the stream was disconnected while a market refresh changed a standing, and the stream reconnects
- **THEN** the orders page shows the changed standing without the user reloading

### Requirement: Wealth signal
The system SHALL deliver a `wealth-updated` signal over the event stream of an account when a wallet poll or an assets poll of one of its characters succeeds, and when a price refresh succeeds for every account that has at least one character with current assets. The signal SHALL name only what changed and SHALL NOT carry balance, asset or price data. No account SHALL receive it for another account's characters. The wealth page and the wealth parts of the dashboard SHALL re-fetch when it arrives. The orders page SHALL NOT re-fetch because of it.

#### Scenario: Signal after a wallet poll
- **WHEN** a signed-in user has the wealth page open and a wallet poll of one of their characters succeeds
- **THEN** their stream delivers a `wealth-updated` signal

#### Scenario: Signals stay within the account
- **WHEN** an assets poll completes for a character of account A
- **THEN** a user signed in to account B receives no `wealth-updated` signal for it

#### Scenario: Signal carries no data
- **WHEN** a `wealth-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

#### Scenario: Orders page ignores it
- **WHEN** a `wealth-updated` signal arrives while the orders page is open
- **THEN** the orders page does not re-fetch
