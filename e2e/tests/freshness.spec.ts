import type { Page } from '@playwright/test';
import { expect, loginWith, openCharacters, pollCharacter, query, stubControl, stubOrder, test, unfoldFreshness, wealthPoll } from '../support/fixtures';

const toggle = (page: Page) => page.getByRole('button', { name: 'Freshness' });

async function needsRelogin(page: Page) {
  await query('UPDATE characters SET needs_relogin = true WHERE character_id = 90000001');
}

async function twoCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

test('the orders freshness starts collapsed, with the tables reaching the strip', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1)] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders');
  await expect(page.locator('#sell-orders #order-1')).toBeVisible();

  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#freshness-body')).toBeHidden();
  await expect(page.getByText('Last update')).toBeHidden();
  await expect(async () => {
    const sell = (await page.locator('#sell-orders').boundingBox())!;
    const strip = (await page.locator('#freshness').boundingBox())!;
    expect(strip.x).toBeGreaterThanOrEqual(sell.x + sell.width);
    expect(strip.width).toBeLessThanOrEqual(40);
    expect(strip.x - (sell.x + sell.width)).toBeLessThanOrEqual(12);
  }).toPass();
});

test('the toggle unfolds and folds the freshness and names itself', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders');

  await expect(toggle(page)).toHaveAttribute('title', 'Show freshness');
  await expect(toggle(page).locator('svg:visible')).toHaveCount(1);
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect(toggle(page)).toHaveAttribute('title', 'Hide freshness');
  await expect(page.locator('#freshness')).toContainText('Last update');
  await expect(page.locator('#freshness')).toContainText('Alice Alpha');

  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(toggle(page)).toHaveAttribute('title', 'Show freshness');
  await expect(page.locator('#freshness-body')).toBeHidden();
});

test('a character needing re-login shows a warning count in the error colour while collapsed', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await needsRelogin(page);
  await page.goto('/orders');

  const badge = page.locator('#freshness .badge.warnings');
  await expect(badge).toHaveText('1');
  await expect(badge).toBeVisible();
  const [badgeColour, errorColour] = await page.evaluate(() => {
    const probe = document.createElement('div');
    probe.style.backgroundColor = 'var(--error)';
    document.body.append(probe);
    const error = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return [getComputedStyle(document.querySelector('#freshness .badge.warnings')!).backgroundColor, error];
  });
  expect(badgeColour).toBe(errorColour);
  await expect(toggle(page)).toHaveAccessibleDescription('1 character needs attention');
});

test('a fresh orders page shows no warning count', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders');
  await expect(toggle(page)).toBeVisible();
  await expect(page.locator('#freshness .badge.warnings')).toHaveCount(0);
});

test('the unfolded state survives a live refresh and a sort change but not a reload', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(1), stubOrder(2, { price: 20 })] });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await page.goto('/orders');
  await unfoldFreshness(page);

  const refreshed = page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/freshness');
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  await refreshed;
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#freshness')).toContainText('Last update');

  await page.locator('#sell-orders').getByRole('link', { name: 'Price' }).click();
  await expect(page).toHaveURL(/sort=price/);
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#freshness')).toContainText('Last update');

  await page.reload();
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#freshness-body')).toBeHidden();
});

test('the wealth freshness starts collapsed without a badge and survives a filter change', async ({ page }) => {
  await twoCharacters(page);
  for (const id of [90000001, 90000002]) {
    await wealthPoll(page, id, 'wallet');
    await wealthPoll(page, id, 'assets');
  }
  await page.getByRole('navigation').getByRole('link', { name: 'Wealth' }).click();
  await expect(page).toHaveURL(/\/wealth$/);

  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('#wealth-freshness .badge.warnings')).toHaveCount(0);
  await expect(page.locator('#wealth-freshness-body')).toBeHidden();
  await unfoldFreshness(page);
  await expect(page.locator('#wealth-freshness')).toContainText('Bob Bravo');

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);
  await expect(toggle(page)).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#wealth-freshness')).toContainText('Alice Alpha');
  await expect(page.locator('#wealth-freshness')).not.toContainText('Bob Bravo');
});

test('the wealth page shows a warning count for a character needing re-login', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await needsRelogin(page);
  await page.goto('/wealth');
  await expect(page.locator('#wealth-freshness .badge.warnings')).toHaveText('1');
});
