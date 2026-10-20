import { expect, loginWith, test } from '../support/fixtures';

test('collapsing the sidebar hides the labels, widens the page, keeps names and titles, and persists across pages', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/wealth');
  const toggle = page.getByRole('button', { name: 'Menu' });
  const labels = page.locator('#main-nav .label');

  // A fresh browser starts expanded.
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(labels.first()).toBeVisible();
  const expandedWidth = (await page.locator('main').boundingBox())!.width;

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  for (const label of await labels.all()) await expect(label).toBeHidden();
  expect((await page.locator('main').boundingBox())!.width).toBeGreaterThan(expandedWidth + 40);
  for (const name of ['Dashboard', 'Characters', 'Wealth', 'Trades', 'Orders', 'Opportunities']) {
    const link = page.getByRole('link', { name, exact: true });
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute('title', name);
    await expect(link.locator('svg')).toBeVisible();
  }
  await expect(page.getByRole('button', { name: 'Sign out' })).toHaveAttribute('title', 'Sign out');

  // The state outlives the page, without a flash of the expanded menu.
  await page.getByRole('link', { name: 'Trades', exact: true }).click();
  await expect(page).toHaveURL(/\/trades$/);
  await expect(page.locator('html')).toHaveAttribute('data-menu', 'collapsed');
  await expect(page.getByRole('button', { name: 'Menu' })).toHaveAttribute('aria-expanded', 'false');
  await expect(labels.first()).toBeHidden();

  await page.getByRole('button', { name: 'Menu' }).click();
  await expect(page.getByRole('button', { name: 'Menu' })).toHaveAttribute('aria-expanded', 'true');
  await expect(labels.first()).toBeVisible();
  await page.reload();
  await expect(labels.first()).toBeVisible();
});

const ENTRIES = ['Dashboard', 'Characters', 'Wealth', 'Trades', 'Orders', 'Opportunities', 'Sign out'];

/** The left edges of the icons (and labels, when shown) of every main-menu entry, plus the toggle's icon box. */
async function menuGeometry(page: import('@playwright/test').Page) {
  return page.evaluate((names) => {
    const left = (el: Element | null) => el!.getBoundingClientRect().left;
    const entries = names.map((name) => {
      const el = document.querySelector(`#main-nav [aria-label="${name}"]`)!;
      return { name, icon: left(el.querySelector('svg')), label: el.querySelector('.label') ? left(el.querySelector('.label')) : null };
    });
    const visible = [...document.querySelectorAll('.menu-toggle svg')].find((s) => (s as SVGElement).getBoundingClientRect().width > 0)!;
    const side = document.getElementById('sidebar')!;
    const cs = getComputedStyle(side);
    return {
      entries,
      toggleIcon: visible.getBoundingClientRect().toJSON(),
      sidebarRight: side.getBoundingClientRect().right,
      sidebarPaddingRight: parseFloat(cs.paddingRight),
    };
  }, ENTRIES);
}

test('the expanded menu lines up: one left edge for the icons, one for the texts', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/dashboard');
  const { entries } = await menuGeometry(page);
  expect(entries).toHaveLength(7);
  for (const e of entries) {
    expect(Math.abs(e.icon - entries[0].icon), `${e.name} icon`).toBeLessThanOrEqual(1);
    expect(Math.abs(e.label! - entries[0].label!), `${e.name} text`).toBeLessThanOrEqual(1);
  }
});

test('the toggle sits at the right edge of the expanded sidebar', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/dashboard');
  const { entries, toggleIcon, sidebarRight, sidebarPaddingRight } = await menuGeometry(page);
  for (const e of entries) expect(toggleIcon.left, e.name).toBeGreaterThan(e.icon);
  const button = await page.locator('.menu-toggle').evaluate((el) => parseFloat(getComputedStyle(el).paddingRight));
  expect(sidebarRight - toggleIcon.right).toBeLessThanOrEqual(sidebarPaddingRight + button + 1);
});

test('the collapsed menu lines up: the toggle and every entry share one left edge', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await loginWith(page, 'Alice Alpha');
  await page.goto('/dashboard');
  await page.getByRole('button', { name: 'Menu' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-menu', 'collapsed');
  const { entries, toggleIcon } = await menuGeometry(page);
  expect(toggleIcon.width).toBeGreaterThan(0);
  for (const e of entries) expect(Math.abs(e.icon - toggleIcon.left), e.name).toBeLessThanOrEqual(1);
});
