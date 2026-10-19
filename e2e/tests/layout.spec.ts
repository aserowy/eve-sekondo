import type { Page } from '@playwright/test';
import { aliceTrained, bookOf, calm, fill } from '../support/opportunities';
import {
  expect, loginWith, openCharacters, pollCharacter, priceRefresh, stubAsset, stubControl, stubOrder, test, wealthPoll,
} from '../support/fixtures';
import { horizontalOverflow } from '../support/layout';

async function threeCharactersWithOrders(page: Page) {
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

test('pages use the full window width right of the sidebar', async ({ page }) => {
  await page.setViewportSize({ width: 2560, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');
  await expect(page.locator('#sell-orders #order-1')).toBeVisible();

  const side = (await page.locator('aside.sidebar').boundingBox())!;
  const wrap = (await page.locator('.table-wrap').first().boundingBox())!;
  // No maximum width: apart from Pico's fluid container padding the table reaches both edges.
  expect(wrap.x - (side.x + side.width)).toBeLessThanOrEqual(48);
  expect(2560 - (wrap.x + wrap.width)).toBeLessThanOrEqual(48);
});

test('the filter section holds the character filter on dashboard, orders, wealth and trades, and the characters page has none', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  for (const path of ['/dashboard', '/orders/sell', '/orders/buy', '/wealth', '/trades']) {
    await page.goto(path);
    const box = page.locator('#filter-box');
    await expect(box, path).toBeVisible();
    const filter = box.getByRole('group', { name: 'Characters' });
    await expect(filter, path).toBeVisible();
    await expect(filter.getByRole('checkbox', { name: 'Alice Alpha' }), path).toBeChecked();
    await expect(page.locator('#main-nav #character-filter'), path).toHaveCount(0);
  }
  await page.goto('/characters');
  await expect(page.locator('#character-filter')).toHaveCount(0);
  await expect(page.locator('#filter-box')).toHaveCount(0);
});

test('the time ranges sit in the filter section of the dashboard, the wealth page and the trades page', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  for (const [path, control] of [['/dashboard', '#range-control'], ['/wealth', '#wealth-range-control'], ['/trades', '#trades-range-control']]) {
    await page.goto(path);
    await expect(page.locator(`#filter-box ${control}`), path).toBeVisible();
    await expect(page.locator(`#main-nav ${control}`), path).toHaveCount(0);
    await expect(page.locator(`main ${control}`), path).toHaveCount(1);
  }
});

test('the filter section is right below the sub-menu and above the cards of an orders page', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await threeCharactersWithOrders(page);
  await page.goto('/orders/sell');
  await expect(page.locator('.kpis')).toBeVisible();

  const menu = (await page.locator('nav.sub-menu').boundingBox())!;
  const filter = (await page.locator('#filter-box').boundingBox())!;
  const cards = (await page.locator('.kpis').boundingBox())!;
  expect(filter.y).toBeGreaterThanOrEqual(menu.y + menu.height - 1);
  expect(filter.y + filter.height).toBeLessThanOrEqual(cards.y);
  // One line of checkboxes: it wraps instead of scrolling, so it stays within the content.
  expect(filter.width).toBeLessThanOrEqual(1280);
});

test('the sidebar and the sub-menu stay at the top of the window while the page scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 400 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: Array.from({ length: 100 }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(50);

  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(300);

  expect((await page.locator('aside.sidebar').boundingBox())!.y).toBe(0);
  expect((await page.locator('nav.sub-menu').boundingBox())!.y).toBe(0);
  expect(await page.evaluate(() => window.scrollX)).toBe(0);
});

test('the breakdown, the inventory and an opportunity table share one table style', async ({ page }) => {
  await threeCharactersWithOrders(page);
  await stubControl('assets', { character_id: 90000001, assets: [stubAsset(1, { type: 34, quantity: 10 }), stubAsset(2, { type: 35, quantity: 5 })] });
  await wealthPoll(page, 90000001, 'assets');
  await priceRefresh(page);

  // Live refreshes swap the content once the stream opens, so styles are read until they settle.
  const background = (row: string) => page.locator(row).evaluate((tr) => getComputedStyle(tr.children[0]).backgroundColor).catch(() => '');

  await page.goto('/dashboard');
  await expect(page.locator('#breakdown tbody tr:nth-child(2)')).toBeVisible();
  await expect.poll(() => background('#breakdown tbody tr:nth-child(2)')).not.toBe('');
  const striped = await background('#breakdown tbody tr:nth-child(2)');
  expect(striped).not.toBe(await background('#breakdown tbody tr:nth-child(1)'));

  await page.goto('/wealth');
  await expect(page.locator('#inventory tbody tr:nth-child(2)')).toBeVisible();
  await expect.poll(() => background('#inventory tbody tr:nth-child(2)')).toBe(striped);

  await page.goto('/opportunities/classic');
  await fill(page, [...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)], [...calm(34), ...calm(35)]);
  await expect(page.locator('#classic tbody tr:nth-child(2)')).toBeVisible();
  await expect.poll(() => background('#classic tbody tr:nth-child(2)')).toBe(striped);
});

test('no page scrolls sideways at 1024px', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  await threeCharactersWithOrders(page);
  for (const path of ['/dashboard', '/orders/sell', '/orders/buy', '/wealth', '/trades', '/characters', '/opportunities/classic']) {
    await page.goto(path);
    await page.waitForTimeout(300);
    expect(await horizontalOverflow(page), path).toBeLessThanOrEqual(0);
  }
});

test('the Log in again link keeps its text and carries an icon', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubControl('revoke', { character_id: 90000001 });
  await page.evaluate(async () => {
    const token = document.querySelector('meta[name="csrf-token"]')!.getAttribute('content')!;
    await fetch('/_test/refresh/90000001', { method: 'POST', headers: { 'X-CSRF-Token': token } });
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 needs-relogin');
  await page.goto('/orders/sell');

  const link = page.locator('#freshness').getByRole('link', { name: 'Log in again' });
  await expect(link).toBeVisible();
  await expect(link.locator('svg')).toHaveCount(1);
  await expect(link).toHaveText('Log in again');
  const order = await link.evaluate((a) => [...a.childNodes].map((n) => n.nodeName.toLowerCase()));
  expect(order[0]).toBe('svg');
});

test('the opportunities page keeps its filter section on a narrow window without scrolling sideways', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  await aliceTrained(page);
  await page.goto('/opportunities/classic');
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
});
