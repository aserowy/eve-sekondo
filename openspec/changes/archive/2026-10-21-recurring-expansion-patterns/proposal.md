# Proposal

## Why

The recurring spread expansion table is meant to find items whose spread is normally steady but blows up by more than 100% at a predictable time. Those moments let a trader buy far below the normal price, when heavy selling empties the buy side, or sell far above it, when heavy buying empties the sell side. Today the table lists items that are simply jumpy all day. It compares an hour's *maximum* spread with the item's *median average* spread at only 1.5×, never checks that the rest of the day is quiet, and only knows the sell side. It also sees just 30 days, so weekly patterns rest on 3 of 4 weeks.

## What Changes

- **Expansion:** an hour counts as expanded when its maximum spread is at least **2×** the median hourly maximum spread of its own UTC day. Each expanded hour is a **dip** (the best buy fell further below the day's median best buy) or a **spike** (the best sell rose further above the day's median best sell).
- **Quiet items only:** an item is excluded when more than 10% of its hours in the last 30 days expanded.
- **Patterns:** a daily hour recurs over the last 28 days, on at least 60% of the days with data and at least 7 of them. A weekday × hour recurs over the last 52 weeks, on at least 60% of the weeks with data and at least 3 of them, and in at least 2 of its last 4 weeks. Dip and spike windows are separate.
- **BREAKING (behavior):** the weekday-from-history fallback and the 7-day gate are removed. A pattern counts as soon as it has enough occurrences. The "Collecting hourly data" note above the table goes away; the footer's "n days of own snapshots" stays.
- **Predicted prices:** peaks and troughs are kept relative to their day's median best sell or buy and applied to the median of the last 7 days.
  - **Spike:** B = current best buy; S = one tick below (p25 of peak ratios × 7-day median best sell).
  - **Dip:** B = one tick above (p75 of trough ratios × 7-day median best buy); S = one tick below the current best sell.

  This is the first table whose rows suggest a buy price.
- **Row:** one row per item, for the window with the highest m. New **Dir** (Dip / Spike) and **V_win** (units taken from the moving side per occurrence) columns.
- **Detail page:** lists all of the item's recurring windows with their next occurrences. The past peaks/troughs chart marks S or B.
- **Explainer:** the signal and playbook cover both directions. The worked example stays the spike example.
- **Retention:** hourly and daily rollups are kept for **1 year** and compressed. Raw snapshots and history stay at 30 days.
- **Computation:** each hour's expansion flag, direction and ratios are computed in SQL, and the analysis reads only expanded hours.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `station-opportunities`: the recurring spread expansion rules, its columns, its buy price, the shared figures' B, the legend entries, the detail page's window section and charts, the explainer, and the removal of the collecting note.
- `station-market-sync`: the rollups are kept for one year, compressed; snapshots and history keep 30 days.

## Impact

- `crates/sekondo/src/opportunities/analysis.rs`: new recurrence logic and dip/spike pricing. The weekday fallback and `MIN_HOURLY_DAYS` gate are removed.
- `crates/sekondo/src/opportunities/stats.rs`: reads flagged hours and stores every window per item.
- `tables.rs`, `page.rs`, `charts.rs`, `legend.rs`, and the templates `opportunities_content.html` and explainer partials.
- `store/mod.rs`, `store/pg/station.rs`, `store/memory/station.rs`: new queries and window storage.
- New migration: rollup retention and compression, plus a table of recurring windows per item replacing the `expansion_*` columns of `station_item_stats`.
- Tests: `tests/analysis.rs`, `station_stats.rs`, `opportunities_*.rs`, and the e2e `opportunities*.spec.ts`.
- Storage: about 380k hourly rollup rows per day across about 15,800 items, roughly 140M rows per year, kept compressed.
