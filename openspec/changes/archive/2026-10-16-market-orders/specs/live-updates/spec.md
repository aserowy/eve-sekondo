# Spec Delta

## Purpose

Pushes change signals to a signed-in user's open pages so they refresh the affected parts on their own when new data arrives, without the user reloading.

## ADDED Requirements

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
