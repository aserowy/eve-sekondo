# Spec Delta

## ADDED Requirements

### Requirement: Undercut volume
For an order that is undercut, the system SHALL determine the undercut volume. This is the sum of the remaining volumes, as last refreshed, of exactly the orders counted as undercutting it. For a buy order, minimum volumes SHALL NOT matter. A top order SHALL have no undercut volume. An order without a standing SHALL have no undercut volume. The copy of the public market orders SHALL keep each order's remaining volume for this.

#### Scenario: Sell order behind two orders
- **WHEN** a sell order of 100 ISK is undercut by orders at 99 ISK with 5 units remaining and at 95 ISK with 20 units remaining at the same station, and an order at 90 ISK with 50 units sits at another station
- **THEN** its undercut volume is 25

#### Scenario: Buy order behind reaching orders
- **WHEN** a buy order of 50 ISK is undercut by a region-range order at 55 ISK with 1,000 units remaining and a station-range order at its station at 52 ISK with 300 units remaining, and a 5-jump order at 60 ISK in another system has 700 units
- **THEN** its undercut volume is 1,300

#### Scenario: Top order
- **WHEN** an order is top
- **THEN** it has no undercut volume

#### Scenario: Volumes follow the refresh
- **WHEN** an undercutting order's remaining volume drops from 20 to 8 and the pair is refreshed
- **THEN** the undercut volume uses 8
