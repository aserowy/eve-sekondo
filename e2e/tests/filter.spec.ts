import { expect, loginWith, openCharacters, pollCharacter, stubControl, stubOrder, test } from '../support/fixtures';
import type { Page } from '@playwright/test';

/** Alice, Bob and Carol on one account, each with one sell order of their own. */
async function threeCharacters(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  for (const name of ['Bob Bravo', 'Carol Charlie']) {
    await page.getByRole('button', { name: 'Add character' }).click();
    await page.getByRole('link', { name }).click();
    await expect(page).toHaveURL(/\/characters$/);
  }
  for (const [id, order] of [
    [90000001, 11],
    [90000002, 22],
    [90000003, 33],
  ] as const) {
    await stubControl('orders', { character_id: id, orders: [stubOrder(order)] });
    await pollCharacter(page, id);
  }
}

test('narrow the selection, bookmark it, and carry it across pages', async ({ page, browser }) => {
  await threeCharacters(page);
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(3);
  await expect(page.getByRole('checkbox')).toHaveCount(3);
  await expect(page.getByRole('checkbox', { checked: true })).toHaveCount(3);

  // Deselect Carol: the URL lists the two others and the tables follow without a full reload.
  await page.evaluate(() => ((window as unknown as { marker: string }).marker = 'same page'));
  await page.getByRole('checkbox', { name: 'Carol Charlie' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001,90000002$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(page.locator('#order-33')).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { marker: string }).marker)).toBe('same page');
  await expect(page.getByRole('checkbox', { name: 'Carol Charlie' })).not.toBeChecked();

  // Narrow further, then select everything again: no `chars` at all.
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(1);
  await page.getByRole('checkbox', { name: 'Bob Bravo' }).check();
  await page.getByRole('checkbox', { name: 'Carol Charlie' }).check();
  await expect(page).not.toHaveURL(/chars/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(3);

  // The selection carries across pages: orders -> dashboard -> orders.
  await page.getByRole('checkbox', { name: 'Alice Alpha' }).uncheck();
  await expect(page).toHaveURL(/chars=90000002,90000003$/);
  const nav = page.getByRole('navigation');
  await nav.getByRole('link', { name: 'Dashboard' }).click();
  await expect(page).toHaveURL(/\/dashboard\?chars=90000002,90000003$/);
  await nav.getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\?chars=90000002,90000003$/);
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).not.toBeChecked();

  // Bookmark: the same URL in a fresh browser session, after signing in, selects the same characters.
  const url = page.url();
  const cookies = await page.context().cookies();
  const other = await browser.newContext();
  await other.addCookies(cookies);
  const reopened = await other.newPage();
  await reopened.goto(url);
  await expect(reopened.locator('#sell-orders tbody.order')).toHaveCount(2);
  await expect(reopened.locator('#order-11')).toHaveCount(0);
  await other.close();
});

test('a foreign character id in the URL is ignored', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', { character_id: 90000001, orders: [stubOrder(11)] });
  await pollCharacter(page, 90000001);

  // Bob belongs to nobody here: his ID is not one of this account's characters.
  await page.goto('/orders?chars=90000002');

  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(1);
  await expect(page.getByRole('checkbox')).toHaveCount(1);
  await expect(page.getByRole('checkbox', { name: 'Alice Alpha' })).toBeChecked();
  await expect(page.getByText('Bob Bravo')).toHaveCount(0);
});

test('the sorting is kept when the selection changes', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('orders', {
    character_id: 90000001,
    orders: [stubOrder(1, { price: 5 }), stubOrder(2, { type: 35, price: 9 })],
  });
  await pollCharacter(page, 90000001);
  await page.goto('/orders?sort=price&dir=desc');
  await expect(page.locator('#sell-orders tbody.order').first()).toHaveAttribute('id', 'order-2');

  // One character only: nothing to deselect, but the form still carries the sort fields.
  await expect(page.locator('#character-filter input[name="sort"]')).toHaveValue('price');
  await expect(page.locator('#character-filter input[name="dir"]')).toHaveValue('desc');
});
