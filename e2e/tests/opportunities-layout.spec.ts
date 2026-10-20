import type { Page } from '@playwright/test';
import { aliceTrained, bookOf, calm, crossed, fill } from '../support/opportunities';
import { expect, openCharacters, stationBook, stationSeed, stubControl, stubMarket, test } from '../support/fixtures';
import { horizontalOverflow } from '../support/layout';

const TABLES = [
  ['expansion', 'Recurring spread expansion'],
  ['classic', 'Classic spread trade'],
  ['reversion', 'Mean reversion'],
  ['trend', 'Trend'],
] as const;

const filterBox = (page: Page) => page.locator('#filter-box');

async function openOpportunities(page: Page, table = 'classic') {
  await aliceTrained(page);
  await page.goto(`/opportunities/${table}`);
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

test('the page is titled by its one table, and its filter section holds the filters and the context', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openOpportunities(page);

  await expect(page.getByRole('heading', { level: 1 })).toHaveText(['Classic spread trade']);
  await expect(page.locator('main .station-title')).toHaveCount(0);
  await expect(page.locator('#classic h2')).toHaveCount(0);
  for (const [id] of TABLES.filter(([id]) => id !== 'classic')) await expect(page.locator(`main #${id}`)).toHaveCount(0);

  const box = filterBox(page);
  await expect(box.locator('#character-filter').getByRole('checkbox', { name: 'Alice Alpha' })).toBeChecked();
  for (const label of ['Minimum margin (%)', 'Minimum ISK/day', 'Item name', 'Capital per item (ISK)']) {
    await expect(box.getByLabel(label)).toBeVisible();
  }
  await expect(box.locator('[aria-invalid="true"]')).toHaveCount(0);
  await expect(box.locator('#filter-context')).toContainText('Jita IV - Moon 4 - Caldari Navy Assembly Plant');
  await expect(box.locator('#filter-context')).toContainText('Fees of Alice Alpha');
  await expect(page.getByRole('button', { name: 'Apply' })).toHaveCount(0);
  await expect(page.locator('#main-nav #character-filter')).toHaveCount(0);
});

test('the sub-menu switches between the four tables without row counts, keeping the selection and the sortings', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openOpportunities(page);
  await stubMarket([...bookOf(34, 100, 130, 1), ...bookOf(35, 100, 130, 3)]);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: [...crossed(34, 1.1), ...crossed(35, 1.2)] })).startsWith('200 ')).toBe(true);
  await page.reload();
  await expect(page.locator('#classic tbody tr')).toHaveCount(2);

  const menu = page.locator('nav.sub-menu');
  await expect(menu.getByRole('link')).toHaveText(TABLES.map(([, title]) => title));
  await expect(menu.locator('.count')).toHaveCount(0);
  await expect(menu.getByRole('link', { name: 'Classic spread trade' })).toHaveAttribute('aria-current', 'page');
  // Not a single digit in the menu: the entries are only titles.
  expect(await menu.innerText()).not.toMatch(/\d/);

  await page.locator('#classic').getByRole('link', { name: 'Item' }).click();
  await expect(page).toHaveURL(/cs=item&cd=asc/);
  await menu.getByRole('link', { name: 'Trend' }).click();
  await expect(page).toHaveURL(/\/opportunities\/trend\?cs=item&cd=asc$/);
  await expect(menu.getByRole('link', { name: 'Trend' })).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('#trend')).toBeVisible();
  await expect(page.locator('#classic')).toHaveCount(0);

  // Raising the minimum margin until a table is empty still shows no number.
  await setFilter(page, 'Minimum margin (%)', '99');
  await expect(page.locator('#trend tbody tr')).toHaveCount(0);
  expect(await menu.innerText()).not.toMatch(/\d/);
});

test('more than fifty matches are paged: the first page has fifty, the second the rest, and a filter goes back to page 1', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withItems(page, 80);
  await expect(page.locator('#classic tbody tr')).toHaveCount(50);
  const pagination = page.locator('.pagination');
  await expect(pagination).toContainText('Page 1 of 2');

  await pagination.getByRole('link', { name: 'Next' }).click();
  await expect(page).toHaveURL(/page=2$/);
  await expect(page.locator('#classic tbody tr')).toHaveCount(30);
  await expect(pagination).toContainText('Page 2 of 2');

  await setFilter(page, 'Item name', 'Type 10');
  await expect(page).not.toHaveURL(/page=/);
  await expect(pagination).not.toContainText('Page 2');
});

test('a rejected filter value is reported inside the filter section and leaves the table alone', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await withItems(page, 1);
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);

  await setFilter(page, 'Capital per item (ISK)', '0');

  const capital = page.locator('#opportunity-filters').getByLabel('Capital per item (ISK)');
  await expect(capital).toHaveAttribute('aria-invalid', 'true');
  const error = filterBox(page).locator('small');
  await expect(error).toContainText('capital');
  await expect(error).toContainText('positive');
  await expect(capital).toHaveValue('0');
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);

  await setFilter(page, 'Capital per item (ISK)', '50000000');
  await expect(filterBox(page).locator('small')).toHaveCount(0);
  await expect(capital).not.toHaveAttribute('aria-invalid', 'true');
  await expect(page.locator('#classic tbody tr')).toHaveCount(1);
});

test('deselecting a character updates the fees text in the filter section', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openOpportunities(page);
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await page.goto('/opportunities/classic');
  const context = filterBox(page).locator('#filter-context');
  await expect(context).toContainText('Fees of Alice Alpha');

  await filterBox(page).getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();

  await expect(page).toHaveURL(/\/opportunities\/classic\?chars=90000002/);
  await expect(context).not.toContainText('Fees of Alice Alpha');
  await expect(context).toContainText('base rates');
  await expect(page.locator('nav.sub-menu a')).toHaveCount(4);
  await expect(page.locator('nav.sub-menu a').first()).toHaveAttribute('href', /chars=90000002/);
});

test('the freshness is at the end of the page, unfolded and without a warning count', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 500 });
  await withItems(page, 30);
  const strip = page.locator('#freshness');
  await expect(strip).toBeVisible();
  await expect(page.getByRole('button', { name: 'Freshness' })).toHaveCount(0);
  await expect(strip.locator('.warnings')).toHaveCount(0);
  await expect(strip).toContainText('Order book');
  await expect(strip).toContainText('Market history');

  const pagination = (await page.locator('.pagination').boundingBox())!;
  expect((await strip.boundingBox())!.y).toBeGreaterThanOrEqual(pagination.y + pagination.height - 1);
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
});

test('a stale book with a failed refresh shows stale and the error in the freshness', async ({ page }) => {
  await openOpportunities(page);
  await stubMarket(bookOf(34, 100, 130, 1));
  await stubControl('market-fail', { region_id: 10000002, page: 1, status: 500 });
  expect(await stationBook(page)).not.toBe('200 updated');
  await page.reload();

  const strip = page.locator('#freshness');
  await expect(strip).toContainText('Stale');
  await expect(strip).toContainText('Last error');
  await expect(strip.locator('.warnings')).toHaveCount(0);
});

test('the filter section wraps in a narrow window and the page does not scroll sideways', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await openOpportunities(page);
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await page.goto('/opportunities/classic');

  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
  const box = (await filterBox(page).boundingBox())!;
  expect(box.width).toBeLessThanOrEqual(1024);

  // The groups wrap onto more lines, 0.75rem apart.
  const groups = await filterBox(page).locator('> *').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()));
  const lines = groups.filter((g) => g.height > 0).sort((a, b) => a.y - b.y);
  const next = lines.find((g) => g.y > lines[0].y + 1)!;
  expect(next).toBeDefined();
  const firstLineBottom = Math.max(...lines.filter((g) => Math.abs(g.y - lines[0].y) <= 1).map((g) => g.y + g.height));
  const rem = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));
  expect(next.y - firstLineBottom).toBeCloseTo(0.75 * rem, 0);
});

test('each opportunity filter has its label above its input, and the four inputs are equally wide', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openOpportunities(page, 'trend');

  const labels = ['Minimum margin (%)', 'Minimum ISK/day', 'Item name', 'Capital per item (ISK)'];
  const fields = [];
  for (const label of labels) {
    const input = page.locator('#opportunity-filters').getByLabel(label);
    await expect(input, label).toBeVisible();
    fields.push(
      await input.evaluate((el) => {
        const label = document.querySelector(`label[for="${el.id}"]`)!;
        const range = document.createRange();
        range.selectNodeContents(label);
        return { text: range.getBoundingClientRect().toJSON(), input: el.getBoundingClientRect().toJSON() };
      }),
    );
  }
  for (const [i, f] of fields.entries()) {
    expect(f.text.bottom, `${labels[i]} above its input`).toBeLessThanOrEqual(f.input.top + 1);
    expect(Math.abs(f.input.width - fields[0].input.width), `${labels[i]} width`).toBeLessThanOrEqual(1);
  }
  // Side by side on one row at this width.
  for (const f of fields) expect(Math.abs(f.input.top - fields[0].input.top)).toBeLessThanOrEqual(1);
});

test('the item name is a plain input, shaped like the number fields beside it', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openOpportunities(page, 'trend');

  const look = (label: string) =>
    page.locator('#opportunity-filters').getByLabel(label).evaluate((el) => {
      const s = getComputedStyle(el);
      return {
        type: (el as HTMLInputElement).type,
        height: el.getBoundingClientRect().height,
        padding: [s.paddingTop, s.paddingRight, s.paddingBottom, s.paddingLeft],
        radius: s.borderRadius,
        image: s.backgroundImage,
      };
    });
  const name = await look('Item name');
  const margin = await look('Minimum margin (%)');
  expect(name.type).toBe('text');
  expect(name.height).toBeCloseTo(margin.height, 1);
  expect(name.padding).toEqual(margin.padding);
  expect(name.radius).toBe(margin.radius);
  expect(name.image).toBe(margin.image);
});
