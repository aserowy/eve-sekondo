import type { Page } from '@playwright/test';
import { aliceTrained, bookOf, calm, crossed, fill } from '../support/opportunities';
import { expect, openCharacters, stationBook, stationSeed, stubControl, stubMarket, test, unfoldFreshness } from '../support/fixtures';
import { horizontalOverflow, verticalOverflow } from '../support/layout';

const TABLES = [
  ['expansion', 'Recurring spread expansion'],
  ['classic', 'Classic spread trade'],
  ['reversion', 'Mean reversion'],
  ['trend', 'Trend'],
] as const;

const filterBox = (page: Page) => page.locator('header.bars #filter-box');

async function openOpportunities(page: Page) {
  await aliceTrained(page);
  await page.locator('#main-nav').getByRole('link', { name: 'Opportunities' }).click();
  await expect(page).toHaveURL(/\/opportunities$/);
}

/** `count` items with a classic spread each: the buy at 100, the sell at 130, and calm history. */
async function withItems(page: Page, count: number) {
  await openOpportunities(page);
  const types = Array.from({ length: count }, (_, i) => 1000 + i);
  await fill(page, types.flatMap((type, i) => bookOf(type, 100, 130, 1 + 2 * i)), types.flatMap((type) => calm(type)));
}

/** Changes a filter input the way a user does: type, then leave the field. */
async function setFilter(page: Page, label: string, value: string) {
  const input = page.locator('#opportunity-filters').getByLabel(label);
  await input.fill(value);
  await input.press('Tab');
}

test('the opportunities page has no heading or loose text, and its filter box holds the filters and the context', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openOpportunities(page);

  await expect(page.locator('h1')).toHaveCount(0);
  await expect(page.locator('main .station-title')).toHaveCount(0);
  expect(await page.locator('main p').evaluateAll((ps) => ps.filter((p) => !p.closest('.panel')).length)).toBe(0);
  for (const [id, title] of TABLES) await expect(page.locator(`main #${id} .panel-title`)).toContainText(title);

  const box = filterBox(page);
  await expect(box.locator('#character-filter').getByRole('checkbox', { name: 'Alice Alpha' })).toBeChecked();
  for (const label of ['Minimum margin (%)', 'Minimum ISK/day', 'Item name', 'Capital per item (ISK)']) {
    await expect(box.getByLabel(label)).toBeVisible();
  }
  await expect(box.locator('#filter-error')).toBeEmpty();
  await expect(box.locator('#filter-context')).toContainText('Jita IV - Moon 4 - Caldari Navy Assembly Plant');
  await expect(box.locator('#filter-context')).toContainText('Fees of Alice Alpha');
  await expect(page.getByRole('button', { name: 'Apply' })).toHaveCount(0);
  await expect(page.locator('#main-nav #character-filter')).toHaveCount(0);
});

test('the classic table shows twenty rows, scrolls inside itself and keeps the page free of sideways scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await withItems(page, 50);
  await expect(page.locator('#classic tbody tr')).toHaveCount(50);

  await expect(async () => {
    const scroller = page.locator('#classic .scroll');
    const box = (await scroller.boundingBox())!;
    const visible = await page.locator('#classic tbody tr').evaluateAll(
      (trs, bottom) => trs.filter((tr) => tr.getBoundingClientRect().bottom <= bottom + 0.5).length,
      box.y + box.height,
    );
    expect(visible).toBe(20);
    const { scrollHeight, clientHeight } = await scroller.evaluate((el) => ({ scrollHeight: el.scrollHeight, clientHeight: el.clientHeight }));
    expect(scrollHeight).toBeGreaterThan(clientHeight);
  }).toPass();
  expect(await verticalOverflow(page)).toBeGreaterThan(0);
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
  // Cells never wrap: every body row is a single line.
  const heights = await page.locator('#classic tbody tr').evaluateAll((trs) => trs.map((tr) => tr.getBoundingClientRect().height));
  expect(Math.max(...heights)).toBeLessThanOrEqual(22);
});

test('the sub-menu has a link per table with its row count, and the counts follow the filters', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openOpportunities(page);
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();

  const menu = page.locator('header.bars #section-nav');
  await expect(menu.getByRole('link')).toHaveCount(4);
  for (const [id, title] of TABLES) {
    const link = menu.locator(`a[href="#${id}"]`);
    await expect(link).toContainText(title);
    const rows = await page.locator(`#${id} tbody tr`).count();
    await expect(link.locator('.count')).toHaveText(String(rows));
  }
  await expect(menu.locator('a[href="#classic"] .count')).toHaveText('2');

  await setFilter(page, 'Minimum margin (%)', '99');

  await expect(page.locator('#classic tbody tr')).toHaveCount(0);
  await expect(menu.locator('a[href="#classic"] .count')).toHaveText('0');
  await expect(menu.locator('a[href="#trend"] .count')).toHaveText(String(await page.locator('#trend tbody tr').count()));
});

test('a rejected filter value is reported inside the filter box and leaves the tables alone', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withItems(page, 1);
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);

  await setFilter(page, 'Capital per item (ISK)', '0');

  const error = filterBox(page).locator('#filter-error');
  await expect(error).toContainText('capital');
  await expect(error).toContainText('positive');
  await expect(page.locator('#opportunity-filters').getByLabel('Capital per item (ISK)')).toHaveValue('0');
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);

  await setFilter(page, 'Capital per item (ISK)', '50000000');
  await expect(error).toBeEmpty();
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);
});

test('deselecting a character updates the fees text in the filter box', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openOpportunities(page);
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await page.locator('#main-nav').getByRole('link', { name: 'Opportunities' }).click();
  const context = filterBox(page).locator('#filter-context');
  await expect(context).toContainText('Fees of Alice Alpha');

  await filterBox(page).getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();

  await expect(page).toHaveURL(/chars=90000002/);
  await expect(context).not.toContainText('Fees of Alice Alpha');
  await expect(context).toContainText('base rates');
  await expect(page.locator('header.bars #section-nav a')).toHaveCount(4);
});

test('the freshness column starts collapsed, counts its warnings and stays in the window while the page scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 500 });
  await withItems(page, 30);
  const strip = page.locator('#opportunity-freshness');
  await expect(strip).toBeVisible();
  await expect(strip).not.toHaveAttribute('data-open', /.*/);
  await expect(page.getByRole('button', { name: 'Freshness' })).toHaveAttribute('aria-expanded', 'false');
  await expect(strip.locator('.warnings')).toHaveCount(0);
  await expect(strip.locator('#opportunity-freshness-body')).toBeHidden();

  await unfoldFreshness(page);
  await expect(strip).toContainText('Order book');
  await page.locator('#section-nav').getByRole('link', { name: 'Trend' }).click();
  await expect(async () => {
    const side = (await strip.boundingBox())!;
    const bars = (await page.locator('header.bars').boundingBox())!;
    expect(side.y).toBeGreaterThanOrEqual(bars.y + bars.height - 1);
    expect(side.y + side.height).toBeLessThanOrEqual(500);
    const tables = (await page.locator('#classic').boundingBox())!;
    expect(side.x).toBeGreaterThanOrEqual(tables.x + tables.width);
  }).toPass();
  expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(0);
});

test('a stale book with a failed refresh shows two warnings on the collapsed strip', async ({ page }) => {
  await openOpportunities(page);
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('market-fail', { region_id: 10000002, page: 1, status: 500 });
  expect(await stationBook(page)).not.toBe('200 updated');
  await page.reload();

  await expect(page.locator('#opportunity-freshness .warnings')).toHaveText('2');
});
