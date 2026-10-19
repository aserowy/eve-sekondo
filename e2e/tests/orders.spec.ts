import {
  expect, feePoll, loginWith, marketPoll, pollCharacter, stubControl, stubMarket, stubMarketOrder, stubOrder,
  stubTradeSkills, stubTransaction, test, wealthPoll,
} from '../support/fixtures';

test('sell and buy orders show up after a poll, sortable, with exact amounts on hover', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\/sell$/);

  // Nothing has been polled yet.
  await expect(page.getByText('Waiting for first update')).toBeVisible();
  await expect(page.getByText('No sell orders.')).toBeVisible();
  await expect(page.locator('.pagination')).toHaveCount(0);
  await page.locator('nav.sub-menu').getByRole('link', { name: 'Buy' }).click();
  await expect(page).toHaveURL(/\/orders\/buy$/);
  await expect(page.getByText('No buy orders.')).toBeVisible();

  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');

  // The stub's fixture: two sell orders and a buy order in a structure.
  const sell = page.locator('#sell-orders');
  await expect(sell.locator('tbody.order')).toHaveCount(2);

  const tritanium = sell.locator('#order-5001');
  await expect(tritanium).toContainText('Alice Alpha');
  await expect(tritanium).toContainText('Tritanium');
  await expect(tritanium).toContainText('Jita IV - Moon 4 - Caldari Navy Assembly Plant');
  await expect(tritanium).toContainText('The Forge');
  await expect(tritanium).toContainText('10 / 20');
  // Price 1,500,000.00 and value (10 x price) 15,000,000.00, shortened, with the exact value on hover.
  await expect(tritanium.getByText('1.5m', { exact: true })).toHaveAttribute('title', '1,500,000.00 ISK');
  await expect(tritanium.getByText('15m', { exact: true })).toHaveAttribute('title', '15,000,000.00 ISK');
  await expect(sell.locator('#order-5002').getByText('950.50', { exact: true })).toBeVisible();
  await expect(sell.locator('#order-5002').getByText('95.05k', { exact: true })).toHaveAttribute(
    'title',
    '95,050.00 ISK',
  );

  await page.locator('nav.sub-menu').getByRole('link', { name: 'Buy' }).click();
  const buy = page.locator('#buy-orders');
  await expect(buy.locator('tbody.order')).toHaveCount(1);
  await expect(page.locator('#sell-orders')).toHaveCount(0);
  const mexallon = buy.locator('#order-5003');
  await expect(mexallon).toContainText('Mexallon');
  await expect(mexallon).toContainText('Alpha Keepstar');
  await expect(mexallon.getByText('2.5m', { exact: true })).toHaveAttribute('title', '2,500,000.00 ISK');

  // Default sorting: item name ascending.
  await page.locator('nav.sub-menu').getByRole('link', { name: 'Sell' }).click();
  await expect(sell.locator('tbody.order').first()).toHaveAttribute('id', 'order-5002');

  // Sort by price, then reverse it. The URL follows without a reload.
  await sell.getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=asc/);
  await expect(sell.locator('tbody.order').first()).toHaveAttribute('id', 'order-5002');
  await sell.getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=desc/);
  await expect(sell.locator('tbody.order').first()).toHaveAttribute('id', 'order-5001');
  // The buy table's sorting is its own.
  await expect(page).not.toHaveURL(/bsort/);
});

test('a stale character keeps its orders listed', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');

  // The refresh token is revoked and the access token is gone: the next poll cannot get a token.
  await stubControl('revoke', { character_id: 90000001 });
  await page.evaluate(async () => {
    const token = document.querySelector('meta[name="csrf-token"]')!.getAttribute('content')!;
    await fetch('/_test/refresh/90000001', { method: 'POST', headers: { 'X-CSRF-Token': token } });
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 needs-relogin');

  await page.goto('/orders/sell');
  await expect(page.locator('#freshness').getByText('Stale')).toBeVisible();
  await expect(page.locator('#freshness').getByRole('link', { name: 'Log in again' })).toBeVisible();
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
});

test('corporation orders carry a marker', async ({ page }) => {
  await loginWith(page, 'Carol Charlie');
  await stubControl('orders', {
    character_id: 90000003,
    orders: [{ ...stubOrder(1), is_corporation: true }, stubOrder(2, { type: 35 })],
  });
  await pollCharacter(page, 90000003);
  await page.goto('/orders/sell');

  await expect(page.locator('#order-1').getByText('Corp', { exact: true })).toBeVisible();
  await expect(page.locator('#order-2').getByText('Corp', { exact: true })).toHaveCount(0);
});

test('the headers list the figures only, in order, for the sell and the buy table', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');

  const labels = async (section: string) =>
    (await page.locator(`${section} thead th`).allInnerTexts()).map((t) => t.replace(/[\u25B2\u25BC]/g, '').trim());
  await page.goto('/orders/sell');
  expect(await labels('#sell-orders')).toEqual([
    'Item', 'Price', 'Standing', 'Behind by', 'Undercut volume', 'Volume', 'Value', 'Margin', 'Time left',
  ]);
  await page.goto('/orders/buy');
  expect(await labels('#buy-orders')).toEqual([
    'Item', 'Price', 'Standing', 'Behind by', 'Undercut volume', 'Volume', 'Escrow', 'Time left',
  ]);
});

test('each order has a dimmed details line: issued, character, location, region', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');

  const details = page.locator('#sell-orders #order-5001 tr.details');
  await expect(details).toHaveText(
    /^\s*\d{4}-\d{2}-\d{2} \d{2}:\d{2} · Alice Alpha · Jita IV - Moon 4 - Caldari Navy Assembly Plant · The Forge\s*$/,
  );
  await expect(details.locator('td')).toHaveAttribute('colspan', '9');
  await page.goto('/orders/buy');
  await expect(page.locator('#buy-orders #order-5003 tr.details td')).toHaveAttribute('colspan', '8');
  await page.goto('/orders/sell');
  // Dimmed: the details line is less prominent than the figures above it.
  await expect(async () => {
    const colours = await page.locator('#sell-orders #order-5001').evaluate((tbody) => [
      getComputedStyle(tbody.querySelector('tr:not(.details) td')!).color,
      getComputedStyle(tbody.querySelector('tr.details td')!).color,
    ]);
    expect(colours[0]).not.toBe('');
    expect(colours[1]).not.toBe(colours[0]);
  }).toPass();
});

test('the Corp marker sits on the details line after the character name', async ({ page }) => {
  await loginWith(page, 'Carol Charlie');
  await stubControl('orders', { character_id: 90000003, orders: [{ ...stubOrder(1), is_corporation: true }] });
  await pollCharacter(page, 90000003);
  await page.goto('/orders/sell');

  const details = page.locator('#order-1 tr.details');
  await expect(details).toContainText('Carol Charlie Corp ·');
  await expect(details.getByText('Corp', { exact: true })).toBeVisible();
  await expect(page.locator('#order-1 tr:not(.details)').getByText('Corp', { exact: true })).toHaveCount(0);
});

test('hovering the details line highlights the whole order', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');

  const order = page.locator('#sell-orders #order-5001');
  const figures = order.locator('tr:not(.details)');
  const background = () => order.evaluate((tbody) => getComputedStyle(tbody.querySelector('tr:not(.details) td')!).backgroundColor);
  const resting = await background();
  await order.locator('tr.details').hover();
  await expect.poll(background).not.toBe(resting);
  await expect(figures).toBeVisible();
});

test('standing and gap show as badges and text, with the exact gap on hover', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { type: 34, price: 1_012_500 }), stubOrder(2, { type: 35, price: 1_500_000 })],
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubMarket([
    stubMarketOrder(1, { type: 34, price: 1_012_500 }),
    stubMarketOrder(11, { type: 34, price: 1_000_000 }),
    stubMarketOrder(12, { type: 34, price: 1_005_000 }),
    stubMarketOrder(13, { type: 34, price: 1_010_000 }),
    stubMarketOrder(2, { type: 35, price: 1_500_000 }),
    stubMarketOrder(21, { type: 35, price: 1_800_000 }),
  ]);

  await page.goto('/orders/sell');
  const dashes = page.locator('#sell-orders #order-1 td').filter({ hasText: '\u2014' });
  await expect(dashes).toHaveCount(4); // no market data yet (standing, gap, undercut volume), and no purchases (margin)

  expect(await marketPoll(page)).toBe('200 2');
  await page.goto('/orders/sell');

  const undercut = page.locator('#sell-orders #order-1');
  await expect(undercut.locator('.badge.undercut')).toHaveText('Undercut 3');
  await expect(undercut.getByText('12.5k (1.3%)', { exact: true })).toHaveAttribute('title', '12,500.00 ISK');
  const top = page.locator('#sell-orders #order-2');
  await expect(top.locator('.badge.top')).toHaveText('Top');
  await expect(top.locator('.badge.undercut')).toHaveCount(0);
  // The two badges differ in colour, and both come from the page's CSS framework.
  const colour = (badge: typeof undercut) => badge.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await colour(top.locator('.badge.top'))).not.toBe(await colour(undercut.locator('.badge.undercut')));
});

test('sorting by standing puts top first, reads the number and keeps orders without data last', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [
      stubOrder(1, { type: 34, price: 100 }),
      stubOrder(2, { type: 35, price: 100 }),
      stubOrder(3, { type: 36, price: 100 }),
      stubOrder(4, { type: 37, price: 100 }),
      { ...stubOrder(5, { type: 38, price: 100 }), location_id: 1000000000001 },
    ],
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  const market = [stubMarketOrder(100, { type: 34, price: 120 })];
  for (const [type, count] of [[35, 3], [36, 12], [37, 1]]) {
    for (let n = 0; n < count; n++) market.push(stubMarketOrder(1000 * type + n, { type, price: 99 - n / 100 }));
  }
  await stubMarket(market);
  await marketPoll(page);
  await page.goto('/orders/sell');

  const ids = async () => page.locator('#sell-orders tbody.order').evaluateAll((rows) => rows.map((r) => r.id));
  const standing = page.locator('#sell-orders').getByRole('link', { name: 'Standing' });
  await standing.click();
  await expect(page).toHaveURL(/sort=standing&dir=asc/);
  await expect.poll(ids).toEqual(['order-1', 'order-4', 'order-2', 'order-3', 'order-5']);

  await page.locator('#sell-orders').getByRole('link', { name: 'Standing' }).click();
  await expect(page).toHaveURL(/sort=standing&dir=desc/);
  await expect.poll(ids).toEqual(['order-3', 'order-2', 'order-4', 'order-1', 'order-5']);
});

test('the undercut volume shows in both tables, empty for top and a dash without data, and sorts', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [
      stubOrder(1, { type: 34, price: 1_012_500 }),
      stubOrder(2, { type: 35, price: 100 }),
      { ...stubOrder(3, { type: 36, price: 100 }), location_id: 1000000000001 },
      stubOrder(4, { type: 34, price: 50, buy: true }),
    ],
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubMarket([
    stubMarketOrder(1, { type: 34, price: 1_012_500 }),
    stubMarketOrder(11, { type: 34, price: 1_000_000, remain: 500 }),
    stubMarketOrder(12, { type: 34, price: 1_005_000, remain: 500 }),
    stubMarketOrder(13, { type: 34, price: 1_010_000, remain: 250 }),
    stubMarketOrder(2, { type: 35, price: 100 }),
    stubMarketOrder(21, { type: 35, price: 200, remain: 77 }),
    stubMarketOrder(4, { type: 34, price: 50, buy: true }),
    stubMarketOrder(41, { type: 34, price: 55, buy: true, range: 'region', remain: 300 }),
    stubMarketOrder(42, { type: 34, price: 60, buy: true, range: 'region', remain: 1000 }),
  ]);
  expect(await marketPoll(page)).toBe('200 3');
  await page.goto('/orders/buy');

  const volumeCell = (row: string) => page.locator(`${row} td`).nth(4);
  await expect(volumeCell('#buy-orders #order-4')).toHaveText('1,300');
  await page.goto('/orders/sell');
  await expect(page.locator('#sell-orders thead th').nth(4)).toContainText('Undercut volume');
  await expect(volumeCell('#sell-orders #order-1')).toHaveText('1,250');
  await expect(volumeCell('#sell-orders #order-2')).toHaveText('');
  await expect(volumeCell('#sell-orders #order-3')).toHaveText('\u2014');

  const ids = async () => page.locator('#sell-orders tbody.order').evaluateAll((rows) => rows.map((r) => r.id));
  await page.locator('#sell-orders').getByRole('link', { name: 'Undercut volume' }).click();
  await expect(page).toHaveURL(/sort=undercut-volume&dir=asc/);
  await expect.poll(ids).toEqual(['order-2', 'order-1', 'order-3']);
  await page.locator('#sell-orders').getByRole('link', { name: 'Undercut volume' }).click();
  await expect(page).toHaveURL(/sort=undercut-volume&dir=desc/);
  await expect.poll(ids).toEqual(['order-1', 'order-2', 'order-3']);
});

/** Alice has fees known (Accounting 5, Broker Relations 5), and three sell orders of 10 units each: 1 earns, 2 loses, 3 has a cost for 4 of its 10 units. */
async function aliceWithMargins(page: import('@playwright/test').Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(5, 5) });
  await stubControl('orders', {
    character_id: 90000001,
    orders: [
      stubOrder(1, { type: 34, price: 150, remain: 10 }),
      stubOrder(2, { type: 36, price: 100, remain: 10 }),
      stubOrder(3, { type: 35, price: 120, remain: 10 }),
      stubOrder(4, { type: 34, price: 50, buy: true }),
    ],
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubControl('transactions', {
    character_id: 90000001,
    transactions: [
      stubTransaction(1, { type: 34, quantity: 10 }),
      stubTransaction(2, { type: 36, quantity: 10 }),
      stubTransaction(3, { type: 35, quantity: 4 }),
    ],
  });
  expect(await wealthPoll(page, 90000001, 'transactions')).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.goto('/orders/sell');
}

test('the margin shows after the value with its exact value and cost on hover, and a loss in the error style', async ({ page }) => {
  await aliceWithMargins(page);

  const labels = (await page.locator('#sell-orders thead th').allInnerTexts()).map((t) => t.replace(/[\u25B2\u25BC]/g, '').trim());
  expect(labels.indexOf('Margin')).toBe(labels.indexOf('Value') + 1);
  const cells = (id: number) => page.locator(`#sell-orders #order-${id} td`);
  // 10 * 150 * (1 - 3.375% - 1.41%) - 10 * 100 = 428.225
  const margin = cells(1).nth(labels.indexOf('Margin'));
  await expect(margin).toHaveText('428.23');
  await expect(margin).toHaveAttribute('title', '428.23 ISK; cost per unit 100.00 ISK');
  await expect(margin.locator('.badge.loss')).toHaveCount(0);
  // 10 * 100 * 0.95215 - 10 * 100 = -47.85
  const loss = cells(2).nth(labels.indexOf('Margin'));
  await expect(loss.locator('.badge.loss')).toHaveText('-47.85');
  const colour = (el: typeof loss) => el.evaluate((e) => getComputedStyle(e).backgroundColor);
  // The error style gives the badge a background of its own.
  expect(await colour(loss.locator('.badge.loss'))).not.toBe('rgba(0, 0, 0, 0)');
});

test('a cost known for part of the units shows a dash and says how many on hover', async ({ page }) => {
  await aliceWithMargins(page);

  const partial = page.locator('#sell-orders #order-3 td[title^="cost known"]');
  await expect(partial).toHaveText('\u2014');
  await expect(partial).toHaveAttribute('title', 'cost known for 4 of 10 units');
});

test('sorting by margin descending puts the best first and orders without a margin last, and the buy table has no margin', async ({ page }) => {
  await aliceWithMargins(page);

  const ids = async () => page.locator('#sell-orders tbody.order').evaluateAll((rows) => rows.map((r) => r.id));
  const header = page.locator('#sell-orders').getByRole('link', { name: 'Margin' });
  await header.click();
  await expect(page).toHaveURL(/sort=margin&dir=asc/);
  await expect.poll(ids).toEqual(['order-2', 'order-1', 'order-3']);
  await page.locator('#sell-orders').getByRole('link', { name: 'Margin' }).click();
  await expect(page).toHaveURL(/sort=margin&dir=desc/);
  await expect.poll(ids).toEqual(['order-1', 'order-2', 'order-3']);

  await page.locator('nav.sub-menu').getByRole('link', { name: 'Buy' }).click();
  await expect(page.locator('#buy-orders').getByRole('link', { name: 'Margin' })).toHaveCount(0);
  await expect(page.locator('#buy-orders #order-4')).toBeVisible();
});

test('the sub-menu switches between the sell and the buy page and keeps each sorting', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/buy?bsort=price&bdir=desc');
  const menu = page.locator('nav.sub-menu');
  await expect(menu.getByRole('link')).toHaveText(['Sell', 'Buy']);
  await expect(menu.getByRole('link', { name: 'Buy' })).toHaveAttribute('aria-current', 'page');

  await menu.getByRole('link', { name: 'Sell' }).click();
  await expect(page).toHaveURL(/\/orders\/sell\?bsort=price&bdir=desc$/);
  await expect(menu.getByRole('link', { name: 'Sell' })).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('#sell-orders')).toBeVisible();

  await menu.getByRole('link', { name: 'Buy' }).click();
  await expect(page).toHaveURL(/\/orders\/buy\?bsort=price&bdir=desc$/);
  await expect(page.locator('#buy-orders th[aria-sort="descending"]')).toContainText('Price');
});

/** Alice with 120 sell orders, price 10 + id, after her first poll. */
async function aliceWith120Sells(page: import('@playwright/test').Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: Array.from({ length: 120 }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell?sort=price&dir=asc');
}

const rowIds = (page: import('@playwright/test').Page) =>
  page.locator('#sell-orders tbody.order').evaluateAll((rows) => rows.map((r) => Number(r.id.replace('order-', ''))));

test('the next page shows orders 51 to 100 with the sorting and selection unchanged, and a page size of 200 shows all', async ({ page }) => {
  await aliceWith120Sells(page);
  const pagination = page.locator('.pagination');
  await expect(pagination).toContainText('Page 1 of 3');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(50);

  await pagination.getByRole('link', { name: 'Next' }).click();
  await expect(page).toHaveURL(/sort=price&dir=asc&page=2$/);
  await expect(pagination).toContainText('Page 2 of 3');
  expect((await rowIds(page))[0]).toBe(51);
  expect((await rowIds(page)).at(-1)).toBe(100);
  await expect(page.locator('#kpi-sell-count .value')).toHaveText('120');

  await pagination.getByLabel('Rows per page').selectOption('200');
  await expect(page).toHaveURL(/sort=price&dir=asc&per_page=200$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(120);
  await expect(pagination).toContainText('Page 1 of 1');
  // The table's top is in view after a page change.
  const top = (await page.locator('#sell-orders').boundingBox())!;
  expect(top.y).toBeGreaterThanOrEqual(0);
});

test('a page past the end shows the last page, and sorting goes back to the first page', async ({ page }) => {
  await aliceWith120Sells(page);

  await page.goto('/orders/sell?page=9');
  await expect(page.locator('.pagination')).toContainText('Page 3 of 3');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(20);

  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=asc$/);
  await expect(page.locator('.pagination')).toContainText('Page 1 of 3');
});

test('the freshness is below the pagination, unfolded, without a toggle', async ({ page }) => {
  await aliceWith120Sells(page);

  await expect(page.getByRole('button', { name: 'Freshness' })).toHaveCount(0);
  const freshness = page.locator('#freshness');
  await expect(freshness).toContainText('Last update');
  await expect(freshness).toContainText('Alice Alpha');
  const bar = (await page.locator('.pagination').boundingBox())!;
  expect((await freshness.boundingBox())!.y).toBeGreaterThanOrEqual(bar.y + bar.height);
});
