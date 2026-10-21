import type { Page } from '@playwright/test';
import { chartData, chartDrawn } from '../support/charts';
import { horizontalOverflow } from '../support/layout';
import { aliceTrained, bookOf, calm, crossed, fill, history } from '../support/opportunities';
import {
  expect, feePoll, liveSettled, loginWith, openCharacters, pollCharacter, stationBook, stationRollups, stationSeed, stubControl, stubMarket,
  stubMarketOrder, stubTradeSkills, test, type SeedSnapshot,
} from '../support/fixtures';

const STATION = 'Jita IV - Moon 4 - Caldari Navy Assembly Plant';

test('open the opportunities page from the orders page with a narrowed selection', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  const nav = page.locator('#main-nav');
  await nav.getByRole('link', { name: 'Orders' }).click();
  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();
  await expect(page).toHaveURL(/\/orders\/sell\?chars=90000002$/);

  await nav.getByRole('link', { name: 'Opportunities' }).click();

  await expect(page).toHaveURL(/\/opportunities\/expansion\?chars=90000002$/);
  await expect(nav.getByRole('link', { name: 'Opportunities' })).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('.page-header')).toContainText(STATION);
  const menu = page.locator('nav.sub-menu');
  await expect(menu.getByRole('link')).toHaveText(['Recurring spread expansion', 'Classic spread trade', 'Mean reversion', 'Trend']);
  await expect(menu.getByRole('link', { name: 'Recurring spread expansion' })).toHaveAttribute('aria-current', 'page');
  await expect(page.getByRole('heading', { name: 'Recurring spread expansion' })).toBeVisible();
  await expect(page.locator('#freshness')).toContainText('Waiting for first refresh');
  await expect(page.locator('#freshness')).toContainText('Stale');
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).not.toBeChecked();
  await expect(page.getByRole('checkbox', { name: 'Bob Bravo' })).toBeChecked();
  // The selection goes on to the other pages.
  await nav.getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\/sell\?chars=90000002$/);
});

test('a book refresh shows its time in the freshness and clears the stale mark', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.locator('#main-nav').getByRole('link', { name: 'Opportunities' }).click();
  const freshness = page.locator('#freshness');
  await expect(freshness).toContainText('Stale');

  await stubMarket([
    stubMarketOrder(1, { type: 34, price: 100, buy: true, range: 'region' }),
    stubMarketOrder(2, { type: 34, price: 110 }),
  ]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');

  await expect(freshness).toContainText('Last refresh:');
  await expect(freshness).not.toContainText('Stale');
  await expect(freshness).not.toContainText('Waiting for first refresh');
});

// ---- the tables ----

test('a classic row shows the sell price one tick below the best sell and the current best buy', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');

  await fill(page, bookOf(34, 100, 120, 1), calm(34));

  const classic = page.locator('#classic');
  const row = classic.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="item"]')).toContainText('Tritanium');
  await expect(row.locator('[data-col="buy"]')).toHaveText('100.00');
  await expect(row.locator('[data-col="sell"]')).toHaveText('119.90');
  await expect(classic.locator('th abbr', { hasText: /^B$/ })).toHaveAttribute('title', /^Current best buy, not a suggested price, except in a dip row/);
  // Alice's fees at Jita 4-4: c = 101.41, r = 119.90 * 0.95215, so a margin of 12.58%.
  await expect(row.locator('[data-col="margin"]')).toHaveText('12.58%');
  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.locator('#fee-note')).toContainText('Broker fee 1.41%');
});

test('sorting the trend table by volatility keeps the classic sorting and puts both in the URL', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Pyerite jumped by 20% and Tritanium by 10%: Pyerite is the more volatile.
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();

  const menu = page.locator('nav.sub-menu');
  const first = () => page.locator('tbody tr').first();
  await page.locator('#classic').getByRole('link', { name: 'Item' }).click();
  await expect(page).toHaveURL(/cs=item&cd=asc/);
  await expect(first()).toContainText('Pyerite');

  await menu.getByRole('link', { name: 'Trend' }).click();
  await expect(page).toHaveURL(/\/opportunities\/trend\?cs=item&cd=asc$/);
  await expect(page.locator('#trend tbody tr')).toHaveCount(2);
  await page.locator('#trend').getByRole('link', { name: 'σ' }).click();
  await expect(page).toHaveURL(/cs=item&cd=asc/);
  await expect(page).toHaveURL(/ts=sigma&td=desc/);
  await expect(first()).toContainText('Pyerite');

  await page.locator('#trend').getByRole('link', { name: 'σ' }).click();
  await expect(page).toHaveURL(/ts=sigma&td=asc/);
  await expect(first()).toContainText('Tritanium');

  // The classic table kept its own sorting, which the URL still holds.
  await menu.getByRole('link', { name: 'Classic spread trade' }).click();
  await expect(page).toHaveURL(/\/opportunities\/classic\?cs=item&cd=asc&ts=sigma&td=asc$/);
  await expect(page.locator('#classic th[aria-sort="ascending"]')).toContainText('Item');
  await expect(first()).toContainText('Pyerite');
});

test('the trend headers are short labels with their full names on hover, and still sort', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/trend');
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();

  const trend = page.locator('#trend');
  const labels = ['Item', 'B', 'S', 'Net', 'm', 'ISK/d', 'SMA5', 'SMA20', 'Cross', 'V7', 'V30', 'σ', 'T_max', 'T_fill', 'V_min', 'n b/s'];
  // The sorted column adds its arrow.
  await expect(trend.locator('thead th')).toHaveText(labels.map((l) => new RegExp(`^${l}( [▲▼])?$`)));
  await expect(trend.locator('thead th').first().locator('abbr')).toHaveCount(0);
  await expect(trend.locator('thead abbr')).toHaveCount(15);
  await expect(trend.locator('thead abbr', { hasText: /^B$/ })).toHaveAttribute('title', /not a suggested price/);

  await trend.getByRole('link', { name: 'σ' }).click();
  await expect(page).toHaveURL(/ts=sigma&td=desc/);
  await expect(trend.locator('th[aria-sort="descending"]')).toContainText('σ');
  await expect(page.locator('tbody tr').first()).toContainText('Pyerite');
});

test('an item the account sells at the station is marked as an own sell', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(36, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...calm(34), ...calm(36)] })).startsWith('200 ')).toBe(true);
  await page.reload();

  const classic = page.locator('#classic');
  // Alice sells Tritanium in Jita 4-4; Mexallon is not hers.
  await expect(classic.locator('tr[data-type-id="34"] [data-col="item"]')).toContainText('own sell');
  await expect(classic.locator('tr[data-type-id="36"] [data-col="item"]')).not.toContainText('own');
});

// ---- recurring expansion and mean reversion ----

/** The minutes between now and `hour`:30 UTC, `daysBack` days ago. */
function minutesBack(daysBack: number, hour: number): number {
  const now = new Date();
  const at = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - daysBack, hour, 30);
  return Math.round((now.getTime() - at) / 60_000);
}

/** 14 days of hourly snapshots at :30 with a spread of 5% (100 / 105); `jump` gives `[buy, sell]` for other hours. */
function hourly(type: number, jump: (back: number, hour: number) => [number, number] | undefined): SeedSnapshot[] {
  const snapshots: SeedSnapshot[] = [];
  for (let back = 1; back <= 14; back++) {
    for (let hour = 0; hour < 24; hour++) {
      const [buy, sell] = jump(back, hour) ?? [100, 105];
      snapshots.push({ type_id: type, minutes_back: minutesBack(back, hour), best_buy: buy, best_sell: sell });
    }
  }
  return snapshots;
}

/** A best sell of 134 + the days back at 18:30 on ten days (not yesterday, which may not be complete yet). */
const evening = (back: number, hour: number): [number, number] | undefined =>
  hour === 18 && back >= 2 && back <= 11 ? [100, 134 + back] : undefined;

/** A best buy of 80 at 11:30 on ten days (not yesterday). */
const morningDip = (back: number, hour: number): [number, number] | undefined =>
  hour === 11 && back >= 2 && back <= 11 ? [80, 105] : undefined;

test('an evening spread expansion that recurred on most days is listed as a daily spike window', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');

  // 14 days of hourly snapshots with a spread of 5%; at 18:30 UTC the best sell peaked at 136 to 145
  // on ten of the days.
  const snapshots = hourly(34, evening);
  expect(await stationSeed(page, { snapshots, history: calm(34) })).toBe('200 336 snapshots, 14 history days');
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();

  const expansion = page.locator('#expansion');
  await expect(expansion).not.toContainText('Collecting hourly data');
  const row = expansion.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="window"]')).toHaveText('daily 18:00');
  await expect(row.locator('[data-col="direction"]')).toHaveText('Spike');
  await expect(row.locator('[data-col="buy"]')).toHaveText('100.00');
  // The peaks are 1.295 to 1.381 times the median best sell of 105.00 (136 to 145 ISK): their 25th
  // percentile is 138.25, and S is one tick below it.
  await expect(row.locator('[data-col="sell"]')).toHaveText('138.20');
  await expect(row.locator('[data-col="window-volume"]')).not.toBeEmpty();
  await expect(row.locator('[data-col="next"]')).toContainText(/\d{4}-\d{2}-\d{2} 18:00/);
});

test('a daily dip is listed with its predicted buy price, one tick below the best sell as its sell price', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { snapshots: hourly(34, morningDip), history: calm(34) })).startsWith('200 ')).toBe(true);
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();

  const expansion = page.locator('#expansion');
  const row = expansion.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="window"]')).toHaveText('daily 11:00');
  await expect(row.locator('[data-col="direction"]')).toHaveText('Dip');
  // The troughs are 0.80 of the median best buy of 100.00: one tick above 80.00 is 80.01. S is one
  // tick below the best sell of 105.00.
  await expect(row.locator('[data-col="buy"]')).toHaveText('80.01');
  await expect(row.locator('[data-col="sell"]')).toHaveText('104.90');
  await expect(expansion.locator('thead abbr', { hasText: /^B$/ })).toHaveAttribute('title', /predicted buy price/);
});

test('an item with a constant spread and one that jumps in 15% of its hours are not listed', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket([...bookOf(34, 100, 110, 1), ...bookOf(35, 100, 105, 3), ...bookOf(36, 100, 105, 5)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // 34: a spread of 10% in every hour. 35: four jumpy hours a day (17%), 18:30 among them on ten
  // days. 36: the evening spike alone.
  const constant = hourly(34, () => [100, 110]);
  const jumpy = hourly(35, (back, hour) => ([2, 7, 13].includes(hour) || evening(back, hour) ? [100, 140] : undefined));
  const calmEvening = hourly(36, evening);
  const history = [...calm(34), ...calm(35), ...calm(36)];
  expect((await stationSeed(page, { snapshots: [...constant, ...jumpy, ...calmEvening], history })).startsWith('200 ')).toBe(true);
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();

  const rows = page.locator('#expansion tr[data-type-id]');
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute('data-type-id', '36');
});

test('with three days of snapshots there is no note above the expansion table and the footer counts the days', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Two seeded days and the snapshot the book refresh just recorded make three days.
  const snapshots = [1, 2].map((back) => ({ type_id: 34, minutes_back: minutesBack(back, 12), best_buy: 100, best_sell: 105 }));
  expect((await stationSeed(page, { snapshots })).startsWith('200 ')).toBe(true);
  await page.reload();

  await expect(page.locator('#expansion')).not.toContainText('Collecting');
  await expect(page.locator('.note')).toHaveCount(0);
  await expect(page.locator('#freshness')).toContainText('3 days of own snapshots');
});

test('an oversold item is listed in the mean reversion table with the SMA 20 rounded down as its sell price', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/reversion');
  await stubMarket(bookOf(34, 120, 125, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // 19 days at 152 and then 119.40: the SMA 20 is 150.37 and the last day is below the lower band.
  const seed = history(34, 25, (back) => (back === 1 ? 119.4 : 152), () => 100_000_000);
  expect((await stationSeed(page, { history: seed })).startsWith('200 ')).toBe(true);
  await page.reload();

  const reversion = page.locator('#reversion');
  const row = reversion.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="sell"]')).toHaveText('150.30');
  await expect(row.locator('[data-col="pb"]')).toHaveText(/^-\d/);
  await expect(row.locator('[data-col="rsi"]')).toHaveText(/^\d+\.\d$/);
  // Buying at 120 and selling at 124.90 earns nothing after the fees: the classic table skips it.
  await page.locator('nav.sub-menu').getByRole('link', { name: 'Classic spread trade' }).click();
  await expect(page).toHaveURL(/\/opportunities\/classic$/);
  await expect(page.locator('#classic')).toBeVisible();
  await expect(page.locator('#classic tr[data-type-id="34"]')).toHaveCount(0);
});


// ---- the filters ----

const filters = (page: Page) => page.locator('#opportunity-filters');

test('a first visit shows the default filters', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/opportunities/classic');

  await expect(filters(page).getByLabel('Minimum margin (%)')).toHaveValue('5');
  await expect(filters(page).getByLabel('Minimum ISK/day')).toHaveValue('5000000');
  await expect(filters(page).getByLabel('Item name')).toHaveValue('');
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('100000000');
});

test('raising the capital removes an item that can no longer fill in time and stays after signing in again', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Tritanium trades 10 million a day, Pyerite 100 million.
  expect((await stationSeed(page, { history: [...calm(34, 10_000_000), ...calm(35, 100_000_000)] })).startsWith('200 ')).toBe(true);
  await page.reload();
  const classic = page.locator('#classic');
  await expect(classic.locator('tbody tr')).toHaveCount(2);

  await filters(page).getByLabel('Capital per item (ISK)').fill('2000000000');
  await filters(page).getByLabel('Item name').press('Tab');

  await expect(classic.locator('tbody tr')).toHaveCount(1);
  await expect(classic.locator('tr[data-type-id="35"]')).toHaveCount(1);
  await expect(classic.locator('tr[data-type-id="34"]')).toHaveCount(0);

  await page.locator('#main-nav').getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await loginWith(page, 'Alice Alpha');
  await page.goto('/opportunities/classic');
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('2000000000');
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);
});

test('the item name search narrows every table', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator('#classic tbody tr')).toHaveCount(2);

  await filters(page).getByLabel('Item name').fill('TRIT');
  await filters(page).getByLabel('Item name').press('Tab');
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);
  await expect(page.locator('#classic tbody tr')).toContainText('Tritanium');

  // The filters are shared: the trend page applies the saved search too.
  await page.locator('nav.sub-menu').getByRole('link', { name: 'Trend' }).click();
  await expect(page).toHaveURL(/\/opportunities\/trend$/);
  await expect(filters(page).getByLabel('Item name')).toHaveValue('TRIT');
  await expect(page.locator('#trend tbody tr')).toHaveCount(1);
  await expect(page.locator('#trend tbody tr')).toContainText('Tritanium');
});

test('a capital of zero is rejected with a message and the tables stay as they were', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: calm(34) })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);

  await filters(page).getByLabel('Capital per item (ISK)').fill('0');
  await filters(page).getByLabel('Item name').press('Tab');

  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveAttribute('aria-invalid', 'true');
  await expect(filters(page).locator('small')).toContainText('capital');
  await expect(filters(page).locator('small')).toContainText('positive');
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('0');
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);
  // Nothing was stored: a reload shows the default again.
  await page.reload();
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('100000000');
});

// ---- the detail page ----

/** Tritanium in the classic and the trend table (a fresh crossing with a rising volume), on the page of `table`. */
async function withTritanium(page: Page, table: 'classic' | 'trend' = 'classic') {
  await aliceTrained(page);
  await page.goto(`/opportunities/${table}`);
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: crossed(34, 1.1) })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator(`#${table} tr[data-type-id="34"]`)).toHaveCount(1);
}

/** Tritanium in the classic table with three earlier hours of snapshots, so that every classic chart has data. */
async function withClassicCharts(page: Page) {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  const snapshots = [300, 240, 180].map((minutes_back, i) => ({
    type_id: 34, minutes_back, best_buy: 100, best_sell: 110 + i * 10,
  }));
  expect((await stationSeed(page, { snapshots, history: calm(34) })).startsWith('200 ')).toBe(true);
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();
  await expect(page.locator('#classic tr[data-type-id="34"]')).toHaveCount(1);
}

test('a row click navigates to the detail page of that table and item, with no floating window', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await withTritanium(page);

  await page.locator('#classic tr[data-type-id="34"]').click();

  await expect(page).toHaveURL(/\/opportunities\/classic\/34$/);
  await expect(page.locator('#opportunity-detail')).toHaveCount(0);
  await expect(page.locator('[role="dialog"], aside:not(.sidebar)')).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(['Tritanium']);
  await expect(page.locator('#figures h2')).toContainText('Tritanium');
  await expect(page.locator('#figures h2')).toContainText('Classic spread trade');
  await expect(page.locator('#main-nav a[aria-current="page"]')).toHaveAttribute('aria-label', 'Opportunities');
  await expect(page.locator('#filter-box #character-filter')).toBeVisible();
  await expect(page.locator('#filter-box #fee-filter').getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.locator('[data-figure="buy"] [data-value]')).toHaveText('100.00');
  await expect(page.locator('[data-figure="sell"] [data-value]')).toHaveText('129.90');
  // A card shows its computation with this item's numbers; the formula is in the legend.
  await expect(page.locator('[data-figure="margin"] .detail')).toHaveText(/ \/ 101\.41$/);
});

test('a classic detail page shows its figures as cards in six groups with their detail lines', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withTritanium(page);
  await page.goto('/opportunities/classic/34');

  const figures = page.locator('#figures');
  await expect(figures.locator('h3')).toHaveText(['Fees', 'Prices', 'Margin', 'Size & liquidity', 'Timing & risk', 'Result']);
  await expect(figures.locator('table')).toHaveCount(0);
  const cost = figures.locator('[data-figure="cost"]');
  await expect(cost.locator('.name abbr')).toHaveText('c');
  await expect(cost.locator('.name abbr')).toHaveAttribute('title', 'Cost per unit');
  await expect(cost.locator('[data-value]')).toHaveText('101.41');
  await expect(cost.locator('.detail')).toHaveText('100.00 × 1.0141');
  await expect(figures.locator('[data-figure="broker"] [data-value]')).toHaveText('1.41%');
  await expect(figures.locator('[data-figure="broker"] .detail')).toHaveCount(0);
  // The detail line is dimmed, like the names.
  const [detail, value] = await cost.evaluate((card) => [
    getComputedStyle(card.querySelector('.detail')!).color,
    getComputedStyle(card.querySelector('[data-value]')!).color,
  ]);
  expect(detail).not.toBe(value);
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
});

test('a row of the trend table opens the trend page, and the item name is a real link', async ({ page }) => {
  await withTritanium(page, 'trend');
  const link = page.locator('#trend tr[data-type-id="34"] [data-col="item"] a');
  await expect(link).toHaveAttribute('href', '/opportunities/trend/34');

  await link.click();

  await expect(page).toHaveURL(/\/opportunities\/trend\/34$/);
  await expect(page.locator('#figures h2')).toContainText('Trend');
  await expect(page.locator('#figures h2')).not.toContainText('Classic spread trade');
});

test('the detail page has a sub-menu with a way back that keeps the sorting, the figures and one entry per chart', async ({ page }) => {
  await withClassicCharts(page);
  await page.locator('#classic').getByRole('link', { name: 'Item' }).click();
  await expect(page).toHaveURL(/cs=item&cd=asc/);
  await page.locator('#classic tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/classic\/34\?.*cs=item&cd=asc/);
  await liveSettled(page);

  const menu = page.locator('.page > nav.sub-menu#section-nav');
  await expect(menu.getByRole('link')).toHaveText([
    '← Classic spread trade', 'Figures', 'Legend', 'Best buy and best sell', 'Spread', 'Units within 1% of the best price',
  ]);
  await expect(menu.getByRole('link', { name: 'Classic spread trade' })).toHaveAttribute('href', /^\/opportunities\/classic\?.*cs=item&cd=asc/);
  expect((await menu.boundingBox())!.height).toBeLessThanOrEqual(48);

  await menu.getByRole('link', { name: 'Spread', exact: true }).click();
  await expect(page).toHaveURL(/#classic-spread$/);
  await menu.getByRole('link', { name: 'Classic spread trade' }).click();
  await expect(page).toHaveURL(/\/opportunities\/classic\?.*cs=item&cd=asc/);
  await expect(page.locator('#classic th[aria-sort="ascending"]')).toContainText('Item');
});

test('the charts of the detail page sit on rows of their own', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await withClassicCharts(page);
  await page.goto('/opportunities/classic/34');
  await chartDrawn(page, 'classic-prices');

  await expect(async () => {
    const figures = (await page.locator('#figures').boundingBox())!;
    let above = figures;
    for (const id of ['classic-prices', 'classic-spread', 'depth']) {
      const row = (await page.locator(`#${id}`).boundingBox())!;
      expect(Math.abs(row.width - figures.width), `${id} as wide as the figures`).toBeLessThanOrEqual(2);
      expect(row.y, `${id} below the previous row`).toBeGreaterThanOrEqual(above.y + above.height - 1);
      above = row;
    }
  }).toPass();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(['Tritanium']);
});

test('deselecting a character recomputes the fees on the detail page, and only the content is swapped', async ({ page }) => {
  await withTritanium(page);
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await page.goto('/opportunities/classic');
  await page.locator('#classic tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/classic\/34$/);
  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();
  // Alice's skills give b 1.41%: c = 100 * 1.0141.
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('101.41');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await page.locator('#filter-box').getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();

  await expect(page).toHaveURL(/\/opportunities\/classic\/34\?chars=90000002$/);
  await expect(page.locator('#fee-note')).toContainText('Base rates');
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('103.00');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('the detail page keeps the item and says so when it no longer qualifies', async ({ page }) => {
  await withTritanium(page);
  await page.locator('#classic tr[data-type-id="34"]').click();
  await expect(page.locator('#figures')).toBeVisible();
  await expect(page.locator('#opportunity-detail-unlisted')).toHaveCount(0);

  // The capital grows until the item can no longer fill in time.
  await page.goBack();
  await page.locator('#opportunity-filters').getByLabel('Capital per item (ISK)').fill('900000000000');
  await page.locator('#opportunity-filters').getByLabel('Item name').press('Tab');
  await expect(page.locator('#classic tr[data-type-id="34"]')).toHaveCount(0);
  await page.goto('/opportunities/classic/34');

  await expect(page.locator('#opportunity-detail-unlisted')).toContainText('No longer listed');
  await expect(page.locator('[data-figure="sell"] [data-value]')).toHaveText('129.90');
});

// ---- the charts of the detail page ----

/** The y values of a line chart's dataset (its points are `{ x, y }`). */
const ys = (points: { y: number | null }[]) => points.map((p) => p.y);

test('the classic page draws the best prices, the spread and the depth of the book', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Three earlier hours with the best sell rising from 110 to 130.
  const snapshots = [300, 240, 180].map((minutes_back, i) => ({
    type_id: 34, minutes_back, best_buy: 100, best_sell: 110 + i * 10,
  }));
  expect((await stationSeed(page, { snapshots, history: calm(34) })).startsWith('200 ')).toBe(true);
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();

  await page.locator('#classic tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/classic\/34$/);

  await chartDrawn(page, 'classic-prices');
  const [buy, sell] = await chartData(page, 'classic-prices');
  // The three seeded hours, and the hour of the book refresh with the best sell of 130.
  expect(ys(sell).slice(0, 3)).toEqual([11000, 12000, 13000]);
  expect(new Set(ys(buy))).toEqual(new Set([10000]));
  const [spread] = await chartData(page, 'classic-spread');
  expect(ys(spread).slice(0, 3)).toEqual([10, 20, 30]);
  const [units] = await chartData(page, 'depth');
  expect(units).toEqual([3, 3]);
  await expect(page.locator('.chart-row h2')).toContainText(['Best buy and best sell', 'Spread', 'Units within 1% of the best price']);
});

test('the mean reversion page draws the average with its bands and the RSI', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/reversion');
  await stubMarket(bookOf(34, 120, 125, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  const seed = history(34, 25, (back) => (back === 1 ? 119.4 : 152), () => 100_000_000);
  expect((await stationSeed(page, { history: seed })).startsWith('200 ')).toBe(true);
  await page.reload();

  await page.locator('#reversion tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/reversion\/34$/);

  await chartDrawn(page, 'reversion-price');
  const [average, sma, upper, lower] = await chartData(page, 'reversion-price');
  expect(average).toHaveLength(25);
  // The SMA 20 starts on the twentieth day and ends at 150.37 ISK, with the last average below the lower band.
  expect(ys(sma).filter((y) => y !== null)).toHaveLength(6);
  expect(ys(sma)[24]).toBeCloseTo(15037, 3);
  expect(ys(upper)[24]!).toBeGreaterThan(ys(sma)[24]!);
  expect(ys(average)[24]!).toBeLessThan(ys(lower)[24]!);
  const [rsi] = await chartData(page, 'rsi');
  expect(ys(rsi)[24]!).toBeLessThan(30);
});

test('the trend page draws the averages with the volume', async ({ page }) => {
  await withTritanium(page, 'trend');

  await page.locator('#trend tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/trend\/34$/);

  await chartDrawn(page, 'trend-price');
  const [average, sma5, sma20] = await chartData(page, 'trend-price');
  expect(average).toHaveLength(30);
  // 27 flat days at 100 and then 110: the SMA 5 above the SMA 20 on the last day.
  expect(ys(sma5)[29]!).toBeGreaterThan(ys(sma20)[29]!);
  expect(ys(sma20)[29]).toBeCloseTo(10150, 3);
  const [volume] = await chartData(page, 'trend-volume');
  expect(volume).toHaveLength(30);
  expect(volume[29]).toBe(200_000_000);
  expect(volume[0]).toBe(100_000_000);
});

test('the expansion page draws the spread by hour and weekday, the peaks with S and lists the recurring windows', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { snapshots: hourly(34, evening), history: calm(34) })).startsWith('200 ')).toBe(true);
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();

  await page.locator('#expansion tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/expansion\/34$/);

  await chartDrawn(page, 'spread-hours');
  const [maxSpread, avgSpread] = await chartData(page, 'spread-hours');
  expect(maxSpread).toHaveLength(24);
  // The evening spread is far wider than the 5% of the other hours.
  expect(maxSpread[18]).toBeGreaterThan(maxSpread[12] * 3);
  expect(avgSpread[12]).toBeCloseTo(5, 1);
  const [weekdays] = await chartData(page, 'spread-weekdays');
  expect(weekdays).toHaveLength(7);
  const [peak, sell] = await chartData(page, 'peaks');
  expect(peak).toHaveLength(10);
  expect(new Set(sell)).toEqual(new Set([13820]));
  await expect(page.locator('#windows h2')).toContainText('Recurring windows');
  const windows = page.locator('#windows tbody tr');
  await expect(windows).toHaveCount(1);
  await expect(windows.first()).toContainText('daily 18:00');
  await expect(windows.first()).toContainText('Spike');
  await expect(windows.first()).toHaveClass(/chosen/);
  await expect(windows.first()).toContainText(/\d{4}-\d{2}-\d{2} 18:00/);
  await expect(page.locator('#section-nav').getByRole('link', { name: 'Recurring windows' })).toHaveAttribute('href', '#windows');
});

test('the expansion page lists both windows of an item and draws the troughs of the dip the row uses', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // A deep dip, at 60.00: buying at 60.01 beats the spike's margin of about 30%.
  const deepDip = (back: number, hour: number): [number, number] | undefined => (morningDip(back, hour) ? [60, 105] : undefined);
  const both = hourly(34, (back, hour) => evening(back, hour) ?? deepDip(back, hour));
  expect((await stationSeed(page, { snapshots: both, history: calm(34) })).startsWith('200 ')).toBe(true);
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();
  // The dip's margin is larger than the spike's, so the dip is the row's window.
  await expect(page.locator('#expansion tr[data-type-id="34"] [data-col="direction"]')).toHaveText('Dip');

  await page.locator('#expansion tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/expansion\/34$/);

  const windows = page.locator('#windows tbody tr');
  await expect(windows).toHaveCount(2);
  await expect(page.locator('#windows')).toContainText('daily 11:00');
  await expect(page.locator('#windows')).toContainText('daily 18:00');
  await expect(page.locator('#windows tr.chosen')).toHaveCount(1);
  await expect(page.locator('#windows tr.chosen')).toContainText('daily 11:00');
  await expect(page.locator('#peaks h2')).toContainText('Past troughs of the window, with B');
  const [trough, buy] = await chartData(page, 'peaks');
  expect(trough).toHaveLength(10);
  expect(new Set(trough)).toEqual(new Set([6000]));
  expect(new Set(buy)).toEqual(new Set([6001]));
  // B is the predicted buy price, so the current best buy has a card of its own.
  await expect(page.locator('[data-figure="best-buy"] [data-value]')).toHaveText('100.00');
  await expect(page.locator('[data-figure="buy"] [data-value]')).toHaveText('60.01');
});

test('the expansion explainer covers dips and spikes, its playbook has steps for both, and its example keeps S at 12,520.00', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await page.locator('details > summary', { hasText: 'How to read this table' }).click();
  const card = page.locator('article.explainer');
  await expect(card.locator('h3')).toHaveText(['Legend', 'Signal', 'Shared figures', 'Worked example', 'Order playbook']);
  await expect(card).toContainText('at least twice the median');
  await expect(card).toContainText('more than 10% of its hours');
  await expect(card.locator('table.example')).toContainText('12,520.00');
  await expect(card.locator('table.example')).toContainText('Spike');
  await expect(card.locator('ol')).toContainText('For a spike row');
  await expect(card.locator('ol')).toContainText('For a dip row');
  await expect(card.locator('ol')).toContainText('at B itself, not one tick above');
});

test('a rejected value marks its own input invalid with the message right below it, and a good value clears it', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/trend');
  const form = page.locator('#opportunity-filters');
  const margin = form.getByLabel('Minimum margin (%)');

  await margin.fill('-1');
  await margin.press('Tab');

  await expect(margin).toHaveAttribute('aria-invalid', 'true');
  await expect(form).not.toHaveClass(/htmx-(added|settling)/); // htmx wires a swapped-in form when it settles
  const message = form.locator('small');
  await expect(message).toHaveCount(1);
  await expect(message).toContainText('minimum margin');
  await expect(margin).toHaveAttribute('aria-describedby', (await message.getAttribute('id'))!);
  const input = (await margin.boundingBox())!;
  const text = (await message.boundingBox())!;
  expect(text.y).toBeGreaterThanOrEqual(input.y + input.height - 1);
  expect(Math.abs(text.x - input.x)).toBeLessThanOrEqual(2);
  for (const other of ['Minimum ISK/day', 'Item name', 'Capital per item (ISK)']) {
    await expect(form.getByLabel(other), other).not.toHaveAttribute('aria-invalid', 'true');
  }
  await expect(margin).toHaveValue('-1');

  await margin.fill('5');
  await margin.press('Tab');

  await expect(form.locator('[aria-invalid="true"]')).toHaveCount(0);
  await expect(form.locator('small')).toHaveCount(0);
  await expect(margin).toHaveValue('5');
});

test('text typed into another field while a save is in flight is kept', async ({ page }) => {
  await aliceTrained(page);
  // The stream's first refresh would abort a save in flight (hx-sync); let it come first.
  const opened = page.waitForResponse((r) => r.url().includes('/opportunities/trend/content'));
  await page.goto('/opportunities/trend');
  await opened;
  await page.waitForTimeout(600);
  const form = page.locator('#opportunity-filters');
  const margin = form.getByLabel('Minimum margin (%)');
  const search = form.getByLabel('Item name');

  // First a rejected value, so the clean state has something to clear.
  await margin.fill('-1');
  await margin.press('Tab');
  await expect(margin).toHaveAttribute('aria-invalid', 'true');
  await expect(form).not.toHaveClass(/htmx-(added|settling)/); // htmx wires a swapped-in form when it settles

  // Hold the next save back while the user types into another field.
  let release!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route('**/opportunities/settings', async (route) => {
    await held;
    await route.continue();
  });
  const saved = page.waitForResponse((r) => r.url().includes('/opportunities/settings'));
  await margin.fill('5');
  await margin.press('Tab');
  await search.fill('pyer');
  release();
  await saved;

  await expect(form.locator('[aria-invalid="true"]')).toHaveCount(0);
  await expect(form.locator('small')).toHaveCount(0);
  await expect(search).toHaveValue('pyer');
});

// ---- the fee character ----

/** Tritanium with Alice (trained, b 1.41%) and Bob (fees unknown: the base rates 3% and 7.5%) on one account. */
async function withTwoCharacters(page: Page, table: 'classic' | 'trend' = 'classic') {
  await withTritanium(page, table);
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await page.goto(`/opportunities/${table}`);
}

test('with two characters the one with the lowest margin is checked, and the groups share their top edge with the fees', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await withTwoCharacters(page);

  const fees = page.locator('#fee-filter');
  await expect(fees.getByRole('group', { name: 'Fees' })).toBeVisible();
  await expect(fees.getByRole('radio')).toHaveCount(2);
  await expect(fees.getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();
  await expect(fees.getByRole('radio', { name: 'Bob Bravo' })).not.toBeChecked();
  const top = async (selector: string) => (await page.locator(selector).first().boundingBox())!.y;
  const margin = (await page.locator('#opportunity-filters').getByText('Minimum margin (%)').boundingBox())!.y;
  expect(Math.abs((await top('#fee-filter legend')) - margin)).toBeLessThanOrEqual(1);
  expect(Math.abs((await top('#character-filter legend')) - margin)).toBeLessThanOrEqual(1);
});

test('choosing another character recomputes the table and the helper text, and the choice is kept', async ({ page }) => {
  await withTwoCharacters(page);
  const row = page.locator('#classic tr[data-type-id="34"]');
  await expect(row.locator('[data-col="margin"]')).toHaveText('21.96%');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await page.locator('#fee-filter').getByRole('radio', { name: 'Bob Bravo' }).check();

  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();
  await expect(page.locator('#fee-note')).toContainText('Base rates');
  // c = 100 * 1.03 = 103, r = 129.90 * 0.895 = 116.2605: a margin of 12.87%.
  await expect(row.locator('[data-col="margin"]')).toHaveText('12.87%');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');

  // Another table page and a fresh load use it as well.
  await page.goto('/opportunities/trend');
  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();
  await page.goto('/opportunities/classic');
  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();
  await expect(page.locator('#classic tr[data-type-id="34"] [data-col="margin"]')).toHaveText('12.87%');
});

test('the chosen character gives way when it is deselected, and comes back when it is selected again', async ({ page }) => {
  await withTwoCharacters(page);
  const fees = page.locator('#fee-filter');
  await fees.getByRole('radio', { name: 'Bob Bravo' }).check();
  await expect(fees.getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();

  await page.locator('#character-filter').getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(fees.getByRole('radio')).toHaveCount(1);
  await expect(fees.getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();

  await page.locator('#character-filter').getByRole('checkbox', { name: 'Bob Bravo' }).check();
  await expect(fees.getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();
});

test('the fee character is chosen on the detail page too, which keeps its address', async ({ page }) => {
  await withTwoCharacters(page);
  await page.locator('#classic tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/classic\/34$/);
  const fees = page.locator('#fee-filter');
  await expect(fees.getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('101.41');

  await fees.getByRole('radio', { name: 'Bob Bravo' }).check();

  await expect(page).toHaveURL(/\/opportunities\/classic\/34$/);
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('103.00');
  await expect(fees.getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();
  await page.goto('/opportunities/classic');
  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Bob Bravo' })).toBeChecked();
});

// ---- the detail legend ----

test('the detail legend is closed on load and lists the figures with their names and formulas', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withTritanium(page, 'trend');
  await page.goto('/opportunities/trend/34');
  const legend = page.locator('article#legend');
  await expect(legend.locator('summary')).toHaveText('Legend');
  await expect(legend.locator('details')).not.toHaveAttribute('open', /.*/);
  await expect(legend.locator('table.legend')).toBeHidden();
  // Below the figures, above the first chart.
  const figures = (await page.locator('#figures').boundingBox())!;
  const card = (await legend.boundingBox())!;
  const chart = (await page.locator('article.chart-row').first().boundingBox())!;
  expect(card.y).toBeGreaterThanOrEqual(figures.y + figures.height - 1);
  expect(chart.y).toBeGreaterThanOrEqual(card.y + card.height - 1);

  await legend.locator('summary').click();
  await expect(legend.locator('tbody th')).toHaveText([
    'b', 't', 'B', 'Best buy', 'Best sell', 'S', 'c', 'r', 'Net', 'm', 'U', 'V7', 'V30', 'ρ', 'q_buy', 'q_sell', 'σ', 'T_fill', 'T_max', 'V_min', 'ISK/d',
  ]);
  await expect(legend.locator('tbody tr').nth(18)).toContainText('Allowed holding time');
  await expect(legend.locator('tbody tr').nth(18)).toContainText('min(3, (m / 2σ)²) days');
  await expect(legend).not.toContainText('Order playbook');
});

test('the detail legend is a table of label, name, meaning and formula, styled like the worked example', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withTritanium(page);
  await page.goto('/opportunities/classic');
  await page.locator('article.explainer summary').click();
  const exampleWeight = await page
    .locator('table.example tbody th')
    .first()
    .evaluate((th) => getComputedStyle(th).fontWeight);

  await page.goto('/opportunities/classic/34');
  const legend = page.locator('article#legend');
  await legend.locator('summary').click();
  const table = legend.locator('table.legend');
  await expect(table).not.toHaveClass(/\bdata\b/);
  await expect(table.locator('thead th')).toHaveText(['Label', 'Name', 'Meaning', 'Formula']);
  const row = (label: string) => table.locator('tbody tr').filter({ has: page.locator('th', { hasText: new RegExp(`^${label}$`) }) });
  await expect(row('T_max').locator('td').nth(2)).toHaveText('min(3, (m / 2σ)²) days');
  await expect(row('T_max').locator('td code')).toHaveCount(1);
  await expect(row('B').locator('td').nth(0)).toHaveText(/^Current best buy, not a suggested price, except in a dip row of the recurring spread expansion table, where it is the predicted buy price$/);
  await expect(row('B').locator('td').nth(2)).toHaveText('');
  expect(await table.locator('tbody th').first().evaluate((th) => getComputedStyle(th).fontWeight)).toBe(exampleWeight);
});

test('the Legend entry of the sub-menu jumps to the legend with its title below the sub-menu', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 500 });
  await withClassicCharts(page);
  await page.goto('/opportunities/classic/34');
  const menu = page.locator('#section-nav');

  await menu.getByRole('link', { name: 'Legend' }).click();

  await expect(page).toHaveURL(/#legend$/);
  await expect(page.locator('#legend summary')).toBeInViewport();
  await expect(async () => {
    const nav = (await menu.boundingBox())!;
    const title = (await page.locator('#legend summary').boundingBox())!;
    expect(title.y).toBeGreaterThanOrEqual(nav.y + nav.height);
  }).toPass();
});

test('an opened detail legend stays open across a live refresh, a fee character change and a character filter change', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withTwoCharacters(page);
  await page.goto('/opportunities/classic/34');
  const details = page.locator('#legend details');
  await page.locator('#legend summary').click();
  await expect(details).toHaveAttribute('open', '');

  // The book changes: the best sell is 140 now.
  await stubMarket(bookOf(34, 100, 140, 1));
  expect(await stationBook(page)).toBe('200 updated');
  await expect(page.locator('[data-figure="sell"] [data-value]')).toHaveText('139.90');
  await expect(details).toHaveAttribute('open', '');

  await page.locator('#fee-filter').getByRole('radio', { name: 'Bob Bravo' }).check();
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('103.00');
  await expect(page.locator('[data-figure="broker"] [data-value]')).toHaveText('3.00%');
  await expect(details).toHaveAttribute('open', '');

  // The character filter swaps the content through hx-select: Alice's fees come back.
  await page.locator('#character-filter').getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('101.41');
  await expect(details).toHaveAttribute('open', '');
});

test('a character without fee data counts with the base rates and the helper text says so', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/opportunities/classic');

  await expect(page.locator('#fee-filter').getByRole('radio', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.locator('#fee-note')).toContainText('Base rates');
});
