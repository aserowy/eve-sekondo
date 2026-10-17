# Spec Delta

## Purpose

Keeps a current copy of every linked character's wallet balance and assets, values the assets with public market prices, and records the history needed to show a trader's wealth over time.

## ADDED Requirements

### Requirement: Poll wallet balance
The system SHALL periodically fetch the wallet balance of every linked character that is not marked "needs re-login", using that character's ESI access token. It SHALL poll each character's wallet at most once per configured order refresh interval, and SHALL NOT poll it before the cache expiry time ESI reported for the previous wallet answer. Wallet polls of different characters SHALL be spread across the interval. A failed wallet poll SHALL keep the last known balance and SHALL NOT affect the order poll, and vice versa. Every wallet request SHALL follow the same ESI etiquette and backoff as the order poll.

#### Scenario: Balance after a poll
- **WHEN** a linked character's wallet poll comes due and ESI reports a balance of 123,456,789.12 ISK
- **THEN** the character's current wallet balance is 123,456,789.12 ISK

#### Scenario: Character needing re-login is skipped
- **WHEN** a character's wallet poll comes due while it is marked "needs re-login"
- **THEN** no wallet request is made for it and its last known balance is kept

#### Scenario: Failed wallet poll
- **WHEN** a wallet poll fails because of an ESI server error
- **THEN** the last known balance is kept, the failure is recorded, and the wallet is polled again later

#### Scenario: Wallet failure does not fail the order poll
- **WHEN** a character's wallet poll fails while its order poll succeeds
- **THEN** its current orders are updated as usual

### Requirement: Poll assets
The system SHALL periodically fetch all assets of every linked character that is not marked "needs re-login", reading every page of the answer. It SHALL poll each character's assets at most once per hour and SHALL NOT poll them before the cache expiry time ESI reported for the previous assets answer. Asset polls of different characters SHALL be spread across the hour. After a successful poll the character's current assets SHALL be exactly the assets ESI returned. A failed poll SHALL keep the last known assets. Every assets request SHALL follow the same ESI etiquette and backoff as the order poll.

#### Scenario: Assets after a poll
- **WHEN** a character's assets poll returns two pages with 1,500 assets in total
- **THEN** the character's current assets are those 1,500 assets

#### Scenario: An item is gone
- **WHEN** a character had assets A and B, and the next successful poll returns only B
- **THEN** only B is a current asset of the character

#### Scenario: Assets wait for the hour
- **WHEN** a character's assets were polled successfully 30 minutes ago
- **THEN** they are not polled again before the hour has passed

#### Scenario: Failed assets poll
- **WHEN** an assets poll fails on its second page because of an ESI server error
- **THEN** the character's current assets are left unchanged and the poll is retried later

### Requirement: Asset location
The system SHALL determine, for every asset, the top-level location it is in: for an asset inside a container or ship, the location of the outermost item that holds it. The top-level location SHALL be a station, a structure or a solar system. A structure SHALL be named as for orders, with the access token of the character that owns the asset. When the location cannot be determined or named, the asset SHALL be shown under "Unknown location (<id>)". Names SHALL be resolved and cached as for orders.

#### Scenario: Item in a container
- **WHEN** an item is in a container that is in a station hangar
- **THEN** the item's location is that station

#### Scenario: Module fitted to a ship in space
- **WHEN** a module is fitted to a ship located in a solar system
- **THEN** the module's location is that solar system

#### Scenario: Inaccessible structure
- **WHEN** an item is in a structure that ESI refuses to describe for the owning character
- **THEN** its location is shown as "Unknown structure (<structure id>)"

### Requirement: Market prices
The system SHALL keep a copy of ESI's market average price for every item, refreshed once a day and not before the cache expiry time ESI reported. The copy SHALL be public data and SHALL NOT depend on any account or token. A failed refresh SHALL keep the previous copy and SHALL be retried with the same backoff and ESI etiquette as the order poll.

#### Scenario: Prices are fetched once for everyone
- **WHEN** two accounts hold assets
- **THEN** the market prices are requested from ESI once per refresh, not once per account

#### Scenario: Failed price refresh
- **WHEN** a price refresh fails because of an ESI server error
- **THEN** the previous prices are kept and the refresh is retried later

### Requirement: Inventory value
The value of an asset SHALL be its quantity multiplied by the current average price of its item. An asset SHALL be unpriced, with no value and counting as zero, when its item has no average price or when it is a blueprint copy. A character's inventory value SHALL be the sum of the values of its current assets.

#### Scenario: Priced asset
- **WHEN** a character holds 250 units of an item with an average price of 4,000.00 ISK
- **THEN** that asset is worth 1,000,000.00 ISK

#### Scenario: Blueprint copy
- **WHEN** a character holds a blueprint copy of an item whose original has an average price
- **THEN** the copy is unpriced and adds nothing to the inventory value

#### Scenario: No price known
- **WHEN** a character holds an item for which ESI reports no average price
- **THEN** the asset is unpriced and adds nothing to the inventory value

### Requirement: Wealth of a character
A character's wealth SHALL be the sum of its last known wallet balance, the value of its personal sell orders (price multiplied by remaining volume), the escrow of its personal buy orders (a missing escrow counting as zero), and its inventory value. Orders placed on behalf of the corporation SHALL NOT count. A component that has never been fetched successfully SHALL count as zero.

#### Scenario: All components
- **WHEN** a character has 100,000,000.00 ISK in its wallet, a personal sell order worth 30,000,000.00 ISK, a personal buy order with 20,000,000.00 ISK escrow and inventory worth 50,000,000.00 ISK
- **THEN** its wealth is 200,000,000.00 ISK

#### Scenario: Corporation orders are excluded
- **WHEN** a character additionally has a corporation sell order worth 40,000,000.00 ISK
- **THEN** its wealth does not change

#### Scenario: Assets not fetched yet
- **WHEN** a character's wallet has been polled but its assets have not
- **THEN** its wealth is its wallet balance plus its personal orders, with an inventory value of zero

### Requirement: Wealth history
The system SHALL record one wallet history entry per successful wallet poll, holding the poll time and the balance. It SHALL record one inventory history entry per character after every successful assets poll and after every successful price refresh, holding the time and the character's inventory value at that moment. Sell value and escrow history SHALL come from the existing poll totals history, counting personal orders only. A failed poll or refresh SHALL record no entry. Entries SHALL be kept without a retention limit and SHALL be deleted when the character is unlinked or changes hands. History SHALL start with this change, with no backfill.

#### Scenario: Wallet entry
- **WHEN** a wallet poll succeeds with a balance of 5,000,000.00 ISK
- **THEN** a wallet history entry with that poll time and 5,000,000.00 ISK exists

#### Scenario: Price change revalues the inventory
- **WHEN** a character's assets are unchanged and a price refresh doubles the average price of its only item
- **THEN** a new inventory history entry with the doubled value exists at the refresh time

#### Scenario: Entries are removed with the character
- **WHEN** a character with wallet, asset and inventory history is unlinked
- **THEN** no wallet balance, asset or history entry of that character remains

### Requirement: Wealth sync status
The system SHALL record, per character and separately for wallet and assets, the time of the last successful poll, the time of the next scheduled poll, and whether the last attempt failed. This status SHALL be available to the views.

#### Scenario: Status after a successful assets poll
- **WHEN** a character's assets are polled successfully
- **THEN** its assets' last successful poll time is the poll time and their next scheduled poll lies at least an hour later
