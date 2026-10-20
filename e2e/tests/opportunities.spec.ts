import type { Page } from '@playwright/test';
import { chartData, chartDrawn } from '../support/charts';
import { aliceTrained, bookOf, calm, crossed, fill, history } from '../support/opportunities';
import {
  expect, feePoll, loginWith, openCharacters, pollCharacter, stationBook, stationRollups, stationSeed, stubControl, stubMarket,
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
  await expect(page.locator('#filter-box #filter-context')).toContainText(STATION);
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
  await expect(classic.getByRole('columnheader', { name: 'Current best buy' })).toBeVisible();
  // Alice's fees at Jita 4-4: c = 101.41, r = 119.90 * 0.95215, so a margin of 12.58%.
  await expect(row.locator('[data-col="margin"]')).toHaveText('12.58%');
  await expect(page.locator('#opportunity-fees')).toContainText('Alice Alpha');
  await expect(page.locator('#opportunity-fees')).toContainText('broker fee 1.41%');
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

test('an evening spread expansion that recurred on most days is listed as a daily window', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');

  // 14 days of hourly snapshots with a spread of 5%; at 18:30 UTC the best sell jumped to 135 to 144
  // on ten of the days.
  const snapshots: SeedSnapshot[] = [];
  for (let back = 1; back <= 14; back++) {
    for (let hour = 0; hour < 24; hour++) {
      const peak = hour === 18 && back <= 10 ? 134 + back : 105;
      snapshots.push({ type_id: 34, minutes_back: minutesBack(back, hour), best_buy: 100, best_sell: peak });
    }
  }
  expect(await stationSeed(page, { snapshots, history: calm(34) })).toBe('200 336 snapshots, 14 history days');
  expect(await stationRollups(page)).toBe('200 ok');
  await page.reload();

  const expansion = page.locator('#expansion');
  await expect(expansion).not.toContainText('Collecting hourly data');
  const row = expansion.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="window"]')).toHaveText('daily 18:00');
  // The peaks are 135 to 144: their 25th percentile is 137.25, and S is one tick below it.
  await expect(row.locator('[data-col="sell"]')).toHaveText('137.20');
  await expect(row.locator('[data-col="next"]')).toContainText(/\d{4}-\d{2}-\d{2} 18:00/);
});

test('with three days of snapshots the expansion table says that hourly data is still collected', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Two seeded days and the snapshot the book refresh just recorded make three days.
  const snapshots = [1, 2].map((back) => ({ type_id: 34, minutes_back: minutesBack(back, 12), best_buy: 100, best_sell: 105 }));
  expect((await stationSeed(page, { snapshots })).startsWith('200 ')).toBe(true);
  await page.reload();

  await expect(page.locator('#expansion')).toContainText('Collecting hourly data (3/7 days)');
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
  await expect(page.locator('#filter-box #filter-context')).toContainText('Alice Alpha');
  await expect(page.locator('[data-figure="buy"] [data-value]')).toHaveText('100.00');
  await expect(page.locator('[data-figure="sell"] [data-value]')).toHaveText('129.90');
  await expect(page.locator('[data-figure="margin"]')).toContainText('(r − c) / c');
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

  const menu = page.locator('.page > nav.sub-menu#section-nav');
  await expect(menu.getByRole('link')).toHaveText([
    '← Classic spread trade', 'Figures', 'Best buy and best sell', 'Spread', 'Units within 1% of the best price',
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
  await expect(page.locator('#filter-context')).toContainText('Fees of Alice Alpha');
  // Alice's skills give b 1.41%: c = 100 * 1.0141.
  await expect(page.locator('[data-figure="cost"] [data-value]')).toHaveText('101.41');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await page.locator('#filter-box').getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();

  await expect(page).toHaveURL(/\/opportunities\/classic\/34\?chars=90000002$/);
  await expect(page.locator('#filter-context')).toContainText('base rates');
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

test('the expansion page draws the spread by hour and weekday, the peaks with S and the next occurrences', async ({ page }) => {
  await aliceTrained(page);
  await page.goto('/opportunities/expansion');
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  const snapshots: SeedSnapshot[] = [];
  for (let back = 1; back <= 14; back++) {
    for (let hour = 0; hour < 24; hour++) {
      const peak = hour === 18 && back <= 10 ? 134 + back : 105;
      snapshots.push({ type_id: 34, minutes_back: minutesBack(back, hour), best_buy: 100, best_sell: peak });
    }
  }
  expect((await stationSeed(page, { snapshots, history: calm(34) })).startsWith('200 ')).toBe(true);
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
  expect(new Set(sell)).toEqual(new Set([13720]));
  await expect(page.locator('#next h2')).toContainText('Next occurrences');
  await expect(page.locator('#next li')).toHaveCount(3);
  await expect(page.locator('#next li').first()).toContainText('18:00');
  await expect(page.locator('#section-nav').getByRole('link', { name: 'Next occurrences' })).toHaveAttribute('href', '#next');
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
