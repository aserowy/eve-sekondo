import { expect, loginWith, openCharacters, pollCharacter, query, stubControl, stubOrder, test, wealthPoll } from '../support/fixtures';
import type { Page } from '@playwright/test';
import { chartData, expectTooltip } from '../support/charts';

/** Alice and Bob on one account, signed in as Alice on the dashboard. */
async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

async function openDashboard(page: Page) {
  await page.locator('#main-nav').getByRole('link', { name: 'Dashboard' }).click();
  await expect(page).toHaveURL(/\/dashboard$/);
}

test('tiles and breakdown show both characters, the corporation share and the exact amounts on hover', async ({ page }) => {
  await twoCharacters(page);
  // Alice: sells 10 x 1,000,000 (one of them for the corporation, 2 x 1,000,000), escrow 2,000,000.
  await stubControl('orders', {
    character_id: 90000001,
    orders: [
      stubOrder(1, { price: 1_000_000, remain: 8 }),
      stubOrder(2, { price: 1_000_000, remain: 2, corp: true }),
      stubOrder(3, { buy: true, escrow: 2_000_000 }),
    ],
  });
  // Bob: sells 5 x 4,000,000.
  await stubControl('orders', { character_id: 90000002, orders: [stubOrder(4, { price: 4_000_000, remain: 5 })] });
  await pollCharacter(page, 90000001);
  await pollCharacter(page, 90000002);
  await openDashboard(page);

  const sellTile = page.locator('#tile-sell-value');
  await expect(sellTile.locator('.value')).toHaveText('30m');
  await expect(sellTile.locator('.value')).toHaveAttribute('title', '30,000,000.00 ISK');
  await expect(sellTile.locator('.detail')).toContainText('of which 2m corp');
  await expect(sellTile.locator('.detail')).toHaveAttribute('title', '2,000,000.00 ISK');
  await expect(page.locator('#tile-escrow .value')).toHaveText('2m');
  await expect(page.locator('#tile-escrow .detail')).toHaveCount(0);
  await expect(page.locator('#tile-sell-count .value')).toHaveText('3');
  await expect(page.locator('#tile-buy-count .value')).toHaveText('1');

  const alice = page.locator('#character-90000001');
  await expect(alice).toContainText('Alice Alpha');
  await expect(alice.getByText('10m', { exact: true })).toHaveAttribute('title', '10,000,000.00 ISK');
  await expect(alice.getByText('2m', { exact: true })).toHaveAttribute('title', '2,000,000.00 ISK');
  await expect(alice).toContainText('Last update:');
  await expect(alice).not.toContainText('Stale');
  const bob = page.locator('#character-90000002');
  await expect(bob.getByText('20m', { exact: true })).toHaveAttribute('title', '20,000,000.00 ISK');
  // Alice was linked first.
  await expect(page.locator('#breakdown tbody tr').first()).toHaveAttribute('id', 'character-90000001');
});

test('a character that was never polled waits for its first update', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');

  await expect(page.locator('#character-90000001')).toContainText('Waiting for first update');
  await expect(page.locator('#tile-sell-value .value')).toHaveText('0.00');
  await expect(page.locator('#tile-sell-value .detail')).toHaveCount(0);
});

test('the split graph has a sell and an escrow bar per character, with exact values on hover', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000, remain: 30 })] });
  await stubControl('orders', { character_id: 90000002, orders: [stubOrder(2, { buy: true, escrow: 2_000_000 })] });
  await pollCharacter(page, 90000001);
  await pollCharacter(page, 90000002);
  await openDashboard(page);

  // Alice: 30m sell, no escrow. Bob: no sell, 2m escrow (amounts in cents).
  await expect.poll(() => chartData(page, 'split')).toEqual([[3_000_000_000, 0], [0, 200_000_000]]);

  await expectTooltip(page, 'split', 0, 0, ['Alice Alpha', 'Sell value: 30,000,000.00 ISK']);
  await expectTooltip(page, 'split', 1, 1, ['Bob Bravo', 'Buy escrow: 2,000,000.00 ISK']);
});

test('the top item graphs show the highest item first, with its exact value on hover', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [
      stubOrder(1, { type: 34, price: 2_000_000, remain: 5 }), // Tritanium 10m
      stubOrder(2, { type: 35, price: 1_000_000, remain: 5 }), // Pyerite 5m
      stubOrder(3, { type: 36, buy: true, escrow: 750_000 }), // Mexallon
      stubOrder(4, { type: 34, buy: true, escrow: 1_250_000.5 }), // Tritanium
    ],
  });
  await pollCharacter(page, 90000001);
  await page.reload();

  await expect.poll(() => chartData(page, 'top-sell')).toEqual([[1_000_000_000, 500_000_000]]);
  await expectTooltip(page, 'top-sell', 0, 0, ['Tritanium', 'Sell value: 10,000,000.00 ISK']);

  await expect.poll(() => chartData(page, 'top-buy')).toEqual([[125_000_050, 75_000_000]]);
  await expectTooltip(page, 'top-buy', 0, 0, ['Tritanium', 'Buy escrow: 1,250,000.50 ISK']);
});

test('a top item graph without orders says so', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  await pollCharacter(page, 90000001);
  await page.reload();

  await expect(page.getByText('No buy orders.')).toBeVisible();
  await expect(page.locator('canvas[data-chart="top-buy"]')).toHaveCount(0);
  await expect(page.locator('canvas[data-chart="top-sell"]')).toHaveCount(1);
});

test('hovering the last point of the value line shows its time, series and exact amount', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 1_234_567_890.12, remain: 1 })],
  });
  await pollCharacter(page, 90000001);
  await page.reload();

  const data = (await chartData(page, 'value-history')).map((series) => series.map((point) => point.y));
  const last = data[0].length - 1;
  expect(data[0][last]).toBe(123_456_789_012); // cents
  expect(data[1][last]).toBe(0);

  await expectTooltip(page, 'value-history', 0, last, [
    'EVE time',
    'Sell value: 1,234,567,890.12 ISK',
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}/,
  ]);

  // The counts graph reads whole numbers.
  const counts = await chartData(page, 'count-history');
  expect(counts.map((series) => series[series.length - 1].y)).toEqual([1, 0]);
});

test('without a poll the history graphs say there is no history yet', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');

  await expect(page.getByText('No history yet.')).toBeVisible();
  await expect(page.locator('canvas[data-chart="value-history"]')).toHaveCount(0);
  await expect(page.locator('canvas[data-chart="count-history"]')).toHaveCount(0);
});

test('switching the range keeps the selection, updates the URL and the graphs, and reloads nothing', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000, remain: 4 })] });
  await pollCharacter(page, 90000001);
  await page.goto('/dashboard?chars=90000001');
  const control = page.locator('#range-control');
  await expect(control.getByRole('link', { name: '7 days' })).toHaveAttribute('aria-current', 'true');
  const weekPoints = (await chartData(page, 'value-history'))[0].length;
  expect(weekPoints).toBeGreaterThanOrEqual(84);

  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await control.getByRole('link', { name: '30 days' }).click();

  await expect(page).toHaveURL(/\/dashboard\?chars=90000001&range=30d$/);
  await expect(control.getByRole('link', { name: '30 days' })).toHaveAttribute('aria-current', 'true');
  await expect(control.getByRole('link', { name: '7 days' })).not.toHaveAttribute('aria-current', 'true');
  await expect.poll(async () => (await chartData(page, 'value-history'))[0].length).toBeGreaterThanOrEqual(120);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
  // The selection stayed: only Alice's row is there.
  await expect(page.locator('#breakdown tbody tr')).toHaveCount(1);

  // Back to the default range drops the parameter.
  await control.getByRole('link', { name: '7 days' }).click();
  await expect(page).toHaveURL(/\/dashboard\?chars=90000001$/);
  await control.getByRole('link', { name: '24 hours' }).click();
  await expect(page).toHaveURL(/range=24h$/);
  await expect.poll(async () => (await chartData(page, 'value-history'))[0].length).toBeLessThanOrEqual(49);
});

test('narrowing the selection keeps the range, and the selection travels to the orders page', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000, remain: 10 })] });
  await stubControl('orders', { character_id: 90000002, orders: [stubOrder(2, { price: 4_000_000, remain: 5 })] });
  await pollCharacter(page, 90000001);
  await pollCharacter(page, 90000002);
  await page.goto('/dashboard?range=30d');
  await expect(page.locator('#tile-sell-value .value')).toHaveText('30m');
  await expect(page.getByRole('checkbox')).toHaveCount(2);

  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();

  await expect(page).toHaveURL(/\/dashboard\?chars=90000001&range=30d$/);
  await expect(page.locator('#tile-sell-value .value')).toHaveText('10m');
  await expect(page.locator('#breakdown tbody tr')).toHaveCount(1);
  await expect(page.locator('#range-control').getByRole('link', { name: '30 days' })).toHaveAttribute('aria-current', 'true');
  expect(await chartData(page, 'split')).toEqual([[1_000_000_000], [0]]);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');

  // Selecting everything again drops `chars` but keeps the range.
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).check();
  await expect(page).toHaveURL(/\/dashboard\?range=30d$/);

  // The selection is carried by the navigation to the orders page.
  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();
  await expect(page).toHaveURL(/chars=90000002&range=30d$/);
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\/sell\?chars=90000002$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(1);
  await expect(page.locator('#order-2')).toBeVisible();
});

test('opening the dashboard, switching the range and narrowing the selection raise no policy violation', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  await pollCharacter(page, 90000001);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(5);

  await page.locator('#range-control').getByRole('link', { name: '90 days' }).click();
  await expect(page).toHaveURL(/range=90d$/);
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001&range=90d$/);
  await expect(page.locator('canvas[data-chart]')).toHaveCount(5);
  // The CSP guard fixture fails the test on any violation reported while the page did all this.
});

async function instanceCount(page: Page): Promise<number> {
  return page.evaluate(() => Object.keys((window as any).Chart.instances).length);
}

test('totals, row, split bar and the last line point refresh after a poll without a reload', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000, remain: 10 })] });
  await pollCharacter(page, 90000001);
  await page.goto('/dashboard');
  await expect(page.locator('#tile-sell-value .value')).toHaveText('10m');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(5);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_500_000, remain: 10 })] });
  await pollCharacter(page, 90000001);

  await expect(page.locator('#tile-sell-value .value')).toHaveText('15m');
  await expect(page.locator('#character-90000001').getByText('15m', { exact: true })).toBeVisible();
  await expect.poll(async () => (await chartData(page, 'split'))[0]).toEqual([1_500_000_000]);
  await expectTooltip(page, 'split', 0, 0, ['Sell value: 15,000,000.00 ISK']);
  const line = (await chartData(page, 'value-history'))[0];
  expect(line[line.length - 1].y).toBe(1_500_000_000);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('a live refresh keeps the selection and the range, and leaks no chart', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000, remain: 10 })] });
  await stubControl('orders', { character_id: 90000002, orders: [stubOrder(2, { price: 4_000_000, remain: 5 })] });
  await pollCharacter(page, 90000001);
  await pollCharacter(page, 90000002);
  await page.goto('/dashboard?chars=90000001&range=24h');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(5);
  await expect.poll(() => instanceCount(page)).toBe(5);

  for (const [round, price] of [1_100_000, 1_200_000, 1_300_000].entries()) {
    await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price, remain: 10 })] });
    await pollCharacter(page, 90000001);
    // Bob's poll signals too, and must not widen the view.
    await pollCharacter(page, 90000002);
    await expect(page.locator('#tile-sell-value .value')).toHaveText(`${price * 10 / 1_000_000}m`);
    expect(round).toBeGreaterThanOrEqual(0);
  }

  await expect(page).toHaveURL(/\/dashboard\?chars=90000001&range=24h$/);
  await expect(page.locator('#breakdown tbody tr')).toHaveCount(1);
  await expect(page.locator('#character-90000002')).toHaveCount(0);
  await expect(page.locator('#range-control').getByRole('link', { name: '24 hours' })).toHaveAttribute('aria-current', 'true');
  expect((await chartData(page, 'value-history'))[0].length).toBeLessThanOrEqual(49);
  // After three refreshes there are exactly the five charts of the page, not fifteen more.
  expect(await instanceCount(page)).toBe(5);
});

test('the wealth tile adds up both characters, shows the exact amount on hover and links to the wealth page with the selection', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [] });
  await stubControl('orders', { character_id: 90000002, orders: [] });
  await stubControl('wallet', { character_id: 90000001, balance: 200_000_000 });
  await stubControl('wallet', { character_id: 90000002, balance: 50_000_000 });
  await wealthPoll(page, 90000001, 'wallet');
  await wealthPoll(page, 90000002, 'wallet');
  await openDashboard(page);

  const tile = page.locator('#tile-wealth');
  await expect(tile.locator('.value')).toHaveText('250m');
  await expect(tile.locator('.value')).toHaveAttribute('title', '250,000,000.00 ISK');
  await expect(tile.getByRole('link', { name: 'Wealth' })).toHaveAttribute('href', '/wealth');

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(tile.locator('.value')).toHaveText('200m');
  await expect(tile.getByRole('link', { name: 'Wealth' })).toHaveAttribute('href', '/wealth?chars=90000001');

  await tile.getByRole('link', { name: 'Wealth' }).click();
  await expect(page).toHaveURL(/\/wealth\?chars=90000001$/);
});

test('one range click updates the value, count and wealth graphs', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000, remain: 4 })] });
  await pollCharacter(page, 90000001);
  await wealthPoll(page, 90000001, 'wallet');
  await page.goto('/dashboard');
  const points = async (kind: string) => (await chartData(page, kind))[0].length;
  expect(await points('wealth-history')).toBeGreaterThanOrEqual(84);

  await page.locator('#range-control').getByRole('link', { name: '30 days' }).click();

  await expect(page).toHaveURL(/range=30d$/);
  for (const kind of ['value-history', 'count-history', 'wealth-history']) {
    await expect.poll(() => points(kind), { message: kind }).toBeGreaterThanOrEqual(120);
  }
});

test('the wealth line steps with the wallet and its last point is the tile', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [] });
  // Polls two hours and one hour ago (the polls themselves only know "now").
  await query(
    `INSERT INTO wallet_balances (character_id, polled_at, balance) VALUES
       (90000001, now() - interval '2 hours', 10000000), (90000001, now() - interval '1 hour', 15000000)`,
  );
  await page.goto('/dashboard?range=24h');

  const line = (await chartData(page, 'wealth-history'))[0].map((point) => point.y);
  expect(line).toContain(1_000_000_000);
  expect(line[line.length - 1]).toBe(1_500_000_000);
  expect(line.indexOf(1_000_000_000)).toBeLessThan(line.indexOf(1_500_000_000));
  expect(line[0]).toBeLessThanOrEqual(1_000_000_000);
  await expect(page.locator('#tile-wealth .value')).toHaveText('15m');
  await expectTooltip(page, 'wealth-history', 0, line.length - 1, ['Wealth: 15,000,000.00 ISK']);
});

test('a wallet poll refreshes the wealth tile and line without a reload', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [] });
  await stubControl('wallet', { character_id: 90000001, balance: 10_000_000 });
  await wealthPoll(page, 90000001, 'wallet');
  await page.goto('/dashboard');
  await expect(page.locator('#tile-wealth .value')).toHaveText('10m');
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await page.waitForTimeout(1000);

  await stubControl('wallet', { character_id: 90000001, balance: 25_000_000 });
  await wealthPoll(page, 90000001, 'wallet');

  await expect(page.locator('#tile-wealth .value')).toHaveText('25m');
  await expect
    .poll(async () => {
      const line = (await chartData(page, 'wealth-history'))[0];
      return line[line.length - 1].y;
    })
    .toBe(2_500_000_000);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
});

test('without a wealth figure the dashboard says there is no wealth history', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');

  await expect(page.getByText('No wealth history yet.')).toBeVisible();
  await expect(page.locator('canvas[data-chart="wealth-history"]')).toHaveCount(0);
});

test('the tiles are cards of one width that fit their content, in a row above the breakdown', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  await pollCharacter(page, 90000001);
  // The stream's first refresh replaces the tiles; measure after it.
  const opened = page.waitForResponse((r) => r.url().includes('/dashboard/summary'));
  await page.goto('/dashboard');
  await opened;
  await page.waitForTimeout(300);

  const ids = ['#tile-sell-value', '#tile-escrow', '#tile-wealth', '#tile-sell-count', '#tile-buy-count'];
  const boxes = await Promise.all(ids.map(async (id) => (await page.locator(id).boundingBox())!));
  for (const box of boxes) {
    expect(box.width, 'wider than high').toBeGreaterThan(box.height);
    expect(Math.abs(box.width - boxes[0].width), 'one size').toBeLessThanOrEqual(1);
    expect(Math.abs(box.y - boxes[0].y), 'one row').toBeLessThanOrEqual(1);
  }
  // Left to right in the order of the cards, and above the breakdown.
  expect(boxes.map((b) => b.x)).toEqual([...boxes.map((b) => b.x)].sort((a, b) => a - b));
  expect(boxes[0].y + boxes[0].height).toBeLessThanOrEqual((await page.locator('#breakdown').boundingBox())!.y);
  await expect(page.locator('#tile-wealth').getByRole('link', { name: 'Wealth' })).toHaveAttribute('href', '/wealth');
  await expect(page.locator('.kpi header')).toHaveCount(0);
});

test('the rows follow each other: tiles, breakdown, three graphs side by side, then value, orders and wealth alone', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_000_000 }), stubOrder(2, { buy: true, escrow: 2_000_000 })] });
  await pollCharacter(page, 90000001);
  await wealthPoll(page, 90000001, 'wallet');
  // The stream's first refresh replaces the rows; measure after it.
  const opened = page.waitForResponse((r) => r.url().includes('/dashboard/summary'));
  await page.goto('/dashboard');
  await opened;
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);
  await page.waitForTimeout(300);

  const box = async (selector: string) => {
    const found = await page.locator(selector).first().boundingBox();
    if (!found) throw new Error(`no box for ${selector}`);
    return found;
  };
  const tiles = await box('#totals');
  const breakdown = await box('#characters');
  const split = await box('canvas[data-chart="split"]');
  const topSell = await box('canvas[data-chart="top-sell"]');
  const topBuy = await box('canvas[data-chart="top-buy"]');
  const value = await box('#value');
  const orders = await box('#orders');
  const wealth = await box('#wealth');

  expect(tiles.y + tiles.height).toBeLessThanOrEqual(breakdown.y + 1);
  // The breakdown is as wide as the content area, and so are the three rows alone.
  const content = await box('#content, #dashboard-view');
  for (const wide of [breakdown, value, orders, wealth]) expect(Math.abs(wide.width - content.width)).toBeLessThanOrEqual(2);
  // The split and both top-item graphs share one row, below the breakdown.
  expect(Math.abs(split.y - topSell.y)).toBeLessThanOrEqual(2);
  expect(Math.abs(split.y - topBuy.y)).toBeLessThanOrEqual(2);
  expect(split.x).toBeLessThan(topSell.x);
  expect(topSell.x).toBeLessThan(topBuy.x);
  expect(split.y).toBeGreaterThanOrEqual(breakdown.y + breakdown.height);
  expect(value.y).toBeGreaterThanOrEqual(split.y + split.height);
  expect(orders.y).toBeGreaterThanOrEqual(value.y + value.height - 1);
  expect(wealth.y).toBeGreaterThanOrEqual(orders.y + orders.height - 1);
  await expect(page.locator('#breakdown').locator('xpath=ancestor::*[contains(@class,"table-wrap")]')).toHaveCount(1);
});

test('thirty characters are all listed in the breakdown without pagination or inner scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await query(
    `INSERT INTO characters (character_id, account_id, name, owner_hash)
     SELECT 91000000 + n, (SELECT account_id FROM characters WHERE character_id = 90000001), 'Extra Character ' || n, 'hash-' || n
     FROM generate_series(1, 29) AS n`,
  );

  await page.goto('/dashboard');

  await expect(page.locator('#breakdown tbody tr')).toHaveCount(30);
  await expect(page.locator('#characters .pagination')).toHaveCount(0);
  const wrap = page.locator('#characters .table-wrap');
  const sizes = await wrap.evaluate((el) => ({ scroll: el.scrollHeight, client: el.clientHeight }));
  expect(sizes.scroll).toBeLessThanOrEqual(sizes.client + 1);
});
