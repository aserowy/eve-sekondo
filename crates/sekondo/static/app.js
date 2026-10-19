// htmx would inject an inline <style> for indicators, which the CSP forbids.
// The indicator styles live in app.css instead.
htmx.config.includeIndicatorStyles = false;
// No eval: the CSP forbids it, and nothing here needs hx-on or hx-vals javascript.
htmx.config.allowEval = false;

// ---- sidebar ----
// The menu is expanded or collapsed per browser (localStorage). The script is loaded in the head, so
// the state is on <html> before the body renders and the menu never flashes open.
const MENU_KEY = 'sekondo.menu';

function menuCollapsed() {
  try {
    return localStorage.getItem(MENU_KEY) === 'collapsed';
  } catch {
    return false;
  }
}

function applyMenu() {
  const collapsed = menuCollapsed();
  if (collapsed) document.documentElement.dataset.menu = 'collapsed';
  else delete document.documentElement.dataset.menu;
  document.querySelector('.menu-toggle')?.setAttribute('aria-expanded', String(!collapsed));
}

applyMenu();
document.addEventListener('DOMContentLoaded', applyMenu);
document.addEventListener('click', (event) => {
  if (!(event.target instanceof Element) || !event.target.closest('.menu-toggle')) return;
  try {
    localStorage.setItem(MENU_KEY, menuCollapsed() ? 'expanded' : 'collapsed');
  } catch {
    // Without storage the menu still toggles for this page.
    document.documentElement.toggleAttribute('data-menu');
  }
  applyMenu();
});

// ---- sub-menu ----
// The sub-menu sticks to the top of the window. Its height is published as --sub-menu-height, so
// that the rows a link jumps to stop below it (scroll-margin-top in app.css). A swapped-in
// sub-menu is observed again.
let subMenuObserver = null;

function watchSubMenu() {
  const menu = document.querySelector('nav.sub-menu');
  subMenuObserver?.disconnect();
  subMenuObserver = null;
  if (!menu) {
    document.documentElement.style.removeProperty('--sub-menu-height');
    return;
  }
  const publish = () => document.documentElement.style.setProperty('--sub-menu-height', `${menu.getBoundingClientRect().height}px`);
  publish();
  subMenuObserver = new ResizeObserver(publish);
  subMenuObserver.observe(menu);
}

document.addEventListener('DOMContentLoaded', watchSubMenu);
document.addEventListener('htmx:load', (event) => {
  if (event.detail.elt instanceof Element && event.detail.elt.querySelector?.('nav.sub-menu, #section-nav')) watchSubMenu();
  else if (event.detail.elt instanceof Element && event.detail.elt.matches('nav.sub-menu')) watchSubMenu();
});

// ---- section sub-menu ----
// The links of #section-nav jump to the rows of a scrolling page through plain URL fragments. The
// link of the row being read is marked aria-current="location": right after a jump it is the row
// jumped to (until the user scrolls on their own, with wheel, keys, touch or the scrollbar);
// otherwise it is the last row whose top has reached the bottom edge of the sub-menu, or the last row
// once the page is scrolled to its end. Only the marker changes: scrolling never touches the URL.
let jumpTarget = null;

function markSection() {
  const links = [...document.querySelectorAll('#section-nav a[href^="#"]')]
    .map((link) => ({ link, row: document.getElementById(link.hash.slice(1)) }))
    .filter((entry) => entry.row);
  if (links.length === 0) return;

  let current = jumpTarget && links.find((entry) => entry.row.id === jumpTarget);
  if (!current) {
    const root = document.documentElement;
    const scrolls = root.scrollHeight > window.innerHeight + 1;
    if (scrolls && window.scrollY + window.innerHeight >= root.scrollHeight - 1) {
      current = links[links.length - 1];
    } else {
      const line = (document.querySelector('nav.sub-menu')?.getBoundingClientRect().bottom ?? 0) + 1;
      current = links.filter((entry) => entry.row.getBoundingClientRect().top <= line).pop() ?? links[0];
    }
  }
  for (const entry of links) {
    if (entry === current) entry.link.setAttribute('aria-current', 'location');
    else entry.link.removeAttribute('aria-current');
  }
}

let markScheduled = false;
function scheduleMark() {
  if (markScheduled) return;
  markScheduled = true;
  requestAnimationFrame(() => {
    markScheduled = false;
    markSection();
  });
}

const SCROLL_KEYS = new Set(['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' ']);
function userScrolled() {
  jumpTarget = null;
  scheduleMark();
}

window.addEventListener('scroll', scheduleMark, { passive: true });
window.addEventListener('resize', scheduleMark);
window.addEventListener('load', scheduleMark);
window.addEventListener('hashchange', () => {
  jumpTarget = location.hash.slice(1) || null;
  scheduleMark();
});
window.addEventListener('wheel', userScrolled, { passive: true });
window.addEventListener('touchmove', userScrolled, { passive: true });
window.addEventListener('keydown', (event) => {
  if (SCROLL_KEYS.has(event.key)) userScrolled();
});
document.addEventListener('pointerdown', (event) => {
  // A press on the page's own scrollbar targets the root element.
  if (event.target === document.documentElement) userScrolled();
});
document.addEventListener('click', (event) => {
  const link = event.target instanceof Element && event.target.closest('#section-nav a[href^="#"]');
  if (!link) return;
  jumpTarget = link.hash.slice(1);
  scheduleMark();
});
document.addEventListener('DOMContentLoaded', () => {
  jumpTarget = location.hash.slice(1) || null;
  markSection();
});
// Swapped markup (a live refresh, a new sub-menu) gets its marker back.
document.addEventListener('htmx:load', scheduleMark);

// The view state of a table page lives in the address bar: the selection, the sortings, the range
// and the page size. Sorting, paging and range links push it there without reloading, so the
// address bar is always current, while the markup of a filter form is not (it is rendered once).
// Both filter forms therefore read the state from the address bar when they send their request.
const VIEW_STATE_KEYS = [
  'sort', 'dir', 'bsort', 'bdir', 'range', 'xs', 'xd', 'cs', 'cd', 'ms', 'md', 'ts', 'td', 'per_page',
];

// The character filter is a list of checkboxes, but the URL carries one comma-separated `chars`
// value (and none at all when every character is selected). A plain form would send `chars` once
// per box, so the request URL is built here, for the page named by the form's own `hx-get`: the
// checked boxes, and the rest of the view state as the address bar holds it, without the page
// number (a different selection starts on the first page). Nothing here evaluates code: the CSP
// forbids it.
document.addEventListener('htmx:configRequest', (event) => {
  const form = event.detail.elt;
  if (!(form instanceof HTMLElement) || !form.hasAttribute('data-character-filter')) return;
  const path = form.getAttribute('hx-get');

  const query = [];
  const boxes = [...form.querySelectorAll('input[type="checkbox"][name="chars"]')];
  const selected = boxes.filter((box) => box.checked).map((box) => box.value);
  if (selected.length > 0 && selected.length < boxes.length) {
    query.push('chars=' + selected.map(encodeURIComponent).join(','));
  }
  const current = new URLSearchParams(location.search);
  for (const key of VIEW_STATE_KEYS) {
    if (current.get(key)) query.push(`${key}=${encodeURIComponent(current.get(key))}`);
  }

  for (const key of Object.keys(event.detail.parameters)) delete event.detail.parameters[key];
  event.detail.path = query.length > 0 ? `${path}?${query.join('&')}` : path;
});

// The opportunity filters save through a POST: it carries the view state of the address bar too,
// so that the answer is the table the user is looking at, and not the one the page was rendered with.
document.addEventListener('htmx:configRequest', (event) => {
  const form = event.detail.elt;
  if (!(form instanceof HTMLElement) || !form.hasAttribute('data-view-state')) return;
  const current = new URLSearchParams(location.search);
  for (const key of ['chars', ...VIEW_STATE_KEYS]) {
    if (current.get(key)) event.detail.parameters[key] = current.get(key);
    else delete event.detail.parameters[key];
  }
});

// A live-refreshing element (`data-live`) asks for its fragment with the query of the address bar,
// which is the view state at the time of the request: a refresh that waited for a user's change
// (sorting, a page, a filter) never brings the old view back.
document.addEventListener('htmx:configRequest', (event) => {
  const element = event.detail.elt;
  if (!(element instanceof HTMLElement) || !element.hasAttribute('data-live')) return;
  event.detail.path = element.getAttribute('data-live') + location.search;
});

// A form that is rejected with 422 names where its answer goes (`data-invalid-target`): the form
// itself, which comes back with the reason. htmx does not swap error answers on its own.
document.addEventListener('htmx:beforeSwap', (event) => {
  const { xhr, requestConfig } = event.detail;
  if (xhr.status !== 422) return;
  const selector = requestConfig?.elt?.dataset?.invalidTarget;
  const target = selector && document.querySelector(selector);
  if (!target) return;
  event.detail.shouldSwap = true;
  event.detail.isError = false;
  event.detail.target = target;
  event.detail.swapOverride = 'outerHTML';
});

// ---- scroll positions ----
// A live refresh (a stream signal or a reconnect) must not move what the user is reading: the
// window's scroll position and the sideways scroll of every `.table-wrap` are recorded per request
// before the swap and put back after it and once more after settling (the page is shorter for a
// moment, and charts redraw in between); the browser clamps a position beyond the new content to
// its end. A position is not put back once the user has scrolled or followed a link in between.
// Changes the user makes (filter, sorting, range, page) ask for the top of the table through
// `show:` on their own swap.
const scrollPositions = new WeakMap();
let scrollInputs = 0;

for (const type of ['wheel', 'touchmove', 'keydown']) {
  window.addEventListener(type, () => (scrollInputs += 1), { passive: true });
}
document.addEventListener('click', (event) => {
  if (event.target instanceof Element && event.target.closest('a[href^="#"]')) scrollInputs += 1;
});
document.addEventListener('pointerdown', (event) => {
  // A press on the page's own scrollbar targets the root element.
  if (event.target === document.documentElement) scrollInputs += 1;
});

function isLiveRefresh(detail) {
  const type = detail.requestConfig?.triggeringEvent?.type ?? '';
  return type.startsWith('sse:') || type === 'htmx:sseOpen';
}

document.addEventListener('htmx:beforeSwap', (event) => {
  if (!isLiveRefresh(event.detail)) return;
  scrollPositions.set(event.detail.xhr, {
    y: window.scrollY,
    wraps: [...document.querySelectorAll('.table-wrap')].map((wrap) => wrap.scrollLeft),
    inputs: scrollInputs,
  });
});

function restoreScroll(event) {
  const saved = scrollPositions.get(event.detail.xhr);
  if (!saved || saved.inputs !== scrollInputs) return;
  document.querySelectorAll('.table-wrap').forEach((wrap, index) => {
    if (saved.wraps[index] !== undefined) wrap.scrollLeft = saved.wraps[index];
  });
  window.scrollTo(window.scrollX, saved.y);
}

document.addEventListener('htmx:afterSwap', restoreScroll);
document.addEventListener('htmx:afterSettle', (event) => {
  restoreScroll(event);
  scrollPositions.delete(event.detail.xhr);
});

// ---- pagination ----
// Previous and next are links with their own hx-get. The page-size <select> has one URL per option,
// so a change fetches the option's content fragment and then puts the option's page URL in the
// address bar, the same pair of URLs the links carry.
document.addEventListener('change', (event) => {
  const select = event.target instanceof Element && event.target.closest('select[data-page-size]');
  const option = select?.selectedOptions[0];
  if (!option) return;
  htmx
    .ajax('GET', option.dataset.content, { target: '#content', swap: 'innerHTML show:#content:top' })
    .then(() => history.pushState({}, '', option.dataset.href));
});

// ---- charts ----
// Chart.js 4.5.1, vendored as static/chart.umd.min.js (MIT, see chart.js.LICENSE.md),
// SHA-256 48444a82d4edcb5bec0f1965faacdde18d9c17db3063d042abada2f705c9f54a.
// It draws on <canvas>, uses no eval and injects no <style>, so the CSP stays as it is. Every
// `<canvas data-chart="kind" data-series="json">` is drawn from its HTML-escaped attribute, and
// redrawn whenever htmx swaps a fresh copy in.
const SELL_COLOR = '#3987e5';
const BUY_COLOR = '#d95926';
// Wallet and inventory take the next two slots of the dataviz palette (dark steps). In the stack
// order sell, escrow, wallet, inventory every touching pair passes the validator's colour-blind
// separation check.
const WALLET_COLOR = '#199e70';
const INVENTORY_COLOR = '#c98500';
const TOTAL_COLOR = '#ebeadf';
// Surface, ink and grid follow the theme in app.css (--panel, --ink-dim): small and dim, so the
// figures carry the picture.
const SURFACE = '#161b21';
const INK = '#8793a2';
const GRID = '#232a33';

if (typeof Chart !== 'undefined') {
  Chart.defaults.font.size = 11;
  Chart.defaults.color = INK;
}

// ISK shortened to at most four significant digits (1.235b), as format::isk_short does for text.
function iskShort(cents) {
  const isk = Math.abs(cents) / 100;
  const sign = cents < 0 ? '-' : '';
  if (isk < 1000) return sign + (Number.isInteger(isk) ? String(isk) : isk.toFixed(2));
  const units = [['k', 1e3], ['m', 1e6], ['b', 1e9], ['t', 1e12]];
  let index = units.length - 1;
  while (index > 0 && isk < units[index][1]) index -= 1;
  for (; index < units.length; index += 1) {
    const [suffix, divisor] = units[index];
    const integerDigits = String(Math.max(1, Math.floor(isk / divisor))).length;
    const rounded = Number((isk / divisor).toPrecision(4));
    if (rounded >= 1000 && index + 1 < units.length) continue;
    const decimals = Math.max(0, 4 - integerDigits);
    return sign + String(Number(rounded.toFixed(decimals))) + suffix;
  }
}

// The tooltip is an HTML element, so tests and screen readers can read it. Its text comes from
// the data (names and exact values formatted by the server) and is set with textContent only.
function externalTooltip(context) {
  const { chart, tooltip } = context;
  const holder = chart.canvas.parentNode;
  let element = holder.querySelector('.chart-tooltip');
  if (!element) {
    element = document.createElement('div');
    element.className = 'chart-tooltip';
    element.setAttribute('role', 'status');
    holder.appendChild(element);
  }
  if (tooltip.opacity === 0 || tooltip.dataPoints.length === 0) {
    element.hidden = true;
    return;
  }
  const lines = [];
  for (const title of tooltip.title) lines.push(['chart-tooltip-title', title]);
  for (const point of tooltip.dataPoints) {
    lines.push(['chart-tooltip-value', `${point.dataset.label}: ${point.dataset.exact[point.dataIndex]}`]);
  }
  // A stacked chart also says what the stack adds up to (formatted by the server).
  const totals = chart.config.options.totals;
  if (totals) lines.push(['chart-tooltip-total', `Total: ${totals[tooltip.dataPoints[0].dataIndex]}`]);
  element.replaceChildren(
    ...lines.map(([className, text]) => {
      const line = document.createElement('div');
      line.className = className;
      line.textContent = text;
      return line;
    }),
  );
  element.hidden = false;
  element.style.left = `${tooltip.caretX}px`;
  element.style.top = `${tooltip.caretY}px`;
}

const AXIS_TEXT = { color: INK };
const LEGEND_LABELS = { color: INK, boxWidth: 8, boxHeight: 8, padding: 6 };
const GRID_LINES = { color: GRID };

function moneyAxis() {
  return {
    beginAtZero: true,
    ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (value) => iskShort(value) },
    grid: GRID_LINES,
  };
}

function baseOptions(extra) {
  return {
    animation: false,
    responsive: true,
    interaction: { mode: 'nearest', intersect: true },
    plugins: {
      legend: { display: true, labels: LEGEND_LABELS },
      tooltip: { external: externalTooltip },
    },
    ...extra,
  };
}

const COLORS = {
  'Sell value': SELL_COLOR,
  'Buy escrow': BUY_COLOR,
  'Sell orders': SELL_COLOR,
  'Buy orders': BUY_COLOR,
  Wallet: WALLET_COLOR,
  Inventory: INVENTORY_COLOR,
  // The total is not a component: a neutral line that stands out from every component colour.
  Wealth: TOTAL_COLOR,
  // The opportunity detail page.
  'Best buy': BUY_COLOR,
  'Best sell': SELL_COLOR,
  Average: TOTAL_COLOR,
  'SMA 5': INVENTORY_COLOR,
  'SMA 20': WALLET_COLOR,
  'Upper band': INK,
  'Lower band': INK,
  'RSI 14': SELL_COLOR,
  Spread: INVENTORY_COLOR,
  Volume: SELL_COLOR,
  'Units within 1%': SELL_COLOR,
  'Average maximum spread': BUY_COLOR,
  'Average spread': SELL_COLOR,
  'Average daily spread': SELL_COLOR,
  Peak: SELL_COLOR,
  S: TOTAL_COLOR,
};

// Bands and reference lines are dashed, so they read as limits and not as data.
const DASHED = new Set(['Upper band', 'Lower band']);

// One bar per character and series: the per-character split.
function splitConfig(data) {
  return {
    type: 'bar',
    data: {
      labels: data.labels,
      datasets: data.series.map((series) => ({
        label: series.name,
        data: series.cents,
        exact: series.exact,
        backgroundColor: COLORS[series.name],
        borderRadius: 4,
        maxBarThickness: 48,
      })),
    },
    options: baseOptions({
      scales: { x: { ticks: { ...AXIS_TEXT, maxTicksLimit: 8 }, grid: { display: false } }, y: moneyAxis() },
    }),
  };
}

// The ten top items as horizontal bars, highest first, in the colour of their side.
function topItemsConfig(data) {
  const series = data.series[0];
  return {
    type: 'bar',
    data: {
      labels: data.labels,
      datasets: [{
        label: series.name,
        data: series.cents,
        exact: series.exact,
        backgroundColor: COLORS[series.name],
        borderRadius: 4,
        maxBarThickness: 24,
      }],
    },
    options: baseOptions({
      indexAxis: 'y',
      plugins: {
        legend: { display: false },
        tooltip: { external: externalTooltip },
      },
      scales: {
        x: { ...moneyAxis() },
        y: { ticks: AXIS_TEXT, grid: { display: false } },
      },
    }),
  };
}

// EVE time (UTC) for an axis tick, as `10-16 14:00`.
function eveTick(milliseconds) {
  return new Date(milliseconds).toISOString().slice(5, 16).replace('T', ' ');
}

// Two lines over time on a linear axis of epoch milliseconds (no date adapter needed). `yAxis` is
// the axis of the value: shortened ISK, or whole numbers for order counts.
function lineConfig(data, yAxis) {
  return {
    type: 'line',
    data: {
      datasets: data.series.map((series) => ({
        label: series.name,
        data: series.values.map((value, index) => ({ x: data.times[index], y: value })),
        exact: series.exact,
        borderColor: COLORS[series.name],
        backgroundColor: COLORS[series.name],
        borderWidth: 2,
        borderDash: DASHED.has(series.name) ? [4, 3] : [],
        pointRadius: 0,
        pointHoverRadius: 5,
        pointHitRadius: 12,
        tension: 0,
        spanGaps: false,
      })),
    },
    options: baseOptions({
      interaction: { mode: 'nearest', axis: 'x', intersect: false },
      plugins: {
        legend: { display: true, labels: LEGEND_LABELS },
        tooltip: {
          external: externalTooltip,
          callbacks: { title: (items) => `${data.labels[items[0].dataIndex]} EVE time` },
        },
      },
      scales: {
        x: {
          type: 'linear',
          min: data.times[0],
          max: data.times[data.times.length - 1],
          ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (value) => eveTick(value) },
          grid: GRID_LINES,
        },
        y: yAxis,
      },
    }),
  };
}

// Four areas stacked on one axis of epoch milliseconds: the top edge is the total. The areas are
// separated by a gap in the surface colour, and one tooltip lists every layer and the total.
function stackConfig(data) {
  return {
    type: 'line',
    data: {
      datasets: data.series.map((series) => ({
        label: series.name,
        data: series.values.map((value, index) => ({ x: data.times[index], y: value })),
        exact: series.exact,
        fill: true,
        borderColor: SURFACE,
        backgroundColor: COLORS[series.name],
        borderWidth: 2,
        pointRadius: 0,
        pointHoverRadius: 5,
        pointHitRadius: 12,
        tension: 0,
      })),
    },
    options: baseOptions({
      totals: data.totals,
      interaction: { mode: 'index', axis: 'x', intersect: false },
      plugins: {
        legend: { display: true, labels: LEGEND_LABELS },
        tooltip: {
          external: externalTooltip,
          callbacks: { title: (items) => `${data.labels[items[0].dataIndex]} EVE time` },
        },
      },
      scales: {
        x: {
          type: 'linear',
          min: data.times[0],
          max: data.times[data.times.length - 1],
          ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (value) => eveTick(value) },
          grid: GRID_LINES,
        },
        y: { ...moneyAxis(), stacked: true },
      },
    }),
  };
}

const valueHistoryConfig = (data) => lineConfig(data, moneyAxis());
const wealthHistoryConfig = valueHistoryConfig;
const countHistoryConfig = (data) =>
  lineConfig(data, {
    beginAtZero: true,
    ticks: { ...AXIS_TEXT, maxTicksLimit: 6, precision: 0 },
    grid: GRID_LINES,
  });

// Prices float: a bar of the average from zero would flatten every move, so these axes do not start at zero.
function priceAxis() {
  return { ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (value) => iskShort(value) }, grid: GRID_LINES };
}

const percentAxis = () => ({
  beginAtZero: true,
  ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (value) => `${value}%` },
  grid: GRID_LINES,
});

const rsiAxis = () => ({ min: 0, max: 100, ticks: { ...AXIS_TEXT, maxTicksLimit: 6 }, grid: GRID_LINES });

// Bars over categories (hours, weekdays, past occurrences); a series of kind `line` is drawn over
// them, as S is over the peaks.
function barsConfig(data, yAxis) {
  return {
    type: 'bar',
    data: {
      labels: data.labels,
      datasets: data.series.map((series) => {
        const line = series.kind === 'line';
        return {
          type: line ? 'line' : 'bar',
          label: series.name,
          data: series.values,
          exact: series.exact,
          backgroundColor: COLORS[series.name],
          borderColor: COLORS[series.name],
          borderWidth: line ? 2 : 0,
          borderRadius: line ? 0 : 3,
          maxBarThickness: 24,
          pointRadius: 0,
          pointHoverRadius: 5,
          order: line ? 0 : 1,
        };
      }),
    },
    options: baseOptions({
      interaction: { mode: 'index', intersect: false },
      scales: { x: { ticks: { ...AXIS_TEXT, maxTicksLimit: 12 }, grid: { display: false } }, y: yAxis },
    }),
  };
}

const CHART_KINDS = {
  // The opportunity detail page.
  'trend-price': (data) => lineConfig(data, priceAxis()),
  'reversion-price': (data) => lineConfig(data, priceAxis()),
  'classic-prices': (data) => lineConfig(data, priceAxis()),
  'classic-spread': (data) => lineConfig(data, percentAxis()),
  rsi: (data) => lineConfig(data, rsiAxis()),
  'trend-volume': (data) =>
    barsConfig(data, { beginAtZero: true, ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (v) => iskShort(v * 100) }, grid: GRID_LINES }),
  depth: (data) =>
    barsConfig(data, { beginAtZero: true, ticks: { ...AXIS_TEXT, maxTicksLimit: 6, callback: (v) => iskShort(v * 100) }, grid: GRID_LINES }),
  'spread-hours': (data) => barsConfig(data, percentAxis()),
  'spread-weekdays': (data) => barsConfig(data, percentAxis()),
  peaks: (data) => barsConfig(data, priceAxis()),
  split: splitConfig,
  'top-sell': topItemsConfig,
  'top-buy': topItemsConfig,
  'value-history': valueHistoryConfig,
  'count-history': countHistoryConfig,
  'wealth-stack': stackConfig,
  'wealth-history': wealthHistoryConfig,
};

function drawCharts(root) {
  // A chart whose canvas was swapped out of the page is dead weight: free it.
  for (const chart of Object.values(Chart.instances)) {
    if (!chart.canvas.isConnected) chart.destroy();
  }
  const canvases = [...root.querySelectorAll('[data-chart]')];
  if (root.matches('[data-chart]')) canvases.unshift(root);
  for (const canvas of canvases) {
    Chart.getChart(canvas)?.destroy();
    const build = CHART_KINDS[canvas.dataset.chart];
    if (build) new Chart(canvas, build(JSON.parse(canvas.dataset.series)));
  }
}

document.addEventListener('htmx:load', (event) => {
  if (typeof Chart !== 'undefined' && event.detail.elt instanceof Element) drawCharts(event.detail.elt);
});

// ---- rows that open a page ----
// A table row with a data-href opens that page when clicked anywhere on it. Its item cell also holds a
// real link, so keyboards, middle clicks and "open in a new tab" work without this handler; clicks
// on links and controls inside the row, and clicks with a modifier key, are left to the browser.
document.addEventListener('click', (event) => {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  if (!(event.target instanceof Element) || event.target.closest('a, button, input, select, textarea, label')) return;
  const row = event.target.closest('tr[data-href]');
  if (row) location.assign(row.dataset.href);
});
