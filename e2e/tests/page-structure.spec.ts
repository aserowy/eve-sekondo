import { expect, loginWith, pollCharacter, stubControl, stubOrder, test } from '../support/fixtures';
import { aliceTrained } from '../support/opportunities';

test('every signed-in page has exactly one page title naming the page', async ({ page }) => {
  await aliceTrained(page);
  const pages = [
    ['/dashboard', 'Dashboard'],
    ['/characters', 'Characters'],
    ['/wealth', 'Wealth'],
    ['/trades', 'Trades'],
    ['/orders/sell', 'Sell orders'],
    ['/orders/buy', 'Buy orders'],
    ['/opportunities/expansion', 'Recurring spread expansion'],
    ['/opportunities/trend', 'Trend'],
  ] as const;
  for (const [path, title] of pages) {
    await page.goto(path);
    await expect(page.getByRole('heading', { level: 1 }), path).toHaveText([title]);
  }
});

test('the page header shows the title above the sub-menu, and the sub-menu above the filter section', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/orders/buy');

  const title = (await page.getByRole('heading', { level: 1 }).boundingBox())!;
  const menu = (await page.locator('nav.sub-menu').boundingBox())!;
  const filters = (await page.locator('#filter-box').boundingBox())!;
  expect(title.y + title.height).toBeLessThanOrEqual(menu.y);
  expect(menu.y + menu.height).toBeLessThanOrEqual(filters.y);
});

test('section titles are smaller than the page title', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/dashboard');
  const size = (locator: import('@playwright/test').Locator) => locator.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  const title = await size(page.getByRole('heading', { level: 1 }));
  const section = await size(page.getByRole('heading', { level: 2, name: 'Per character' }));
  expect(section).toBeLessThan(title);
});

test('the blocks of a page are separated by one and the same spacing', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: Array.from({ length: 60 }, (_, i) => stubOrder(i + 1)) });
  await pollCharacter(page, 90000001);
  await page.goto('/orders/sell');
  await expect(page.locator('nav.pagination')).toBeVisible();

  const cards = (await page.locator('.kpis').boundingBox())!;
  const table = (await page.locator('#sell-orders').boundingBox())!;
  const pager = (await page.locator('nav.pagination').boundingBox())!;
  const cardsToTable = table.y - (cards.y + cards.height);
  const tableToPager = pager.y - (table.y + table.height);
  expect(cardsToTable).toBeGreaterThan(0);
  expect(Math.abs(cardsToTable - tableToPager)).toBeLessThan(1);
});

test('KPI cards fit their content: one width, wider than high, no header band', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { buy: true })] });
  await pollCharacter(page, 90000001);
  await page.goto('/orders/buy');

  const cards = page.locator('.kpis .kpi');
  await expect(cards.first()).toBeVisible();
  await expect(cards.locator('header')).toHaveCount(0);
  const boxes = await Promise.all((await cards.all()).map(async (card) => (await card.boundingBox())!));
  expect(boxes.length).toBeGreaterThan(1);
  for (const box of boxes) {
    expect(box.width).toBeGreaterThan(box.height);
    expect(Math.abs(box.width - boxes[0].width)).toBeLessThanOrEqual(1);
  }
});
