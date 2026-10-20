# Design

## Context

See proposal.md for the motivation and specs/ui-layout/spec.md for the requirements.

- `base.html` renders `<header class="page-header"><h1>…</h1></header>`, then `{% block subnav %}`, then `{% block filters %}`. Every page with a sub-menu fills `subnav`: the dashboard, the orders pages, the opportunity table pages and the opportunity detail page.
- The sub-menu is `position: sticky; top: 0`. `app.js` publishes the sub-menu's height as `--sub-menu-height`, and jump targets use it as `scroll-margin-top`. The section marker reads the sub-menu's bottom edge. None of this depends on what comes before the sub-menu in the DOM.
- In vendored Pico, `th` links inherit `a { text-decoration: var(--pico-text-decoration) }`, which is `underline`. `thead th` sets `--pico-border-width: 0.1875rem`, so the header row has a thicker bottom line than body rows. That line stays.
- "Item name" is `<input type="search">`. Pico gives search inputs a 5rem border radius, a magnifier background image and extra left padding. WebKit and Blink also draw a clear button.
- `.filter-box` is a wrapping flex row with `gap: 0.25rem 1.5rem`. Its children are the groups: the `#character-filter` form, `#range-control`, the `#opportunity-filters` form and `#filter-context`. Inside a group, the fieldset's checkboxes are `1rem` apart, and so are the opportunity fields.

## Goals / Non-Goals

**Goals:**
- One template edit puts the sub-menu on top for every page that has one.
- Only the app stylesheet and markup change. Pico stays vendored as it is.

**Non-Goals:**
- Changing the sub-menu's tab look, its sticky behaviour or the section marker logic.
- Restyling the sort arrows or the header line.
- Keeping a separate search semantic (`role="searchbox"`) for "Item name". It is one filter among the others.

## Decisions

1. **Swap the blocks in `base.html`.** `{% block subnav %}` moves above the `<header class="page-header">`. The `<header>` keeps only the title. Moving the sub-menu inside `<header>` was the alternative. It was rejected because the sticky element would then be confined to the header's box and would stop sticking once the header scrolls away. The sub-menu stays a direct child of the page column, as it is today.
   - The page column keeps its top padding, so on load the sub-menu starts one spacing below the window top. It sticks at `top: 0` once scrolled. It is still the column's first element, which is what the spec requires.
   - The title's top margin is reset to 0, so the gap between the sub-menu's divider and the title is the sub-menu's own `margin-bottom`.
2. **Header links are styled in `app.css`, not via Pico variables.** The new rule is `th a, th a:hover, th a:focus-visible { text-decoration: none; }`. It sits next to the existing `th a { color: inherit; }`. Setting `--pico-text-decoration` on `th` was considered. It is less obvious, and Pico re-sets the variable on `a:hover`.
3. **`type="text"` for "Item name".** It keeps `maxlength="100"`, its label and its error wiring, so the server and htmx behaviour do not change. Overriding Pico's search styles in CSS was the alternative. It was rejected because it fights the framework, and the Dense theme requirement keeps form fields at Pico defaults.
4. **The group gap lives on `.filter-box`.** It becomes `gap: 0.75rem 3rem`. Gaps inside groups (fieldset `1rem`, opportunity grid `0 1rem`) are untouched, so the character filter stands apart. Because `#filter-context` already uses `margin-left: auto`, it still ends up at the right end.

## Risks / Trade-offs

- [Existing e2e tests assert title-before-sub-menu order and positions] → Update `page-structure.spec.ts` and `subnav.spec.ts` in the same task as the template swap.
- [The wider row gap moves the opportunity filters right, so they may wrap earlier at 1280px] → The opportunity grid already wraps internally with `flex: 1 1 50rem`. The "Narrow window wraps" scenario covers that the page never scrolls sideways.
- [Losing the browser's clear "×" on "Item name"] → Accepted: the user wants all text filters to look the same. Clearing the field by hand still triggers the same filter request.
