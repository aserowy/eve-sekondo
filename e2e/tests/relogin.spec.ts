import { expect, loginWith, openCharacters, query, stubControl, test } from '../support/fixtures';
import type { Page } from '@playwright/test';

/** Triggers a token refresh for a character, like the background poller will (test-only hook). */
async function refreshToken(page: Page, characterId: number): Promise<string> {
  return page.evaluate(async (id) => {
    const token = document.querySelector('meta[name="csrf-token"]')!.getAttribute('content')!;
    const res = await fetch(`/_test/refresh/${id}`, { method: 'POST', headers: { 'X-CSRF-Token': token } });
    return `${res.status} ${await res.text()}`;
  }, characterId);
}

test('a revoked refresh token shows the badge, and logging in again clears it', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  const alice = page.locator('#character-90000001');
  await expect(alice.getByText('Needs re-login')).toHaveCount(0);

  await stubControl('revoke', { character_id: 90000001 });
  expect(await refreshToken(page, 90000001)).toBe('200 needs-relogin');

  await page.reload();
  await expect(alice.getByText('Needs re-login')).toBeVisible();
  await expect(alice.getByRole('link', { name: 'Log in again' })).toBeVisible();
  // The character keeps its data.
  await expect(alice).toContainText('Alice Alpha');
  expect(await query('SELECT needs_relogin FROM characters')).toEqual([{ needs_relogin: true }]);

  await alice.getByRole('link', { name: 'Log in again' }).click();
  await page.getByRole('link', { name: 'Alice Alpha' }).click();

  await expect(page).toHaveURL(/\/characters$/);
  await expect(alice.getByText('Needs re-login')).toHaveCount(0);
  await expect(page.locator('.character')).toHaveCount(1);
  expect(await query('SELECT needs_relogin FROM characters')).toEqual([{ needs_relogin: false }]);

  // And the new tokens work: a refresh succeeds again.
  expect(await refreshToken(page, 90000001)).toBe('200 ok');
});

test('a temporary SSO failure does not show the badge', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await stubControl('fail-next-token');
  expect(await refreshToken(page, 90000001)).toBe('503 temporary');

  await page.reload();
  await expect(page.locator('#character-90000001')).toBeVisible();
  await expect(page.getByText('Needs re-login')).toHaveCount(0);
});

test('reduced scopes at refresh show the badge', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await stubControl('scopes', { character_id: 90000001, scopes: ['esi-universe.read_structures.v1'] });
  expect(await refreshToken(page, 90000001)).toBe('200 needs-relogin');

  await page.reload();
  await expect(page.locator('#character-90000001').getByText('Needs re-login')).toBeVisible();
});
