import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync } from 'node:fs';
import https from 'node:https';
import path from 'node:path';
import { Client } from 'pg';
import {
  APP_PORT, APP_URL, CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN, E2E_DATABASE, STUB_PORT, STUB_URL,
  TOKEN_ENCRYPTION_KEY, certs, root,
} from './support/env';

const rootCa = readFileSync(path.join(certs, 'rootCA.pem'));

function waitFor(url: string, name: string, timeoutMs = 60_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      https
        .get(url, { ca: rootCa }, (res) => {
          res.resume();
          if (res.statusCode === 200) resolve();
          else retry();
        })
        .on('error', retry);
    };
    const retry = () =>
      Date.now() > deadline ? reject(new Error(`${name} did not become ready at ${url}`)) : setTimeout(attempt, 250);
    attempt();
  });
}

/** Drops and recreates the dedicated e2e database. Never touches the app's own database. */
async function recreateE2eDatabase(user: string, password: string): Promise<void> {
  const client = new Client({
    host: 'localhost',
    port: 5432,
    user,
    password,
    database: 'postgres',
    // verify-full equivalent: the chain is checked against the mkcert CA and the host name is checked.
    ssl: { ca: rootCa, rejectUnauthorized: true },
  });
  await client.connect();
  try {
    await client.query(`DROP DATABASE IF EXISTS ${E2E_DATABASE} WITH (FORCE)`);
    await client.query(`CREATE DATABASE ${E2E_DATABASE}`);
  } finally {
    await client.end();
  }
}

export default async function globalSetup() {
  process.loadEnvFile(path.join(root, '.env'));
  const user = process.env.POSTGRES_USER ?? 'sekondo';
  const password = process.env.POSTGRES_PASSWORD;
  if (!password) throw new Error('POSTGRES_PASSWORD is not set (see .env.example)');

  // The stub must be the one that was verified against real EVE; fails before anything is built or started.
  try {
    execFileSync('cargo', ['run', '--quiet', '-p', 'esi-contract', '--bin', 'stub-fingerprint', '--', '--check'], {
      cwd: root,
      stdio: ['ignore', 'inherit', 'pipe'],
    });
  } catch (e) {
    const stderr = (e as { stderr?: Buffer }).stderr?.toString().trim();
    throw new Error(stderr || 'the stub could not be checked against its verification record');
  }

  // The test-hooks feature adds POST /_test/refresh/{id} and /_test/poll/{id}; only this e2e build has it.
  // Two builds: with `-p sekondo -p eve-stub --features sekondo/test-hooks` cargo leaves the feature off the sekondo bin.
  execFileSync('cargo', ['build', '-p', 'sekondo', '--features', 'test-hooks'], { cwd: root, stdio: 'inherit' });
  execFileSync('cargo', ['build', '-p', 'eve-stub'], { cwd: root, stdio: 'inherit' });
  await recreateE2eDatabase(user, password);

  const logDir = path.join(__dirname, 'test-results');
  mkdirSync(logDir, { recursive: true });
  const children: ChildProcess[] = [];
  const start = (name: string, bin: string, args: string[], env: NodeJS.ProcessEnv) => {
    const log = openSync(path.join(logDir, `${name}.log`), 'w');
    const child = spawn(path.join(root, 'target', 'debug', bin), args, {
      cwd: root,
      env: { ...process.env, ...env },
      stdio: ['ignore', log, log],
    });
    closeSync(log);
    children.push(child);
  };

  start('eve-stub', 'eve-stub', [
    '--cert', path.join(certs, 'dev.pem'),
    '--key', path.join(certs, 'dev-key.pem'),
    '--listen', `127.0.0.1:${STUB_PORT}`,
    '--client-id', CLIENT_ID,
    '--client-secret', CLIENT_SECRET,
    '--control-token', CONTROL_TOKEN,
  ], {});

  const databaseUrl =
    `postgres://${encodeURIComponent(user)}:${encodeURIComponent(password)}@localhost:5432/${E2E_DATABASE}` +
    `?sslmode=verify-full&sslrootcert=${encodeURIComponent(path.join(certs, 'rootCA.pem'))}`;
  start('sekondo', 'sekondo', [], {
    DATABASE_URL: databaseUrl,
    BIND_ADDR: `127.0.0.1:${APP_PORT}`,
    TLS_CERT_PATH: path.join(certs, 'dev.pem'),
    TLS_KEY_PATH: path.join(certs, 'dev-key.pem'),
    TLS_EXTRA_CA_PATH: path.join(certs, 'rootCA.pem'),
    PUBLIC_BASE_URL: APP_URL,
    EVE_CLIENT_ID: CLIENT_ID,
    EVE_CLIENT_SECRET: CLIENT_SECRET,
    EVE_SSO_BASE_URL: STUB_URL,
    ESI_BASE_URL: STUB_URL,
    TOKEN_ENCRYPTION_KEY,
    // The background scheduler stays out of the way; tests poll through the test hook.
    ORDER_POLL_INTERVAL_MINUTES: '1440',
    // Same for the trading station: tests drive its sync through the /_test/station-* hooks.
    STATION_SYNC: 'off',
  });

  try {
    await waitFor(`${STUB_URL}/oauth/jwks`, 'eve-stub');
    await waitFor(`${APP_URL}/healthz`, 'sekondo');
  } catch (e) {
    children.forEach((c) => c.kill());
    throw e;
  }

  return async () => {
    children.forEach((c) => c.kill());
  };
}
