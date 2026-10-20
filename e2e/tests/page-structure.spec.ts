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

/** Opens the sell orders page and waits for the stream's first refresh, so nothing is swapped while measuring. */
async function openSell(page: import('@playwright/test').Page, query = '') {
  const refreshed = page.waitForResponse((r) => r.url().includes('/orders/sell/content'));
  await page.goto(`/orders/sell${query}`);
  await refreshed;
  await page.waitForTimeout(300);
}

async function sellOrders(page: import('@playwright/test').Page, count: number) {
  await stubControl('orders', { character_id: 90000001, orders: Array.from({ length: count }, (_, i) => stubOrder(i + 1)) });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
}

test('the pagination of a short page sits directly above the footer, inside main', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await sellOrders(page, 2);
  await openSell(page);
  await expect(page.locator('nav.pagination')).toBeVisible();
  await expect(page.locator('footer.page-footer')).toBeVisible();

  const pager = (await page.locator('nav.pagination').boundingBox())!;
  const table = (await page.locator('#sell-orders').boundingBox())!;
  const footer = (await page.locator('footer.page-footer').boundingBox())!;
  expect(footer.y + footer.height).toBeCloseTo(1080, 0);
  expect(footer.y - (pager.y + pager.height)).toBeLessThan(40);
  expect(pager.y - (table.y + table.height)).toBeGreaterThan(200);
  await expect(page.locator('main nav.pagination')).toHaveCount(1);
});

test('the pagination of a long page follows the table and scrolls in', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await sellOrders(page, 200);
  await openSell(page, '?per_page=200');
  await expect(page.locator('nav.pagination')).toHaveCount(1);
  await expect(page.locator('nav.pagination')).not.toBeInViewport();
  await expect(page.locator('footer.page-footer')).toHaveCount(1);

  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(page.locator('nav.pagination')).toBeInViewport();
  const pager = (await page.locator('nav.pagination').boundingBox())!;
  const table = (await page.locator('#sell-orders').boundingBox())!;
  const footer = (await page.locator('footer.page-footer').boundingBox())!;
  expect(pager.y - (table.y + table.height)).toBeLessThan(40);
  expect(footer.y).toBeGreaterThanOrEqual(pager.y + pager.height);
});

test('the pagination keeps its place after a page change', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await sellOrders(page, 60);
  await openSell(page);
  await page.getByRole('link', { name: 'Next' }).click();
  await expect(page).toHaveURL(/page=2/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(10);
  await expect(page.locator('footer.page-footer')).toBeVisible();

  const pager = (await page.locator('nav.pagination').boundingBox())!;
  const footer = (await page.locator('footer.page-footer').boundingBox())!;
  expect(footer.y - (pager.y + pager.height)).toBeLessThan(40);
  expect(footer.y + footer.height).toBeCloseTo(1080, 0);
});
