import {
  expect, loginWith, marketPoll, marketPollOutsideBrowser, pollCharacter, pollOutsideBrowser, stubControl, stubMarket,
  stubMarketOrder, stubOrder, test,
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
  await expect(page.locator('#sell-orders tbody tr')).toHaveCount(2);
  await expect(page.locator('#sell-orders tbody tr').first()).toHaveAttribute('id', 'order-2');

  // Prices change; the order of the rows flips under the same sorting, Bob stays out.
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 50 }), stubOrder(2, { type: 35, price: 9 })],
  });
  await pollCharacter(page, 90000001);
  await pollCharacter(page, 90000002);

  await expect(page.locator('#sell-orders tbody tr').first()).toHaveAttribute('id', 'order-1');
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
  await expect(page.locator('#sell-orders tbody tr')).toHaveCount(2);
  await expect(page.locator('#order-100')).toHaveCount(0);

  // Order 2 is undercut once. Descending, it comes first now; Bob stays out.
  await stubMarket([
    stubMarketOrder(1, { price: 100 }),
    stubMarketOrder(2, { type: 35, price: 100 }),
    stubMarketOrder(3, { type: 35, price: 90 }),
  ]);
  await marketPoll(page);

  await expect(page.locator('#order-2 .badge.undercut')).toHaveText('Undercut 1');
  await expect(page.locator('#sell-orders tbody tr').first()).toHaveAttribute('id', 'order-2');
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
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
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
