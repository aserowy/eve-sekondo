import { expect, loginWith, openCharacters, pollCharacter, stubControl, stubOrder, test } from '../support/fixtures';
import type { Page } from '@playwright/test';

/** Alice and Bob on one account, signed in as Alice on the dashboard. */
async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

async function openDashboard(page: Page) {
  await page.getByRole('navigation').getByRole('link', { name: 'Dashboard' }).click();
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
  await expect(sellTile.locator('.amount')).toHaveText('30m');
  await expect(sellTile.locator('.amount')).toHaveAttribute('title', '30,000,000.00 ISK');
  await expect(sellTile.locator('.corp-share')).toContainText('of which 2m corp');
  await expect(sellTile.locator('.corp-share span')).toHaveAttribute('title', '2,000,000.00 ISK');
  await expect(page.locator('#tile-escrow .amount')).toHaveText('2m');
  await expect(page.locator('#tile-escrow .corp-share')).toHaveCount(0);
  await expect(page.locator('#tile-sell-count .amount')).toHaveText('3');
  await expect(page.locator('#tile-buy-count .amount')).toHaveText('1');

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
  await expect(page.locator('#tile-sell-value .amount')).toHaveText('0.00');
  await expect(page.locator('#tile-sell-value .corp-share')).toHaveCount(0);
});

/** Waits until the chart of this kind is drawn: the page may still be navigating. */
async function chartDrawn(page: Page, kind: string) {
  await page.waitForFunction(
    (kind) => {
      const canvas = document.querySelector(`canvas[data-chart="${kind}"]`);
      return Boolean(canvas && (window as any).Chart?.getChart(canvas));
    },
    kind,
  );
}

/** The page position of the middle of a bar or point of a chart, for hovering it. */
async function chartPoint(page: Page, kind: string, dataset: number, index: number) {
  await chartDrawn(page, kind);
  return page.evaluate(
    ([kind, dataset, index]) => {
      const canvas = document.querySelector<HTMLCanvasElement>(`canvas[data-chart="${kind}"]`)!;
      const chart = (window as any).Chart.getChart(canvas);
      canvas.scrollIntoView({ block: 'center' });
      const centre = chart.getDatasetMeta(dataset).data[index].getCenterPoint();
      const box = canvas.getBoundingClientRect();
      return { x: box.left + centre.x, y: box.top + centre.y };
    },
    [kind, dataset, index] as const,
  );
}

async function chartData(page: Page, kind: string): Promise<any[][]> {
  let data: any[][] = [];
  // A live refresh can replace the chart between waiting for it and reading it: read again.
  await expect(async () => {
    await chartDrawn(page, kind);
    data = await page.evaluate((kind) => {
      const canvas = document.querySelector<HTMLCanvasElement>(`canvas[data-chart="${kind}"]`)!;
      return (window as any).Chart.getChart(canvas).data.datasets.map((d: { data: unknown[] }) => d.data);
    }, kind);
  }).toPass();
  return data;
}

async function hover(page: Page, kind: string, dataset: number, index: number) {
  const { x, y } = await chartPoint(page, kind, dataset, index);
  await page.mouse.move(x, y);
}

/**
 * Hovers a bar or point and expects the chart's tooltip to hold these texts. A live refresh may
 * replace the charts right after the page opens, so hovering and reading are retried together.
 */
async function expectTooltip(page: Page, kind: string, dataset: number, index: number, texts: (string | RegExp)[]) {
  await expect(async () => {
    await hover(page, kind, dataset, index);
    const tooltip = page.locator(`canvas[data-chart="${kind}"] + .chart-tooltip`);
    for (const text of texts) await expect(tooltip).toContainText(text, { timeout: 1000 });
  }).toPass();
}

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
  await expect(page.locator('#tile-sell-value .amount')).toHaveText('30m');
  await expect(page.getByRole('checkbox')).toHaveCount(2);

  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();

  await expect(page).toHaveURL(/\/dashboard\?chars=90000001&range=30d$/);
  await expect(page.locator('#tile-sell-value .amount')).toHaveText('10m');
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
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\?chars=90000002$/);
  await expect(page.locator('#sell-orders tbody tr')).toHaveCount(1);
  await expect(page.locator('#order-2')).toBeVisible();
});

test('opening the dashboard, switching the range and narrowing the selection raise no policy violation', async ({ page }) => {
  await twoCharacters(page);
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  await pollCharacter(page, 90000001);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(4);

  await page.locator('#range-control').getByRole('link', { name: '90 days' }).click();
  await expect(page).toHaveURL(/range=90d$/);
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001&range=90d$/);
  await expect(page.locator('canvas[data-chart]')).toHaveCount(4);
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
  await expect(page.locator('#tile-sell-value .amount')).toHaveText('10m');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(4);
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));

  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price: 1_500_000, remain: 10 })] });
  await pollCharacter(page, 90000001);

  await expect(page.locator('#tile-sell-value .amount')).toHaveText('15m');
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
  await expect(page.locator('canvas[data-chart]')).toHaveCount(4);
  await expect.poll(() => instanceCount(page)).toBe(4);

  for (const [round, price] of [1_100_000, 1_200_000, 1_300_000].entries()) {
    await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1, { price, remain: 10 })] });
    await pollCharacter(page, 90000001);
    // Bob's poll signals too, and must not widen the view.
    await pollCharacter(page, 90000002);
    await expect(page.locator('#tile-sell-value .amount')).toHaveText(`${price * 10 / 1_000_000}m`);
    expect(round).toBeGreaterThanOrEqual(0);
  }

  await expect(page).toHaveURL(/\/dashboard\?chars=90000001&range=24h$/);
  await expect(page.locator('#breakdown tbody tr')).toHaveCount(1);
  await expect(page.locator('#character-90000002')).toHaveCount(0);
  await expect(page.locator('#range-control').getByRole('link', { name: '24 hours' })).toHaveAttribute('aria-current', 'true');
  expect((await chartData(page, 'value-history'))[0].length).toBeLessThanOrEqual(49);
  // After three refreshes there are exactly the four charts of the page, not twelve more.
  expect(await instanceCount(page)).toBe(4);
});
