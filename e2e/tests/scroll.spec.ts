import type { Page } from '@playwright/test';
import {
  expect, loginWith, marketPoll, openCharacters, pollCharacter, stubControl, stubMarket, stubMarketOrder, stubOrder,
  stubTransaction, test, wealthPoll,
} from '../support/fixtures';

const scrollY = (page: Page) => page.evaluate(() => window.scrollY);
const scrollLeft = (page: Page) => page.locator('.table-wrap').first().evaluate((el) => el.scrollLeft);

/**
 * Runs `action`, then waits until the live refresh it causes has replaced what the page shows: the
 * table wrapper is tagged first and must be gone afterwards.
 */
async function afterRefresh(page: Page, path: string, action: () => Promise<unknown>) {
  await page.locator('.table-wrap').first().evaluate((el) => ((el as unknown as { stale: boolean }).stale = true));
  const refreshed = page.waitForResponse((r) => r.url().includes(path));
  await action();
  await refreshed;
  await expect
    .poll(() => page.locator('.table-wrap').first().evaluate((el) => Boolean((el as unknown as { stale?: boolean }).stale)))
    .toBe(false);
}

/** Alice with 200 sell orders and a few buy orders on a narrow page: it scrolls down and the table scrolls sideways. */
async function manyOrders(page: Page) {
  await page.setViewportSize({ width: 800, height: 600 });
  await loginWith(page, 'Alice Alpha');
  const orders = [
    ...Array.from({ length: 200 }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
    ...Array.from({ length: 5 }, (_, i) => stubOrder(1001 + i, { buy: true, price: 10 + i })),
  ];
  await stubControl('orders', { character_id: 90000001, orders });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  const opened = page.waitForResponse((r) => r.url().includes('/orders/sell/content'));
  await page.goto('/orders/sell?per_page=200');
  await opened;
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(200);
  await page.waitForTimeout(300);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
}

const samePage = (page: Page) => page.evaluate(() => (window as unknown as { marker: string }).marker);

test('a poll keeps where the page is scrolled and how far the table is scrolled sideways', async ({ page }) => {
  await manyOrders(page);
  const wrap = page.locator('.table-wrap').first();
  const sizes = await wrap.evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth }));
  expect(sizes.scroll).toBeGreaterThan(sizes.client);
  await wrap.evaluate((el) => (el.scrollLeft = 80));
  await page.evaluate(() => window.scrollTo(0, 1500));
  await expect.poll(() => scrollY(page)).toBeGreaterThan(1000);
  const [y, x] = [await scrollY(page), await scrollLeft(page)];
  expect(x).toBeGreaterThan(0);

  await afterRefresh(page, '/orders/sell/content', () => pollCharacter(page, 90000001));

  expect(Math.abs((await scrollY(page)) - y)).toBeLessThanOrEqual(1);
  expect(Math.abs((await scrollLeft(page)) - x)).toBeLessThanOrEqual(1);
  expect(await samePage(page)).toBe('same page');
});

test('a market refresh keeps where the page is scrolled', async ({ page }) => {
  await manyOrders(page);
  await page.evaluate(() => window.scrollTo(0, 2500));
  await expect.poll(() => scrollY(page)).toBeGreaterThan(2000);
  const y = await scrollY(page);

  // Someone undercuts order 1: its standing changes, which signals market-updated.
  await afterRefresh(page, '/orders/sell/content', async () => {
    await stubMarket([stubMarketOrder(1, { price: 10 }), stubMarketOrder(9001, { price: 5 })]);
    expect(await marketPoll(page)).toBe('200 1');
  });

  expect(Math.abs((await scrollY(page)) - y)).toBeLessThanOrEqual(1);
});

test('a shorter list after a refresh shows its end instead of staying beyond it', async ({ page }) => {
  await manyOrders(page);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => scrollY(page)).toBeGreaterThan(3000);

  await afterRefresh(page, '/orders/sell/content', async () => {
    await stubControl('orders', {
      character_id: 90000001,
      orders: Array.from({ length: 60 }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
    });
    await pollCharacter(page, 90000001);
  });

  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(60);
  const end = await page.evaluate(() => document.documentElement.scrollHeight - window.innerHeight - window.scrollY);
  expect(end).toBeLessThanOrEqual(1);
});

test('sorting shows the sorted table from its top', async ({ page }) => {
  await manyOrders(page);
  await page.evaluate(() => window.scrollTo(0, 2000));
  await expect.poll(() => scrollY(page)).toBeGreaterThan(1500);

  // The header scrolled out of view: activating its link by script is what a keyboard user does.
  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).evaluate((a: HTMLElement) => a.click());

  await expect(page).toHaveURL(/sort=price&dir=asc/);
  await expect(page.locator('#sell-orders th[aria-sort="ascending"]')).toContainText('Price');
  // Right below the sub-menu, which stays at the top of the window.
  const top = (await page.locator('#content').boundingBox())!;
  const menu = (await page.locator('nav.sub-menu').boundingBox())!;
  expect(top.y).toBeGreaterThanOrEqual(menu.y + menu.height - 1);
  expect(top.y).toBeLessThanOrEqual(menu.y + menu.height + 20);
});

test('changing the character filter shows the filtered table from its top', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await stubControl('orders', { character_id: 90000001, orders: Array.from({ length: 200 }, (_, i) => stubOrder(i + 1)) });
  await pollCharacter(page, 90000001);
  await page.goto('/orders/sell?per_page=200');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(200);
  await page.evaluate(() => window.scrollTo(0, 1500));

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck({ force: true });

  await expect(page).toHaveURL(/chars=90000001/);
  await expect.poll(() => scrollY(page)).toBeLessThan(400);
});

test('a scrolled trades table keeps its position on trades-updated', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await loginWith(page, 'Alice Alpha');
  const transactions = Array.from({ length: 200 }, (_, i) =>
    stubTransaction(i + 1, { type: 34, quantity: 1, price: 100 + i, daysAgo: 1 + i / 100 }),
  );
  await stubControl('transactions', { character_id: 90000001, transactions });
  expect(await wealthPoll(page, 90000001, 'transactions')).toBe('200 updated');
  const opened = page.waitForResponse((r) => r.url().includes('/trades/table'));
  await page.goto('/trades?per_page=200');
  await opened;
  await expect(page.locator('#trades tbody tr')).toHaveCount(200);
  await page.waitForTimeout(300);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await page.evaluate(() => window.scrollTo(0, 1800));
  await expect.poll(() => scrollY(page)).toBeGreaterThan(1500);
  const y = await scrollY(page);

  await afterRefresh(page, '/trades/table', async () => {
    await stubControl('transactions', {
      character_id: 90000001,
      transactions: [...transactions, stubTransaction(500, { type: 35, quantity: 1, price: 10, daysAgo: 0 })],
    });
    expect(await wealthPoll(page, 90000001, 'transactions')).toBe('200 updated');
  });

  expect(Math.abs((await scrollY(page)) - y)).toBeLessThanOrEqual(1);
  expect(await samePage(page)).toBe('same page');
});
