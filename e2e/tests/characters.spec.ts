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

test('the characters page uses Pico: no panels, one line per character, and the body keeps its default size', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);

  const portrait = (await page.locator('#character-90000001 img').boundingBox())!;
  expect(portrait.width).toBe(32);
  expect(portrait.height).toBe(32);
  // Portrait, name and Unlink share one line.
  const centre = async (selector: string) => {
    const box = (await page.locator(selector).boundingBox())!;
    return box.y + box.height / 2;
  };
  const centres = [
    await centre('#character-90000001 img'),
    await centre('#character-90000001 strong'),
    await centre('#character-90000001 .unlink'),
  ];
  expect(Math.max(...centres) - Math.min(...centres)).toBeLessThanOrEqual(2);
  await expect(page.locator('.panel, .panel-title')).toHaveCount(0);
  const fonts = await page.evaluate(() => ({
    body: getComputedStyle(document.body).fontSize,
    root: getComputedStyle(document.documentElement).fontSize,
  }));
  expect(fonts.body).toBe(fonts.root);
});

test('the error and landing pages are Pico articles with a button', async ({ page }) => {
  await page.goto('/');
  const article = page.locator('main article.card');
  await expect(article).toBeVisible();
  await expect(article.getByRole('button', { name: 'Log in with EVE Online' })).toHaveAttribute('href', '/auth/login');
  await expect(page.locator('.panel')).toHaveCount(0);
  // Without a sidebar the page has the whole width.
  const main = (await page.locator('main').boundingBox())!;
  expect(main.width).toBeGreaterThan(1200);
});
