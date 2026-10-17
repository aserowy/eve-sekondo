# Spec Delta

## ADDED Requirements

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
