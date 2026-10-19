import { expect, loginWith, openCharacters, test } from '../support/fixtures';

test('the sidebar lists the pages in order, navigates between them, and signs out at the bottom', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  const nav = page.locator('aside.sidebar #main-nav');

  await expect(page.getByRole('heading', { level: 1 })).toHaveText(['Dashboard']);
  await expect(nav.locator('a, button')).toHaveText([
    'Dashboard', 'Characters', 'Wealth', 'Trades', 'Orders', 'Opportunities', 'Sign out',
  ]);
  await expect(nav.getByRole('link', { name: 'Dashboard' })).toHaveAttribute('aria-current', 'page');

  const steps: [string, RegExp][] = [
    ['Characters', /\/characters$/],
    ['Wealth', /\/wealth$/],
    ['Trades', /\/trades$/],
    ['Orders', /\/orders\/sell$/],
    ['Opportunities', /\/opportunities\/expansion$/],
    ['Dashboard', /\/dashboard$/],
  ];
  for (const [name, url] of steps) {
    await nav.getByRole('link', { name }).click();
    await expect(page).toHaveURL(url);
    await expect(nav.getByRole('link', { name })).toHaveAttribute('aria-current', 'page');
    await expect(nav.locator('[aria-current="page"]')).toHaveCount(1);
  }

  // Sign out sits at the bottom of the sidebar, below every other entry.
  const bottom = (await nav.getByRole('button', { name: 'Sign out' }).boundingBox())!;
  const last = (await nav.getByRole('link', { name: 'Opportunities' }).boundingBox())!;
  expect(bottom.y).toBeGreaterThan(last.y + last.height);
  const side = (await page.locator('aside.sidebar').boundingBox())!;
  expect(side.y + side.height - (bottom.y + bottom.height)).toBeLessThanOrEqual(24);

  await nav.getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();
  await expect(page.locator('#main-nav')).toHaveCount(0);
});

test('every sidebar entry has an icon, a label, and its name as hover title', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  const nav = page.locator('#main-nav');
  for (const name of ['Dashboard', 'Characters', 'Wealth', 'Trades', 'Orders', 'Opportunities', 'Sign out']) {
    const control = nav.getByRole(name === 'Sign out' ? 'button' : 'link', { name });
    await expect(control).toHaveAttribute('title', name);
    await expect(control.locator('svg')).toHaveCount(1);
    await expect(control.locator('.label')).toHaveText(name);
  }
});

test('the links carry the selection and never a range', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  const nav = page.locator('#main-nav');

  await page.goto('/wealth');
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/\/wealth\?chars=90000001/);
  await nav.getByRole('link', { name: 'Trades' }).click();

  await expect(page).toHaveURL(/\/trades\?chars=90000001$/);
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.getByRole('checkbox', { name: 'Bob Bravo' })).not.toBeChecked();

  await page.goto('/dashboard?range=30d');
  await nav.getByRole('link', { name: 'Trades' }).click();
  await expect(page).toHaveURL(/\/trades$/);
  await expect(page.locator('#trades-range-control a[aria-current="true"]')).toHaveText('7 days');
});
