import { expect, loginWith, pollCharacter, pollOutsideBrowser, stubControl, stubOrder, test } from '../support/fixtures';

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
