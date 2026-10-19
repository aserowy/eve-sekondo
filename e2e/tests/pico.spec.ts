import { expect, loginWith, test } from '../support/fixtures';

test('every page loads Pico and the app stylesheet from the app origin, and the body uses Pico\'s font size', async ({ page }) => {
  const sheets: string[] = [];
  const violations: string[] = [];
  page.on('response', (r) => {
    if (r.request().resourceType() === 'stylesheet') sheets.push(new URL(r.url()).origin + new URL(r.url()).pathname);
  });
  page.on('console', (m) => {
    if (m.text().includes('Content Security Policy')) violations.push(m.text());
  });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/characters');
  await expect(page.locator('main.container-fluid')).toBeVisible();

  const origin = new URL(page.url()).origin;
  expect(new Set(sheets)).toEqual(new Set([`${origin}/static/pico.min.css`, `${origin}/static/app.css`]));
  expect(violations).toEqual([]);

  const sizes = await page.evaluate(() => ({
    body: parseFloat(getComputedStyle(document.body).fontSize),
    pico: getComputedStyle(document.documentElement).getPropertyValue('--pico-font-size').trim(),
  }));
  expect(sizes.pico).toMatch(/%$/);
  expect(sizes.body).toBeCloseTo((parseFloat(sizes.pico) / 100) * 16, 1);
});
