# Proposal

## Why

The groups in the filter section are aligned to their bottom edge. When one group gets taller, for example because the opportunity filters wrap onto a second row or an error message appears under an input, the character filter and the text beside it slide down with that bottom edge, so the filter section jumps around. On the opportunity pages, the right end of the filter section is also a long line of text. It names the station and whose fees the analyses use, but the trader cannot change those fees: the fees are always those of the selected character with the lowest minimum margin, even when the trader wants to see the margins of the character who will actually trade.

## What Changes

- Every group in the filter section starts at the top of its line. A group that grows taller (wrapped fields, an error message) no longer moves its neighbours. Labels and legends of one line share their top edge, so the inputs and the range buttons still line up when no error is shown. This replaces the bottom-edge rule.
- On the opportunity table pages and the opportunity detail page, the text "station · fees" at the right end of the filter section is removed and split in two:
  - The station name becomes a dimmed subtitle directly below the page title.
  - The fees become a new **Fees** group in the filter section: the legend "Fees" above one radio button per selected character. The analyses use the sales tax and broker fee of the checked character at the station. Under the radios, helper text shows the rates in use, or says that base rates are used.
- The fee character is stored per account with the other opportunity settings, kept across sessions, and used by every table and by the detail page. When nothing is stored, or the stored character is not selected, the selected character with the lowest minimum margin is checked (today's rule). With no character selected, there are no radios and base rates are used. A checked character whose fees are not known yet counts with base rates (b = 3%, t = 7.5%), and the helper text says so.
- Choosing a fee character saves it and recomputes the page without an apply button, like the other opportunity filters.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ui-layout`:
  - the page structure: a page header may hold a subtitle below the title (used by the opportunity pages);
  - the filter box: top alignment instead of bottom alignment, the Fees group as a filter group, and the opportunity pages' filter sections without descriptive text.
- `station-opportunities`:
  - the opportunities page and the opportunity detail page: station name as subtitle, Fees group in the filter section;
  - fees used for the analyses: the checked fee character instead of always the best one, with the best one as default;
  - opportunity filters: the fee character stored per account with the other settings.

## Impact

- `crates/sekondo/static/app.css`: `.filter-box` alignment, `#filter-context` styles removed, Fees group layout.
- `crates/sekondo/templates/base.html`: optional subtitle in the page header.
- `crates/sekondo/templates/opportunities*.html`, `opportunity_detail.html`: subtitle, Fees group instead of `opportunities_filter_context.html`, OOB updates of the Fees group.
- `crates/sekondo/src/opportunities/{tables,analysis,page,settings}.rs`: fee choice that respects a stored character, the view model for the Fees group, a save endpoint for the fee character.
- `crates/sekondo/src/store` (pg and memory) plus `crates/store-contract`: a nullable fee character on the opportunity settings, and a new migration.
- `crates/sekondo/src/app.rs`: route for saving the fee character.
- Tests: Rust tests that match `filter-context` or the fees note, and e2e `filter.spec.ts`, `opportunities.spec.ts`, `opportunities-layout.spec.ts`, `page-structure.spec.ts`.
- `README.md`: layout and opportunities sections.
