import { expect, test as base, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import https from 'node:https';
import path from 'node:path';
import { Client } from 'pg';
import { CLIENT_ID, CONTROL_TOKEN, E2E_DATABASE, STUB_URL, certs } from './env';

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

/** Log in through the stub's character picker the way a player would. */
export async function loginWith(page: Page, characterName: string): Promise<void> {
  await page.goto('/');
  await page.getByRole('button', { name: 'Log in with EVE Online' }).click();
  await page.getByRole('link', { name: characterName }).click();
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
      await withDatabase((c) => c.query('TRUNCATE accounts, sessions CASCADE'));
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
