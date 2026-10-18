import { expect, loginWith, test } from '../support/fixtures';

test('navigate between the signed-in pages, and sign out from the navigation', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  const nav = page.locator('#main-nav');

  // The dashboard is the landing page and shows the totals; the other pages are one navigation link away.
  await expect(page.getByRole('heading', { level: 1 })).toHaveCount(0);
  await expect(page.locator('#tile-sell-value')).toBeVisible();
  await expect(page.locator('#tile-escrow')).toBeVisible();
  await expect(nav.getByRole('link', { name: 'Dashboard' })).toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders$/);
  await expect(nav.getByRole('link', { name: 'Orders' })).toHaveAttribute('aria-current', 'page');
  await expect(nav.getByRole('link', { name: 'Dashboard' })).not.toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Wealth' }).click();
  await expect(page).toHaveURL(/\/wealth$/);
  await expect(nav.getByRole('link', { name: 'Wealth' })).toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Opportunities' }).click();
  await expect(page).toHaveURL(/\/opportunities$/);
  await expect(nav.getByRole('link', { name: 'Opportunities' })).toHaveAttribute('aria-current', 'page');
  await expect(nav.getByRole('link', { name: 'Wealth' })).not.toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Characters' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await expect(nav.getByRole('link', { name: 'Characters' })).toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Dashboard' }).click();
  await expect(page).toHaveURL(/\/dashboard$/);

  await nav.getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();
  await expect(page.locator('#main-nav')).toHaveCount(0);
});

test('characters and sign out are icon-only controls in a single thin top bar', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await page.setViewportSize({ width: 1920, height: 1080 });
  const nav = page.locator('#main-nav');

  for (const name of ['Characters', 'Sign out']) {
    const control = nav.getByRole(name === 'Characters' ? 'link' : 'button', { name });
    await expect(control).toHaveAttribute('title', name);
    await expect(control.locator('svg')).toHaveCount(1);
    await expect(control).toHaveText('');
  }

  const bar = (await nav.boundingBox())!;
  expect(bar.height).toBeLessThanOrEqual(36);
  const centres = await nav.locator('a, button').evaluateAll((els) =>
    els.map((e) => {
      const r = e.getBoundingClientRect();
      return r.top + r.height / 2;
    }),
  );
  expect(Math.max(...centres) - Math.min(...centres), 'all controls on one row').toBeLessThanOrEqual(3);
  await expect(page.getByRole('heading', { level: 1 })).toHaveCount(0);
});
