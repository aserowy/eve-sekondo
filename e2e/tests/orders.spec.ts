import {
  expect, loginWith, marketPoll, pollCharacter, stubControl, stubMarket, stubMarketOrder, stubOrder, test,
} from '../support/fixtures';

test('sell and buy orders show up after a poll, sortable, with exact amounts on hover', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders$/);

  // Nothing has been polled yet.
  await expect(page.getByText('Waiting for first update')).toBeVisible();
  await expect(page.getByText('No sell orders.')).toBeVisible();
  await expect(page.getByText('No buy orders.')).toBeVisible();

  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.reload();

  // The stub's fixture: two sell orders and a buy order in a structure.
  const sell = page.locator('#sell-orders');
  const buy = page.locator('#buy-orders');
  await expect(sell.locator('tbody tr')).toHaveCount(2);
  await expect(buy.locator('tbody tr')).toHaveCount(1);

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

  const mexallon = buy.locator('#order-5003');
  await expect(mexallon).toContainText('Mexallon');
  await expect(mexallon).toContainText('Alpha Keepstar');
  await expect(mexallon.getByText('2.5m', { exact: true })).toHaveAttribute('title', '2,500,000.00 ISK');

  // Default sorting: item name ascending.
  await expect(sell.locator('tbody tr').first()).toHaveAttribute('id', 'order-5002');

  // Sort by price, then reverse it. The URL follows without a reload.
  await sell.getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=asc/);
  await expect(sell.locator('tbody tr').first()).toHaveAttribute('id', 'order-5002');
  await sell.getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=desc/);
  await expect(sell.locator('tbody tr').first()).toHaveAttribute('id', 'order-5001');
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

  await page.goto('/orders');
  await expect(page.locator('#freshness').getByText('Stale')).toBeVisible();
  await expect(page.locator('#freshness').getByRole('link', { name: 'Log in again' })).toBeVisible();
  await expect(page.locator('#sell-orders tbody tr')).toHaveCount(2);
});

test('corporation orders carry a marker', async ({ page }) => {
  await loginWith(page, 'Carol Charlie');
  await stubControl('orders', {
    character_id: 90000003,
    orders: [{ ...stubOrder(1), is_corporation: true }, stubOrder(2, { type: 35 })],
  });
  await pollCharacter(page, 90000003);
  await page.goto('/orders');

  await expect(page.locator('#order-1').getByText('Corp', { exact: true })).toBeVisible();
  await expect(page.locator('#order-2').getByText('Corp', { exact: true })).toHaveCount(0);
});

test('location and region are the last two columns of both tables', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders');

  const labels = async (section: string) =>
    (await page.locator(`${section} thead th`).allInnerTexts()).map((t) => t.replace(/[\u25B2\u25BC]/g, '').trim());
  const sell = await labels('#sell-orders');
  const buy = await labels('#buy-orders');
  expect(sell.slice(-3)).toEqual(['Time left', 'Location', 'Region']);
  expect(buy.slice(-3)).toEqual(['Time left', 'Location', 'Region']);

  // The cells follow the headers.
  const cells = await page.locator('#sell-orders #order-5001 td').allInnerTexts();
  expect(cells.slice(-2)).toEqual(['Jita IV - Moon 4 - Caldari Navy Assembly Plant', 'The Forge']);
  expect(cells).toHaveLength(sell.length);
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

  await page.goto('/orders');
  const dashes = page.locator('#sell-orders #order-1 td').filter({ hasText: '\u2014' });
  await expect(dashes).toHaveCount(2); // no market data yet

  expect(await marketPoll(page)).toBe('200 2');
  await page.goto('/orders');

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
  await page.goto('/orders');

  const ids = async () => page.locator('#sell-orders tbody tr').evaluateAll((rows) => rows.map((r) => r.id));
  const standing = page.locator('#sell-orders').getByRole('link', { name: 'Standing' });
  await standing.click();
  await expect(page).toHaveURL(/sort=standing&dir=asc/);
  await expect.poll(ids).toEqual(['order-1', 'order-4', 'order-2', 'order-3', 'order-5']);

  await page.locator('#sell-orders').getByRole('link', { name: 'Standing' }).click();
  await expect(page).toHaveURL(/sort=standing&dir=desc/);
  await expect.poll(ids).toEqual(['order-3', 'order-2', 'order-4', 'order-1', 'order-5']);
});
