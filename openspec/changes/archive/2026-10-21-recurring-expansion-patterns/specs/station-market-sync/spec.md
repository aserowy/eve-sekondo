# Spec Delta

## RENAMED Requirements

- FROM: `### Requirement: Thirty-day window`
- TO: `### Requirement: Retention window`

## MODIFIED Requirements

### Requirement: Retention window
The system SHALL keep station snapshots and history for 30 days, and the hourly and daily rollups for 365 days, and SHALL delete anything older. Snapshots older than one day and rollups older than seven days SHALL be stored compressed without losing any value.

#### Scenario: Old data is dropped
- **WHEN** a snapshot is older than 30 days
- **THEN** it is no longer stored, and the analyses do not read it

#### Scenario: Rollups outlive the snapshots
- **WHEN** an hourly rollup is 200 days old
- **THEN** it is still stored and the recurring spread expansion analysis reads it, although the snapshots it was built from are gone

#### Scenario: Old rollups are dropped
- **WHEN** an hourly or daily rollup is older than 365 days
- **THEN** it is no longer stored

#### Scenario: Compression loses nothing
- **WHEN** a snapshot from three days ago or an hourly rollup from three weeks ago is read
- **THEN** every value it was recorded with is returned unchanged
