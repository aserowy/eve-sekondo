import { expect, test as base, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import https from 'node:https';
import path from 'node:path';
import { Client } from 'pg';
import { APP_URL, CLIENT_ID, CONTROL_TOKEN, E2E_DATABASE, STUB_URL, certs } from './env';

const rootCa = readFileSync(path.join(certs, 'rootCA.pem'));

// A 1x1 transparent PNG, served instead of the real EVE image server so tests never leave the machine.
const PIXEL = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
);

async function withDatabase<T>(fn: (client: Client) => Promise<T>): Promise<T> {
  const client = new Client({
    host: 'localhost',
    port: 5432,
    user: process.env.POSTGRES_USER ?? 'sekondo',
    password: process.env.POSTGRES_PASSWORD,
    database: E2E_DATABASE,
    ssl: { ca: rootCa, rejectUnauthorized: true },
  });
  await client.connect();
  try {
    return await fn(client);
  } finally {
    await client.end();
  }
}

/** Direct access to the e2e database, for assertions that the UI cannot make. */
export async function query<T extends Record<string, unknown>>(sql: string, params: unknown[] = []): Promise<T[]> {
  return withDatabase(async (c) => (await c.query(sql, params)).rows as T[]);
}

/** Calls an eve-stub control endpoint (verifying its certificate against the mkcert CA). */
export function stubControl(name: string, body: unknown = {}): Promise<void> {
  const payload = JSON.stringify(body);
  return new Promise((resolve, reject) => {
    const req = https.request(
      `${STUB_URL}/_stub/${name}`,
      {
        method: 'POST',
        ca: rootCa,
        headers: {
          authorization: `Bearer ${CONTROL_TOKEN}`,
          'content-type': 'application/json',
          'content-length': Buffer.byteLength(payload),
        },
      },
      (res) => {
        res.resume();
        res.on('end', () =>
          res.statusCode && res.statusCode < 300
            ? resolve()
            : reject(new Error(`stub control ${name} failed: ${res.statusCode}`)),
        );
      },
    );
    req.on('error', reject);
    req.end(payload);
  });
}

/** One ESI order as the stub serves it (a sell order unless `buy`; `escrow` defaults to 1000). */
export function stubOrder(
  id: number,
  opts: { type?: number; price?: number; remain?: number; buy?: boolean; corp?: boolean; escrow?: number } = {},
) {
  return {
    order_id: id,
    type_id: opts.type ?? 34,
    region_id: 10000002,
    location_id: 60003760,
    is_buy_order: opts.buy ?? false,
    is_corporation: opts.corp ?? false,
    price: opts.price ?? 10,
    volume_remain: opts.remain ?? 5,
    volume_total: 10,
    min_volume: 1,
    range: 'station',
    issued: new Date(Date.now() - 5 * 86_400_000).toISOString().replace(/\.\d+Z$/, 'Z'),
    duration: 90,
    ...(opts.buy ? { escrow: opts.escrow ?? 1000 } : {}),
  };
}

/** One public market order as the stub serves it (a sell order at the station of `stubOrder` unless told). */
export function stubMarketOrder(
  id: number,
  opts: { type?: number; price?: number; buy?: boolean; location?: number; system?: number; range?: string } = {},
) {
  return {
    order_id: id,
    type_id: opts.type ?? 34,
    location_id: opts.location ?? 60003760,
    system_id: opts.system ?? 30000142,
    is_buy_order: opts.buy ?? false,
    price: opts.price ?? 10,
    range: opts.range ?? 'station',
    duration: 90,
    issued: new Date(Date.now() - 86_400_000).toISOString().replace(/\.\d+Z$/, 'Z'),
    min_volume: 1,
    volume_remain: 3,
    volume_total: 5,
  };
}

/** Replaces the public market orders of The Forge in the stub. */
export function stubMarket(orders: ReturnType<typeof stubMarketOrder>[]): Promise<void> {
  return stubControl('market-orders', { region_id: 10000002, orders });
}

/** Refreshes the market pairs of the page's account now through the test-only hook. */
export async function marketPoll(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const token = document.querySelector('meta[name="csrf-token"]')!.getAttribute('content')!;
    const res = await fetch('/_test/market-poll', { method: 'POST', headers: { 'X-CSRF-Token': token } });
    return `${res.status} ${await res.text()}`;
  });
}

/** Polls a character now through the test-only hook, from inside the page (so it is the user's own session). */
export async function pollCharacter(page: Page, characterId: number): Promise<string> {
  return page.evaluate(async (id) => {
    const token = document.querySelector('meta[name="csrf-token"]')!.getAttribute('content')!;
    const res = await fetch(`/_test/poll/${id}`, { method: 'POST', headers: { 'X-CSRF-Token': token } });
    return `${res.status} ${await res.text()}`;
  }, characterId);
}

/** Log in through the stub's character picker the way a player would. */
export async function loginWith(page: Page, characterName: string): Promise<void> {
  await page.goto('/');
  await page.getByRole('button', { name: 'Log in with EVE Online' }).click();
  await page.getByRole('link', { name: characterName }).click();
  await expect(page).toHaveURL(/\/dashboard$/);
}

/** Follow the navigation to the characters page. */
export async function openCharacters(page: Page): Promise<void> {
  await page.getByRole('navigation').getByRole('link', { name: 'Characters' }).click();
  await expect(page).toHaveURL(/\/characters$/);
}

/** Every test starts with an empty database, a reset stub, and no network access for portraits. */
export const test = base.extend<{ cspGuard: void; cleanSlate: void }>({
  cspGuard: [
    async ({ page }, use) => {
      const violations: string[] = [];
      page.on('console', (msg) => {
        const text = msg.text();
        if (/Content Security Policy|violates the following|Refused to (load|apply|execute|connect)/i.test(text)) {
          violations.push(text);
        }
      });
      await use();
      expect(violations, 'CSP violations reported in the console').toEqual([]);
    },
    { auto: true },
  ],
  cleanSlate: [
    async ({ page }, use) => {
      await withDatabase((c) => c.query('TRUNCATE accounts, sessions, market_orders, market_sync CASCADE'));
      await stubControl('reset');
      await page.route('https://images.evetech.net/**', (route) =>
        route.fulfill({ status: 200, contentType: 'image/png', body: PIXEL }),
      );
      await use();
    },
    { auto: true },
  ],
});

export { expect, CLIENT_ID };

/** Polls a character from outside the browser (so while the page is offline), with the session's cookie. */
export function pollOutsideBrowser(characterId: number, cookies: { name: string; value: string }[], csrf: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const req = https.request(
      `${APP_URL}/_test/poll/${characterId}`,
      {
        method: 'POST',
        ca: rootCa,
        headers: {
          cookie: cookies.map((c) => `${c.name}=${c.value}`).join('; '),
          origin: APP_URL,
          'x-csrf-token': csrf,
        },
      },
      (res) => {
        let body = '';
        res.on('data', (chunk) => (body += chunk));
        res.on('end', () => resolve(`${res.statusCode} ${body}`));
      },
    );
    req.on('error', reject);
    req.end();
  });
}

/** Refreshes the market pairs from outside the browser (so while the page's stream is cut). */
export function marketPollOutsideBrowser(cookies: { name: string; value: string }[], csrf: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const req = https.request(
      `${APP_URL}/_test/market-poll`,
      {
        method: 'POST',
        ca: rootCa,
        headers: {
          cookie: cookies.map((c) => `${c.name}=${c.value}`).join('; '),
          origin: APP_URL,
          'x-csrf-token': csrf,
        },
      },
      (res) => {
        let body = '';
        res.on('data', (chunk) => (body += chunk));
        res.on('end', () => resolve(`${res.statusCode} ${body}`));
      },
    );
    req.on('error', reject);
    req.end();
  });
}
