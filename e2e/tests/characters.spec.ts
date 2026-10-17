import { expect, loginWith, openCharacters, test } from '../support/fixtures';

test('log in through the stub picker, then add a second character', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);

  await expect(page.locator('#characters')).toBeVisible();
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

test('the landing page sends signed-in users to the dashboard', async ({ page }) => {
  await loginWith(page, 'Carol Charlie');
  await page.goto('/');
  await expect(page).toHaveURL(/\/dashboard$/);
});

test('an anonymous visit to the characters page lands on the login page', async ({ page }) => {
  await page.goto('/characters');
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();
});

test('Add character and Unlink keep their text and carry an icon in front of it', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);

  for (const control of [
    page.getByRole('button', { name: 'Add character' }),
    page.locator('#character-90000001').getByRole('button', { name: 'Unlink' }),
  ]) {
    await expect(control.locator('svg')).toHaveCount(1);
    const nodes = await control.evaluate((el) => [...el.childNodes].map((n) => n.nodeName.toLowerCase()));
    expect(nodes[0]).toBe('svg');
    expect(nodes.slice(1)).toEqual(['#text']);
  }
});

test('the characters page lists characters as dense rows', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);

  const portrait = (await page.locator('#character-90000001 img').boundingBox())!;
  expect(portrait.width).toBe(32);
  expect(portrait.height).toBe(32);
  const row = (await page.locator('#character-90000001').boundingBox())!;
  expect(row.height).toBeLessThanOrEqual(44);
});
