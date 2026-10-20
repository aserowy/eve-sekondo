# Spec Delta

## MODIFIED Requirements

### Requirement: Legend
Every short label used by the opportunity tables and the detail page SHALL have one legend entry: the short label, its full name, its meaning and, for a computed figure, its formula. The same label SHALL have the same entry, and the same full name on hover, wherever it is shown. The entries SHALL cover Item (including the "own buy" and "own sell" marks), b, t, B, Best sell, S, c, r, Net, m, U, V7, V30, ρ, q_buy, q_sell, σ, T_fill, T_max, V_min, ISK/d, n b/s, Window, Next (UTC), %B, RSI, SMA5, SMA20 and Cross. The formulas SHALL be those of the shared figures and of the table rules, for example c = B × (1 + b), m = (r − c) / c, T_max = min(3, (m / 2σ)²) days, and V_min as the V at which T_fill equals T_max. The entry of B SHALL say that it is the current best buy and not a suggested price.

The legend of a table lists the table's columns in their order. The legend of a detail page lists its figures in the order of the figures section.

A legend SHALL be shown as a table with the header row Label, Name, Meaning and Formula, and one row per entry. The label SHALL be the row's header. The name SHALL be plain text. The Formula cell SHALL hold only the right-hand side of the formula, in code style, and SHALL be empty for an entry without a formula. A legend table SHALL look like the worked example's table and SHALL NOT be the page's data table: it is neither sortable nor a link to a detail page. In a narrow window the meaning SHALL wrap within its cell, the label and the formula SHALL stay on one line, and the page SHALL NOT scroll sideways.

#### Scenario: Same wording everywhere
- **WHEN** a user reads the legend entry of T_max on the trend page and on a trend detail page
- **THEN** both show the same full name, meaning and formula min(3, (m / 2σ)²) days

#### Scenario: Table legend follows the columns
- **WHEN** a user opens the legend on the mean reversion page
- **THEN** it lists Item, B, S, Net, m, ISK/d, V7, V30, σ, T_max, T_fill, V_min, n b/s, %B and RSI, and no entry for Window or SMA5

#### Scenario: Legend as a table
- **WHEN** a user opens the Legend card on a classic detail page
- **THEN** it shows a table headed Label, Name, Meaning and Formula, whose row c reads c, Cost per unit, its meaning and B × (1 + b), and whose row B has an empty Formula cell

#### Scenario: Legend in a narrow window
- **WHEN** a user opens the explainer on the trend page in a window 600 pixels wide
- **THEN** the meanings wrap within their cells, the labels and formulas stay on one line each, and the page does not scroll sideways
