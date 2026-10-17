# Spec Delta

## MODIFIED Requirements

### Requirement: Market signal
The system SHALL deliver a `market-updated` signal over the event stream of an account when a market refresh changes the standing, the gap or the undercut volume of at least one current order of that account. The signal SHALL name only what changed and SHALL NOT carry order or price data. Accounts none of whose orders changed standing, gap or undercut volume SHALL NOT receive it, and no account SHALL receive it for another account's orders. The orders page SHALL re-fetch its tables when the signal arrives, keeping the current character filter and table sorting, without a full page reload. The dashboard SHALL NOT re-fetch because of it.

#### Scenario: Standing changes after a market refresh
- **WHEN** a user has the orders page open with a sell order that is top, and a market refresh brings in another sell order at a lower price at the same station
- **THEN** their stream delivers a `market-updated` signal and the order's standing changes to "Undercut 1" without the user reloading

#### Scenario: Only the undercut volume changes
- **WHEN** a user's sell order is undercut by one order, and a market refresh shows that order with fewer units remaining at the same price
- **THEN** their stream delivers a `market-updated` signal and the undercut volume shows the new number without the user reloading

#### Scenario: Nothing changed
- **WHEN** a market refresh brings in orders that change no standing, gap or undercut volume of the user's orders
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

### Requirement: Fees signal
The system SHALL deliver a `fees-updated` signal over the event stream of an account when a skills or standings poll of one of its characters, or the lookup of a station owner where one of its characters has orders, changes a sales tax, broker fee or minimum margin of that character. The signal SHALL name only what changed and SHALL NOT carry skill, standing or fee data. No account SHALL receive it for another account's characters. The orders page SHALL re-fetch its character entries and its tables when it arrives, because the margins depend on the fees, keeping the current selection and sorting. The dashboard and the wealth page SHALL NOT re-fetch because of it.

#### Scenario: Skill trained
- **WHEN** a user has the orders page open and a skills poll of one of their characters reports a higher Accounting level
- **THEN** their stream delivers a `fees-updated` signal and the character's sales tax shows the new rate without the user reloading

#### Scenario: Margins follow the fees
- **WHEN** a user has the orders page open with a sell order that has a margin, and a skills poll of its character reports a higher Accounting level
- **THEN** the order's margin shows the value with the lower sales tax without the user reloading

#### Scenario: Nothing changed
- **WHEN** a skills poll reports the same levels as before
- **THEN** no `fees-updated` signal is delivered

#### Scenario: Signals stay within the account
- **WHEN** a standings poll changes a fee of a character of account A
- **THEN** a user signed in to account B receives no `fees-updated` signal for it

#### Scenario: Signal carries no data
- **WHEN** a `fees-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name

## ADDED Requirements

### Requirement: Purchase signal
The system SHALL deliver an `orders-updated` signal over the event stream of an account when a transactions poll of one of its characters stores at least one new purchase lot. A poll that stores no new purchase SHALL NOT deliver it. The orders page SHALL then show the margins with the new purchases.

#### Scenario: New purchase arrives
- **WHEN** a user has the orders page open with a sell order whose margin reads "—" because its cost is not fully known, and a transactions poll stores the missing purchase
- **THEN** their stream delivers an `orders-updated` signal and the order's margin is shown without the user reloading

#### Scenario: Nothing new
- **WHEN** a transactions poll returns only transactions that are already stored
- **THEN** no signal is delivered for it
