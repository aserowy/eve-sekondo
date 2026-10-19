import type { Page } from '@playwright/test';
import { verticalOverflow } from '../support/layout';
import { expect, loginWith, openCharacters, pollCharacter, query, stubControl, stubOrder, test, wealthPoll } from '../support/fixtures';

async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

test('every table page has its freshness at the end, unfolded, without a heading, with no toggle and no warning count', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await query('UPDATE characters SET needs_relogin = true');

  for (const path of ['/orders/sell', '/orders/buy', '/wealth', '/trades', '/opportunities/expansion', '/opportunities/trend']) {
    await page.goto(path);
    const freshness = page.locator('#freshness');
    await expect(freshness, path).toBeVisible();
    await expect(freshness.getByRole('heading'), path).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Freshness' }), path).toHaveCount(0);
    await expect(page.locator('.freshness-toggle, .warnings'), path).toHaveCount(0);
    // The freshness is the last thing on the page.
    const last = await page.locator('main section, main nav.pagination, main .kpis').evaluateAll((els) =>
      Math.max(...els.map((el) => el.getBoundingClientRect().bottom)),
    );
    expect((await freshness.boundingBox())!.y + (await freshness.boundingBox())!.height, path).toBeGreaterThanOrEqual(last - 1);
  }
});

test('a character needing re-login is stale with a link to log in again on the orders, wealth and trades pages', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await query('UPDATE characters SET needs_relogin = true WHERE character_id = 90000001');

  for (const path of ['/orders/sell', '/wealth', '/trades']) {
    await page.goto(path);
    const freshness = page.locator('#freshness');
    await expect(freshness, path).toContainText('Alice Alpha');
    await expect(freshness.getByText('Stale').first(), path).toBeVisible();
    await expect(freshness.getByRole('link', { name: 'Log in again' }), path).toBeVisible();
  }
});

test('the orders freshness survives a live refresh and a sort change', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1), stubOrder(2, { price: 20 })] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');

  const refreshed = page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/freshness');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await refreshed;
  await expect(page.locator('#freshness')).toContainText('Last update');

  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price/);
  await expect(page.locator('#freshness')).toContainText('Last update');
});

test('the wealth freshness follows the selection', async ({ page }) => {
  await twoCharacters(page);
  for (const id of [90000001, 90000002]) {
    await wealthPoll(page, id, 'wallet');
    await wealthPoll(page, id, 'assets');
  }
  await page.goto('/wealth');
  await expect(page.locator('#freshness')).toContainText('Bob Bravo');

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();

  await expect(page).toHaveURL(/chars=90000001/);
  await expect(page.locator('#freshness')).toContainText('Alice Alpha');
  await expect(page.locator('#freshness')).not.toContainText('Bob Bravo');
});

test('the trades freshness marks the characters whose transactions are stale', async ({ page }) => {
  await twoCharacters(page);
  await wealthPoll(page, 90000001, 'transactions');
  await wealthPoll(page, 90000002, 'transactions');
  await page.goto('/trades');
  await expect(page.locator('#freshness').getByText('Stale')).toHaveCount(0);

  await query(
    "UPDATE wealth_sync SET last_success_at = now() - interval '3 hours' WHERE kind = 'transactions' AND character_id = 90000002",
  );
  await page.goto('/trades');

  await expect(page.locator('#freshness li', { hasText: 'Bob Bravo' }).getByText('Stale')).toBeVisible();
  await expect(page.locator('#freshness li', { hasText: 'Alice Alpha' }).getByText('Stale')).toHaveCount(0);
});

test('on a short page the freshness footer sits at the bottom edge of the window', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1), stubOrder(2, { price: 20 })] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell');

  const footer = page.getByRole('contentinfo');
  await expect(footer.locator('#freshness')).toContainText('Alice Alpha');
  const box = (await footer.boundingBox())!;
  expect(Math.abs(box.y + box.height - 1080)).toBeLessThan(1);
  expect(await verticalOverflow(page)).toBeLessThanOrEqual(0);
});

test('on a long page the freshness footer follows the content and only comes into view at the end', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: Array.from({ length: 200 }, (_, i) => stubOrder(i + 1, { price: 10 + i })),
  });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders/sell?per_page=200');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(200);

  const footer = page.getByRole('contentinfo');
  await expect(footer).not.toBeInViewport();

  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(footer).toBeInViewport();
  const pager = (await page.locator('nav.pagination').boundingBox())!;
  expect((await footer.boundingBox())!.y).toBeGreaterThanOrEqual(pager.y + pager.height);
});

test('the freshness is the content-info landmark next to the main landmark, and pages without freshness have no footer', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.goto('/trades');
  await expect(page.getByRole('main')).toHaveCount(1);
  await expect(page.getByRole('main').locator('#content')).toHaveCount(1);
  await expect(page.getByRole('contentinfo')).toHaveCount(1);
  await expect(page.getByRole('contentinfo').locator('#freshness')).toHaveCount(1);

  for (const path of ['/dashboard', '/characters']) {
    await page.goto(path);
    await expect(page.getByRole('main'), path).toHaveCount(1);
    await expect(page.getByRole('contentinfo'), path).toHaveCount(0);
  }
});

test('the freshness footer is quiet: no heading, small dimmed text and one line per character', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await twoCharacters(page);
  await wealthPoll(page, 90000001, 'transactions');
  await wealthPoll(page, 90000002, 'transactions');
  await page.goto('/trades');

  const footer = page.getByRole('contentinfo');
  await expect(footer.getByRole('heading')).toHaveCount(0);
  const style = await footer.evaluate((el) => {
    const s = getComputedStyle(el);
    return { size: s.fontSize, border: s.borderTopStyle };
  });
  expect(style).toEqual({ size: '14px', border: 'solid' });

  const entries = footer.locator('#freshness li');
  await expect(entries).toHaveCount(2);
  for (const entry of await entries.all()) {
    await expect(entry).toContainText('Last update:');
    await expect(entry).toContainText('Next update:');
    const lines = await entry.evaluate((el) => el.getBoundingClientRect().height / parseFloat(getComputedStyle(el).lineHeight));
    expect(lines).toBeLessThan(1.5);
  }
});
