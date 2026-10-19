import type { Page } from '@playwright/test';
import { aliceTrained, bookOf, crossed, fill } from '../support/opportunities';
import { expect, pollCharacter, stubAsset, stubControl, stubOrder, stubTransaction, test, wealthPoll } from '../support/fixtures';

/**
 * Visits every page and uses every sub-menu link, sorting link and pagination control on it. The
 * `cspGuard` fixture of the suite fails the test on the first Content Security Policy violation the
 * console reports, so the sweep only has to make the pages do everything they do.
 */
async function useControls(page: Page, path: string) {
  await page.goto(path);
  await expect(page.locator('main')).toBeVisible();
  const menu = page.locator('nav.sub-menu');
  if (await menu.count()) {
    for (const link of await menu.getByRole('link').all()) {
      const href = await link.getAttribute('href');
      if (href && !href.startsWith('#')) {
        await link.click();
        await expect(page).toHaveURL(new RegExp(href.split('?')[0].replace(/\//g, '\\/')));
      } else {
        await link.click();
      }
    }
  }
  const next = page.locator('.pagination').getByRole('link', { name: 'Next' });
  if (await next.count()) {
    await next.click();
    await expect(page.locator('.pagination')).toContainText('Page 2');
    await page.locator('.pagination').getByRole('link', { name: 'Previous' }).click();
    await page.locator('.pagination').getByLabel('Rows per page').selectOption('100');
    await expect(page.locator('.pagination')).toContainText('Page 1');
  }
  const sorter = page.locator('main thead th a').first();
  if (await sorter.count()) await sorter.click();
  await page.waitForTimeout(150);
}

test('every page and every sub-menu link, sorting link and pagination control raise no policy violation', async ({ page }) => {
  await aliceTrained(page);
  await stubControl('orders', {
    character_id: 90000001,
    orders: [
      ...Array.from({ length: 60 }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
      ...Array.from({ length: 60 }, (_, i) => stubOrder(1001 + i, { buy: true, price: 10 + i })),
    ],
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubControl('assets', {
    character_id: 90000001,
    assets: Array.from({ length: 60 }, (_, i) => stubAsset(i + 1, { type: 1000 + i, quantity: 1 })),
  });
  await wealthPoll(page, 90000001, 'assets');
  await wealthPoll(page, 90000001, 'wallet');
  await stubControl('transactions', {
    character_id: 90000001,
    transactions: Array.from({ length: 60 }, (_, i) => stubTransaction(i + 1, { type: 34, quantity: 1, price: 100, buy: false, daysAgo: 1 })),
  });
  await wealthPoll(page, 90000001, 'transactions');
  await page.goto('/opportunities/classic');
  await fill(page, [...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)], crossed(34, 1.1).concat(crossed(35, 1.2)));

  for (const path of [
    '/dashboard',
    '/characters',
    '/wealth',
    '/trades',
    '/orders/sell',
    '/orders/buy',
    '/opportunities/expansion',
    '/opportunities/classic',
    '/opportunities/reversion',
    '/opportunities/trend',
  ]) {
    await useControls(page, path);
  }

  // The detail page of a row, its sub-menu and its jumps.
  await page.goto('/opportunities/classic');
  await page.locator('#classic tr[data-type-id="34"]').click();
  await expect(page).toHaveURL(/\/opportunities\/classic\/34/);
  for (const link of await page.locator('nav.sub-menu').getByRole('link').all()) {
    const href = await link.getAttribute('href');
    if (href?.startsWith('#')) await link.click();
  }
  await page.locator('nav.sub-menu').getByRole('link').first().click();
  await expect(page).toHaveURL(/\/opportunities\/classic(\?|$)/);

  // The sidebar toggle, the filters and the character filter.
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.locator('#opportunity-filters').getByLabel('Item name').fill('trit');
  await page.locator('#opportunity-filters').getByLabel('Item name').press('Tab');
  await page.waitForTimeout(300);
});
