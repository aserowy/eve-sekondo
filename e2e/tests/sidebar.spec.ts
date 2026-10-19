import { expect, loginWith, test } from '../support/fixtures';

test('collapsing the sidebar hides the labels, widens the page, keeps names and titles, and persists across pages', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/wealth');
  const toggle = page.getByRole('button', { name: 'Menu' });
  const labels = page.locator('#main-nav .label');

  // A fresh browser starts expanded.
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(labels.first()).toBeVisible();
  const expandedWidth = (await page.locator('main').boundingBox())!.width;

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  for (const label of await labels.all()) await expect(label).toBeHidden();
  expect((await page.locator('main').boundingBox())!.width).toBeGreaterThan(expandedWidth + 40);
  for (const name of ['Dashboard', 'Characters', 'Wealth', 'Trades', 'Orders', 'Opportunities']) {
    const link = page.getByRole('link', { name, exact: true });
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute('title', name);
    await expect(link.locator('svg')).toBeVisible();
  }
  await expect(page.getByRole('button', { name: 'Sign out' })).toHaveAttribute('title', 'Sign out');

  // The state outlives the page, without a flash of the expanded menu.
  await page.getByRole('link', { name: 'Trades', exact: true }).click();
  await expect(page).toHaveURL(/\/trades$/);
  await expect(page.locator('html')).toHaveAttribute('data-menu', 'collapsed');
  await expect(page.getByRole('button', { name: 'Menu' })).toHaveAttribute('aria-expanded', 'false');
  await expect(labels.first()).toBeHidden();

  await page.getByRole('button', { name: 'Menu' }).click();
  await expect(page.getByRole('button', { name: 'Menu' })).toHaveAttribute('aria-expanded', 'true');
  await expect(labels.first()).toBeVisible();
  await page.reload();
  await expect(labels.first()).toBeVisible();
});
