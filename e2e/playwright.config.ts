import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';
import { APP_URL, certs, spkiHash } from './support/env';

// Chromium trusts exactly this one certificate (computed, never hard-coded). ignoreHTTPSErrors stays off.
const spki = spkiHash(path.join(certs, 'dev.pem'));

export default defineConfig({
  testDir: './tests',
  globalSetup: './global-setup.ts',
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: APP_URL,
    launchOptions: { args: [`--ignore-certificate-errors-spki-list=${spki}`] },
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
