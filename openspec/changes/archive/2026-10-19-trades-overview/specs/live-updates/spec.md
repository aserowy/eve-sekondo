# Spec Delta

## ADDED Requirements

### Requirement: Trades signal
The system SHALL deliver a `trades-updated` signal over the event stream of an account when a transactions poll of one of its characters stores at least one new purchase or sale. A poll that stores nothing new SHALL NOT deliver it. The signal SHALL name only what changed and SHALL NOT carry transaction data. No account SHALL receive it for another account's characters. The trades page SHALL re-fetch its table and its freshness when it arrives. The dashboard, the orders page and the wealth page SHALL NOT re-fetch because of it. A new purchase SHALL still deliver `orders-updated` as described by the purchase signal.

#### Scenario: New sale arrives
- **WHEN** a user has the trades page open and a transactions poll stores a new sale of one of their characters
- **THEN** their stream delivers a `trades-updated` signal and no `orders-updated` signal, and the sale is listed without the user reloading

#### Scenario: New purchase arrives
- **WHEN** a transactions poll stores a new purchase
- **THEN** the stream delivers both a `trades-updated` and an `orders-updated` signal

#### Scenario: Nothing new
- **WHEN** a transactions poll returns only transactions that are already stored
- **THEN** no `trades-updated` signal is delivered for it

#### Scenario: Signals stay within the account
- **WHEN** a transactions poll stores a sale of a character of account A
- **THEN** a user signed in to account B receives no `trades-updated` signal for it

#### Scenario: Trades table keeps its place
- **WHEN** a user has scrolled the trades table down, and a `trades-updated` signal arrives
- **THEN** the refreshed trades table is at the same scroll position
