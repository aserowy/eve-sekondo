import { expect, loginWith, openCharacters, pollCharacter, query, stubControl, stubOrder, test, unfoldFreshness } from '../support/fixtures';
import { chartDrawn } from '../support/charts';
import { expectInsideViewport, horizontalOverflow, verticalOverflow } from '../support/layout';

async function ordersPageWithOneSellOrder(page: import('@playwright/test').Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders');
  await expect(page.locator('#sell-orders #order-1')).toBeVisible();
}

test('the theme is dense: small type, short rows, one stylesheet', async ({ page }) => {
  const sheets: string[] = [];
  page.on('response', (r) => {
    if (r.request().resourceType() === 'stylesheet') sheets.push(new URL(r.url()).pathname);
  });
  await ordersPageWithOneSellOrder(page);

  const fontSize = await page.evaluate(() => parseFloat(getComputedStyle(document.body).fontSize));
  expect(fontSize).toBeLessThanOrEqual(13);
  const row = await page.locator('#sell-orders #order-1 tr:not(.details)').boundingBox();
  expect(row!.height).toBeLessThanOrEqual(22);
  expect(await page.evaluate(() => document.styleSheets.length)).toBe(1);
  expect(new Set(sheets)).toEqual(new Set(['/static/app.css']));
});

test('pages use the full window width', async ({ page }) => {
  await page.setViewportSize({ width: 2560, height: 1080 });
  await ordersPageWithOneSellOrder(page);

  // The tables refresh themselves once the event stream opens, which swaps them for fresh copies.
  await expect(async () => {
    for (const id of ['#sell-orders', '#buy-orders']) {
      expect((await page.locator(id).boundingBox())!.x, id).toBeLessThanOrEqual(8);
    }
    const side = (await page.locator('#freshness').boundingBox())!;
    expect(2560 - (side.x + side.width), '#freshness').toBeLessThanOrEqual(8);
  }).toPass();
});

test('the dashboard does not scroll horizontally at 1024px', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  await loginWith(page, 'Alice Alpha');
  await expect(page.locator('#tile-sell-value')).toBeVisible();
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
});

test('the character filter lives in the top bar of dashboard, orders and wealth, not on the characters page', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  for (const path of ['/dashboard', '/orders', '/wealth']) {
    await page.goto(path);
    const filter = page.getByRole('navigation').getByRole('group', { name: 'Characters' });
    await expect(filter, path).toBeVisible();
    await expect(filter.getByRole('checkbox', { name: 'Alice Alpha' }), path).toBeChecked();
    await expect(page.locator('main #character-filter'), path).toHaveCount(0);
  }
  await page.goto('/characters');
  await expect(page.locator('#character-filter')).toHaveCount(0);
});

test('the dashboard time range control sits in the top bar', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await expect(page.getByRole('navigation').locator('#range-control')).toBeVisible();
  await expect(page.locator('main #range-control')).toHaveCount(0);
});

async function threeCharactersWithOrders(page: import('@playwright/test').Page) {
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

test('the dashboard lays out tiles, breakdown, one row of three graphs, then each over-time graph on its own row', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await threeCharactersWithOrders(page);
  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);

  // The summary refreshes itself once the event stream opens, which swaps its tiles and canvases for fresh ones.
  await expect(async () => {
    for (const tile of ['#tile-sell-value', '#tile-escrow', '#tile-wealth', '#tile-sell-count', '#tile-buy-count', '#breakdown']) {
      await expectInsideViewport(page, page.locator(tile));
    }
    const canvas = (kind: string) => page.locator(`canvas[data-chart="${kind}"]`);
    for (const kind of ['split', 'top-sell', 'top-buy']) await expectInsideViewport(page, canvas(kind));

    const box = async (selector: string) => (await page.locator(selector).boundingBox())!;
    const tiles = await box('#tiles');
    const breakdown = await box('.breakdown');
    expect(Math.abs(breakdown.width - tiles.width), 'breakdown as wide as the tiles').toBeLessThanOrEqual(2);
    expect(breakdown.y).toBeGreaterThanOrEqual(tiles.y + tiles.height - 1);

    const [split, sell, buy] = [await box('.split'), await box('.top-sell'), await box('.top-buy')];
    expect(Math.abs(split.y - sell.y)).toBeLessThanOrEqual(2);
    expect(Math.abs(sell.y - buy.y)).toBeLessThanOrEqual(2);
    expect(sell.x).toBeGreaterThanOrEqual(split.x + split.width - 1);
    expect(buy.x).toBeGreaterThanOrEqual(sell.x + sell.width - 1);
    expect(split.y).toBeGreaterThanOrEqual(breakdown.y + breakdown.height - 1);

    let above = split;
    for (const panel of ['.history-value', '.history-count', '.history-wealth']) {
      const b = await box(panel);
      expect(Math.abs(b.width - tiles.width), `${panel} as wide as the tiles`).toBeLessThanOrEqual(2);
      expect(b.y, `${panel} below the previous row`).toBeGreaterThanOrEqual(above.y + above.height - 1);
      expect(b.height, `${panel} height`).toBeGreaterThanOrEqual(16 * 11);
      above = b;
    }
  }).toPass();
});

test('with twenty characters the breakdown scrolls inside itself, keeps its header, and the graph row stays on the first screen', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  // The stub only knows three characters, so the other nineteen are linked straight in the database.
  await query(
    `INSERT INTO characters (character_id, account_id, name, owner_hash)
     SELECT 91000000 + n, (SELECT account_id FROM characters WHERE character_id = 90000001), 'Extra Character ' || n, 'hash-' || n
     FROM generate_series(1, 19) AS n`,
  );
  const refreshed = page.waitForResponse((r) => r.url().includes('/dashboard/summary'));
  await page.goto('/dashboard');
  await expect(page.locator('#breakdown tbody tr')).toHaveCount(20);
  // The event stream opening refreshes the summary once; scrolling before that would be undone by it.
  await refreshed;
  await page.waitForTimeout(300);

  const scroller = page.locator('.breakdown .scroll');
  await expect(async () => {
    const { scrollHeight, clientHeight } = await scroller.evaluate((el) => ({ scrollHeight: el.scrollHeight, clientHeight: el.clientHeight }));
    expect(scrollHeight).toBeGreaterThan(clientHeight);
  }).toPass();

  await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight));
  await expect(page.locator('#character-91000019')).toBeVisible();
  const box = (await scroller.boundingBox())!;
  const header = (await page.locator('#breakdown thead th').first().boundingBox())!;
  expect(header.y).toBeGreaterThanOrEqual(box.y - 1);
  expect(header.y).toBeLessThan(box.y + 24);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  for (const panel of ['.split', '.top-sell', '.top-buy']) await expectInsideViewport(page, page.locator(panel));
});

test('orders page: freshness right of the tables, buy below sell, the sell table scrolls on its own', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  const sells = Array.from({ length: 200 }, (_, i) => stubOrder(i + 1, { price: 10 + i }));
  await stubControl('orders', { character_id: 90000001, orders: [...sells, stubOrder(1001, { buy: true })] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  const refreshed = page.waitForResponse((r) => r.url().includes('/orders/tables'));
  await page.goto('/orders');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(200);
  // The event stream opening refreshes the tables once; scrolling before that would be undone by it.
  await refreshed;
  await page.waitForTimeout(300);
  await unfoldFreshness(page);

  // The tables refresh themselves once the event stream opens, so geometry is read until it settles.
  await expect(async () => {
    const sell = (await page.locator('#sell-orders').boundingBox())!;
    const buy = (await page.locator('#buy-orders').boundingBox())!;
    const side = (await page.locator('#freshness').boundingBox())!;
    expect(side.x).toBeGreaterThanOrEqual(sell.x + sell.width);
    expect(side.x).toBeGreaterThanOrEqual(buy.x + buy.width);
    expect(buy.y).toBeGreaterThanOrEqual(sell.y + sell.height - 1);
    expect(buy.x).toBeLessThan(side.x);
    expect(await verticalOverflow(page)).toBeLessThanOrEqual(0);
  }).toPass();

  const scroller = page.locator('#sell-orders .scroll');
  await expect(async () => {
    const { scrollHeight, clientHeight } = await scroller.evaluate((el) => ({ scrollHeight: el.scrollHeight, clientHeight: el.clientHeight }));
    expect(scrollHeight).toBeGreaterThan(clientHeight);
  }).toPass();
  const buyBefore = (await page.locator('#buy-orders').boundingBox())!;
  await scroller.evaluate((el) => (el.scrollTop = el.scrollHeight));
  await expect(page.locator('#order-200')).toBeInViewport();

  const box = (await scroller.boundingBox())!;
  const header = (await page.locator('#sell-orders thead th').first().boundingBox())!;
  expect(header.y).toBeGreaterThanOrEqual(box.y - 1);
  expect(header.y).toBeLessThan(box.y + 24);
  expect(await page.locator('#buy-orders').boundingBox()).toEqual(buyBefore);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  expect(await verticalOverflow(page)).toBeLessThanOrEqual(0);
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
  await page.goto('/orders');
  await unfoldFreshness(page);

  const link = page.locator('#freshness').getByRole('link', { name: 'Log in again' });
  await expect(link).toBeVisible();
  await expect(link.locator('svg')).toHaveCount(1);
  await expect(link).toHaveText('Log in again');
  const order = await link.evaluate((a) => [...a.childNodes].map((n) => n.nodeName.toLowerCase()));
  expect(order[0]).toBe('svg');
});

test('wealth page: freshness right of the graph and the items, items below the graph, no page scroll', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/wealth');
  await unfoldFreshness(page);

  // The panels refresh themselves once the event stream opens, so geometry is read until it settles.
  await expect(async () => {
    const graph = (await page.locator('#wealth-graph').boundingBox())!;
    const items = (await page.locator('#wealth-items').boundingBox())!;
    const side = (await page.locator('#wealth-freshness').boundingBox())!;
    expect(side.x).toBeGreaterThanOrEqual(graph.x + graph.width);
    expect(side.x).toBeGreaterThanOrEqual(items.x + items.width);
    expect(items.y).toBeGreaterThanOrEqual(graph.y + graph.height - 1);
    expect(await verticalOverflow(page)).toBeLessThanOrEqual(0);
  }).toPass();
  // The range control is in the graph panel's title row.
  const title = (await page.locator('#wealth-graph .panel-title').boundingBox())!;
  const control = (await page.locator('#wealth-range-control').boundingBox())!;
  expect(control.y).toBeGreaterThanOrEqual(title.y);
  expect(control.y + control.height).toBeLessThanOrEqual(title.y + title.height);
});

test('the wealth page Log in again link keeps its text and carries an icon', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await stubControl('revoke', { character_id: 90000001 });
  await page.evaluate(async () => {
    const token = document.querySelector('meta[name="csrf-token"]')!.getAttribute('content')!;
    await fetch('/_test/refresh/90000001', { method: 'POST', headers: { 'X-CSRF-Token': token } });
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 needs-relogin');
  await page.goto('/wealth');
  await unfoldFreshness(page);

  const link = page.locator('#wealth-freshness').getByRole('link', { name: 'Log in again' });
  await expect(link).toHaveText('Log in again');
  await expect(link.locator('svg')).toHaveCount(1);
});

test('charts use small, dimmed axis text, grid lines and compact legends', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await threeCharactersWithOrders(page);
  await page.goto('/dashboard');
  for (const kind of ['split', 'top-sell', 'value-history', 'count-history']) await chartDrawn(page, kind);

  await expect(async () => {
    const styles = await page.evaluate(() => {
      const luminance = (colour: string) => {
        const [r, g, b] = colour.match(/[\da-f]{2}/gi)!.map((h) => parseInt(h, 16) / 255);
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      };
      const Chart = (window as any).Chart;
      const body = getComputedStyle(document.body).color.match(/\d+/g)!.map(Number);
      const text = (0.2126 * body[0] + 0.7152 * body[1] + 0.0722 * body[2]) / 255;
      return ['split', 'top-sell', 'value-history', 'count-history'].map((kind) => {
        const chart = Chart.getChart(document.querySelector(`canvas[data-chart="${kind}"]`));
        const scales = chart.options.scales;
        const y = scales.y;
        const ticks = scales.x.ticks;
        return {
          kind,
          fontSize: Chart.defaults.font.size,
          tickColourDimmer: luminance(ticks.color) < text,
          maxTicks: y.ticks.maxTicksLimit ?? scales.x.ticks.maxTicksLimit,
          legendBox: chart.options.plugins.legend.display ? chart.options.plugins.legend.labels.boxWidth : 0,
        };
      });
    });
    for (const style of styles) {
      expect(style.fontSize, style.kind).toBeLessThanOrEqual(11);
      expect(style.tickColourDimmer, style.kind).toBe(true);
      expect(style.maxTicks, style.kind).toBeLessThanOrEqual(8);
      expect(style.legendBox, style.kind).toBeLessThanOrEqual(10);
    }
  }).toPass();
});

test('walking through every signed-in page raises no policy violation', async ({ page }) => {
  // The cspGuard fixture fails the test on any violation the console reports while this runs.
  await threeCharactersWithOrders(page);
  const nav = page.getByRole('navigation');

  await page.goto('/dashboard');
  await expect(page.locator('canvas[data-chart]')).toHaveCount(6);
  await nav.locator('#range-control').getByRole('link', { name: '30 days' }).click();
  await expect(page).toHaveURL(/range=30d$/);
  await nav.getByRole('checkbox', { name: 'Carol Charlie' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001,90000002&range=30d$/);

  await nav.getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\?chars=90000001,90000002$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await unfoldFreshness(page);
  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price&dir=asc/);
  await nav.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(1);

  await nav.getByRole('link', { name: 'Wealth' }).click();
  await expect(page).toHaveURL(/\/wealth\?chars=90000001/);
  await unfoldFreshness(page);
  await page.locator('#wealth-range-control').getByRole('link', { name: '30 days' }).click();
  await expect(page.locator('#wealth-range-control [aria-current="true"]')).toHaveText('30 days');
  await nav.getByRole('checkbox', { name: 'Bob Bravo' }).check();

  await nav.getByRole('link', { name: 'Characters' }).click();
  await expect(page).toHaveURL(/\/characters(\?.*)?$/);
  await expect(page.locator('.character')).toHaveCount(3);
  await page.waitForTimeout(500);
});
