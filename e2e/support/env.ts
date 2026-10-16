import { createHash, X509Certificate } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';

export const root = path.resolve(__dirname, '..', '..');
export const certs = path.join(root, 'certs');

export const APP_PORT = 8443;
export const STUB_PORT = 9443;
export const APP_URL = `https://localhost:${APP_PORT}`;
export const STUB_URL = `https://localhost:${STUB_PORT}`;

export const E2E_DATABASE = 'sekondo_e2e';
export const CLIENT_ID = 'e2e-client';
export const CLIENT_SECRET = 'e2e-secret';
export const CONTROL_TOKEN = 'e2e-control-token';
/** base64 of 32 bytes; only for the throwaway e2e database. */
export const TOKEN_ENCRYPTION_KEY = Buffer.alloc(32, 7).toString('base64');

/** base64(sha256(SPKI)) of a PEM certificate, the format Chromium's SPKI pin list expects. */
export function spkiHash(pemPath: string): string {
  const cert = new X509Certificate(readFileSync(pemPath));
  const der = cert.publicKey.export({ type: 'spki', format: 'der' });
  return createHash('sha256').update(der).digest('base64');
}
