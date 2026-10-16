// htmx would inject an inline <style> for indicators, which the CSP forbids.
// The indicator styles live in app.css instead.
htmx.config.includeIndicatorStyles = false;
// No eval: the CSP forbids it, and nothing here needs hx-on or hx-vals javascript.
htmx.config.allowEval = false;

// The character filter is a list of checkboxes, but the URL carries one comma-separated `chars`
// value (and none at all when every character is selected), plus whatever else the page's view
// state holds, only when it is not the default: the orders page's sorting, the dashboard's range.
// A plain form would send `chars` once per box and every field, so the request URL is built here,
// for the page named by the form's own `hx-get`. Nothing here evaluates code: the CSP forbids it.
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
  const value = (name) => form.querySelector(`input[name="${name}"]`)?.value;
  for (const [sort, dir] of [['sort', 'dir'], ['bsort', 'bdir']]) {
    if (value(sort) && !(value(sort) === 'item' && value(dir) === 'asc')) {
      query.push(`${sort}=${encodeURIComponent(value(sort))}`, `${dir}=${encodeURIComponent(value(dir))}`);
    }
  }
  if (value('range') && value('range') !== '7d') {
    query.push(`range=${encodeURIComponent(value('range'))}`);
  }

  for (const key of Object.keys(event.detail.parameters)) delete event.detail.parameters[key];
  event.detail.path = query.length > 0 ? `${path}?${query.join('&')}` : path;
});

// ---- charts ----
// Chart.js 4.5.1, vendored as static/chart.umd.min.js (MIT, see chart.js.LICENSE.md),
// SHA-256 48444a82d4edcb5bec0f1965faacdde18d9c17db3063d042abada2f705c9f54a.
// It draws on <canvas>, uses no eval and injects no <style>, so the CSP stays as it is. Every
// `<canvas data-chart="kind" data-series="json">` is drawn from its HTML-escaped attribute, and
// redrawn whenever htmx swaps a fresh copy in.
const SELL_COLOR = '#3987e5';
const BUY_COLOR = '#d95926';
const INK = '#c3c2b7';
const GRID = '#2c2c2a';

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
const GRID_LINES = { color: GRID };

function moneyAxis() {
  return {
    beginAtZero: true,
    ticks: { ...AXIS_TEXT, callback: (value) => iskShort(value) },
    grid: GRID_LINES,
  };
}

function baseOptions(extra) {
  return {
    animation: false,
    responsive: true,
    maintainAspectRatio: false,
    interaction: { mode: 'nearest', intersect: true },
    plugins: {
      legend: { display: true, labels: { color: INK } },
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
};

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
      scales: { x: { ticks: AXIS_TEXT, grid: { display: false } }, y: moneyAxis() },
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
        pointRadius: 0,
        pointHoverRadius: 5,
        pointHitRadius: 12,
        tension: 0,
      })),
    },
    options: baseOptions({
      interaction: { mode: 'nearest', axis: 'x', intersect: false },
      plugins: {
        legend: { display: true, labels: { color: INK } },
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
          ticks: { ...AXIS_TEXT, maxTicksLimit: 8, callback: (value) => eveTick(value) },
          grid: GRID_LINES,
        },
        y: yAxis,
      },
    }),
  };
}

const valueHistoryConfig = (data) => lineConfig(data, moneyAxis());
const countHistoryConfig = (data) =>
  lineConfig(data, {
    beginAtZero: true,
    ticks: { ...AXIS_TEXT, precision: 0 },
    grid: GRID_LINES,
  });

const CHART_KINDS = {
  split: splitConfig,
  'top-sell': topItemsConfig,
  'top-buy': topItemsConfig,
  'value-history': valueHistoryConfig,
  'count-history': countHistoryConfig,
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
