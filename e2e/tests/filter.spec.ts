import { expect, loginWith, openCharacters, pollCharacter, stubControl, stubOrder, test } from '../support/fixtures';
import type { Page } from '@playwright/test';

/** Alice, Bob and Carol on one account, each with one sell order of their own. */
async function threeCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  for (const name of ['Bob Bravo', 'Carol Charlie']) {
    await page.getByRole('button', { name: 'Add character' }).click();
    await page.getByRole('link', { name }).click();
    await expect(page).toHaveURL(/\/characters$/);
  }
  for (const [id, order] of [
    [90000001, 11],
    [90000002, 22],
    [90000003, 33],
  ] as const) {
    await stubControl('orders', { character_id: id, orders: [stubOrder(order)] });
    await pollCharacter(page, id);
  }
}

test('narrow the selection, bookmark it, and carry it across pages', async ({ page, browser }) => {
  await threeCharacters(page);
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(3);
  await expect(page.getByRole('checkbox')).toHaveCount(3);
  await expect(page.getByRole('checkbox', { checked: true })).toHaveCount(3);

  // Deselect Carol: the URL lists the two others and the tables follow without a full reload.
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await page.getByRole('checkbox', { name: 'Carol Charlie' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001,90000002$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(page.locator('#order-33')).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
  await expect(page.getByRole('checkbox', { name: 'Carol Charlie' })).not.toBeChecked();

  // Narrow further, then select everything again: no `chars` at all.
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(1);
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).check();
  await page.getByRole('checkbox', { name: 'Carol Charlie' }).check();
  await expect(page).not.toHaveURL(/chars/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(3);

  // The selection carries across pages: orders -> dashboard -> orders.
  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();
  await expect(page).toHaveURL(/chars=90000002,90000003$/);
  const nav = page.locator('#main-nav');
  await nav.getByRole('link', { name: 'Dashboard' }).click();
  await expect(page).toHaveURL(/\/dashboard\?chars=90000002,90000003$/);
  await nav.getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\/sell\?chars=90000002,90000003$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).not.toBeChecked();

  // Bookmark: the same URL in a fresh browser session, after signing in, selects the same characters.
  const url = page.url();
  const cookies = await page.context().cookies();
  const other = await browser.newContext();
  await other.addCookies(cookies);
  const reopened = await other.newPage();
  await reopened.goto(url);
  await expect(reopened.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(reopened.locator('#order-11')).toHaveCount(0);
  await other.close();
});

test('a foreign character id in the URL is ignored', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(11)] });
  await pollCharacter(page, 90000001);

  // Bob belongs to nobody here: his ID is not one of this account's characters.
  await page.goto('/orders/sell?chars=90000002');

  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(1);
  await expect(page.getByRole('checkbox')).toHaveCount(1);
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.getByText('Bob Bravo')).toHaveCount(0);
});

test('the sorting is kept when the selection changes', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 5 }), stubOrder(2, { type: 35, price: 9 })],
  });
  await pollCharacter(page, 90000001);
  await page.goto('/orders/sell?sort=price&dir=desc');
  await expect(page.locator('#sell-orders tbody.order').first()).toHaveAttribute('id', 'order-2');

  // One character only: nothing to deselect, but the form still carries the sort fields.
  await expect(page.locator('#character-filter input[name="sort"]')).toHaveValue('price');
  await expect(page.locator('#character-filter input[name="dir"]')).toHaveValue('desc');
});

test('the character filter is a group with a visible "Characters" legend over side-by-side checkboxes', async ({ page }) => {
  await threeCharacters(page);
  await page.goto('/orders/sell');

  const group = page.locator('#filter-box').getByRole('group', { name: 'Characters' });
  await expect(group).toHaveCount(1);
  await expect(group.locator('legend')).toBeVisible();
  await expect(group.locator('legend')).toHaveText('Characters');
  const boxes = group.getByRole('checkbox');
  await expect(boxes).toHaveCount(3);
  for (const name of ['Alice Alpha', 'Bob Bravo', 'Carol Charlie']) await expect(group.getByRole('checkbox', { name })).toHaveCount(1);

  const rects = await boxes.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()));
  for (const r of rects) expect(Math.abs(r.y - rects[0].y)).toBeLessThanOrEqual(2);
  expect(rects[0].x).toBeLessThan(rects[1].x);
  expect(rects[1].x).toBeLessThan(rects[2].x);
  const legend = (await group.locator('legend').boundingBox())!;
  expect(legend.y + legend.height).toBeLessThanOrEqual(rects[0].y + 2);
});

/** Alice and Bob on one account. */
async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

const rem = (page: Page) => page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));

/** The horizontal gap between the character filter's right edge and the left edge of `next`. */
async function gapAfterCharacters(page: Page, next: string) {
  const characters = (await page.locator('#character-filter').boundingBox())!;
  const other = (await page.locator(next).boundingBox())!;
  return other.x - (characters.x + characters.width);
}

test('the character filter stands 3rem apart from the time range, wider than the gap between checkboxes', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await page.goto('/trades');

  const gap = await gapAfterCharacters(page, '#trades-range-control');
  expect(gap).toBeCloseTo(3 * (await rem(page)), 0);
  const labels = await page.locator('#character-filter fieldset label').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()));
  expect(labels).toHaveLength(2);
  expect(gap).toBeGreaterThan(labels[1].x - (labels[0].x + labels[0].width));
});

test('the character filter stands 3rem apart from the opportunity filters, wider than the gap between their fields', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await page.goto('/opportunities/trend');

  const margin = (await page.locator('#opportunity-filters').getByLabel('Minimum margin (%)').boundingBox())!;
  const isk = (await page.locator('#opportunity-filters').getByLabel('Minimum ISK/day').boundingBox())!;
  const characters = (await page.locator('#character-filter').boundingBox())!;
  const gap = margin.x - (characters.x + characters.width);
  expect(gap).toBeCloseTo(3 * (await rem(page)), 0);
  expect(gap).toBeGreaterThan(isk.x - (margin.x + margin.width));
});

test('the groups of the filter section have no divider line or border', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/dashboard');
  for (const selector of ['#character-filter', '#range-control']) {
    const widths = await page.locator(selector).evaluate((el) => {
      const s = getComputedStyle(el);
      return [s.borderTopWidth, s.borderRightWidth, s.borderBottomWidth, s.borderLeftWidth];
    });
    expect(widths, selector).toEqual(['0px', '0px', '0px', '0px']);
  }
});

const top = async (page: Page, selector: string) => (await page.locator(selector).first().boundingBox())!.y;

test('the groups of the trend page share their top edge', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await page.goto('/opportunities/trend');

  const legend = await top(page, '#character-filter legend');
  const margin = (await page.locator('#opportunity-filters').getByText('Minimum margin (%)').boundingBox())!.y;
  const fees = await top(page, '#fee-filter legend');
  expect(Math.abs(legend - margin)).toBeLessThanOrEqual(1);
  expect(Math.abs(fees - margin)).toBeLessThanOrEqual(1);
});

test('the character legend is level with the range label', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await page.goto('/trades');

  const legend = await top(page, '#character-filter legend');
  const range = await top(page, '#trades-range-control > span');
  expect(Math.abs(legend - range)).toBeLessThanOrEqual(1);
});

test('wrapped opportunity fields do not move the character filter', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 900 });
  await twoCharacters(page);
  await page.goto('/opportunities/trend');

  const margin = (await page.locator('#opportunity-filters').getByText('Minimum margin (%)').boundingBox())!.y;
  const item = (await page.locator('#opportunity-filters').getByText('Item name').boundingBox())!.y;
  const legend = await top(page, '#character-filter legend');
  expect(item, 'the fields wrap onto a second row').toBeGreaterThan(margin + 20);
  expect(Math.abs(legend - margin)).toBeLessThanOrEqual(1);
  expect(Math.abs((await top(page, '#fee-filter legend')) - margin)).toBeLessThanOrEqual(1);
});

test('an error under a field does not move the character filter', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await page.goto('/opportunities/trend');
  const before = await top(page, '#character-filter legend');

  await page.locator('#opportunity-filters').getByLabel('Minimum margin (%)').fill('-1');
  await page.locator('#opportunity-filters').getByLabel('Minimum margin (%)').blur();
  await expect(page.locator('#opportunity-filters [aria-invalid="true"]')).toHaveCount(1);

  expect(await top(page, '#character-filter legend')).toBeCloseTo(before, 0);
});
