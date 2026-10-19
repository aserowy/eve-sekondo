import type { Page } from '@playwright/test';
import {
  expect, feePoll, loginWith, openCharacters, pollCharacter, query, stubControl, stubTradeSkills, test,
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
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();
  await expect(page).toHaveURL(/\/orders\/sell$/);
}

/** One of a character's three fee cards. */
const card = (page: Page, character: number, key: 'sales-tax' | 'broker-fee' | 'min-margin') =>
  page.locator(`#fee-${character}-${key}`);

test('an NPC station shows its sales tax, broker fee and minimum margin with the inputs on hover', async ({ page }) => {
  await aliceTrained(page);

  await expect(card(page, ALICE, 'sales-tax').locator('.value')).toHaveText('3.38%');
  await expect(card(page, ALICE, 'broker-fee').locator('.value')).toHaveText('1.41%');
  await expect(card(page, ALICE, 'min-margin').locator('.value')).toHaveText('6.51%');
  await expect(card(page, ALICE, 'sales-tax').locator('.value')).toHaveAttribute('title', '3.3750% (Accounting 5)');
  await expect(card(page, ALICE, 'broker-fee').locator('.value')).toHaveAttribute(
    'title',
    '1.4100% (Broker Relations 5, faction standing 2.00, corporation standing 1.50)',
  );
  await expect(card(page, ALICE, 'min-margin').locator('.value')).toHaveAttribute('title', '6.5063%');
  for (const key of ['sales-tax', 'broker-fee', 'min-margin'] as const) {
    await expect(card(page, ALICE, key).locator('.detail')).toHaveText(`Alice Alpha · ${JITA}`);
  }
});

test('the fee cards are on the buy page too', async ({ page }) => {
  await aliceTrained(page);

  await page.locator('nav.sub-menu').getByRole('link', { name: 'Buy' }).click();

  await expect(page).toHaveURL(/\/orders\/buy$/);
  await expect(card(page, ALICE, 'sales-tax').locator('.value')).toHaveText('3.38%');
});

test('a character docked in a player structure shows the owner fee and no minimum margin', async ({ page }) => {
  await aliceTrained(page);
  // The stub has the character docked in the structure when the location is polled next.
  await stubControl('location', { character_id: ALICE, location: { solar_system_id: 30000144, structure_id: 1000000000001 } });
  expect(await feePoll(page)).toBe('200 ok');
  await page.reload();

  await expect(card(page, ALICE, 'sales-tax').locator('.value')).toHaveText('3.38%');
  await expect(card(page, ALICE, 'broker-fee').locator('.value')).toHaveText('0.5% + owner fee');
  await expect(card(page, ALICE, 'min-margin').locator('.value')).toHaveText('—');
  await expect(card(page, ALICE, 'broker-fee').locator('.detail')).toHaveText('Alice Alpha · Alpha Keepstar');
});

test('a character whose skills were not polled yet shows dashes', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  expect(await pollCharacter(page, ALICE)).toBe('200 updated');
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();

  for (const key of ['sales-tax', 'broker-fee', 'min-margin'] as const) {
    await expect(card(page, ALICE, key).locator('.value')).toHaveText('—');
  }
});

test('a character that has not granted the location scope uses Jita and offers to log in again', async ({ page }) => {
  await aliceTrained(page);
  await query("UPDATE characters SET scopes = array_remove(scopes, 'esi-location.read_location.v1') WHERE character_id = $1", [ALICE]);
  await page.reload();

  for (const key of ['sales-tax', 'broker-fee', 'min-margin'] as const) {
    const link = card(page, ALICE, key).getByRole('link', { name: 'Log in again' });
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute('href', '/auth/add');
    await expect(link.locator('svg')).toHaveCount(1);
    await expect(card(page, ALICE, key).locator('.detail')).toContainText(JITA);
  }
});

test('narrowing the selection narrows the fee cards', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await openCharacters(page);
  await page.getByRole('button', { name: 'Add character' }).click();
  await page.getByRole('link', { name: 'Bob Bravo' }).click();
  await expect(page).toHaveURL(/\/characters$/);
  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(5, 5) });
  await stubControl('skills', { character_id: BOB, skills: stubTradeSkills(3, 2) });
  for (const id of [ALICE, BOB]) expect(await pollCharacter(page, id)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();

  await expect(page.locator('[id^="fee-"][id$="-sales-tax"]')).toHaveCount(2);
  // Bob: Accounting 3 and Broker Relations 2 without standings: 3 - 0.6 = 2.40%, 7.5 x 0.67 = 5.025%.
  await expect(card(page, BOB, 'broker-fee').locator('.value')).toHaveText('2.40%');
  await expect(card(page, BOB, 'sales-tax').locator('.value')).toHaveText('5.03%');

  await page.getByRole('checkbox', { name: 'Bob Bravo' }).uncheck();
  await expect(page).toHaveURL(/chars=90000001$/);
  await expect(page.locator('[id^="fee-"][id$="-sales-tax"]')).toHaveCount(1);
  await expect(card(page, ALICE, 'sales-tax')).toBeVisible();
  await expect(page.locator('#freshness')).not.toContainText('Bob Bravo');
});

test('a trained skill updates the sales tax without a reload and re-fetches the content, whose margins depend on it', async ({ page }) => {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: ALICE, skills: stubTradeSkills(4, 5) });
  expect(await pollCharacter(page, ALICE)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
  // Opening the stream makes the page re-fetch its parts once; wait for that before counting.
  const opened = Promise.all([
    page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/freshness'),
    page.waitForResponse((r) => new URL(r.url()).pathname === '/orders/sell/content'),
  ]);
  await page.locator('#main-nav').getByRole('link', { name: 'Orders' }).click();
  await opened;
  const tax = card(page, ALICE, 'sales-tax').locator('.value');
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
  expect(fetched).toContain('/orders/sell/content');
  expect(navigations, 'the page was not reloaded').toEqual([]);
});
