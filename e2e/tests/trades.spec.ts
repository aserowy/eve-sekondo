import type { Page } from '@playwright/test';
import {
  expect, feePoll, loginWith, openCharacters, stubControl, stubTradeSkills, stubTransaction, test, wealthPoll,
} from '../support/fixtures';

const ALICE = 90000001;
const BOB = 90000002;
const JITA = 'Jita IV - Moon 4 - Caldari Navy Assembly Plant';

/**
 * Alice (Accounting 5, Broker Relations 5) has traded three items:
 * - Tritanium: bought 10 at 100, sold 10 at 150 → margin 428.23
 * - Pyerite: bought 4 at 100, sold 10 at 150 → cost known for 4 of 10 units
 * - Mexallon: bought 10 at 100, sold 10 at 90 → margin -143.07
 * and a purchase of 1,000 Isogen at 2,500,000.
 */
async function aliceTraded(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(5, 5) });
  await stubControl('transactions', {
    character_id: ALICE,
    transactions: [
      stubTransaction(1, { type: 34, quantity: 10, price: 100, daysAgo: 5 }),
      stubTransaction(2, { type: 34, quantity: 10, price: 150, buy: false, daysAgo: 2 }),
      stubTransaction(3, { type: 35, quantity: 4, price: 100, daysAgo: 5 }),
      stubTransaction(4, { type: 35, quantity: 10, price: 150, buy: false, daysAgo: 2 }),
      stubTransaction(5, { type: 36, quantity: 10, price: 100, daysAgo: 5 }),
      stubTransaction(6, { type: 36, quantity: 10, price: 90, buy: false, daysAgo: 2 }),
      stubTransaction(7, { type: 37, quantity: 1000, price: 2_500_000, daysAgo: 1 }),
      stubTransaction(8, { type: 34, quantity: 1, price: 150, buy: false, daysAgo: 10 }),
    ],
  });
  expect(await wealthPoll(page, ALICE, 'transactions')).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.goto('/trades');
}

const row = (page: Page, id: number) => page.locator(`#trades tr[data-transaction="${id}"]`);

test('a sale row and a purchase row read with their values', async ({ page }) => {
  await aliceTraded(page);

  await expect(page.locator('#main-nav').getByRole('link', { name: 'Trades' })).toHaveAttribute('aria-current', 'page');
  const sale = row(page, 2).locator('td');
  await expect(sale.nth(1)).toHaveText('Alice Alpha');
  await expect(sale.nth(2)).toHaveText('Tritanium');
  await expect(sale.nth(3)).toHaveText(JITA);
  await expect(sale.nth(4)).toHaveText('Sell');
  await expect(sale.nth(5)).toHaveText('10');
  await expect(sale.nth(6)).toHaveText('150.00');
  await expect(sale.nth(7)).toHaveText('1.5k');
  await expect(sale.nth(8)).toHaveText('428.23');
  await expect(sale.nth(0)).toHaveText(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);

  const purchase = row(page, 7).locator('td');
  await expect(purchase.nth(4)).toHaveText('Buy');
  await expect(purchase.nth(5)).toHaveText('1,000');
  await expect(purchase.nth(6)).toHaveText('2.5m');
  await expect(purchase.nth(7)).toHaveText('2.5b');
  await expect(purchase.nth(8)).toHaveText('');
});

test('hovering a margin shows its exact value, and a partly known cost says how much is known', async ({ page }) => {
  await aliceTraded(page);

  await expect(row(page, 2).locator('td').nth(8)).toHaveAttribute('title', '428.23 ISK; cost per unit 100.00 ISK');
  const partial = row(page, 4).locator('td').nth(8);
  await expect(partial).toHaveText('—');
  await expect(partial).toHaveAttribute('title', 'cost known for 4 of 10 units');
});

test('a negative margin is shown in the error style', async ({ page }) => {
  await aliceTraded(page);

  const margin = row(page, 6).locator('td').nth(8).locator('mark');
  await expect(margin).toHaveText('-143.07');
  await expect(margin).toHaveClass(/loss/);
});

test('the default range lists seven days and the range control switches to 30 days keeping the selection', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await stubControl('transactions', {
    character_id: ALICE,
    transactions: [
      stubTransaction(1, { type: 34, quantity: 10, price: 100, daysAgo: 12 }),
      stubTransaction(2, { type: 34, quantity: 10, price: 150, buy: false, daysAgo: 2 }),
      stubTransaction(3, { type: 34, quantity: 1, price: 150, buy: false, daysAgo: 10 }),
    ],
  });
  await wealthPoll(page, ALICE, 'transactions');
  await page.goto(`/trades?chars=${ALICE}`);
  await expect(page.locator('#trades-range-control a[aria-current="true"]')).toHaveText('7 days');
  await expect(page.locator('#trades tbody tr')).toHaveCount(1);

  await page.locator('#trades-range-control').getByRole('button', { name: '30 days' }).click();

  await expect(page).toHaveURL(new RegExp(`/trades\\?chars=${ALICE}&range=30d$`));
  await expect(page.locator('#trades tbody tr')).toHaveCount(3);
  await expect(page.locator('#trades-range-control a[aria-current="true"]')).toHaveText('30 days');
  // The filter keeps the range it was switched to.
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).check();
  await expect(page).toHaveURL(/\/trades\?range=30d$/);
});

test('narrowing the selection leaves only the other character\'s trades', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  for (const [character, ids] of [[ALICE, [1]], [BOB, [2]]] as const) {
    await stubControl('transactions', {
      character_id: character,
      transactions: ids.map((id) => stubTransaction(id, { type: 34, quantity: 10, price: 100, daysAgo: 1 })),
    });
    await wealthPoll(page, character, 'transactions');
  }
  await page.goto('/trades');
  await expect(page.locator('#trades tbody tr')).toHaveCount(2);

  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();

  await expect(page).toHaveURL(new RegExp(`/trades\\?chars=${BOB}$`));
  await expect(page.locator('#trades tbody tr')).toHaveCount(1);
  await expect(row(page, 2)).toBeVisible();
});

test('the kpi cards sum the range and say how many sales the margin leaves out', async ({ page }) => {
  await aliceTraded(page);

  // Bought 1,000 + 400 + 1,000 + 2,500,000,000; sold 1,500 + 1,500 + 900.
  await expect(page.locator('#kpi-bought .value')).toHaveText('2.5b');
  await expect(page.locator('#kpi-sold .value')).toHaveText('3.9k');
  // 428.23 - 143.07; the partly known sale is left out.
  const margin = page.locator('#kpi-margin .value');
  await expect(margin).toHaveText('285.16');
  await expect(margin).toHaveAttribute('title', '285.16 ISK; 1 sale left out');
  await expect(page.locator('#trades tfoot')).toHaveCount(0);
  // The range control sits in the filter section, above the cards.
  const range = (await page.locator('#filter-box #trades-range-control').boundingBox())!;
  const cards = (await page.locator('.kpis').boundingBox())!;
  expect(range.y + range.height).toBeLessThanOrEqual(cards.y);
});

/** Alice with 120 sales in the last two hours, one per minute: transaction i is `121 - i` minutes old. */
async function aliceWith120Sales(page: import('@playwright/test').Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('transactions', {
    character_id: ALICE,
    transactions: Array.from({ length: 120 }, (_, i) => ({
      ...stubTransaction(i + 1, { type: 34, quantity: 1, price: 100, buy: false, daysAgo: 0 }),
      date: new Date(Date.now() - (121 - (i + 1)) * 60_000 - 60_000).toISOString().replace(/\.\d+Z$/, 'Z'),
    })),
  });
  expect(await wealthPoll(page, ALICE, 'transactions')).toBe('200 updated');
  await page.goto('/trades');
}

const transactionIds = (page: import('@playwright/test').Page) =>
  page.locator('#trades tbody tr').evaluateAll((rows) => rows.map((r) => Number(r.getAttribute('data-transaction'))));

test('the table is paginated newest first, the cards cover every page, and a range switch goes back to page 1', async ({ page }) => {
  await aliceWith120Sales(page);
  await expect(page.locator('.pagination')).toContainText('Page 1 of 3');
  await expect(page.locator('#trades tbody tr')).toHaveCount(50);
  await expect(page.locator('#kpi-sold .value')).toHaveAttribute('title', '12,000.00 ISK');
  expect((await transactionIds(page))[0]).toBe(120);

  await page.locator('.pagination').getByRole('link', { name: 'Next' }).click();
  await expect(page.locator('.pagination')).toContainText('Page 2 of 3');
  await page.locator('.pagination').getByRole('link', { name: 'Next' }).click();
  await expect(page).toHaveURL(/page=3$/);
  await expect(page.locator('.pagination')).toContainText('Page 3 of 3');
  expect(await transactionIds(page)).toEqual(Array.from({ length: 20 }, (_, i) => 20 - i));
  await expect(page.locator('#kpi-sold .value')).toHaveAttribute('title', '12,000.00 ISK');

  await page.locator('#trades-range-control').getByRole('button', { name: '30 days' }).click();
  await expect(page).toHaveURL(/\/trades\?range=30d$/);
  await expect(page.locator('.pagination')).toContainText('Page 1 of 3');
  await expect(page.locator('#trades-range-control a[aria-current="true"]')).toHaveText('30 days');
});

test('a filter change goes back to page 1 and the freshness is at the end, unfolded', async ({ page }) => {
  await aliceWith120Sales(page);
  await page.locator('.pagination').getByRole('link', { name: 'Next' }).click();
  await expect(page.locator('.pagination')).toContainText('Page 2 of 3');
  await expect(page.getByRole('button', { name: 'Freshness' })).toHaveCount(0);
  const bar = (await page.locator('.pagination').boundingBox())!;
  const freshness = page.locator('#freshness');
  await expect(freshness).toContainText('Last update');
  expect((await freshness.boundingBox())!.y).toBeGreaterThanOrEqual(bar.y + bar.height);

  await page.getByRole('combobox', { name: 'Rows per page' }).selectOption('25');
  await expect(page).toHaveURL(/per_page=25$/);
  await expect(page.locator('.pagination')).toContainText('Page 1 of 5');
});

test('the time range is a group of joined buttons under a visible "Range" label', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/trades?range=30d');

  const control = page.locator('#filter-box #trades-range-control');
  await expect(control.getByText('Range', { exact: true })).toBeVisible();
  const group = control.getByRole('group', { name: 'Range' });
  await expect(group).toHaveCount(1);
  const buttons = group.getByRole('button');
  await expect(buttons).toHaveText(['24 hours', '7 days', '30 days', '90 days']);
  for (const button of await buttons.all()) await expect(button).toHaveJSProperty('tagName', 'A');

  // Only the 30-day button is current, and it is the highlighted one: the others are the secondary outline style.
  await expect(group.locator('[aria-current="true"]')).toHaveText('30 days');
  const classes = await buttons.evaluateAll((els) => els.map((el) => [el.textContent, el.classList.contains('secondary') && el.classList.contains('outline')]));
  expect(classes).toEqual([['24 hours', true], ['7 days', true], ['30 days', false], ['90 days', true]]);

  // Joined: the buttons touch each other.
  const rects = await buttons.evaluateAll((els) => els.map((el) => el.getBoundingClientRect().toJSON()));
  for (let i = 1; i < rects.length; i++) expect(Math.abs(rects[i].x - (rects[i - 1].x + rects[i - 1].width))).toBeLessThanOrEqual(2);
  // The label is above the group.
  const label = (await control.getByText('Range', { exact: true }).boundingBox())!;
  expect(label.y + label.height).toBeLessThanOrEqual(rects[0].y + 1);
});
