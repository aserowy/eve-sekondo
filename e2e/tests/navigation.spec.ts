import { expect, loginWith, test } from '../support/fixtures';

test('navigate between the signed-in pages, and sign out from the navigation', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  const nav = page.getByRole('navigation');

  // The stub dashboard is the landing page, says so, and links on to the orders.
  await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
  await expect(page.getByText('dashboard is coming')).toBeVisible();
  await expect(nav.getByRole('link', { name: 'Dashboard' })).toHaveAttribute('aria-current', 'page');

  await page.getByRole('main').getByRole('link', { name: 'market orders' }).click();
  await expect(page).toHaveURL(/\/orders$/);
  await expect(nav.getByRole('link', { name: 'Orders' })).toHaveAttribute('aria-current', 'page');
  await expect(nav.getByRole('link', { name: 'Dashboard' })).not.toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Characters' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await expect(nav.getByRole('link', { name: 'Characters' })).toHaveAttribute('aria-current', 'page');

  await nav.getByRole('link', { name: 'Dashboard' }).click();
  await expect(page).toHaveURL(/\/dashboard$/);

  await nav.getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(/localhost:8443\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();
  await expect(page.getByRole('navigation')).toHaveCount(0);
});
