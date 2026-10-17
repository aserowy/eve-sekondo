import { APP_URL } from '../support/env';
import { expect, loginWith, openCharacters, query, test } from '../support/fixtures';

test('sign out, then log in again with either linked character', async ({ page, browser }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page.locator('.character')).toHaveCount(2);

  const [session] = (await page.context().cookies()).filter((c) => c.name === '__Host-sekondo');
  expect(session, 'the session cookie exists').toBeTruthy();
  expect(session).toMatchObject({ secure: true, httpOnly: true, sameSite: 'Lax', path: '/' });

  await page.getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(/localhost:\d+\/$/);
  await expect(page.getByRole('button', { name: 'Log in with EVE Online' })).toBeVisible();

  await page.goto('/characters');
  await expect(page).toHaveURL(/localhost:\d+\/$/);

  // Replaying the pre-logout cookie in another browser does not authenticate.
  const attacker = await browser.newContext();
  await attacker.addCookies([{ ...session }]);
  const stolen = await attacker.newPage();
  await stolen.goto(APP_URL + '/characters');
  await expect(stolen).toHaveURL(/localhost:\d+\/$/);
  await attacker.close();

  // Log in again with the *other* character: same account, both characters.
  await loginWith(page, 'Bob Bravo');
  await openCharacters(page);
  await expect(page.locator('.character')).toHaveCount(2);
  await expect(page.locator('#character-90000001')).toContainText('Alice Alpha');
  expect(await query('SELECT id FROM accounts')).toHaveLength(1);
});
