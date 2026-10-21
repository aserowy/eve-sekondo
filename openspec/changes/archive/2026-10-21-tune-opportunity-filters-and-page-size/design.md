# Design

## Context

- The opportunity filters live in one `opportunity_settings` row per account (`min_margin`, `min_isk_per_day`, `search`, `capital`, `fee_character_id`). They are read through `Settings::load` and written by `POST /opportunities/settings` (CSRF via the `X-CSRF-Token` header that `base.html` puts on `<body hx-headers>`). The fee character has its own store methods and its own POST route, so saving the filters never touches it.
- Paging is shared by every table page through `pagination.rs`: `PER_PAGE_SIZES = [25, 50, 100, 200]` and `DEFAULT_PER_PAGE = 50`. `Paging::parse` falls back to the default for a missing or unknown `per_page`, and `Paging::query` leaves out a `per_page` equal to the default. The page-size `<select>` is driven by `app.js`: on change it GETs the option's content URL and then pushes the option's page URL.
- The opportunity view state (`ViewState` in `opportunities/page.rs`) carries `per_page` only in the URL. The detail page's back link and the sub-menu drop it, which is why the size is lost today.
- The `#opportunity-filters` grid has `gap: 0 1rem`, and its inputs have Pico's default height (body line height plus `--pico-form-element-spacing-vertical` padding on both sides).

## Goals / Non-Goals

**Goals:**
- Store one page size per account for the opportunity table pages, without a state change on a GET request.
- Keep `pagination.rs` the single place that knows the page sizes. Other pages get the new sizes and default without code of their own.

**Non-Goals:**
- No stored page size for the orders, wealth or trades pages. They keep URL-only paging with the new default of 25.
- The page number is not kept when going back from a detail page. It stays at page 1, as today.

## Decisions

### Input height via CSS on the filter section, not Pico variables
One rule in `app.css`: `.filter-box input:not([type=checkbox], [type=radio]) { height: 2rem; padding-block: 0; }`. Pico sets `box-sizing: border-box`, so the 2rem includes the border, and the text stays centred vertically in a single-line input. The font size is not touched.
*Alternative:* overriding `--pico-form-element-spacing-vertical` on `.filter-box`. Rejected, because it also changes the spacing of checkboxes, radios and the range buttons, and the resulting height would depend on line height and border instead of being a fixed 2rem.

### Row gap on the opportunity grid
`#opportunity-filters` gets `gap: 0.75rem 1rem`. This uses the same 0.75rem as the gap between wrapped lines of the filter section. The column gap is unchanged.

### Maximum margin as a sixth column of `opportunity_settings`
A migration adds `max_margin numeric NOT NULL DEFAULT 0.80`, so existing accounts get 80% without a backfill. `Settings` gains `max_margin` (a fraction, default `0.80`) and `Form` gains a `max_margin` text field. `Form::validate` checks the fields in form order: min margin, max margin (a number ≥ 0 and ≥ the validated min margin), ISK/day, search, capital. That way the first rejected field is still the first one on screen. `tables::rows` drops a row with `figures.margin > settings.max_margin`. `tables::detail` gets the reason "Its net margin is above your maximum margin.", checked after the minimum margin reason. Both stores (PG upsert column list, memory row) carry the field.

### Stored page size: own column, own route, POST with CSRF
- A migration adds `per_page integer` (nullable; NULL means the account never chose a size) to `opportunity_settings`. `OpportunityStore` gets `load_page_size` and `save_page_size`, modelled on the fee character methods, so that saving the filters and saving the page size never overwrite each other.
- `POST /opportunities/page-size` with body `per_page=<n>` checks that `n` is in `PER_PAGE_SIZES`, stores it and answers `204`. An unknown size answers `422` and stores nothing. The route sits behind the existing CSRF middleware like the other opportunity POSTs.
- *Why not save on GET when `per_page` is in the URL:* that would be a state change on a GET request without a CSRF check. A third-party page could then change a user's preference just by embedding a link. The requirement is that a URL `per_page` takes precedence without being saved.
- *Why not URL-only:* the size would still be lost on a fresh visit, and the user chose per-account storage.

### Paging with a page-supplied fallback
`pagination.rs` gets `PER_PAGE_SIZES = [10, 25, 50, 100, 200]` and `DEFAULT_PER_PAGE = 25`. A new `Paging::parse_or(page, per_page, fallback)` is used when `per_page` is missing; an unknown value still falls back to 25. `Paging::query_or(fallback)` leaves out `per_page` when it equals the fallback. `parse`/`query` stay as the `fallback = DEFAULT_PER_PAGE` case, so orders, wealth and trades need no changes. The opportunity `ViewState` loads the stored size in `environment()` (the account's settings are already loaded there) and builds its links with `query_or(stored)`. As a result, links of the size the user stored carry no `per_page`, and the detail page's back link and the sub-menu fall back to the stored size naturally.

### Page-size control on opportunity pages
`PageView` gets an optional `save: Option<String>`, and `pagination.html` renders it as `data-save` on the select. Only the opportunity page sets it, to `/opportunities/page-size`. In `app.js`, when `data-save` is present, the change handler first does `htmx.ajax('POST', save, { source: select, values: { per_page }, swap: 'none' })`. Using `source: select` means the `<body hx-headers>` CSRF header is inherited. Then it runs the existing content GET and `pushState`. The option links on opportunity pages are built without `per_page` when the option is the size being saved: since the option's size becomes the stored size, the link is built for `fallback = option size`, which omits `per_page`. If the POST fails, the GET is not made, and the select is reset to the size shown.

## Risks / Trade-offs

- [Changing the default to 25 breaks e2e and unit tests that count 50 rows] → Update the affected tests in the same task as the default change (`pagination.rs` tests, and the `layout`, `live`, `subnav`, `opportunities-layout` specs). Set `per_page` explicitly where a test needs a large page.
- [Two requests for one size change (POST, then GET)] → They are sequential and only happen on a deliberate change. In exchange, the existing content GET path is reused unchanged.
- [A 2rem input at Pico's larger root font sizes] → rem scales with the root, so the height stays proportional to the font. The Playwright test compares the height with the computed root font size, not a fixed pixel value.
- [A stored size and a URL size disagree] → The URL wins and is not saved, as the spec states. Links on that page carry the URL's `per_page`, because it differs from the stored fallback.

## Migration Plan

One additive migration, `ALTER TABLE opportunity_settings ADD COLUMN max_margin numeric NOT NULL DEFAULT 0.80, ADD COLUMN per_page integer;`, applied by the existing sqlx migrator at startup. To roll back, deploy the previous build. The extra columns are ignored by it.
