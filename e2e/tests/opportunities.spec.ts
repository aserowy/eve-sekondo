import type { Page } from '@playwright/test';
import { chartData, chartDrawn } from '../support/charts';
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
  const nav = page.getByRole('navigation');
  await nav.getByRole('link', { name: 'Orders' }).click();
  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();
  await expect(page).toHaveURL(/\/orders\?chars=90000002$/);

  await nav.getByRole('link', { name: 'Opportunities' }).click();

  await expect(page).toHaveURL(/\/opportunities\?chars=90000002$/);
  await expect(nav.getByRole('link', { name: 'Opportunities' })).toHaveAttribute('aria-current', 'page');
  await expect(page.getByRole('heading', { name: STATION })).toBeVisible();
  for (const title of ['Recurring spread expansion', 'Classic spread trade', 'Mean reversion', 'Trend']) {
    await expect(page.getByRole('heading', { name: title })).toBeVisible();
  }
  await expect(page.locator('#opportunity-freshness')).toContainText('Waiting for first refresh');
  await expect(page.locator('#opportunity-freshness')).toContainText('Stale');
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).not.toBeChecked();
  await expect(page.getByRole('checkbox', { name: 'Bob Bravo' })).toBeChecked();
  // The selection goes on to the other pages.
  await nav.getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\?chars=90000002$/);
});

test('a book refresh shows its time in the freshness and clears the stale mark', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  const freshness = page.locator('#opportunity-freshness');
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

/** Alice with Accounting 5 and Broker Relations 5 (b 1.41%, t 3.375%) and her orders polled. */
async function aliceTrained(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(5, 5) });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
}

/** A buy and a sell order of an item at the station: the buy within region range, so it counts there. */
function bookOf(type: number, buy: number, sell: number, id: number) {
  return [
    stubMarketOrder(id, { type, price: buy, buy: true, range: 'region' }),
    stubMarketOrder(id + 1, { type, price: sell }),
  ];
}

/** `days` days of history up to yesterday: `average(daysBack)` and a volume of `volume(daysBack)`. */
function history(type: number, days: number, average: (back: number) => number, volume: (back: number) => number) {
  return Array.from({ length: days }, (_, i) => {
    const back = days - i;
    const avg = average(back);
    return { type_id: type, days_back: back, average: avg, highest: avg * 1.01, lowest: avg * 0.99, volume: volume(back) };
  });
}

/** A calm price with a small wobble: 14 days of about 105 ISK, `volume` traded each day (100 million). */
const calm = (type: number, volume = 100_000_000) => history(type, 14, (back) => (back % 2 ? 105.5 : 104.5), () => volume);

/** A price that jumped by `jump` three days ago after 27 flat days, with the volume doubling in the last five. */
const crossed = (type: number, jump: number) =>
  history(type, 30, (back) => (back > 3 ? 100 : 100 * jump), (back) => (back > 5 ? 100_000_000 : 200_000_000));

async function fill(page: Page, book: ReturnType<typeof stubMarketOrder>[], seed: ReturnType<typeof history>) {
  await stubMarket(book);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: seed })).startsWith('200 ')).toBe(true);
  await page.reload();
}

test('a classic row shows the sell price one tick below the best sell and the current best buy', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();

  await fill(page, bookOf(34, 100, 120, 1), calm(34));

  const classic = page.locator('#opportunity-classic');
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
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Pyerite jumped by 20% and Tritanium by 10%: Pyerite is the more volatile.
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();

  const classic = page.locator('#opportunity-classic');
  const trend = page.locator('#opportunity-trend');
  await expect(trend.locator('tbody tr')).toHaveCount(2);
  await classic.getByRole('link', { name: 'Item' }).click();
  await expect(page).toHaveURL(/cs=item&cd=asc/);
  await expect(classic.locator('tbody tr').first()).toContainText('Pyerite');

  await trend.getByRole('link', { name: 'σ' }).click();
  await expect(page).toHaveURL(/cs=item&cd=asc/);
  await expect(page).toHaveURL(/ts=sigma&td=desc/);
  await expect(trend.locator('tbody tr').first()).toContainText('Pyerite');
  await expect(classic.locator('tbody tr').first()).toContainText('Pyerite');

  await trend.getByRole('link', { name: 'σ' }).click();
  await expect(page).toHaveURL(/ts=sigma&td=asc/);
  await expect(trend.locator('tbody tr').first()).toContainText('Tritanium');
  await expect(classic.locator('tbody tr').first()).toContainText('Pyerite');
});

test('an item the account sells at the station is marked as an own sell', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(36, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...calm(34), ...calm(36)] })).startsWith('200 ')).toBe(true);
  await page.reload();

  const classic = page.locator('#opportunity-classic');
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
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
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

  const expansion = page.locator('#opportunity-expansion');
  await expect(expansion).not.toContainText('Collecting hourly data');
  const row = expansion.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="window"]')).toHaveText('daily 18:00');
  // The peaks are 135 to 144: their 25th percentile is 137.25, and S is one tick below it.
  await expect(row.locator('[data-col="sell"]')).toHaveText('137.20');
  await expect(row.locator('[data-col="next"]')).toContainText(/\d{4}-\d{2}-\d{2} 18:00/);
});

test('with three days of snapshots the expansion table says that hourly data is still collected', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket(bookOf(34, 100, 105, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Two seeded days and the snapshot the book refresh just recorded make three days.
  const snapshots = [1, 2].map((back) => ({ type_id: 34, minutes_back: minutesBack(back, 12), best_buy: 100, best_sell: 105 }));
  expect((await stationSeed(page, { snapshots })).startsWith('200 ')).toBe(true);
  await page.reload();

  await expect(page.locator('#opportunity-expansion')).toContainText('Collecting hourly data (3/7 days)');
  await expect(page.locator('#opportunity-freshness')).toContainText('3 days of own snapshots');
});

test('an oversold item is listed in the mean reversion table with the SMA 20 rounded down as its sell price', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket(bookOf(34, 120, 125, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // 19 days at 152 and then 119.40: the SMA 20 is 150.37 and the last day is below the lower band.
  const seed = history(34, 25, (back) => (back === 1 ? 119.4 : 152), () => 100_000_000);
  expect((await stationSeed(page, { history: seed })).startsWith('200 ')).toBe(true);
  await page.reload();

  const reversion = page.locator('#opportunity-reversion');
  const row = reversion.locator('tr[data-type-id="34"]');
  await expect(row.locator('[data-col="sell"]')).toHaveText('150.30');
  await expect(row.locator('[data-col="pb"]')).toHaveText(/^-\d/);
  await expect(row.locator('[data-col="rsi"]')).toHaveText(/^\d+\.\d$/);
  // Buying at 120 and selling at 124.90 earns nothing after the fees: the classic table skips it.
  await expect(page.locator('#opportunity-classic tr[data-type-id="34"]')).toHaveCount(0);
});


// ---- the filters ----

const filters = (page: Page) => page.locator('#opportunity-filters');

test('a first visit shows the default filters', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();

  await expect(filters(page).getByLabel('Minimum margin (%)')).toHaveValue('5');
  await expect(filters(page).getByLabel('Minimum ISK/day')).toHaveValue('5000000');
  await expect(filters(page).getByLabel('Item name')).toHaveValue('');
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('100000000');
});

test('raising the capital removes an item that can no longer fill in time and stays after signing in again', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  // Tritanium trades 10 million a day, Pyerite 100 million.
  expect((await stationSeed(page, { history: [...calm(34, 10_000_000), ...calm(35, 100_000_000)] })).startsWith('200 ')).toBe(true);
  await page.reload();
  const classic = page.locator('#opportunity-classic');
  await expect(classic.locator('tbody tr')).toHaveCount(2);

  await filters(page).getByLabel('Capital per item (ISK)').fill('2000000000');
  await filters(page).getByRole('button', { name: 'Apply' }).click();

  await expect(classic.locator('tbody tr')).toHaveCount(1);
  await expect(classic.locator('tr[data-type-id="35"]')).toHaveCount(1);
  await expect(classic.locator('tr[data-type-id="34"]')).toHaveCount(0);

  await page.getByRole('navigation').getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('2000000000');
  await expect(page.locator('#opportunity-classic tbody tr')).toHaveCount(1);
});

test('the item name search narrows every table', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator('#opportunity-classic tbody tr')).toHaveCount(2);
  await expect(page.locator('#opportunity-trend tbody tr')).toHaveCount(2);

  await filters(page).getByLabel('Item name').fill('TRIT');
  await filters(page).getByRole('button', { name: 'Apply' }).click();

  for (const table of ['classic', 'trend']) {
    await expect(page.locator(`#opportunity-${table} tbody tr`)).toHaveCount(1);
    await expect(page.locator(`#opportunity-${table} tbody tr`)).toContainText('Tritanium');
  }
});

test('a capital of zero is rejected with a message and the tables stay as they were', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: calm(34) })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator('#opportunity-classic tbody tr')).toHaveCount(1);

  await filters(page).getByLabel('Capital per item (ISK)').fill('0');
  await filters(page).getByRole('button', { name: 'Apply' }).click();

  await expect(page.getByRole('alert')).toContainText('capital');
  await expect(page.getByRole('alert')).toContainText('positive');
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('0');
  await expect(page.locator('#opportunity-classic tbody tr')).toHaveCount(1);
  // Nothing was stored: a reload shows the default again.
  await page.reload();
  await expect(filters(page).getByLabel('Capital per item (ISK)')).toHaveValue('100000000');
});

// ---- the detail window ----

/** Tritanium in the classic and the trend table: a fresh crossing with a rising volume. */
async function withTritanium(page: Page) {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: crossed(34, 1.1) })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator('#opportunity-classic tr[data-type-id="34"]')).toHaveCount(1);
  await expect(page.locator('#opportunity-trend tr[data-type-id="34"]')).toHaveCount(1);
}

const detail = (page: Page) => page.locator('#opportunity-detail');

test('a row click opens the detail window beside the tables and Escape closes it', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await withTritanium(page);
  await expect(detail(page)).toBeHidden();

  await page.locator('#opportunity-classic tr[data-type-id="34"]').click();

  await expect(detail(page)).toBeVisible();
  await expect(detail(page)).toHaveAttribute('role', 'dialog');
  await expect(detail(page).getByRole('heading')).toContainText('Tritanium');
  await expect(detail(page).getByRole('heading')).toContainText('Classic spread trade');
  await expect(detail(page)).toContainText('Alice Alpha');
  await expect(detail(page).locator('[data-figure="buy"] [data-value]')).toHaveText('100.00');
  await expect(detail(page).locator('[data-figure="sell"] [data-value]')).toHaveText('129.90');
  await expect(detail(page).locator('[data-figure="margin"]')).toContainText('(r − c) / c');
  // It does not cover the controls of the page.
  const window = (await detail(page).boundingBox())!;
  const controls = page.locator('#main-nav a, #main-nav button, #opportunity-filters input, #opportunity-filters button');
  for (const box of await controls.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()))) {
    const overlaps = box.left < window.x + window.width && box.right > window.x && box.top < window.y + window.height && box.bottom > window.y;
    expect(overlaps, `a control at ${JSON.stringify(box)} is covered by the window`).toBe(false);
  }

  await page.keyboard.press('Escape');
  await expect(detail(page)).toBeHidden();
  await expect(detail(page)).toBeEmpty();
});

test('on a narrower screen the page makes room and the window still covers no control', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await withTritanium(page);
  await page.locator('#opportunity-classic tr[data-type-id="34"]').click();
  await expect(detail(page)).toBeVisible();

  const window = (await detail(page).boundingBox())!;
  const controls = page.locator('#opportunity-filters input, #opportunity-filters button');
  for (const box of await controls.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()))) {
    const overlaps = box.left < window.x + window.width && box.right > window.x && box.top < window.y + window.height && box.bottom > window.y;
    expect(overlaps, `a control at ${JSON.stringify(box)} is covered by the window`).toBe(false);
  }
  // The tables end where the window begins; wider tables scroll inside their own panel.
  for (const id of ['expansion', 'classic', 'reversion', 'trend']) {
    const table = (await page.locator(`#opportunity-${id}`).boundingBox())!;
    expect(table.x + table.width, `the ${id} table runs under the window`).toBeLessThanOrEqual(window.x);
  }
  // And closing it gives the room back.
  await page.keyboard.press('Escape');
  await expect(detail(page)).toBeHidden();
});

test('the close button closes the window too', async ({ page }) => {
  await withTritanium(page);
  await page.locator('#opportunity-trend tr[data-type-id="34"]').click();
  await expect(detail(page)).toBeVisible();

  await detail(page).getByRole('button', { name: 'Close' }).click();

  await expect(detail(page)).toBeHidden();
});

test('dragging the title of the window does not move it', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await withTritanium(page);
  await page.locator('#opportunity-classic tr[data-type-id="34"]').click();
  await expect(detail(page)).toBeVisible();
  const before = (await detail(page).boundingBox())!;

  const title = (await detail(page).getByRole('heading').boundingBox())!;
  await page.mouse.move(title.x + 10, title.y + title.height / 2);
  await page.mouse.down();
  await page.mouse.move(title.x - 300, title.y + 200, { steps: 8 });
  await page.mouse.up();

  expect(await detail(page).boundingBox()).toEqual(before);
  await expect(detail(page)).toBeVisible();
});

test('opening a row of another table replaces the open window', async ({ page }) => {
  await withTritanium(page);
  await page.locator('#opportunity-classic tr[data-type-id="34"]').click();
  await expect(detail(page).getByRole('heading')).toContainText('Classic spread trade');

  await page.locator('#opportunity-trend tr[data-type-id="34"]').click();

  await expect(detail(page).getByRole('heading')).toContainText('Trend');
  await expect(detail(page).getByRole('heading')).not.toContainText('Classic spread trade');
  await expect(page.locator('#opportunity-detail')).toHaveCount(1);
  await expect(detail(page).getByRole('heading')).toHaveCount(1);
});

test('the window keeps the item and says so when it no longer qualifies', async ({ page }) => {
  await withTritanium(page);
  await page.locator('#opportunity-classic tr[data-type-id="34"]').click();
  await expect(detail(page)).toBeVisible();
  await expect(page.locator('#opportunity-detail-unlisted')).toHaveCount(0);

  // The capital grows until the item can no longer fill in time; the window is opened again.
  await page.locator('#opportunity-filters').getByLabel('Capital per item (ISK)').fill('900000000000');
  await page.locator('#opportunity-filters').getByRole('button', { name: 'Apply' }).click();
  await expect(page.locator('#opportunity-classic tr[data-type-id="34"]')).toHaveCount(0);
  await page.evaluate(async () => {
    const res = await fetch('/opportunities/detail/classic/34');
    document.getElementById('opportunity-detail')!.innerHTML = await res.text();
  });

  await expect(page.locator('#opportunity-detail-unlisted')).toContainText('No longer listed');
  await expect(detail(page).locator('[data-figure="sell"] [data-value]')).toHaveText('129.90');
});


// ---- the charts of the window ----

/** The y values of a line chart's dataset (its points are `{ x, y }`). */
const ys = (points: { y: number | null }[]) => points.map((p) => p.y);

test('the classic window draws the best prices, the spread and the depth of the book', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
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

  await page.locator('#opportunity-classic tr[data-type-id="34"]').click();

  await chartDrawn(page, 'classic-prices');
  const [buy, sell] = await chartData(page, 'classic-prices');
  // The three seeded hours, and the hour of the book refresh with the best sell of 130.
  expect(ys(sell).slice(0, 3)).toEqual([11000, 12000, 13000]);
  expect(new Set(ys(buy))).toEqual(new Set([10000]));
  const [spread] = await chartData(page, 'classic-spread');
  expect(ys(spread).slice(0, 3)).toEqual([10, 20, 30]);
  const [units] = await chartData(page, 'depth');
  expect(units).toEqual([3, 3]);
  await expect(page.locator('#opportunity-detail figcaption')).toContainText(['Best buy and best sell', 'Spread', 'Units within 1% of the best price']);
});

test('the mean reversion window draws the average with its bands and the RSI', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket(bookOf(34, 120, 125, 1));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  const seed = history(34, 25, (back) => (back === 1 ? 119.4 : 152), () => 100_000_000);
  expect((await stationSeed(page, { history: seed })).startsWith('200 ')).toBe(true);
  await page.reload();

  await page.locator('#opportunity-reversion tr[data-type-id="34"]').click();

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

test('the trend window draws the averages with the volume', async ({ page }) => {
  await withTritanium(page);

  await page.locator('#opportunity-trend tr[data-type-id="34"]').click();

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

test('the expansion window draws the spread by hour and weekday, the peaks with S and the next occurrences', async ({ page }) => {
  await aliceTrained(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
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

  await page.locator('#opportunity-expansion tr[data-type-id="34"]').click();

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
  await expect(page.locator('#opportunity-detail')).toContainText('Next occurrences');
  await expect(page.locator('#opportunity-detail .next-occurrences li')).toHaveCount(3);
  await expect(page.locator('#opportunity-detail .next-occurrences li').first()).toContainText('18:00');
});
