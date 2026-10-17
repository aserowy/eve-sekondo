import { chartData, chartDrawn, expectTooltip } from '../support/charts';
import {
  expect, loginWith, openCharacters, pollCharacter, priceRefresh, query, stubAsset, stubControl, stubOrder, test, wealthPoll,
} from '../support/fixtures';
import type { Page } from '@playwright/test';

/** Alice and Bob on one account, signed in as Alice. */
async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

test('the wealth page opens from the dashboard with the narrowed selection and shows the freshness', async ({ page }) => {
  await twoCharacters(page);
  await wealthPoll(page, 90000001, 'wallet');
  await wealthPoll(page, 90000001, 'assets');
  await page.getByRole('navigation').getByRole('link', { name: 'Dashboard' }).click();
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);

  await page.getByRole('navigation').getByRole('link', { name: 'Wealth' }).click();

  await expect(page).toHaveURL(/\/wealth\?chars=90000001$/);
  await expect(page.locator('#wealth-items')).toBeVisible();
  await expect(page.getByRole('navigation').getByRole('link', { name: 'Wealth' })).toHaveAttribute('aria-current', 'page');
  const freshness = page.locator('#wealth-freshness');
  await expect(freshness).toContainText('Alice Alpha');
  await expect(freshness).not.toContainText('Bob Bravo');
  await expect(freshness.locator('[data-kind="wallet"]')).toContainText('Last update:');
  await expect(freshness.locator('[data-kind="assets"]')).not.toContainText('Stale');
  // The checkbox state matches the URL.
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.getByRole('checkbox', { name: 'Bob Bravo' })).not.toBeChecked();
  // The CSP guard fixture fails the test on any violation reported while the page did all this.
});

test('narrowing the selection on the wealth page keeps the page and leaves the default sorting out of the URL', async ({ page }) => {
  await twoCharacters(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Wealth' }).click();
  await expect(page).toHaveURL(/\/wealth$/);

  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();

  await expect(page).toHaveURL(/\/wealth\?chars=90000002$/);
  await expect(page.locator('#wealth-freshness')).not.toContainText('Alice Alpha');
  await expect(page.locator('#wealth-freshness')).toContainText('Bob Bravo');
});

test('a character that needs a new login shows stale wallet and assets and a link to log in', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await query('UPDATE characters SET needs_relogin = true');

  await page.goto('/wealth');

  const freshness = page.locator('#wealth-freshness');
  await expect(freshness.locator('[data-kind="wallet"]').getByText('Stale')).toBeVisible();
  await expect(freshness.locator('[data-kind="assets"]').getByText('Stale')).toBeVisible();
  await expect(freshness.getByRole('link', { name: 'Log in again' })).toBeVisible();
});

/** Alice holds 10 Tritanium, 1000 Pyerite, a Pyerite blueprint copy and an item without a price. */
async function aliceWithAssets(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('assets', {
    character_id: 90000001,
    assets: [
      stubAsset(1, { type: 34, quantity: 10 }),
      stubAsset(2, { type: 35, quantity: 1000 }),
      stubAsset(3, { type: 35, quantity: 2, copy: true }),
      stubAsset(4, { type: 36, quantity: 7 }),
    ],
  });
  await wealthPoll(page, 90000001, 'assets');
  await priceRefresh(page);
}

const itemNames = (page: Page) => page.locator('#inventory tbody tr td:nth-child(2)');

test('the inventory is sorted by value, highest first, with unpriced rows last', async ({ page }) => {
  await aliceWithAssets(page);

  await page.goto('/wealth');

  await expect(itemNames(page).nth(0)).toHaveText('Pyerite');
  await expect(itemNames(page).nth(1)).toHaveText('Tritanium');
  await expect(page.locator('#inventory tbody tr')).toHaveCount(4);
  await expect(page.locator('#inventory th[aria-sort="descending"]')).toContainText('Value');
  await expect(page.locator('#wealth-items')).toContainText('2 unpriced');
  await expect(page.locator('#wealth-items')).toContainText('Total value:');
});

test('sorting by item puts the order in the URL and the same header reverses it', async ({ page }) => {
  await aliceWithAssets(page);
  await page.goto('/wealth');

  await page.locator('#inventory').getByRole('link', { name: 'Item' }).click();

  await expect(page).toHaveURL(/\/wealth\?sort=item&dir=asc$/);
  await expect(itemNames(page)).toHaveText(['Mexallon', 'Pyerite', 'Pyerite', 'Tritanium']);

  await page.locator('#inventory').getByRole('link', { name: 'Item' }).click();

  await expect(page).toHaveURL(/\/wealth\?sort=item&dir=desc$/);
  await expect(itemNames(page)).toHaveText(['Tritanium', 'Pyerite', 'Pyerite', 'Mexallon']);
});

test('a blueprint copy has a row of its own that shows a dash for price and value', async ({ page }) => {
  await aliceWithAssets(page);
  await page.goto('/wealth');

  const copy = page.locator('#inventory tbody tr', { has: page.getByText('Pyerite', { exact: true }) }).filter({
    has: page.locator('td:nth-child(3)', { hasText: /^2$/ }),
  });

  await expect(copy).toHaveCount(1);
  await expect(copy.locator('td:nth-child(4)')).toHaveText('\u2014');
  await expect(copy.locator('td:nth-child(5)')).toHaveText('\u2014');
});

test('hovering a shortened amount shows the exact one', async ({ page }) => {
  await aliceWithAssets(page);
  await page.goto('/wealth');

  const pyerite = page.locator('#inventory tbody tr').first();

  // 1000 x 10.25 ISK.
  await expect(pyerite.locator('td:nth-child(5)')).toHaveText('10.25k');
  await expect(pyerite.locator('td:nth-child(5)')).toHaveAttribute('title', '10,250.00 ISK');
  await expect(pyerite.locator('td:nth-child(4)')).toHaveAttribute('title', '10.25 ISK');
});

/** Alice: 100m wallet, 30m sell orders, 20m escrow and 50m of inventory, all polled. */
async function aliceWithEverything(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('wallet', { character_id: 90000001, balance: 100_000_000 });
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 3_000_000, remain: 10 }), stubOrder(2, { buy: true, escrow: 20_000_000 })],
  });
  await stubControl('assets', { character_id: 90000001, assets: [stubAsset(1, { type: 34, quantity: 5_000_000 })] });
  await stubControl('prices', { prices: [{ type_id: 34, average_price: 10 }] });
  await pollCharacter(page, 90000001);
  await wealthPoll(page, 90000001, 'wallet');
  await wealthPoll(page, 90000001, 'assets');
  await priceRefresh(page);
}

const STACK = 'wealth-stack';
// The stack, bottom to top: sell orders, buy escrow, wallet, inventory.
const INVENTORY = 3;

test('the stack ends at the total wealth and the tooltip lists the four values and the total', async ({ page }) => {
  await aliceWithEverything(page);

  await page.goto('/wealth');

  const data = await chartData(page, STACK);
  const last = data.map((points) => points[points.length - 1].y);
  expect(last).toEqual([3_000_000_000, 2_000_000_000, 10_000_000_000, 5_000_000_000]);
  await expectTooltip(page, STACK, INVENTORY, data[0].length - 1, [
    'Wallet: 100,000,000.00 ISK',
    'Sell orders: 30,000,000.00 ISK',
    'Buy escrow: 20,000,000.00 ISK',
    'Inventory: 50,000,000.00 ISK',
    'Total: 200,000,000.00 ISK',
  ]);
});

test('switching the range updates the URL and the graph but not the list', async ({ page }) => {
  await aliceWithEverything(page);
  await page.goto('/wealth');
  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('7 days');
  await chartData(page, STACK);
  // Let the refresh the page makes when its stream opens settle before marking the list.
  await page.waitForTimeout(1000);
  await page.evaluate(() => ((document.querySelector('#inventory') as any).marker = 'same list'));
  const span = async () => {
    await chartDrawn(page, STACK);
    return page.evaluate(() => {
      const chart = (window as any).Chart.getChart(document.querySelector('canvas[data-chart="wealth-stack"]'));
      return (chart.scales.x.max - chart.scales.x.min) / 86_400_000;
    });
  };
  expect(Math.round(await span())).toBe(7);

  await page.locator('#wealth-range-control').getByRole('link', { name: '30 days' }).click();

  await expect(page).toHaveURL(/\/wealth\?range=30d$/);
  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('30 days');
  await expect.poll(async () => Math.round(await span())).toBe(30);
  expect(await page.evaluate(() => (document.querySelector('#inventory') as any).marker)).toBe('same list');
});

test('an unknown range falls back to seven days', async ({ page }) => {
  await aliceWithEverything(page);

  await page.goto('/wealth?range=1y');

  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('7 days');
});

test('the dashboard range is not carried to the wealth page', async ({ page }) => {
  await aliceWithEverything(page);
  await page.goto('/dashboard?range=30d');

  await page.getByRole('navigation').getByRole('link', { name: 'Wealth' }).click();

  await expect(page).toHaveURL(/\/wealth$/);
  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('7 days');
});

test('without any recorded figure the graph says there is no history yet', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');

  await page.goto('/wealth');

  await expect(page.locator('#wealth-graph')).toContainText('No history yet.');
  await expect(page.locator('canvas[data-chart="wealth-stack"]')).toHaveCount(0);
});

test('the range chosen on the graph survives a change of the selection', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await page.getByRole('navigation').getByRole('link', { name: 'Wealth' }).click();

  await page.locator('#wealth-range-control').getByRole('link', { name: '90 days' }).click();
  await expect(page).toHaveURL(/range=90d$/);
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();

  await expect(page).toHaveURL(/\/wealth\?chars=90000001&range=90d$/);
  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('90 days');
});
