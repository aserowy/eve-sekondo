# Design

## Context

- `.filter-box` in `crates/sekondo/static/app.css` is a wrapping flex row with `align-items: flex-end`. The range control's label spacing (`.range-control > span`) was tuned so that its buttons share the bottom edge of the inputs beside them.
- On the opportunity pages, the right end of the filter section is `opportunities_filter_context.html` (`#filter-context`, "station · fees note"). It is rendered in the full page and pushed out-of-band by `opportunities_content_fragment.html` and `opportunities_detail_fragment.html`, so it follows changes to the character filter, the settings and the live refreshes.
- The fees come from `tables::choose_station_fees`, which reads the selected characters' fees at the station and hands them to `analysis::choose_fees` (lowest minimum margin wins, unknown fees count as base rates). The result, `FeeChoice`, lives in the page's `Environment` and feeds both the table pages and the detail page.
- Opportunity settings (`Settings`: min margin, min ISK/day, search, capital) are stored per account in `opportunity_settings` behind `OpportunityStore::{load_settings, save_settings}`, with pg and memory implementations checked by `crates/store-contract`. They are saved through `POST /opportunities/settings`, which validates the whole form and answers with the content fragment.
- The page header in `base.html` is `<header class="page-header"><h1>{% block page_title %}</h1></header>`.

## Goals / Non-Goals

**Goals:**
- Top alignment that holds for every filter section, without per-group tuning.
- One fee-choice rule, shared by the table pages and the detail page, that respects a stored choice and falls back to today's rule.

**Non-Goals:**
- Fees other than at the analysed station, or using several characters' fees at once.
- Changing how the margin, ISK/day or any other figure is computed from b and t.
- Changing the fees used on the orders and trades pages.

## Decisions

### Top alignment with `align-items: flex-start`

Set `.filter-box { align-items: flex-start; }` and leave the inner layout of each group alone. Legends, labels and the range label all use Pico's label spacing, so their tops line up and the first row of controls follows. The range label's `margin-bottom` stays as it is, so its buttons still start level with the inputs below the field labels.

*Alternative*: CSS grid with named rows for label, control and helper. This gives exact alignment, but it doesn't fit groups with different inner shapes (a wrapping checkbox fieldset next to a multi-row field grid), and it fights the wrapping requirement.

### Subtitle as Pico's `<hgroup>`

`base.html` gets an optional `{% block page_subtitle %}`. When a page fills it, the header renders `<hgroup><h1>…</h1><p>…</p></hgroup>`. Pico already dims the last child of an `hgroup` and keeps it at body size, so the stylesheet only needs the header's existing heading margin moved onto the `hgroup`. Pages without a subtitle keep the bare `<h1>`, so their DOM is unchanged.

*Alternative*: a `<p class="subtitle">` styled in `app.css`. That is more CSS for something Pico already does.

### Fee character stored as a nullable column of `opportunity_settings`

A new migration adds `fee_character_id BIGINT NULL` to `opportunity_settings`. The store trait gets `load_fee_character(account_id) -> Option<i64>` and `save_fee_character(account_id, Option<i64>)`, and the existing `save_settings` upsert never touches the column. This keeps the filter form and the fee choice independent: saving the filters can't clear the fee character and the other way round. An account without a settings row gets one with default settings when it first saves a fee character. The column has no foreign key, because unlinking a character must not cascade into settings. A stale id is harmless: it is never in the selection, so the fallback applies.

*Alternative*: a field on `Settings`, saved by the filter form. Then both forms would have to carry each other's values, and a rejected filter value would also block saving the fee character.

### Fee choice rule

`analysis::choose_fees` gains a `preferred: Option<i64>` argument. If the preferred character is among the given characters, it wins, with base rates when its fees are unknown. Otherwise the current lowest-minimum-margin rule applies unchanged. `choose_station_fees` loads the preferred id and passes it on. `FeeChoice` keeps `source` and `base_rates`, and the view builds a `FeeGroupView { radios: Vec<{id, name, checked}>, note }` from it. The note is a tidied-up version of today's `fees_note` ("Broker fee 1.41%, sales tax 3.375%" / "Base rates: broker fee 3%, sales tax 7.5%, until the character's skills and standings are known").

### Saving through `POST /opportunities/fee-character`

The Fees group is its own `<form id="fee-filter">` with `hx-post="/opportunities/fee-character"` and `hx-trigger="change"`. It carries the same hidden view state as the character filter (chars, sorts, per_page, and either `table` or `table` + `type_id` on the detail page). The handler:
1. checks the CSRF token like the settings endpoint;
2. checks that the id is one of the account's linked characters, and answers 422 without saving otherwise;
3. saves it, then renders the content fragment (table page, page 1, `HX-Replace-Url` like `save_settings`) or the detail fragment (when `type_id` is present).

Both fragments push the Fees group out-of-band in place of today's `#filter-context`, so the checked radio and the note always match the computed figures, also after a character-filter change or a live refresh.

*Alternatives*: putting the radios inside `#opportunity-filters` (but the detail page has no such form, and the field grid would swallow the group); answering 204 plus a client event that makes the regions refetch (an extra round trip, and a second code path for the detail page).

### Removing the descriptive-text slot

`opportunities_filter_context.html` and the `#filter-context` CSS are deleted. The station name moves into the subtitle block of `opportunities.html` and `opportunity_detail.html`. It is a constant, so it needs no out-of-band update.

## Risks / Trade-offs

- [Legends and labels end up with different top offsets because of Pico margins] → The Playwright scenario "Groups aligned at the top" measures this. If needed, reset the `legend` margin inside `.filter-box` only.
- [The Fees group adds width, so the trend page wraps earlier at 1280px] → This is accepted. The wrapping requirement covers it, and the groups now stay top-aligned when they wrap.
- [Live refresh races with a fee save] → The fee form uses the same `hx-sync="#content:replace"` as the filter forms, so the latest answer wins.
- [Existing tests assert `#filter-context` and "Fees of …"] → Rewrite them against the Fees group and the subtitle in the same tasks that change the markup.

## Migration Plan

The additive migration (a nullable column) needs no backfill. Existing accounts start with no stored choice, so they keep today's behaviour until they pick a character. To roll back, revert the code. The column can stay.
