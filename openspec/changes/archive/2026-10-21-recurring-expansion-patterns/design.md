# Design

## Context

- `analysis::recurring_windows` gets every hourly rollup of the last 30 days for every item, as `HourPoint`s with `spread_avg`, `spread_max` and `sell_max_cents`. It applies the old rules in Rust. That is about 11M rows each time the hourly rollup advances.
- `stats::refresh_expansions_if_advanced` stores the best window in the `expansion_*` columns of `station_item_stats`. The detail page (`page.rs`) recomputes the windows live from the same rollups to draw the peaks chart and the next occurrences.
- `station_hourly` already keeps `best_buy_min`, `best_buy_avg`, `best_sell_max`, `best_sell_avg`, `spread_max`, `buy_taken`, `sell_taken` and `snapshots` per item and hour. It and `station_daily` are continuous aggregates, kept for 30 days and not compressed (TimescaleDB 2.17).
- There are about 15,800 items, so roughly 380k hourly rows per day. A year is about 140M rows, which can no longer be loaded into memory once an hour.
- Every store method exists twice: in Postgres (`store/pg`) and in memory (`store/memory`). The `store-contract` crate checks the two against shared cases, and the plain test run uses the memory store.

## Goals / Non-Goals

**Goals:**
- Never re-read a completed day's rollups once it has been evaluated. The work per refresh is bounded by the number of expanded hours, not by the size of a year of rollups.
- Keep the maths (thresholds, percentiles, tick rounding, window rules) in pure Rust functions with unit tests. SQL only collects and stores.
- Choose the row's window with current fees and the current book at request time, as every other table does.

**Non-Goals:**
- Patterns other than hour of day and weekday × hour (no biweekly, monthly or sub-hour windows).
- Back-filling more than the 30 days of rollups that exist today. The year fills up from now on.
- Changing the other three tables.

## Decisions

### 1. Evaluate each completed day once, into two small tables

A new step, `flag_completed_days`, runs whenever the hourly rollup has advanced past the end of a UTC day that has not been flagged yet. For each such day it:
- computes per item the day's hour count and the medians of `spread_max`, `best_buy_avg` and `best_sell_avg` (Postgres: `percentile_cont(0.5)` grouped by item and day)
- writes one row per item and day into **`station_flagged_days`** `(region, station, type_id, day, hours, expanded_hours)`
- for covered days (hours ≥ 20), writes one row per expanded hour into **`station_expanded_hours`** `(region, station, type_id, start, direction, ratio, extreme_cents, taken)`. `ratio` is the trough or peak, `extreme_cents` is `best_buy_min` or `best_sell_max` for the chart, and `taken` is `buy_taken` or `sell_taken`.

A day's flags never change once the day has ended, so this is the "running state" from the discussion. On the first start it back-fills every completed day still in `station_hourly`, at most 30 days today. Both tables are pruned to 365 days in the same step.

*Alternatives:*
- **Recompute over the full year in SQL each hour:** about 140M rows aggregated hourly, too heavy.
- **Keep loading `HourPoint`s:** does not scale to a year.
- **A continuous aggregate for the flags:** ordered-set aggregates such as `percentile_cont` are not supported in continuous aggregates.

### 2. The memory store flags with the same pure function

`analysis::flag_day(hours: &[HourFigures]) -> DayFlags` holds the expansion and direction maths. The memory store calls it per item and day. The Postgres store does the same in SQL with the same definitions:
- median by `percentile_cont(0.5)`
- `>= 2 *` for the expansion test
- strict `>` comparison of the two relative moves for a dip

A contract case with a hand-built day pins both stores to identical flags and ratios. This includes a tie between the two moves (a spike) and an uncovered day (no expanded rows).

### 3. Recurrence in Rust from the flagged tables

`analysis::recurring_windows(flagged_days, expanded_hours, medians_7d, now) -> Vec<Window>` replaces the old function. Per item it:
- applies the 10% quiet rule from the `station_flagged_days` rows of the last 30 days
- builds the hour-of-day windows (28 days) and the weekday × hour windows (52 weeks, with 2 of the last 4 weeks), counting covered days as the denominators
- splits windows by direction
- computes the predicted price from the ratios and the 7-day medians

`WindowKind::Weekday`, `DayPoint`, `daily_points` and `MIN_HOURLY_DAYS` are removed.

The 7-day medians come from one query on `station_hourly` over the last 7 days, about 2.6M rows aggregated in SQL into one row per item. Recurrence still runs when the hourly rollup advances (the existing `expansions_stale` / `expansion_at` marker), but it now reads only the two flag tables and that query.

### 4. Store every window, choose the row's window at request time

A new table **`station_item_windows`** `(region, station, type_id, kind, weekday, hour, direction, price_cents, occurrences, covered, v_win)` replaces the columns `expansion_kind`, `expansion_weekday`, `expansion_hour` and `expansion_sell`. `expansion_at` stays as the staleness marker. `price_cents` is S for a spike and B for a dip.

`tables.rs` builds one candidate per window. A spike uses B = current best buy and S = `price_cents`. A dip uses B = `price_cents` and S = one tick below the current best sell. The row keeps the candidate with the highest m, ties broken by the next start. Because m depends on fees and the current book, this choice cannot be stored ahead of time. The detail page reads the same rows for its "Recurring windows" section, and reads `station_expanded_hours` for the chosen window's occurrences in ISK. It no longer recomputes the windows live.

*Alternative:* store only the best window, as now. Rejected because the best window depends on the account's fees, and the detail page needs every window.

### 5. Retention and compression

A migration changes the retention of `station_hourly` and `station_daily` to `365 days` and adds compression on both continuous aggregates, after 7 days. The refresh `start_offset` of 2 and 4 days stays inside the uncompressed range. Snapshots and history keep 30 days.

The policy changes follow the same one-step-per-transaction pattern as `20261018000003`, to avoid the job-catalog deadlock. The new tables are plain tables, pruned by the flagging step.

### 6. Pricing helpers

`tick_above` is added next to `tick_below` (lowest valid price above x), using the same four-significant-digit rule. `percentile_75` joins `percentile_25` as one `percentile(sorted, q)` helper.

### 7. Footer and note

The note in `templates/opportunities_content.html` and the `own_days_needed` field are removed. `own_hourly_days` keeps counting UTC days with hourly rollups, now over everything retained, so the footer's "n days of own snapshots" can grow past 30.

### 8. New columns, legend and explainer

- `SortKey` gains `Direction` and `WindowVolume`, and `Term` gains `Dir`, `V_win` and `Best buy`.
- The legend entry of B gains the dip exception.
- The expansion explainer's signal input becomes the ratio form. The worked example numbers stay, because the 25th percentile 1.2525 × 10,000.00 = 12,525, so S is still 12,520.00.

## Risks / Trade-offs

- **[Risk] TimescaleDB compression on continuous aggregates conflicts with refreshes or the retention change.** → Compress after 7 days, well beyond the 2- and 4-day refresh windows. Verify with the store-contract verification run against a freshly migrated Postgres, and test once against a copy of the dev database.
- **[Risk] The SQL and Rust flagging drift apart** (median of an even count, ties, rounding of cents). → The contract case from decision 2 covers an even hour count, a tie and an uncovered day.
- **[Risk] A day flagged before its rollup is complete** (late snapshots, cagg lag). → Only flag a day once `latest_hour` is at least `day + 1 day + 1 hour`. Remember the flagged days in `station_flagged_days`, so a day is never flagged twice.
- **[Trade-off] Weekly windows need weeks of data**, and hour-of-day windows need 7 covered days. Right after this change, with 30 days of rollups, weekly windows can recur only with 3 of 4 weeks. The table is honest about this by showing fewer rows, not a note.
- **[Trade-off] The 10% quiet rule may hide items with two strong daily windows plus noise.** → The share is a single constant and can be tuned later.
- **[Risk] Storage grows about 12×** for the rollups. → Compression typically brings it to around a tenth. Watch `hypertable_size` after the first weeks.

## Migration Plan

1. Migration: retention to 365 days and compression on `station_hourly` and `station_daily`. Create `station_flagged_days`, `station_expanded_hours` and `station_item_windows`. Drop the `expansion_kind`, `expansion_weekday`, `expansion_hour` and `expansion_sell` columns, and reset `expansion_at` to NULL so the next refresh recomputes.
2. On the first start the flagging step back-fills every completed day still in `station_hourly`.
3. Rollback: revert the commit and add a migration that restores the columns. The longer-retained rollups are harmless to the old code.

## Open Questions

- Whether the 7-day medians should exclude expanded hours. They are medians, so a few expanded hours barely move them. This can be decided while testing without changing the specs.
