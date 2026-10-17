# Spec Delta

## ADDED Requirements

### Requirement: Fees signal
The system SHALL deliver a `fees-updated` signal over the event stream of an account when a skills or standings poll of one of its characters, or the lookup of a station owner where one of its characters has orders, changes a sales tax, broker fee or minimum margin of that character. The signal SHALL name only what changed and SHALL NOT carry skill, standing or fee data. No account SHALL receive it for another account's characters. The orders page SHALL re-fetch its character entries when it arrives, keeping the current selection and sorting. The orders tables, the dashboard and the wealth page SHALL NOT re-fetch because of it.

#### Scenario: Skill trained
- **WHEN** a user has the orders page open and a skills poll of one of their characters reports a higher Accounting level
- **THEN** their stream delivers a `fees-updated` signal and the character's sales tax shows the new rate without the user reloading

#### Scenario: Nothing changed
- **WHEN** a skills poll reports the same levels as before
- **THEN** no `fees-updated` signal is delivered

#### Scenario: Signals stay within the account
- **WHEN** a standings poll changes a fee of a character of account A
- **THEN** a user signed in to account B receives no `fees-updated` signal for it

#### Scenario: Signal carries no data
- **WHEN** a `fees-updated` signal is delivered
- **THEN** it contains nothing beyond the signal name
