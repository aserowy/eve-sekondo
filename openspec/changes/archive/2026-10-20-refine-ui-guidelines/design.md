# Design

## Context

See proposal.md (Why) for the motivation and `specs/ui-layout/spec.md` for the behaviour. The relevant current state:

- **Page column.** `.shell > .page` is a flex column. `main` has `flex: 1 0 auto`, which is how the footer reaches the window bottom. Every table page renders `main > section#content > (…, pagination)`. `#content` is swapped by htmx (`innerHTML`) on live refreshes, page changes and filter changes.
- **Sidebar.** Links, the Sign out `<button class="link">` and the `.menu-toggle` button share one flex rule. Pico still gives buttons `justify-content: center` and text centring, so the toggle's and Sign out's icons drift away from the links' icon column.
- **Sub-menu.** `.sub-menu` is `display: flex` with a gap, but some sub-menus render inside Pico's `nav`, whose `ul` rule spreads entries with `justify-content: space-between`. Implementation checks each sub-menu template (`orders_sub_menu.html`, `opportunities_sub_menu.html`, `opportunity_detail_section_nav.html`, the dashboard's inline nav) against that rule.
- **Table cells.** Cells use `padding: 0.1rem 0.5rem` with `line-height: 1.2` at 0.875rem, which gives a row of about 17px plus padding.
- **Filters.** `.filter-box` forces labels to `inline-flex` and inputs to `width: auto; margin: 0`, which overrides Pico's form layout. The opportunity filter error is a single `<span id="filter-error">` that the 422 answer replaces (`data-invalid-target`). `Form::validate` returns only a `String`.
- **Graphs.** Graphs are a `<section>` holding an `<h2>` and a `div.chart`. The section carries the fragment id for the dashboard and detail-page section sub-menus.

## Goals / Non-Goals

**Goals:**
- Make every rule in the spec delta pass as a Playwright assertion on computed geometry, not only by eye.
- Keep the htmx swap targets and ids the same, so live refresh, scroll keeping and the sub-menu marker keep working untouched.

**Non-Goals:**
- Changing which filters exist, their names or their values.
- Changing the characters, landing or error pages beyond what the shared rules imply.
- Restyling KPI cards. They are already cards.

## Decisions

### Graph cards: `<article>` replaces the graph's `<section>`
The graph's wrapper becomes `<article class="graph" id="…">` and keeps its id, `<h2>`, `div.chart` and empty-state `<p>`. Pico's `article` gives the card background, padding and radius. The app stylesheet only zeroes the article's bottom margin, so the existing block spacing stays even, and lets cards in `.graph-row` stretch to the row height (grid items already stretch). `scroll-margin-top` on `[id]` already applies, so jumps land on the card's title.
*Alternative*: wrap the `div.chart` alone in an article and keep the title outside. Rejected, because the user asked for the title inside the card.

### Sidebar: one icon column, toggle on the right
All sidebar controls get `justify-content: flex-start; text-align: left` and the same padding. The icon is the first flex child, so icons share one left edge, and labels follow at a fixed gap. In the expanded state, the toggle is `margin-left: auto; width: auto`, so its icon sits at the sidebar's right edge. In the collapsed state (`html[data-menu="collapsed"]`), it goes back to `margin-left: 0`, so its icon joins the column. The page entries are `<a>` and Sign out is a `<button>`, so the rule resets Pico's button centring explicitly instead of relying on inheritance.

### Sub-menu: packed entries
`.sub-menu` and any `ul` inside it get `justify-content: flex-start` and a fixed `gap` (1.5rem, as today). Where a sub-menu is a Pico `nav > ul`, the `li` items have their Pico padding zeroed, so the gap alone sets the spacing. The test compares the gaps between two sub-menus to catch a template that slips back to `space-between`.

### Pagination at the page bottom via flex
`main` becomes `display: flex; flex-direction: column`. Its `section#content` gets `flex: 1 0 auto; display: flex; flex-direction: column`, and `.pagination` gets `margin-top: auto`. On a short page, the spare height that `main` already takes flows into `#content` and pushes the pagination down to just above the footer. On a long page there is no spare height, so the pagination follows the table. Nothing is sticky or fixed, so "scrolls out" comes for free. The htmx swaps replace only `#content`'s children, so the placement survives refreshes. The few direct children of `#content` (KPIs, sections, `.table-wrap`) keep their own margins, because flex children do not collapse margins. The existing `margin-bottom: var(--pico-spacing)` rules already give even spacing. Implementation verifies this with the even-spacing scenario.
*Alternative*: `position: sticky; bottom: 0` for the pagination. Rejected: the user wants it to scroll out with the table, not to stay pinned.

### Table cells: 0.3rem × 0.75rem
`th, td { padding: 0.3rem 0.75rem }`. Both values are fixed in rem in the spec, not derived from a pixel target. A line of 0.875rem × 1.15 = 1.00625rem (16.1px), plus 2 × 0.3rem of padding and the collapsed row border (about 2px in Chromium), gives about 27.7px, under the 1.75rem row limit (28px at the 16px base). The line-height drops from 1.2 to 1.15 for this: at 1.2 the row measures 28.39px. Playwright compares measured pixels against rem values converted with the root font size. The `.records` details row keeps its smaller font and inherits the padding.

### Filters: Pico's form structure
- **Character filter.** The `<fieldset aria-label>` becomes `<fieldset><legend>Characters</legend>…</fieldset>`. The checkbox labels are laid out in a wrapping flex row with a gap. That layout is the only app CSS, because Pico stacks checkbox labels by default.
- **Opportunity filters.** Each field stays `<label>Text <input></label>`. Pico renders the label text above a full-width input. The form becomes a CSS grid with `repeat(auto-fit, minmax(11rem, 1fr))` columns, so the four inputs are equally wide and wrap. All `.filter-box input` / `label` overrides are removed except `margin-bottom: 0` on the last element, which keeps the box compact.
- **Range control.** `<div role="group" aria-label="Time range">` with `<a role="button">` children, and the current link marked `aria-current="true"`. A visible "Range" label sits above the group (a `<span>` referenced by `aria-labelledby`). That keeps the group's bottom edge aligned with the label-above inputs next to it. Non-current buttons get Pico's `secondary outline` classes and the current one stays primary. These are Pico classes, not app CSS.
- **Filter-box row.** The box's flex row uses `align-items: flex-end`, so inputs, checkboxes and the button group share a bottom edge. The context text keeps `margin-left: auto`.

### Per-field errors
`Form::validate` returns `Result<Settings, FieldError>` with `FieldError { field: Field, message: String }`, where `Field` is one of the four inputs. Today's message strings are kept. The rejected view renders the whole `#opportunity-filters` form with the typed values, `aria-invalid="true"` and `aria-describedby` on the bad input, and `<small id="<field>-error">` with the message below it. `data-invalid-target` changes from `#filter-error` to `#opportunity-filters`, so the 422 answer replaces the form (outerHTML). The inputs get stable ids, so htmx's focus restore keeps the caret in the field the user moved to. A successful save no longer touches the form from the server: swapping the inputs out of band would reset text the user typed into another field while the save was in flight (a Playwright test with a held save showed this). Instead, `app.js` clears `aria-invalid`, `aria-describedby` and the `<small>` messages of a form with `data-invalid-target` once its request succeeded, and leaves the inputs alone. The `#filter-error` span and `opportunities_filter_error.html` are gone; `opportunities_filter_error_fragment.html` is the form.
*Alternative*: keep one error span and point at the field with JS. Rejected: that duplicates server knowledge in the client, and Pico's `aria-invalid` + `<small>` pattern only works next to the input.

### Dense theme wording
The spec now lets the app stylesheet set the filter section's and the graph cards' layout. The deleted overrides (input width and margins, label inline-flex) bring form fields back to Pico's defaults, which the new "Form fields keep Pico's size" scenario checks.

## Risks / Trade-offs

- [An out-of-band form swap on a successful save would reset a field the user is typing in] → It did, in a Playwright test with a held save. The clean state is applied by clearing `aria-invalid`, `aria-describedby` and the `<small>` messages client-side instead of swapping the inputs (see Per-field errors); the test now asserts that the typed text survives.
- [Taller filter section: labels above inputs and a legend add a line] → Accepted. The user asked for Pico's structure, and the section is no longer sticky.
- [Sidebar width changes when the toggle moves right] → The sidebar keeps its width from its widest label. The toggle uses `width: auto`, so it adds no width.
- [Flex `#content` changes margin behaviour inside the table pages] → Covered by the even-spacing and pagination scenarios on the wealth, sell orders and trades pages.
- [Existing tests assert on `#filter-error`, inline labels or the old row height] → Updated in the same tasks that change the markup.
