import { expect, loginWith, pollCharacter, stubControl, stubOrder, test } from '../support/fixtures';

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
