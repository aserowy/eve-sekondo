# Spec Delta

## ADDED Requirements

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
