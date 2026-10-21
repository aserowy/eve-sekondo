# Proposal

## Why

The opportunity filters take more room than they need and are hard to read once they wrap: the inputs are as tall as Pico's default, and a wrapped row's label sits directly on the input above it. Items with very high margins are mostly outliers or traps, but there is no way to hide them. And the tables show 50 rows by default with no smaller option. Whatever page size is chosen is lost as soon as the user opens a detail page and goes back, or switches to another table.

## What Changes

- Every text and number field of a filter section gets a lower input, about 2rem high, with the same font size. Only the vertical padding shrinks.
- Wrapped rows of fields are 0.75rem apart, so a label no longer sits directly on the input above it. The 1rem gap between columns stays.
- New opportunity filter "Maximum margin (%)", default 80%, right after "Minimum margin (%)". It hides rows whose m is above it, is stored per account like the other filters, must be at least 0 and at least the minimum margin, and cannot be empty. A detail page whose item it hides says so.
- **BREAKING** (URL defaults): every table page's page sizes become 10, 25, 50, 100 and 200, and a missing or unknown `per_page` now selects 25 instead of 50. This applies to the orders, wealth, trades and opportunity pages.
- On the opportunity table pages, the chosen page size is stored per account, shared by all four tables and kept across sessions. A table page opened without `per_page`, for example through the sub-menu or the "← table" link of a detail page, uses the stored size. Choosing a size saves it, which needs the account's CSRF token.

## Capabilities

### New Capabilities

_None._

### Modified Capabilities

- `ui-layout`: the Filter box requirement gains the input height and the row gap of wrapped fields. The Pagination requirement changes the page sizes and the default, and lets a page use a stored page size when `per_page` is missing. The KPI scenario that relies on the default page size is updated.
- `station-opportunities`: the Opportunity filters requirement gains the maximum margin and a stored page size. The Table rows and columns requirement's scenario follows the new default page size. The detail page names the maximum margin as a reason an item is not listed.

## Impact

- `crates/sekondo/static/app.css`: filter input height and the row gap of `#opportunity-filters`.
- `crates/sekondo/src/pagination.rs`: page sizes and default, plus a way for a page to supply its own fallback size.
- `crates/sekondo/src/opportunities/settings.rs`, `tables.rs` and `page.rs`, plus `templates/opportunities_filters.html`: the maximum margin field, its validation, the row filter and the detail reason; reading and saving the stored page size; a new POST route for the page size.
- `crates/sekondo/static/app.js`: on opportunity pages the page-size select posts the new size instead of only fetching it.
- Store: a new migration adds `max_margin` and `per_page` to `opportunity_settings`; both the PG and memory stores change.
- Tests: unit tests in `pagination.rs` and `settings.rs`. The e2e tests that expect 50 rows by default (`layout`, `live`, `subnav`, `opportunities-layout`) need updating, and new Playwright tests cover the new behaviour.
