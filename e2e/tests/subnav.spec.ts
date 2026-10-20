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
  const bars = (await page.locator('nav.sub-menu').boundingBox())!;
  return bars.y + bars.height;
};

/** The panel's title is fully visible, right below the bars (a card's own top padding counts as part of the gap). */
async function expectTitleBelowBars(page: Page, panel: string) {
  await expect(async () => {
    const padding = await page.locator(panel).evaluate((el) => parseFloat(getComputedStyle(el).paddingTop));
    const title = (await page.locator(`${panel} h2`).boundingBox())!;
    const bottom = await barsBottom(page);
    expect(title.y).toBeGreaterThanOrEqual(bottom - 1);
    expect(title.y).toBeLessThanOrEqual(bottom + 12 + padding);
  }).toPass();
}

test('the dashboard sub-menu lists its six rows in order at the top of the page and stays there while scrolling', async ({ page }) => {
  await dashboardWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);

  const menu = page.locator('.page > nav.sub-menu#section-nav');
  await expect(menu.getByRole('link')).toHaveText(['Totals', 'Characters', 'Split & top items', 'Value', 'Orders', 'Wealth']);
  await expect(menu.locator('svg, .count')).toHaveCount(0);
  const top = (await menu.boundingBox())!;
  expect(top.height).toBeLessThanOrEqual(40);

  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(300);
  expect(Math.abs((await menu.boundingBox())!.y)).toBeLessThan(1);
  const height = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--sub-menu-height'));
  expect(parseFloat(height)).toBeCloseTo((await menu.boundingBox())!.height, 0);
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
  await expect(page.locator('article.graph#wealth')).toHaveCount(1);
  await expect(async () => {
    const title = (await page.locator('#wealth h2').boundingBox())!;
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
    // The sub-menu sticks to the window top, so its bottom edge is its height once the page scrolls.
    const bottom = document.querySelector('nav.sub-menu')!.getBoundingClientRect().height;
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

test('the sub-menu is a tab bar: the current tab is underlined in the primary colour, the others are dimmed', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/orders/buy');

  const tabs = await page.locator('nav.sub-menu a').evaluateAll((links) => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--pico-primary)';
    document.body.append(probe);
    const primary = getComputedStyle(probe).color;
    const muted = getComputedStyle(document.documentElement).getPropertyValue('--pico-muted-color').trim();
    probe.style.color = muted;
    const dimmed = getComputedStyle(probe).color;
    probe.remove();
    return links.map((a) => {
      const s = getComputedStyle(a);
      return {
        text: a.textContent!.trim(),
        underlined: parseFloat(s.borderBottomWidth) >= 2 && s.borderBottomColor === primary,
        dimmed: s.color === dimmed,
      };
    });
  });
  expect(tabs).toEqual([
    { text: 'Sell', underlined: false, dimmed: true },
    { text: 'Buy', underlined: true, dimmed: false },
  ]);
  const divider = await page.locator('nav.sub-menu').evaluate((el) => getComputedStyle(el).borderBottomStyle);
  expect(divider).toBe('solid');
});

test('only the sub-menu stays at the top: the page title scrolls out of view', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 400 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: Array.from({ length: 100 }, (_, i) => stubOrder(i + 1)) });
  await pollCharacter(page, 90000001);
  await page.goto('/orders/sell');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(50);

  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(300);

  expect(Math.abs((await page.locator('nav.sub-menu').boundingBox())!.y)).toBeLessThan(1);
  await expect(page.getByRole('heading', { level: 1 })).not.toBeInViewport();
});

test('the entries are packed at the left with one fixed gap, never spread over the bar', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');

  const entries = (selector: string) =>
    page.locator(`${selector} a`).evaluateAll((links) => links.map((a) => a.getBoundingClientRect().toJSON()));
  const gaps = (boxes: { x: number; right: number }[]) => boxes.slice(1).map((b, i) => b.x - boxes[i].right);

  await page.goto('/dashboard');
  const dashboard = await entries('#section-nav');
  const dashboardGap = gaps(dashboard)[0];

  await page.goto('/orders/sell');
  const bar = (await page.locator('nav.sub-menu').boundingBox())!;
  const orders = await entries('nav.sub-menu');
  expect(orders).toHaveLength(2);
  expect(orders[0].x).toBeCloseTo(bar.x, 0);
  expect(gaps(orders)[0]).toBeCloseTo(dashboardGap, 0);
  expect(orders[1].right).toBeLessThan(bar.x + bar.width / 2);

  await page.goto('/opportunities/trend');
  const tables = await entries('nav.sub-menu');
  expect(tables).toHaveLength(4);
  for (const g of gaps(tables)) expect(g).toBeCloseTo(dashboardGap, 0);
});
