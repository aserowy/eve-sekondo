import type { Page } from '@playwright/test';
import {
  expect, loginWith, marketPoll, openCharacters, pollCharacter, query, stubControl, stubMarket, stubMarketOrder, stubOrder, test, wealthPoll, stubAsset, feePoll, stubTradeSkills, unfoldFreshness,
} from '../support/fixtures';

const scroller = (page: Page, key: string) => page.locator(`[data-scroll-key="${key}"]`);

const scrollTop = (page: Page, key: string) => scroller(page, key).evaluate((el) => el.scrollTop);

async function scrollTo(page: Page, key: string, fraction: number): Promise<number> {
  return scroller(page, key).evaluate((el, f) => {
    el.scrollTop = Math.round((el.scrollHeight - el.clientHeight) * f);
    return el.scrollTop;
  }, fraction);
}

/**
 * Runs `action`, then waits until the live refresh it causes has replaced the panel `key`: the old
 * panel element is tagged first and must be gone afterwards.
 */
async function afterRefresh(page: Page, key: string, path: string, action: () => Promise<unknown>) {
  await scroller(page, key).evaluate((el) => ((el as unknown as { stale: boolean }).stale = true));
  const refreshed = page.waitForResponse((r) => r.url().includes(path));
  await action();
  await refreshed;
  await expect
    .poll(() => scroller(page, key).evaluate((el) => Boolean((el as unknown as { stale?: boolean }).stale)))
    .toBe(false);
}

/** Alice with 200 sell and 60 buy orders on the orders page, after the stream's first refresh. */
async function manyOrders(page: Page, sells = 200) {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  const orders = [
    ...Array.from({ length: sells }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
    ...Array.from({ length: 60 }, (_, i) => stubOrder(1001 + i, { buy: true, price: 10 + i })),
  ];
  await stubControl('orders', { character_id: 90000001, orders });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  const opened = page.waitForResponse((r) => r.url().includes('/orders/tables'));
  await page.goto('/orders');
  await opened;
  await page.waitForTimeout(300);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
}

test('a poll keeps the scroll position of the sell and the buy table', async ({ page }) => {
  await manyOrders(page);
  const sell = await scrollTo(page, 'sell-orders', 0.5);
  const buy = await scrollTo(page, 'buy-orders', 0.3);
  expect(sell).toBeGreaterThan(0);
  expect(buy).toBeGreaterThan(0);

  await afterRefresh(page, 'sell-orders', '/orders/tables', () => pollCharacter(page, 90000001));

  expect(Math.abs((await scrollTop(page, 'sell-orders')) - sell)).toBeLessThanOrEqual(1);
  expect(Math.abs((await scrollTop(page, 'buy-orders')) - buy)).toBeLessThanOrEqual(1);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('a market refresh keeps the scroll position', async ({ page }) => {
  await manyOrders(page);
  const sell = await scrollTo(page, 'sell-orders', 0.6);

  // Someone undercuts order 1: its standing changes, which signals market-updated.
  await afterRefresh(page, 'sell-orders', '/orders/tables', async () => {
    await stubMarket([stubMarketOrder(1, { price: 10 }), stubMarketOrder(9001, { price: 5 })]);
    expect(await marketPoll(page)).toBe('200 1');
  });

  expect(Math.abs((await scrollTop(page, 'sell-orders')) - sell)).toBeLessThanOrEqual(1);
});

test('a shorter table after a refresh shows its end', async ({ page }) => {
  await manyOrders(page);
  await scrollTo(page, 'sell-orders', 1);

  await afterRefresh(page, 'sell-orders', '/orders/tables', async () => {
    await stubControl('orders', { character_id: 90000001, orders: Array.from({ length: 60 }, (_, i) => stubOrder(i + 1, { price: 10 + i })) });
    await pollCharacter(page, 90000001);
  });

  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(60);
  const end = await scroller(page, 'sell-orders').evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop);
  expect(end).toBeLessThanOrEqual(1);
  await expect(page.locator('#order-60')).toBeInViewport();
});

test('sorting shows the sorted table from the top', async ({ page }) => {
  await manyOrders(page);
  await scrollTo(page, 'sell-orders', 0.5);

  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=asc/);
  await expect(page.locator('#sell-orders th[aria-sort="ascending"]')).toContainText('Price');
  expect(await scrollTop(page, 'sell-orders')).toBe(0);
});

test('changing the character filter shows the filtered table from the top', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await stubControl('orders', { character_id: 90000001, orders: Array.from({ length: 200 }, (_, i) => stubOrder(i + 1)) });
  await pollCharacter(page, 90000001);
  await page.goto('/orders');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(200);
  await scrollTo(page, 'sell-orders', 0.5);

  await page.getByRole('navigation').getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);
  expect(await scrollTop(page, 'sell-orders')).toBe(0);
});

/** Alice plus nineteen characters linked straight in the database (the stub only knows three). */
async function twentyCharacters(page: Page) {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await query(
    `INSERT INTO characters (character_id, account_id, name, owner_hash)
     SELECT 91000000 + n, (SELECT account_id FROM characters WHERE character_id = 90000001), 'Extra Character ' || n, 'hash-' || n
     FROM generate_series(1, 19) AS n`,
  );
}

test('an orders signal keeps the scroll position of the dashboard breakdown', async ({ page }) => {
  await twentyCharacters(page);
  const opened = page.waitForResponse((r) => r.url().includes('/dashboard/summary'));
  await page.goto('/dashboard');
  await opened;
  await page.waitForTimeout(300);
  const top = await scrollTo(page, 'breakdown', 1);
  expect(top).toBeGreaterThan(0);

  await afterRefresh(page, 'breakdown', '/dashboard/summary', () => pollCharacter(page, 90000001));

  expect(Math.abs((await scrollTop(page, 'breakdown')) - top)).toBeLessThanOrEqual(1);
});

test('a wealth signal keeps the scroll position of the items table', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('assets', { character_id: 90000001, assets: Array.from({ length: 80 }, (_, i) => stubAsset(i + 1, { type: 34 + i })) });
  expect(await wealthPoll(page, 90000001, 'assets')).toBe('200 updated');
  const opened = page.waitForResponse((r) => r.url().includes('/wealth/items'));
  await page.goto('/wealth');
  await opened;
  await page.waitForTimeout(300);
  await expect(page.locator('#inventory tbody tr')).toHaveCount(80);
  const top = await scrollTo(page, 'inventory', 0.5);
  expect(top).toBeGreaterThan(0);

  await afterRefresh(page, 'inventory', '/wealth/items', () => wealthPoll(page, 90000001, 'wallet'));

  expect(Math.abs((await scrollTop(page, 'inventory')) - top)).toBeLessThanOrEqual(1);
});

test('a fees signal keeps the scroll position of the freshness column', async ({ page }) => {
  await twentyCharacters(page);
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(5, 5) });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await feePoll(page);
  const opened = page.waitForResponse((r) => r.url().includes('/orders/freshness'));
  await page.goto('/orders');
  await opened;
  await page.waitForTimeout(300);
  await unfoldFreshness(page);
  const top = await scrollTo(page, 'freshness', 1);
  expect(top).toBeGreaterThan(0);

  // Accounting 4 changes Alice's sales tax, which signals fees-updated.
  await afterRefresh(page, 'freshness', '/orders/freshness', async () => {
    await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(4, 5) });
    await feePoll(page);
  });

  expect(Math.abs((await scrollTop(page, 'freshness')) - top)).toBeLessThanOrEqual(1);
});
