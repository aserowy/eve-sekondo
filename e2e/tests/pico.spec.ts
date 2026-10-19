import { expect, loginWith, test } from '../support/fixtures';

test('every page loads Pico and the app stylesheet from the app origin', async ({ page }) => {
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
  await expect(page.locator('.page.container-fluid > main')).toBeVisible();

  const origin = new URL(page.url()).origin;
  expect(new Set(sheets)).toEqual(new Set([`${origin}/static/pico.min.css`, `${origin}/static/app.css`]));
  expect(violations).toEqual([]);

});

test('the base font stays at 16px however wide the window is', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  for (const width of [1024, 1280, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/characters');
    const sizes = await page.evaluate(() => ({
      root: getComputedStyle(document.documentElement).fontSize,
      body: getComputedStyle(document.body).fontSize,
    }));
    expect(sizes, `at ${width}px`).toEqual({ root: '16px', body: '16px' });
  }
});
