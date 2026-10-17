import type { Page } from '@playwright/test';
import { chartData, chartDrawn } from '../support/charts';
import {
  stationBook, stationSeed, expect, feePoll, loginWith, marketPoll, marketPollOutsideBrowser, pollCharacter, pollOutsideBrowser, stubControl,
  stubMarket, stubMarketOrder, stubOrder, stubAsset, stubTradeSkills, stubTransaction, test, unfoldFreshness, wealthPoll,
} from '../support/fixtures';

test('the tables refresh after a poll without reloading', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 10 })] });
  await pollCharacter(page, 90000001);
  await page.goto('/orders');
  await expect(page.locator('#order-1')).toContainText('10.00');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 12.5 })] });
  await pollCharacter(page, 90000001);

  await expect(page.locator('#order-1')).toContainText('12.50');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('filter and sorting are kept by a live refresh', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Characters' }).click();
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await stubControl('orders', { character_id: 90000002, orders: [stubOrder(100, { price: 99 })] });
  await pollCharacter(page, 90000002);

  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 5 }), stubOrder(2, { type: 35, price: 9 })],
  });
  await pollCharacter(page, 90000001);
  await page.goto('/orders?chars=90000001&sort=price&dir=desc');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(page.locator('#sell-orders tbody.order').first()).toHaveAttribute('id', 'order-2');

  // Prices change; the order of the rows flips under the same sorting, Bob stays out.
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 50 }), stubOrder(2, { type: 35, price: 9 })],
  });
  await pollCharacter(page, 90000001);
  await pollCharacter(page, 90000002);

  await expect(page.locator('#sell-orders tbody.order').first()).toHaveAttribute('id', 'order-1');
  await expect(page.locator('#order-100')).toHaveCount(0);
  await expect(page).toHaveURL(/chars=90000001&sort=price&dir=desc$/);
});

test('missed signals are caught up after the stream reconnects', async ({ page, context }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 10 })] });
  await pollCharacter(page, 90000001);

  // The stream cannot connect: the page is open, but no signal reaches it. (`context.setOffline`
  // does not cut a stream that is already open in Chromium, so the connection is refused instead.)
  await page.route('**/events', (route) => route.abort('connectionrefused'));
  await page.goto('/orders');
  await expect(page.locator('#order-1')).toContainText('10.00');

  // A poll changes the order meanwhile (called from outside the page).
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 77 })] });
  const token = await page.locator('meta[name="csrf-token"]').getAttribute('content');
  expect(await pollOutsideBrowser(90000001, await context.cookies(), token!)).toBe('200 updated');
  await page.waitForTimeout(500);
  await expect(page.locator('#order-1')).toContainText('10.00');

  // The connection is back: the page re-fetches on reconnect and shows the change.
  await page.unroute('**/events');
  await expect(page.locator('#order-1')).toContainText('77.00', { timeout: 20_000 });
});

/** Alice sells item 34 at 100, and is the best price of a market that has been refreshed. */
async function topSeller(page: import('@playwright/test').Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 100 })] });
  await pollCharacter(page, 90000001);
  await stubMarket([stubMarketOrder(1, { price: 100 })]);
  expect(await marketPoll(page)).toBe('200 1');
}

test('a standing changes after a market refresh without reloading', async ({ page }) => {
  await topSeller(page);
  await page.goto('/orders');
  await expect(page.locator('#order-1 .badge.top')).toHaveText('Top');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { price: 90 })]);
  expect(await marketPoll(page)).toBe('200 1');

  await expect(page.locator('#order-1 .badge.undercut')).toHaveText('Undercut 1');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('the undercut volume changes after a market refresh without reloading', async ({ page }) => {
  await topSeller(page);
  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { price: 90, remain: 20 })]);
  expect(await marketPoll(page)).toBe('200 1');
  await page.goto('/orders');
  const cell = page.locator('#order-1 td').nth(4);
  await expect(cell).toHaveText('20');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  // Same standing and gap: only the units in front change.
  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { price: 90, remain: 8 })]);
  expect(await marketPoll(page)).toBe('200 1');

  await expect(cell).toHaveText('8');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('filter and sorting are kept by a market refresh', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Characters' }).click();
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await stubControl('orders', { character_id: 90000002, orders: [stubOrder(100, { price: 99 })] });
  await pollCharacter(page, 90000002);
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 100 }), stubOrder(2, { type: 35, price: 100 })],
  });
  await pollCharacter(page, 90000001);
  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { type: 35, price: 100 })]);
  await marketPoll(page);
  await page.goto('/orders?chars=90000001&sort=standing&dir=desc');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(page.locator('#order-100')).toHaveCount(0);

  // Order 2 is undercut once. Descending, it comes first now; Bob stays out.
  await stubMarket([
    stubMarketOrder(1, { price: 100 }),
    stubMarketOrder(2, { type: 35, price: 100 }),
    stubMarketOrder(3, { type: 35, price: 90 }),
  ]);
  await marketPoll(page);

  await expect(page.locator('#order-2 .badge.undercut')).toHaveText('Undercut 1');
  await expect(page.locator('#sell-orders tbody.order').first()).toHaveAttribute('id', 'order-2');
  await expect(page.locator('#order-100')).toHaveCount(0);
  await expect(page).toHaveURL(/chars=90000001&sort=standing&dir=desc$/);
});

test('a market refresh that changes nothing does not refresh the page', async ({ page }) => {
  await topSeller(page);
  await page.goto('/orders');
  await expect(page.locator('#order-1 .badge.top')).toHaveText('Top');
  // Let the refresh the page makes when its stream opens settle before counting.
  await page.waitForTimeout(1000);
  const refreshes: string[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/orders/tables') || request.url().includes('/orders/freshness')) {
      refreshes.push(request.url());
    }
  });

  // Other orders that change neither the standing nor the gap of the user's order.
  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(5, { price: 150 })]);
  expect(await marketPoll(page)).toBe('200 1');
  await page.waitForTimeout(1500);

  expect(refreshes).toEqual([]);
  await expect(page.locator('#order-1 .badge.top')).toHaveText('Top');
});

test('the dashboard does not refresh because of a market refresh', async ({ page }) => {
  await topSeller(page);
  await page.goto('/dashboard');
  await expect(page.locator('#tile-sell-value')).toBeVisible();
  await page.waitForTimeout(1000);
  const refreshes: string[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/dashboard/summary')) refreshes.push(request.url());
  });

  // This one does change a standing, so the orders page would refresh.
  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { price: 90 })]);
  expect(await marketPoll(page)).toBe('200 1');
  await page.waitForTimeout(1500);

  expect(refreshes).toEqual([]);
});

test('a missed market signal is caught up after the stream reconnects', async ({ page, context }) => {
  await topSeller(page);
  await page.route('**/events', (route) => route.abort('connectionrefused'));
  await page.goto('/orders');
  await expect(page.locator('#order-1 .badge.top')).toHaveText('Top');

  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { price: 90 })]);
  const token = await page.locator('meta[name="csrf-token"]').getAttribute('content');
  expect(await marketPollOutsideBrowser(await context.cookies(), token!)).toBe('200 1');
  await page.waitForTimeout(500);
  await expect(page.locator('#order-1 .badge.top')).toHaveText('Top');

  await page.unroute('**/events');
  await expect(page.locator('#order-1 .badge.undercut')).toHaveText('Undercut 1', { timeout: 20_000 });
});

// ---- the wealth page ----

const WALLET = 2; // the stack, bottom to top: sell orders, buy escrow, wallet, inventory

test('the last wallet point shows a new balance after a wallet poll, without a reload', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('wallet', { character_id: 90000001, balance: 10_000_000 });
  await wealthPoll(page, 90000001, 'wallet');
  await page.goto('/wealth');
  const lastWallet = async () => {
    const data = await chartData(page, 'wealth-stack');
    return data[WALLET][data[WALLET].length - 1].y;
  };
  expect(await lastWallet()).toBe(1_000_000_000);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await stubControl('wallet', { character_id: 90000001, balance: 15_000_000 });
  await wealthPoll(page, 90000001, 'wallet');

  await expect.poll(lastWallet).toBe(1_500_000_000);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('selection, range and sorting are kept by a live wealth refresh', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Characters' }).click();
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await stubControl('assets', {
    character_id: 90000001,
    assets: [stubAsset(1, { type: 34, quantity: 5 }), stubAsset(2, { type: 35, quantity: 5 })],
  });
  await stubControl('assets', { character_id: 90000002, assets: [stubAsset(3, { type: 36, quantity: 5 })] });
  await wealthPoll(page, 90000001, 'assets');
  await wealthPoll(page, 90000002, 'assets');
  await wealthPoll(page, 90000001, 'wallet');
  await page.goto('/wealth?chars=90000001&range=24h&sort=item&dir=asc');
  await unfoldFreshness(page);
  await expect(page.locator('#inventory tbody tr')).toHaveCount(2);
  await expect(page.locator('#inventory th[aria-sort="ascending"]')).toContainText('Item');
  await page.waitForTimeout(1000);

  await stubControl('wallet', { character_id: 90000001, balance: 77_000_000 });
  await wealthPoll(page, 90000001, 'wallet');
  await wealthPoll(page, 90000002, 'wallet');

  await expect.poll(async () => {
    const data = await chartData(page, 'wealth-stack');
    return data[WALLET][data[WALLET].length - 1].y;
  }).toBe(7_700_000_000);
  await expect(page).toHaveURL(/chars=90000001&range=24h&sort=item&dir=asc$/);
  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('24 hours');
  await expect(page.locator('#inventory tbody tr')).toHaveCount(2);
  await expect(page.locator('#inventory th[aria-sort="ascending"]')).toContainText('Item');
  await expect(page.locator('#wealth-freshness')).not.toContainText('Bob Bravo');
  await chartDrawn(page, 'wealth-stack');
  expect(
    await page.evaluate(() => {
      const chart = (window as any).Chart.getChart(document.querySelector('canvas[data-chart="wealth-stack"]'));
      return Math.round((chart.scales.x.max - chart.scales.x.min) / 3_600_000);
    }),
  ).toBe(24);
});

test('the wealth page does not refresh because of a market refresh', async ({ page }) => {
  await topSeller(page);
  await page.goto('/wealth');
  await expect(page.locator('#wealth-items')).toBeVisible();
  await page.waitForTimeout(1000);
  const refreshes: string[] = [];
  page.on('request', (request) => {
    if (/\/wealth\/(graph|items|freshness)/.test(request.url())) refreshes.push(request.url());
  });

  // This one does change a standing, so the orders page would refresh.
  await stubMarket([stubMarketOrder(1, { price: 100 }), stubMarketOrder(2, { price: 90 })]);
  expect(await marketPoll(page)).toBe('200 1');
  await page.waitForTimeout(1500);

  expect(refreshes).toEqual([]);
});

test('missed wealth signals are caught up after the stream reconnects', async ({ page, context }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('wallet', { character_id: 90000001, balance: 10_000_000 });
  await wealthPoll(page, 90000001, 'wallet');
  await page.route('**/events', (route) => route.abort('connectionrefused'));
  await page.goto('/wealth');
  const lastWallet = async () => {
    const data = await chartData(page, 'wealth-stack');
    return data[WALLET][data[WALLET].length - 1].y;
  };
  expect(await lastWallet()).toBe(1_000_000_000);

  await stubControl('wallet', { character_id: 90000001, balance: 20_000_000 });
  await wealthPoll(page, 90000001, 'wallet');
  await page.waitForTimeout(500);
  expect(await lastWallet()).toBe(1_000_000_000);

  await page.unroute('**/events');
  await expect.poll(lastWallet, { timeout: 20_000 }).toBe(2_000_000_000);
});

/** Alice sells 10 units of item 34 at 150 and has the cost of all 10 (at 100 each), with Accounting 5 and Broker Relations 5. */
async function aliceWithAMargin(page: import('@playwright/test').Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(5, 5) });
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { type: 34, price: 150, remain: 10 }), stubOrder(3, { type: 35, price: 120, remain: 10 })],
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubControl('transactions', {
    character_id: 90000001,
    transactions: [stubTransaction(1, { type: 34, quantity: 10 }), stubTransaction(2, { type: 35, quantity: 4 })],
  });
  expect(await wealthPoll(page, 90000001, 'transactions')).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.goto('/orders');
}

const marginCell = (page: import('@playwright/test').Page, order: number) =>
  page.locator(`#sell-orders #order-${order} td`).nth(7);

test('a margin changes after a fee change without reloading', async ({ page }) => {
  await aliceWithAMargin(page);
  await expect(marginCell(page, 1)).toHaveText('428.23');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  // Accounting 4 raises the sales tax to 4.2%: 10 * 150 * (1 - 4.2% - 1.41%) - 1000 = 415.85
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(4, 5) });
  expect(await feePoll(page)).toBe('200 ok');

  await expect(marginCell(page, 1)).toHaveText('415.85');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('a margin appears after the missing purchase is polled without reloading', async ({ page }) => {
  await aliceWithAMargin(page);
  await expect(marginCell(page, 3)).toHaveText('\u2014');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  // The other 6 units of item 35 were bought too: 10 * 120 * 0.95215 - 10 * 100 = 142.58
  await stubControl('transactions', {
    character_id: 90000001,
    transactions: [
      stubTransaction(1, { type: 34, quantity: 10 }),
      stubTransaction(2, { type: 35, quantity: 4 }),
      stubTransaction(3, { type: 35, quantity: 6 }),
    ],
  });
  expect(await wealthPoll(page, 90000001, 'transactions')).toBe('200 updated');

  await expect(marginCell(page, 3)).toHaveText('142.58');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

// ---- the opportunities page ----


function crossedHistory(type: number, jump: number) {
  return Array.from({ length: 30 }, (_, i) => {
    const back = 30 - i;
    return {
      type_id: type,
      days_back: back,
      average: back > 3 ? 100 : 100 * jump,
      highest: (back > 3 ? 100 : 100 * jump) * 1.01,
      lowest: (back > 3 ? 100 : 100 * jump) * 0.99,
      volume: back > 5 ? 100_000_000 : 200_000_000,
    };
  });
}

function stationBookOf(sell: number) {
  return [34, 35].flatMap((type, i) => [
    stubMarketOrder(10 + i * 2, { type, price: 100, buy: true, range: 'region' }),
    stubMarketOrder(11 + i * 2, { type, price: sell }),
  ]);
}

async function opportunitiesWithTwoTrendItems(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(5, 5) });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.getByRole('navigation').getByRole('link', { name: 'Opportunities' }).click();
  await stubMarket(stationBookOf(130));
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect(
    (await stationSeed(page, { history: [...crossedHistory(34, 1.1), ...crossedHistory(35, 1.2)] })).startsWith('200 '),
  ).toBe(true);
  await page.reload();
}

test('a station book refresh updates the tables and the open window in place and keeps sorting, filters, scroll and window', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 420 });
  await opportunitiesWithTwoTrendItems(page);
  const classicSell = page.locator('#opportunity-classic tr[data-type-id="34"] [data-col="sell"]');
  const trend = page.locator('#opportunity-trend');
  await expect(classicSell).toHaveText('129.90');

  // Sort the trend table by volatility, change a filter, scroll down and open the window of Tritanium.
  await trend.getByRole('link', { name: 'σ' }).click();
  await expect(page).toHaveURL(/ts=sigma&td=desc/);
  await page.locator('#opportunity-filters').getByLabel('Capital per item (ISK)').fill('50000000');
  await page.locator('#opportunity-filters').getByRole('button', { name: 'Apply' }).click();
  await expect(classicSell).toHaveText('129.90');
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await trend.locator('tr[data-type-id="34"]').click();
  const detail = page.locator('#opportunity-detail');
  await expect(detail.locator('[data-figure="sell"] [data-value]')).toHaveText('129.90');
  const scrollBefore = await page.evaluate(() => window.scrollY);
  expect(scrollBefore, 'the page is scrolled').toBeGreaterThan(50);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  // The book changes: the best sell is 140 now, and the station is refreshed.
  await stubMarket(stationBookOf(140));
  expect(await stationBook(page)).toBe('200 updated');

  await expect(page.locator('#opportunity-classic tr[data-type-id="34"] [data-col="sell"]')).toHaveText('139.90');
  await expect(detail.locator('[data-figure="sell"] [data-value]')).toHaveText('139.90');
  await expect(trend.locator('tbody tr').first()).toContainText('Pyerite');
  await expect(page).toHaveURL(/ts=sigma&td=desc/);
  await expect(page.locator('#opportunity-filters').getByLabel('Capital per item (ISK)')).toHaveValue('50000000');
  await expect(detail).toBeVisible();
  await expect(detail.getByRole('heading')).toContainText('Trend');
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
  expect(Math.abs((await page.evaluate(() => window.scrollY)) - scrollBefore)).toBeLessThanOrEqual(2);
});

test('the orders page does not fetch anything when the station is refreshed', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  // Run the character's first order and fee polls now, so their signals cannot land while counting.
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.goto('/orders');
  await expect(page.locator('#order-tables')).toBeVisible();
  // Let the refresh the page makes when its stream opens settle before counting.
  await page.waitForTimeout(1000);
  const fetched: string[] = [];
  page.on('request', (request) => {
    if (/\/(orders|opportunities)\//.test(new URL(request.url()).pathname)) fetched.push(request.url());
  });

  expect(await stationBook(page)).toBe('200 updated');
  await page.waitForTimeout(1000);

  expect(fetched).toEqual([]);
});
