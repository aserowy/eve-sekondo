import { expect, test } from '../support/fixtures';

test('the landing page renders the login button without CSP violations', async ({ page }) => {
  await page.goto('https://localhost:8443/');
  const login = page.getByRole('button', { name: 'Log in with EVE Online' });
  await expect(login).toBeVisible();
  await expect(login).toHaveAttribute('href', '/auth/login');
});

test('the CSP guard catches a violation', async ({ page }) => {
  test.fail(); // expected to fail: proves the fixture reports violations
  await page.goto('https://localhost:8443/');
  await page.evaluate(() => {
    const img = document.createElement('img');
    img.src = 'https://example.invalid/blocked.png';
    document.body.appendChild(img);
  });
  await page.waitForTimeout(500);
});
