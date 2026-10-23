// Runs against the actual worktree dev Cockpit, never a mocked gateway.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

test('native SQLite service is healthy through real Cockpit IPC', {
  skip: !process.env.OSG_SQLITE_NATIVE_ACCEPTANCE,
  timeout: 60_000,
}, async () => {
  const require = createRequire(import.meta.url);
  const { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9224');
  try {
    const page = browser.contexts().flatMap(context => context.pages()).find(page => page.url().startsWith(process.env.OSG_COCKPIT_DEV_URL ?? 'http://127.0.0.1:1421/'));
    assert.ok(page, 'The feature worktree dev Cockpit must be running.');
    const status = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('persistence_status'));
    assert.equal(status.healthy, true);
    assert.equal(status.schemaVersion, 2);
    assert.equal(status.journalMode, 'delete');
    assert.equal(status.synchronous, 'full');
    assert.equal(status.busyTimeoutMs, 250);
    assert.match(status.path, /cockpit\.sqlite3$/);
    if (process.env.OSG_SQLITE_ACCEPTANCE_DB) assert.equal(status.path.replaceAll('\\','/'), process.env.OSG_SQLITE_ACCEPTANCE_DB.replaceAll('\\','/'));
    assert.ok(await page.getByRole('navigation', {name:'Main navigation'}).isVisible());
    console.log(JSON.stringify({url:page.url(),persistence:status}));
  } finally { await browser.close(); }
});
