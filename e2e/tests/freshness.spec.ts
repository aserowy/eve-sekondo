import type { Page } from '@playwright/test';
import { expect, loginWith, openCharacters, pollCharacter, query, stubControl, stubOrder, test, wealthPoll } from '../support/fixtures';

async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

test('every table page has its freshness at the end, unfolded, with no toggle and no warning count', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await query('UPDATE characters SET needs_relogin = true');

  for (const path of ['/orders/sell', '/orders/buy', '/wealth', '/trades', '/opportunities/expansion', '/opportunities/trend']) {
    await page.goto(path);
    const freshness = page.locator('#freshness');
    await expect(freshness, path).toBeVisible();
    await expect(freshness.getByRole('heading', { name: 'Freshness' }), path).toBeVisible();
    await expect(page.getByRole('button', { name: 'Freshness' }), path).toHaveCount(0);
    await expect(page.locator('.freshness-toggle, .warnings'), path).toHaveCount(0);
    // The freshness is the last thing on the page.
    const last = await page.locator('main section, main nav.pagination, main .kpis').evaluateAll((els) =>
      Math.max(...els.map((el) => el.getBoundingClientRect().bottom)),
    );
    expect((await freshness.boundingBox())!.y + (await freshness.boundingBox())!.height, path).toBeGreaterThanOrEqual(last - 1);
  }
});

test('a character needing re-login is stale with a link to log in again on the orders, wealth and trades pages', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await query('UPDATE characters SET needs_relogin = true WHERE character_id = 90000001');

  for (const path of ['/orders/sell', '/wealth', '/trades']) {
    await page.goto(path);
    const freshness = page.locator('#freshness');
    await expect(freshness, path).toContainText('Alice Alpha');
    await expect(freshness.getByText('Stale').first(), path).toBeVisible();
    await expect(freshness.getByRole('link', { name: 'Log in again' }), path).toBeVisible();
  }
});

test('the orders freshness survives a live refresh and a sort change', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1), stubOrder(2, { price: 20 })] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');

  const refreshed = page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/freshness');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await refreshed;
  await expect(page.locator('#freshness')).toContainText('Last update');

  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price/);
  await expect(page.locator('#freshness')).toContainText('Last update');
});

test('the wealth freshness follows the selection', async ({ page }) => {
  await twoCharacters(page);
  for (const id of [90000001, 90000002]) {
    await wealthPoll(page, id, 'wallet');
    await wealthPoll(page, id, 'assets');
  }
  await page.goto('/wealth');
  await expect(page.locator('#freshness')).toContainText('Bob Bravo');

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();

  await expect(page).toHaveURL(/chars=90000001/);
  await expect(page.locator('#freshness')).toContainText('Alice Alpha');
  await expect(page.locator('#freshness')).not.toContainText('Bob Bravo');
});

test('the trades freshness marks the characters whose transactions are stale', async ({ page }) => {
  await twoCharacters(page);
  await wealthPoll(page, 90000001, 'transactions');
  await wealthPoll(page, 90000002, 'transactions');
  await page.goto('/trades');
  await expect(page.locator('#freshness').getByText('Stale')).toHaveCount(0);

  await query(
    "UPDATE wealth_sync SET last_success_at = now() - interval '3 hours' WHERE kind = 'transactions' AND character_id = 90000002",
  );
  await page.goto('/trades');

  await expect(page.locator('#freshness li', { hasText: 'Bob Bravo' }).getByText('Stale')).toBeVisible();
  await expect(page.locator('#freshness li', { hasText: 'Alice Alpha' }).getByText('Stale')).toHaveCount(0);
});
