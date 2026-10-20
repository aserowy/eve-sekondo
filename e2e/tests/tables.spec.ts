import type { Page } from '@playwright/test';
import { expect, feePoll, loginWith, pollCharacter, stubControl, stubTransaction, test, wealthPoll } from '../support/fixtures';
import { horizontalOverflow } from '../support/layout';

const ALICE = 90000001;

async function aliceTraded(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('transactions', {
    character_id: ALICE,
    transactions: [
      stubTransaction(1, { type: 34, quantity: 10, price: 100, daysAgo: 5 }),
      stubTransaction(2, { type: 34, quantity: 10, price: 150, buy: false, daysAgo: 2 }),
      stubTransaction(3, { type: 35, quantity: 4, price: 100, daysAgo: 5 }),
    ],
  });
  expect(await wealthPoll(page, ALICE, 'transactions')).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  // The stream's first refresh replaces the table; measure after it.
  const opened = page.waitForResponse((r) => r.url().includes('/trades/table'));
  await page.goto('/trades');
  await opened;
  await expect(page.locator('#trades tbody tr')).toHaveCount(3);
  await page.waitForTimeout(300);
}

test('tables are compact: small text, short rows, no wrapping, aligned figures, zebra and hover', async ({ page }) => {
  await aliceTraded(page);
  const table = page.locator('#trades');
  const root = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));

  const style = await table.evaluate((el) => {
    const root = parseFloat(getComputedStyle(document.documentElement).fontSize);
    const cell = el.querySelector('tbody tr td') as HTMLElement;
    const num = el.querySelector('tbody td.num') as HTMLElement;
    return {
      ratio: parseFloat(getComputedStyle(cell).fontSize) / root,
      nowrap: getComputedStyle(cell).whiteSpace,
      numAlign: getComputedStyle(num).textAlign,
      numFigures: getComputedStyle(num).fontVariantNumeric,
    };
  });
  expect(style.ratio).toBeCloseTo(0.875, 2);
  expect(style.nowrap).toBe('nowrap');
  expect(style.numAlign).toBe('right');
  expect(style.numFigures).toContain('tabular-nums');

  for (const row of await table.locator('tbody tr').all()) {
    expect((await row.boundingBox())!.height).toBeLessThanOrEqual(1.75 * root);
  }

  const background = (n: number) => table.locator(`tbody tr:nth-child(${n})`).evaluate((tr) => getComputedStyle(tr.children[0]).backgroundColor);
  expect(await background(2)).not.toBe(await background(1));
  expect(await background(3)).toBe(await background(1));
  const before = await background(1);
  await table.locator('tbody tr:nth-child(1)').hover();
  expect(await background(1)).not.toBe(before);
});

test('table cells are padded 0.3rem above and below and 0.75rem left and right', async ({ page }) => {
  await aliceTraded(page);
  const padding = await page.locator('#trades tbody td').evaluateAll((cells) => {
    const root = parseFloat(getComputedStyle(document.documentElement).fontSize);
    return cells.map((c) => {
      const s = getComputedStyle(c);
      return [s.paddingTop, s.paddingBottom, s.paddingLeft, s.paddingRight].map((v) => parseFloat(v) / root);
    });
  });
  expect(padding.length).toBeGreaterThan(0);
  for (const p of padding) expect(p).toEqual([0.3, 0.3, 0.75, 0.75]);
});

test('a single-line sell order row is at most 1.75rem high', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, ALICE)).toBe('200 updated');
  await page.goto('/orders/sell');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  const { root, heights } = await page.evaluate(() => ({
    root: parseFloat(getComputedStyle(document.documentElement).fontSize),
    heights: [...document.querySelectorAll('#sell-orders tbody.order tr:first-child')].map((r) => r.getBoundingClientRect().height),
  }));
  expect(heights.length).toBe(2);
  for (const h of heights) expect(h).toBeLessThanOrEqual(1.75 * root);
});

test('a wide table scrolls sideways inside its container and never makes the page scroll sideways', async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 800 });
  await aliceTraded(page);

  const wrap = page.locator('.table-wrap').first();
  const sizes = await wrap.evaluate((el) => ({ scroll: el.scrollWidth, client: el.clientWidth, overflowX: getComputedStyle(el).overflowX }));
  expect(sizes.overflowX).toBe('auto');
  expect(sizes.scroll).toBeGreaterThan(sizes.client);
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
});

test('no page scrolls sideways at 1024px', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  await aliceTraded(page);
  for (const path of ['/dashboard', '/characters', '/wealth', '/trades', '/orders/sell', '/opportunities/expansion']) {
    await page.goto(path);
    expect(await horizontalOverflow(page), path).toBeLessThanOrEqual(0);
  }
});
