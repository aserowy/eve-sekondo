# Proposal

## Why

The move to Pico CSS left the signed-in pages looking unfinished. Pico scales its root font up to 125–131% on desktop widths, so everything outside the tables is oversized. KPI cards are forced square and stretched into large, mostly empty boxes. Pages have no title. The freshness is a heavy heading-plus-list block inside the content. Nothing uses the semantic `header`/`main`/`footer` structure that Pico is built around. This change applies Pico the way it is meant to be used, together with current guidance for data-dense dashboards: a clear page hierarchy, one spacing rhythm, compact stat tiles and status shown as a quiet footer.

## What Changes

- Every signed-in page is laid out as a page column right of the sidebar, made of a `header` (page title, sub-menu, filter section), a `main` (KPI cards and content) and, on pages with freshness, a `footer`.
- **Freshness becomes a footer.** The footer sits at the bottom of the window when the page is shorter than the window. When the page is taller, it comes after the content and only scrolls into view at the end. It is never fixed over the content. Its entries are compact single lines in small, muted text, with no "Freshness" heading. Badges and "Log in again" stay inline. The wording of the freshness texts does not change.
- Pico's responsive font scaling is turned off: the base font size is Pico's 100% at every window width. Tables stay at 0.875rem.
- **KPI cards are no longer square.** **BREAKING** (visual contract): cards become compact stat tiles of natural height and equal width. Each tile shows a small muted name, a large value and an optional detail line, with no header band. Wrapping, KPI coverage and ISK shortening are unchanged. The dashboard tiles follow.
- Every signed-in page gets an `h1` page title at the top of its header. The sub-menu sits below the title, styled as an underlined tab bar, and stays the only element that sticks to the top while scrolling.
- In-page section titles are visually smaller than the page title. Blocks inside a page are separated by one consistent vertical spacing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ui-layout`: the theme requirement allows the app stylesheet to set the base font size and lay out the page header, footer and stat tiles. A new page structure requirement covers the header, main and footer, the page title and section headings. The sub-menu becomes a tab bar inside the header. The table page layout moves freshness into the footer. KPI cards drop the square shape. Freshness at the end of the page becomes a sticky page footer.
- `dashboard`: the "Tiles are cards" scenario no longer requires square cards.

## Impact

- Templates in `crates/sekondo/templates/`: `base.html` (page column with header/main/footer blocks and an SSE scope that covers the footer), every page template (title, moving freshness into the footer block), the four `*_freshness.html` partials and `kpi_card.html`.
- `crates/sekondo/static/app.css`: base font override, page column layout, sticky footer, tab-style sub-menu, stat tiles and heading sizes.
- No changes to routes, handlers, view models, live-update signals or data. The freshness section keeps `id="freshness"` and its live refresh.
- e2e tests: update the layout, KPI, freshness and pico specs, and add tests for footer placement (short and tall pages), base font size, page titles and stat tile shape.
