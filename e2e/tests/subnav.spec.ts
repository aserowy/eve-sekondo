import type { Page } from '@playwright/test';
import { expect, loginWith, openCharacters, pollCharacter, stubControl, stubOrder, test } from '../support/fixtures';

async function dashboardWithOrders(page: Page) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  for (const name of ['Bob Bravo', 'Carol Charlie']) {
    await page.getByRole('button', { name: 'Add character' }).click();
    await page.getByRole('link', { name }).click();
    await expect(page).toHaveURL(/\/characters$/);
  }
  for (const [id, order] of [[90000001, 11], [90000002, 22], [90000003, 33]] as const) {
    await stubControl('orders', { character_id: id, orders: [stubOrder(order), stubOrder(order + 100, { type: 35, buy: true })] });
    await pollCharacter(page, id);
  }
}

const barsBottom = async (page: Page) => {
  const bars = (await page.locator('header.bars').boundingBox())!;
  return bars.y + bars.height;
};

/** The panel's title is fully visible, right below the bars. */
async function expectTitleBelowBars(page: Page, panel: string) {
  await expect(async () => {
    const title = (await page.locator(`${panel} .panel-title`).boundingBox())!;
    const bottom = await barsBottom(page);
    expect(title.y).toBeGreaterThanOrEqual(bottom - 1);
    expect(title.y).toBeLessThanOrEqual(bottom + 12);
  }).toPass();
}

test('the dashboard sub-menu lists its six rows in order below the top bar', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');

  const menu = page.locator('header.bars #section-nav');
  await expect(menu.getByRole('link')).toHaveText(['Totals', 'Characters', 'Split & top items', 'Value', 'Orders', 'Wealth']);
  const bar = (await page.locator('#main-nav').boundingBox())!;
  const box = (await menu.boundingBox())!;
  expect(box.height).toBeLessThanOrEqual(24);
  expect(Math.abs(box.y - (bar.y + bar.height))).toBeLessThanOrEqual(1);
});

test('activating a sub-menu link scrolls the row below the bars and sets the fragment', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);

  await page.locator('#section-nav').getByRole('link', { name: 'Value' }).click();

  await expect(page).toHaveURL(/\/dashboard#value$/);
  await expectTitleBelowBars(page, '#value');
  expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(0);

  await page.locator('#section-nav').getByRole('link', { name: 'Wealth' }).click();
  await expect(page).toHaveURL(/\/dashboard#wealth$/);
  await expect(async () => {
    const title = (await page.locator('#wealth .panel-title').boundingBox())!;
    expect(title.y).toBeGreaterThanOrEqual((await barsBottom(page)) - 1);
    expect(title.y + title.height).toBeLessThanOrEqual(800);
  }).toPass();
});

test('opening the dashboard with a row fragment starts scrolled to that row', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard#value');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);

  await expectTitleBelowBars(page, '#value');
  expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
});

const current = (page: Page) => page.locator('#section-nav [aria-current="location"]');

/** Scrolls by hand until the row's top is at the bottom edge of the bars. */
async function scrollRowToBars(page: Page, row: string) {
  await page.evaluate((selector) => {
    const bottom = document.getElementById('bars')!.getBoundingClientRect().bottom;
    window.scrollBy(0, document.querySelector(selector)!.getBoundingClientRect().top - bottom);
  }, row);
}

test('scrolling by hand marks the row at the top of the content area and leaves the URL alone', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);
  const url = page.url();
  await expect(current(page)).toHaveText('Totals');

  await scrollRowToBars(page, '#value');
  await expect(current(page)).toHaveText('Value');
  expect(page.url()).toBe(url);

  await scrollRowToBars(page, '#characters');
  await expect(current(page)).toHaveText('Characters');
  expect(page.url()).toBe(url);
});

test('scrolling to the end of the page marks the last row', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);

  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));

  await expect(current(page)).toHaveText('Wealth');
});

test('right after a jump the row jumped to is marked, until the user scrolls again', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);

  // The page ends before "Orders" can reach the top, so the end-of-page rule alone would say "Wealth".
  await page.locator('#section-nav').getByRole('link', { name: 'Orders' }).click();
  await expect(current(page)).toHaveText('Orders');
  await expect(current(page)).toHaveCount(1);

  await page.mouse.move(600, 500);
  await page.mouse.wheel(0, 2000);
  await expect(current(page)).toHaveText('Wealth');
});

test('the marker survives a live refresh of the dashboard', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);
  await page.waitForTimeout(500);
  await scrollRowToBars(page, '#value');
  await expect(current(page)).toHaveText('Value');

  const refreshed = page.waitForResponse((r) => r.url().includes('/dashboard/summary'));
  await pollCharacter(page, 90000001);
  await refreshed;
  await page.waitForTimeout(300);

  await expect(current(page)).toHaveText('Value');
  expect(await page.evaluate(() => location.hash)).toBe('');
});
