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

  await page.locator('#trades-range-control').getByRole('link', { name: '30 days' }).click();

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

test('the totals row sums the rows and says how many sales it leaves out', async ({ page }) => {
  await aliceTraded(page);

  const totals = page.locator('#trades tfoot tr');
  // Bought 1,000 + 400 + 1,000 + 2,500,000,000; sold 1,500 + 1,500 + 900.
  await expect(totals.locator('span').nth(0)).toHaveText('2.5b');
  await expect(totals.locator('span').nth(1)).toHaveText('3.9k');
  // 428.23 - 143.07; the partly known sale is left out.
  const margin = totals.locator('td[data-column="margin"]');
  await expect(margin).toHaveText('285.16');
  await expect(margin).toHaveAttribute('title', '285.16 ISK; 1 sale left out');
});
