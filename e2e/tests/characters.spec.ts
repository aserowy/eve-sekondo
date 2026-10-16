import { expect, loginWith, test } from '../support/fixtures';

test('log in through the stub picker, then add a second character', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');

  await expect(page.getByRole('heading', { name: 'Your characters' })).toBeVisible();
  const alice = page.locator('#character-90000001');
  await expect(alice).toContainText('Alice Alpha');
  await expect(alice.getByRole('img', { name: 'Portrait of Alice Alpha' })).toHaveAttribute(
    'src',
    'https://images.evetech.net/characters/90000001/portrait?size=64',
  );
  await expect(page.locator('.character')).toHaveCount(1);

  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();

  await expect(page).toHaveURL(/\/characters$/);
  await expect(page.locator('.character')).toHaveCount(2);
  await expect(page.locator('#character-90000001')).toContainText('Alice Alpha');
  await expect(page.locator('#character-90000002')).toContainText('Bob Bravo');
});

test('the landing page sends signed-in users to their characters', async ({ page }) => {
  await loginWith(page, 'Carol Charlie');
  await page.goto('/');
  await expect(page).toHaveURL(/\/characters$/);
});

test('an anonymous visit to the characters page lands on the login page', async ({ page }) => {
  await page.goto('/characters');
  await expect(page).toHaveURL(/localhost:8443\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();
});
