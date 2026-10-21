# Tasks

## 1. Pricing and flagging maths

- [x] 1.1 Add `tick_above` next to `tick_below` in `analysis.rs` and generalise `percentile_25` into `percentile(sorted, q)`. Verify with unit tests in `tests/analysis.rs`: one tick above 1,234,500.00 is 1,235,000.00, above 820.00 is 820.10, and p25 / p75 interpolate linearly.
- [x] 1.2 Add the pure `analysis::flag_day(hours) -> DayFlags`. It computes covered (≥ 20 hours), the per-day medians of max spread, average best buy and average best sell, the expanded hours (max spread ≥ 2 × median), the direction (dip only when its relative move is strictly larger), the ratio (trough or peak), the extreme price in cents and the taken units of the moving side. Verify with unit tests: constant spread gives no expanded hour; one 18:00 sell jump gives a spike with its peak ratio; a buy drop gives a dip; a tie gives a spike; an even hour count uses the mean of the middle two; a 19-hour day is not covered.

## 2. Storage

- [x] 2.1 Write the migration, one step per transaction like `20261018000003`:
  - retention of `station_hourly` and `station_daily` to 365 days, and compression on both after 7 days
  - create `station_flagged_days`, `station_expanded_hours` and `station_item_windows`
  - drop the `expansion_kind`, `expansion_weekday`, `expansion_hour` and `expansion_sell` columns, and reset `expansion_at`

  Verify that the app migrates a fresh database and the dev database without error, and that `timescaledb_information.jobs` shows the 365-day retention and the compression policies.
- [x] 2.2 Add the store methods to `StationStore` and implement them in `store/pg/station.rs` (SQL flagging per design decision 1) and `store/memory/station.rs` (via `flag_day`):
  - `unflagged_days`
  - `flag_days`
  - `flagged_days`
  - `expanded_hours`
  - `medians_7d`
  - `replace_windows` / `windows`
  - `prune_flags`

  Remove `daily_points`. Verify with new `store-contract` cases for an even hour count, a tie, an uncovered day, pruning past 365 days and window round-trips: the plain test run passes on the memory store, and the store-contract verification command passes against a freshly migrated Postgres.

## 3. Flagging step

- [x] 3.1 Add `stats::flag_completed_days`. It flags every completed UTC day not yet in `station_flagged_days` once `latest_hour ≥ day + 1 day + 1 hour`, then prunes to 365 days. Call it before the recurrence in the hourly refresh path and in the rollup test hook. Verify with `tests/station_stats.rs`: a day is flagged once, a running day is not flagged, the first run back-fills all retained days, and a second run writes nothing.

## 4. Recurrence

- [x] 4.1 Rewrite `analysis::recurring_windows(flagged_days, expanded_hours, medians_7d, now)`:
  - the 10% quiet rule over 30 days
  - hour-of-day windows over 28 days (≥ 60%, ≥ 7)
  - weekday × hour windows over 52 weeks (≥ 60%, ≥ 3, 2 of the last 4)
  - separate directions
  - spike S from p25 of peaks × 7-day median sell; dip B from p75 of troughs × 7-day median buy
  - V_win, and occurrences with their extreme price for the chart

  Remove `WindowKind::Weekday`, `DayPoint` and `MIN_HOURLY_DAYS`. Verify with unit tests for every scenario of the "Recurring spread expansion table" requirement: daily evening expansion, weekly dip (B = 820.10), constant spread, jumpy all day, too irregular, died out, directions are separate, old peaks follow today's price, and first days after the release.
- [x] 4.2 Rewrite `stats::refresh_expansions_if_advanced` to read the flag tables and the 7-day medians and store every window with `replace_windows`. Verify with `tests/station_stats.rs`: an item with a spike and a dip window stores both, and an item that turns jumpy loses its windows.

## 5. Expansion table rows

- [x] 5.1 In `tables.rs`, build one candidate per stored window (spike: B = best buy, S = stored price; dip: B = stored price, S = one tick below best sell), keep the one with the highest m (ties: earliest next start), and run the shared figures and the hard exclusion on it. Verify with `tests/opportunities_tables.rs`: highest m wins, the dip row's c uses the predicted B, and a dip whose B is not below S is excluded by its margin.
- [x] 5.2 Add the Dir and V_win columns (sortable, `data-col="direction"` and `"window-volume"`), the legend entries Best buy, Dir and V_win, and the dip exception in B's full name and legend entry. Verify with `tests/opportunities_legend.rs` and `tests/opportunities_page.rs` (header order Item … Window, Dir, V_win, Next (UTC)). Add a Playwright test in `e2e/tests/opportunities.spec.ts` that seeds a weekly or daily dip and checks the row's Dir "Dip", its B and its S, and the hover title of B.
- [x] 5.3 Update the e2e test "an evening spread expansion that recurred on most days" for the new rule: Dir "Spike", and S from the peak ratios × 7-day median. Add a Playwright test where an item with a constant 10% spread and one where 15% of hours jump are not listed. Verify with `npx playwright test --workers=1 opportunities.spec.ts`.

## 6. Detail page

- [x] 6.1 Replace "Next occurrences" with the "Recurring windows" section. It lists every window with Dir, predicted price, occurrences/covered, V_win and its next three starts, and marks the row's window. It needs a sub-menu entry "Recurring windows". Add the Best buy card when B is not the current best buy. Verify with `tests/opportunities_detail.rs`.
- [x] 6.2 Change `charts::peaks` to draw the chosen window's occurrences in ISK from `station_expanded_hours`: max best sell with S marked for a spike, min best buy with B marked for a dip. The detail page no longer calls the recurrence live. Verify with `tests/opportunity_charts.rs`, and update the Playwright test "the expansion page draws the spread by hour and weekday…" to check the sub-menu entry "Recurring windows", both windows of a two-window item and the troughs chart of a dip.

## 7. Explainer and footer

- [x] 7.1 Remove the "Collecting hourly data" note and `own_days_needed`, and let `own_hourly_days` count every retained day. Verify with `tests/opportunities_page.rs`. Replace the Playwright test "with three days of snapshots…" with one that checks there is no note above the table and the footer reads "3 days of own snapshots".
- [x] 7.2 Update the expansion explainer:
  - the signal covers the 2 × daily-median expansion, the 10% quiet rule, dips and spikes, and the S and B predictions
  - the worked example shows the signal input in ratio form, the Dir "Spike" and unchanged numbers
  - the playbook has spike and dip steps
  - the common steps say "unless the table's own steps say otherwise"

  Verify with the legend/explainer tests in `tests/opportunities_legend.rs` and a Playwright test that opens the explainer on `/opportunities/expansion` and checks the dip playbook and S = 12,520.00.

## 8. Integration

- [x] 8.1 Run the full suite sequentially: `CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3 nice -n 19 cargo test`, the store-contract verification against Postgres, and `npx playwright test --workers=1`. Everything must be green.
- [x] 8.2 Run the app against the dev database for at least one hourly refresh. Verify that `station_flagged_days` holds the back-filled days, the expansion table no longer lists items with a constant spread, and `hypertable_size` of the rollups is noted for later comparison.
- [x] 8.3 Update the Purpose of `openspec/specs/station-market-sync/spec.md`: rollups are kept for a year, snapshots and history for 30 days. Verify with `openspec validate --specs --strict`.
