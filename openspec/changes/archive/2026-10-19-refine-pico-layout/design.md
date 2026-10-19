# Design

## Context

`base.html` renders `<div class="shell">` with the sidebar and one `<main class="container-fluid">`. That `main` holds the `subnav`, `filters` and `content` blocks. Live pages open the SSE connection (`hx-ext="sse" sse-connect="/events"`) on a `div` inside `content`, and that div holds both `#content` and `#freshness`. Freshness partials (`orders_freshness.html`, `trades_freshness.html`, `wealth_freshness.html`, `opportunities_freshness.html`) start with `<h2>Freshness</h2>` and a `ul.freshness`. `app.css` overrides only tables, the sidebar, the sub-menu, the KPI cards, pagination and the filter box. It leaves Pico's `--pico-font-size` ramp (100% → 131.25%) in force. KPI cards are `article.kpi` with a `header`, `aspect-ratio: 1`, in a `repeat(auto-fill, minmax(9rem, 1fr))` grid. `app.js` publishes the height of `nav.sub-menu` as `--sub-menu-height` for `scroll-margin-top` and for the section marker.

Several e2e tests assert that pages have no `h1` (navigation, opportunities, opportunities-layout). They also check `main.container-fluid`, `main > nav.sub-menu#section-nav`, the "Freshness" heading and `.kpi header`. These assertions encode the old layout and change with this one.

## Goals / Non-Goals

**Goals:**
- One page skeleton in `base.html` that every signed-in page fills through blocks.
- A sticky-footer layout that needs no JavaScript.
- Keep every existing id, `data-live` endpoint, `hx-select-oob` target and SSE trigger, so live updates, scroll restore and the section marker keep working untouched.

**Non-Goals:**
- No change to handlers, view models, routes or SSE signals, apart from passing a page title where a template cannot derive it.
- No new colours, fonts or icons, and no customised Pico build. Overrides stay in `app.css`.
- No change to the landing and error pages beyond what the font-size override does to them.

## Decisions

### Page column skeleton

`base.html` becomes:

```
<div class="shell">
  <aside class="sidebar">…</aside>
  <div class="page" {% block page_attrs %}{% endblock %}>
    <header class="page-header"><h1>{% block page_title %}{% endblock %}</h1></header>
    {% block subnav %}{% endblock %}
    {% block filters %}{% endblock %}
    <main>{% block content %}{% endblock %}</main>
    {% block footer %}{% endblock %}
  </div>
</div>
```

`.page` is `display: flex; flex-direction: column; min-height: 100vh` and carries the horizontal padding that `container-fluid` gave before. `main` gets `flex: 1 0 auto`. A footer, when present, therefore sits at the bottom of the window on short pages and after the content on long ones. This is the classic sticky footer; nothing is `position: fixed`.

- *Why the sub-menu and filters sit outside `<header>`:* `position: sticky` only sticks inside the parent's box. Inside a short `<header>`, the sub-menu would scroll away together with it. As a direct child of the full-height `.page`, it sticks for the whole page. The `<header>` carries only the title, and the visual order title → sub-menu → filters is what the spec requires.
- *Alternative:* `header { display: contents }` to keep all three in one element. It was rejected because `display: contents` has a history of dropping landmark semantics.
- Pages without a sidebar (landing, error) keep their current markup.

### SSE scope covers the footer

Live pages move `hx-ext="sse" sse-connect="/events"` from their inner div to `.page` through `{% block page_attrs %}`. `#content` (or `#opportunity-detail-content`) stays inside `<main>`, and `<footer class="page-footer"><section id="freshness" …></section></footer>` comes from `{% block footer %}`. Both live inside the SSE element, so `sse:*` triggers and `htmx:sseOpen from:body` keep working. The ids and `hx-select-oob="#freshness"` stay unchanged.
- *Alternative:* a second SSE connection in the footer. It was rejected because it doubles connections and races the content refresh.

### Page title

Each page template fills `page_title`: Dashboard, Characters, Wealth, Trades, "Sell orders"/"Buy orders" (from `view.side`), the opportunity table's name (from the view's current table title), and on the detail page `view.name`. In-content headings that would repeat the title are removed: the characters page's "Characters" `h2`, and the opportunity table's own `h2` on table pages. The detail page's first section keeps its "item · table" title, because the station-opportunities spec requires it. The `<title>` element is unchanged.

### Base font and headings

`app.css` sets `:root { --pico-font-size: 100%; }`. Pico's breakpoint rules use the same `:root` selector and `app.css` loads after Pico, so the override wins at every width without `!important`. Headings: `.page-header h1` at 1.5rem with a small bottom margin, and `main h2` at 1.125rem. Both are set through Pico's per-heading `--pico-font-size` variable, so line-height and margins scale with it.

### Sub-menu as tab bar

`.sub-menu` keeps `position: sticky; top: 0` and the page background. It gets `border-bottom: 1px solid var(--pico-muted-border-color)`. Links get `padding-block: 0.5rem` and a 2px transparent bottom border with `margin-bottom: -1px`, so the current link's `border-bottom-color: var(--pico-primary)` sits on the divider. Non-current links use `--pico-muted-color`. `app.js` keeps publishing `--sub-menu-height` unchanged; only its selector `main > nav…` in tests changes.

### Vertical rhythm

The live content containers (`#content`, `#dashboard-view` sections, `#opportunity-detail-content`, the characters `section`) become flex columns with `gap: var(--pico-spacing)`, and their direct children have `margin-block: 0`. This replaces the per-component `margin-bottom` on `.kpis`, `.table-wrap` and `.pagination`, so the KPI→table and table→pagination gaps are equal by construction. The page header, sub-menu and filter box use the same spacing below them.

### KPI stat tiles

`kpi_card.html` replaces `<header>{{ name }}</header>` with `<small class="name">`. The value and detail lines are unchanged. `.kpi` drops `aspect-ratio` and the fixed header band. It uses Pico's article background with compact padding (`0.75rem 1rem`) and `margin: 0`. The grid becomes `repeat(auto-fill, minmax(12rem, 1fr))`: every card has the same width, rows take the tallest card's height (grid default stretch), and empty tracks keep cards from stretching across the whole row. The three-line clamp on the detail stays.

### Compact freshness footer

`.page-footer` gets `border-top: 1px solid var(--pico-muted-border-color)`, `padding-block: 0.5rem`, `font-size: 0.875rem` and `--pico-muted-color`. The `<h2>Freshness</h2>` is removed from all four partials. `ul.freshness` becomes a borderless list without bullets. Each `li` is a wrapping inline flex row (`display: flex; flex-wrap: wrap; gap: 0 0.75rem; align-items: baseline`). On the wealth page the `[data-kind]` divs become inline segments of that row. All texts ("Last update:", "Next update:", "Last refresh:", "Stale"…), `data-kind` attributes and the "Log in again" link stay as they are, so the content assertions in the existing tests hold. Badges keep their colours but get a smaller padding so they fit the line.

## Risks / Trade-offs

- [Removing the `h2` that duplicates the title breaks tests that look up tables by heading] → Update those tests to use the `h1` or the table section's id. This is a test change, not behaviour.
- [16px base text on 4K screens may feel small to someone used to the scaled version] → Browser zoom still works, and the spec asks for density.
- [Moving `sse-connect` to `.page` also opens the connection for the sub-menu and filters' lifetime] → It is the same single connection per page as before. The characters page and other non-live pages do not set `page_attrs`, so they open none.
- [Sticky footer with `min-height: 100vh` plus Pico's body margins can overshoot by a few pixels and add a scrollbar] → `.page` uses `min-height: 100dvh` with `box-sizing: border-box`, and a test asserts no vertical overflow on a short page.
- [The scroll-restore logic in `app.js` keeps "the end of the page" in view] → The footer is still the last element, so the existing live-updates scenario (freshness still in view after a refresh) holds. The live-updates e2e test covers it.
