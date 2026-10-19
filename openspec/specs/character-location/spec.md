# character-location Specification

## Purpose

Knows where each linked character is currently docked, so that fees can be shown for the station or structure the character actually trades from, with Jita 4-4 as the fallback when the location is unknown.

## Requirements

### Requirement: Location scope is optional
The location scope `esi-location.read_location.v1` SHALL be requested at every login but SHALL NOT be required. A character whose granted scopes lack it SHALL NOT be marked "needs re-login" for that reason, and all its other polls SHALL continue. Logging in again with the scope granted SHALL enable the location poll for the character.

#### Scenario: Character linked before the location scope
- **WHEN** a character linked with only the six required scopes has its token refreshed
- **THEN** it is not marked "needs re-login", its order, wealth, fee and transactions polls continue, and its location is not polled

#### Scenario: Scope granted by logging in again
- **WHEN** the user logs in again with that character and grants the location scope
- **THEN** its location is polled from then on

### Requirement: Poll the location
The system SHALL poll the current location of every linked character that has granted the location scope and does not need re-login, at most once per hour and never before the cache expiry time ESI reported for its previous answer, on the same schedule as the skills and standings polls. A failed poll SHALL keep the last known location and SHALL be retried with the same backoff and ESI etiquette as the order poll. It SHALL NOT affect the order, wealth, fee or transactions polls of that character.

#### Scenario: Location after a poll
- **WHEN** a character's location poll reports it docked at Amarr VIII (Oris) - Emperor Family Academy
- **THEN** that station is the character's docked location

#### Scenario: Polls wait for the hour
- **WHEN** a character's location was polled successfully 20 minutes ago
- **THEN** it is not polled again until an hour has passed since that poll

#### Scenario: Character needing re-login is skipped
- **WHEN** a character with the location scope is marked "needs re-login"
- **THEN** its location is not polled and its last known location is kept

#### Scenario: Failed poll
- **WHEN** a location poll fails because of an ESI server error
- **THEN** the last known location is kept and the poll is retried later

### Requirement: Docked location
A character's docked location SHALL be the station or player-owned structure the latest successful location poll reported it in. When a poll reports the character in space, without a station or structure, the docked location SHALL stay the last one reported. A character that was never reported docked SHALL have no docked location. The docked location SHALL be named the same way as order locations, including the fallback for a structure the character's token cannot look up.

#### Scenario: Undocked
- **WHEN** a character was last reported docked in Jita 4-4 and its next poll reports it in space
- **THEN** its docked location is still Jita 4-4

#### Scenario: Docked in a structure
- **WHEN** a poll reports a character in a player-owned structure
- **THEN** that structure is its docked location

### Requirement: Fee location
The fee location of a character SHALL be its docked location when it has one, and otherwise Jita IV - Moon 4 - Caldari Navy Assembly Plant. A character that has not granted the location scope SHALL always use Jita IV - Moon 4 - Caldari Navy Assembly Plant.

#### Scenario: Never docked
- **WHEN** a character with the location scope has only been reported in space
- **THEN** its fee location is Jita IV - Moon 4 - Caldari Navy Assembly Plant

#### Scenario: Without the scope
- **WHEN** a character has not granted the location scope
- **THEN** its fee location is Jita IV - Moon 4 - Caldari Navy Assembly Plant

### Requirement: Location is removed with the character
A character's docked location and location sync state SHALL be deleted when the character is unlinked or changes hands.

#### Scenario: Unlink
- **WHEN** a character is unlinked
- **THEN** no location or location sync row of it remains
