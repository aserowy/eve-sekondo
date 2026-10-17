import { expect, loginWith, openCharacters, query, test } from '../support/fixtures';

async function addCharacter(page: import('@playwright/test').Page, name: string) {
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

test('unlink one of two characters, then the last one', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await addCharacter(page, 'Bob Bravo');
  await expect(page.locator('.character')).toHaveCount(2);

  const messages: string[] = [];
  page.on('dialog', async (dialog) => {
    messages.push(dialog.message());
    await dialog.accept();
  });

  await page.locator('#character-90000002').getByRole('button', { name: 'Unlink' }).click();
  await expect(page.locator('#character-90000002')).toHaveCount(0);
  await expect(page.locator('.character')).toHaveCount(1);
  expect(messages[0]).toContain('Unlink Bob Bravo');
  expect(await query('SELECT character_id FROM characters')).toEqual([{ character_id: '90000001' }]);

  await page.locator('#character-90000001').getByRole('button', { name: 'Unlink' }).click();
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();
  expect(await query('SELECT 1 FROM characters')).toEqual([]);
  expect(await query('SELECT 1 FROM accounts')).toEqual([]);

  // Signed out: the characters page sends us back.
  await page.goto('/characters');
  await expect(page).toHaveURL(/localhost:\d+\/$/);
});

test('declining the confirmation keeps the character', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  page.on('dialog', (dialog) => dialog.dismiss());

  await page.locator('#character-90000001').getByRole('button', { name: 'Unlink' }).click();
  await page.waitForTimeout(300);

  await expect(page.locator('#character-90000001')).toBeVisible();
  expect(await query('SELECT 1 FROM characters')).toHaveLength(1);
});
