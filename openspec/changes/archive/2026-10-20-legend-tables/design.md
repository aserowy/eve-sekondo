# Design

## Context

Both legends render `opportunities_legend.html`, a `<dl class="legend">` with one `<dt>`/`<dd>` pair per `LegendEntry { short, name, meaning, formula }`. The explainer card includes it with the table's entries, and the detail page's Legend card (`hx-preserve`) includes it with the detail entries. The worked examples are `<table class="example">` with `<th scope="row">` labels, styled by `article.explainer table.example` rules in `static/app.css`. Data tables use `.table-wrap`, which makes the table `width: max-content` and scrolls it sideways, so its cells never wrap.

## Goals / Non-Goals

**Goals:**
- One template change gives both legends the table layout.
- The legend table and the worked example share one style.

**Non-Goals:**
- Changes to the entries' wording or order, to `legend.rs`, or to the worked examples' content.
- A sortable or clickable legend.

## Decisions

### The legend template renders a table
`opportunities_legend.html` becomes `<table class="explainer-table legend">` with a `<thead>` row of `<th scope="col">` cells Label, Name, Meaning and Formula, and a `<tbody>` row per entry: `<th scope="row">` with the short label, then `<td>` cells for the name, the meaning and `<code>{{ formula }}</code>` when there is one. The Formula cell has no content without a formula. The `LegendEntry` view is unchanged, because it already carries the four fields.
*Alternative*: build the cells in Rust. That adds view code for what the template already expresses.

### One shared class for both explainer tables
The worked examples' `class="example"` and the legend tables both get a shared `explainer-table` class, and the `article.explainer table.example` rules move to it, so both look alike in the explainer card and the detail Legend card. The `.legend dt`/`dd` rules are removed. Neither table carries `data`, so the page's one data table and the scripts that sort and open rows (which select `table.data`) are unaffected.
*Alternative*: style `.legend` separately. That duplicates the rules and lets the two drift.

### Wrapping without sideways page scroll
The meaning cell wraps (`white-space: normal`); the label and formula cells do not (`nowrap`). The legend table does not use `.table-wrap`, whose `max-content` width would stop the meaning from wrapping. It sits in its own `overflow-x: auto` container at full width, so a window too narrow even for the longest label and formula scrolls the legend inside its card rather than the page.

## Risks / Trade-offs

- [The %B formula, about 55 characters, is wide on one line] → It sets the Formula column's width. At very narrow widths the legend scrolls inside its own container, and the page itself does not.
- [Tests read `<dl>`/`<dt>`/`<dd>`] → The tasks switch them to table rows and cells.
