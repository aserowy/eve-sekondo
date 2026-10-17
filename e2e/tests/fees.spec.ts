import type { Page } from '@playwright/test';
import {
  expect, feePoll, loginWith, openCharacters, pollCharacter, stubControl, stubTradeSkills, test,
} from '../support/fixtures';

const ALICE = 90000001;
const BOB = 90000002;
const JITA = 'Jita IV - Moon 4 - Caldari Navy Assembly Plant';

/** The stub's Alice: two sell orders in Jita 4-4 and a buy order in Alpha Keepstar. Accounting 5, Broker Relations 5. */
async function aliceTrained(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(5, 5) });
  expect(await pollCharacter(page, ALICE)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders$/);
}

const row = (page: Page, location: string) =>
  page.locator('#freshness tr').filter({ has: page.getByRole('cell', { name: location, exact: true }) });

test('an NPC station shows its sales tax, broker fee and minimum margin with the inputs on hover', async ({ page }) => {
  await aliceTrained(page);

  const jita = row(page, JITA);
  const tax = jita.locator('[data-fee="sales-tax"]');
  const fee = jita.locator('[data-fee="broker-fee"]');
  const margin = jita.locator('[data-fee="min-margin"]');
  await expect(tax).toHaveText('3.38%');
  await expect(fee).toHaveText('1.41%');
  await expect(margin).toHaveText('6.51%');
  await expect(tax).toHaveAttribute('title', '3.3750% (Accounting 5)');
  await expect(fee).toHaveAttribute(
    'title',
    '1.4100% (Broker Relations 5, faction standing 2.00, corporation standing 1.50)',
  );
  await expect(margin).toHaveAttribute('title', '6.5063%');
});

test('a station and a structure are listed side by side, the structure with an unknown owner fee', async ({ page }) => {
  await aliceTrained(page);

  await expect(page.locator('#freshness tbody tr')).toHaveCount(2);
  // Ordered by location name.
  await expect(page.locator('#freshness tbody tr').first()).toContainText('Alpha Keepstar');
  const keepstar = row(page, 'Alpha Keepstar');
  await expect(keepstar.locator('[data-fee="sales-tax"]')).toHaveText('3.38%');
  await expect(keepstar.locator('[data-fee="broker-fee"]')).toHaveText('0.5% + owner fee');
  await expect(keepstar.locator('[data-fee="min-margin"]')).toHaveText('—');
  await expect(row(page, JITA).locator('[data-fee="broker-fee"]')).toHaveText('1.41%');
});

test('a character whose skills were not polled yet shows dashes', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, ALICE)).toBe('200 updated');
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();

  const jita = row(page, JITA);
  for (const figure of ['sales-tax', 'broker-fee', 'min-margin']) {
    await expect(jita.locator(`[data-fee="${figure}"]`)).toHaveText('—');
  }
});

test('narrowing the selection narrows the locations and fees', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(5, 5) });
  await stubControl('skills', { character_id: BOB, skills: stubTradeSkills(3, 2) });
  for (const id of [ALICE, BOB]) expect(await pollCharacter(page, id)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();

  const alice = page.locator('#freshness li').filter({ hasText: 'Alice Alpha' });
  const bob = page.locator('#freshness li').filter({ hasText: 'Bob Bravo' });
  await expect(alice.locator('tbody tr')).toHaveCount(2);
  // Bob: Accounting 3 and Broker Relations 2 without standings: 3 - 0.6 = 2.40%, 7.5 x 0.67 = 5.025%.
  await expect(bob.locator('[data-fee="broker-fee"]')).toHaveText('2.40%');
  await expect(bob.locator('[data-fee="sales-tax"]')).toHaveText('5.03%');

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);
  await expect(page.locator('#freshness')).toContainText('Alice Alpha');
  await expect(page.locator('#freshness')).not.toContainText('Bob Bravo');
  await expect(page.locator('#freshness tbody tr')).toHaveCount(2);
});

test('a trained skill updates the sales tax without a reload and re-fetches the order tables, whose margins depend on it', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(4, 5) });
  expect(await pollCharacter(page, ALICE)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  // Opening the stream makes the page re-fetch both parts once; wait for that before counting.
  const opened = Promise.all([
    page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/freshness'),
    page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/tables'),
  ]);
  await page.getByRole('navigation').getByRole('link', { name: 'Orders' }).click();
  await opened;
  const tax = row(page, JITA).locator('[data-fee="sales-tax"]');
  // 7.5 x (1 - 0.11 x 4)
  await expect(tax).toHaveText('4.20%');
  await expect(page.locator('#sell-orders tbody.order')).toHaveCount(2);

  const fetched: string[] = [];
  page.on('request', (request) => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/orders')) fetched.push(url.pathname);
  });
  const navigations: string[] = [];
  page.on('framenavigated', (frame) => navigations.push(frame.url()));

  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(5, 5) });
  expect(await feePoll(page)).toBe('200 ok');

  await expect(tax).toHaveText('3.38%');
  expect(fetched).toContain('/orders/freshness');
  expect(fetched).toContain('/orders/tables');
  expect(navigations, 'the page was not reloaded').toEqual([]);
});
