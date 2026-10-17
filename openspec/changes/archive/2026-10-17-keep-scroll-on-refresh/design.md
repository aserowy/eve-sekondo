# Design

## Context

- Scrolling panels are `.scroll` elements (from `cockpit-ui`): `#sell-orders .scroll`, `#buy-orders .scroll`, the dashboard breakdown's `.scroll`, `#wealth-items .scroll`, and the freshness lists `#freshness ul.freshness.scroll` and `#wealth-freshness ul.freshness.scroll`.
- Live refreshes are htmx requests on containers whose `hx-trigger` lists `sse:<signal>` events and `htmx:sseOpen from:body`. They swap `innerHTML` of `#order-tables`, `#freshness`, `#dashboard-summary`, `#wealth-items` and `#wealth-freshness`, so the `.scroll` elements are new nodes with `scrollTop` 0.
- User view changes (filter, sort headers, dashboard range) swap `outerHTML` of `#orders-view`, `#dashboard-view` or `#wealth-view`. The wealth range swaps `#wealth-graph`, which has no scroller.
- `app.js` already listens to htmx events (`htmx:configRequest`, `htmx:load`). The CSP forbids inline scripts and styles, but setting `scrollTop` from `app.js` is fine.

## Goals / Non-Goals

**Goals:**
- Keep pixel scroll offsets of every scroller across a signal-triggered swap.

**Non-Goals:**
- Anchoring to a row (keeping the same order in view when rows are inserted above it). The pixel offset is kept; a row added above shifts the view by one row.
- Remembering positions across navigation or reloads.
- Server changes.

## Decisions

### 1. Stable key per scroller
Each scroller gets `data-scroll-key` in its template (`sell-orders`, `buy-orders`, `breakdown`, `inventory`, `freshness`, `wealth-freshness`). Positions are matched by key, not by DOM position or element identity, because the swap creates new nodes.
- *Alternative:* derive the key from the nearest ancestor `id`. The breakdown and the freshness lists have no own id and would need one anyway, and an explicit attribute keeps the contract visible in the templates.

### 2. Only signal-triggered swaps restore
On `htmx:beforeSwap`, `app.js` checks `event.detail.requestConfig.triggeringEvent?.type`. If it starts with `sse:` or is `htmx:sseOpen`, it records `{scrollTop, scrollLeft}` for every `[data-scroll-key]` inside the swap target. On `htmx:afterSwap` for the same request, it applies them to the elements with the same keys inside the target. Browsers clamp `scrollTop` to the maximum, which gives "show the end" for shorter content without extra code.
- Filter, sort and range requests are triggered by `change`/`click`, so they are left alone and show the new view at the top. That matches the spec's distinction without listing user actions.
- *Alternative:* htmx `hx-swap` modifiers (`show:none`, `scroll:`). They control the window or the target's scroll, not nested scrollers inside replaced content.
- *Alternative:* idiomorph / morph swaps that keep nodes in place. That adds a dependency and changes how every refresh swaps, which is a much larger change.

### 3. Recorded positions travel with the request
Positions are stored on the request's `event.detail` (or a `WeakMap` keyed by the XHR), so overlapping refreshes of different containers (`#order-tables` and `#freshness` both refresh on `fees-updated`) don't overwrite each other.

## Risks / Trade-offs

- [Rows inserted above the visible rows shift the view by a few rows] → Accepted (non-goal). Revisit with row anchoring if it bothers in practice.
- [htmx renames `triggeringEvent` in a future version] → htmx is vendored at 2.0.4 and only changes on purpose. The Playwright scenarios fail if the hook stops working.
- [A user scrolls while the refresh is in flight] → The position recorded at swap time (`beforeSwap`) is used, which is the latest one, so nothing jumps back.

## Migration Plan

Front-end only, shipped with the next release after `cockpit-ui`. Rollback is reverting the commit.
