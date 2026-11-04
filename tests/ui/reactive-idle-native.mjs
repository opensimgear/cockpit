import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');

test('native Cockpit Devices stays idle without periodic gateway reads', { timeout: 90_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  try {
    const page = browser.contexts().flatMap(context => context.pages())
      .find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
    assert.ok(page, 'Start the native development Cockpit first.');
    const navigation = page.getByRole('navigation', { name: 'Main navigation' });
    await navigation.getByRole('button', { name: /^Devices(?:\s|$)/ }).click();
    await page.getByRole('heading', { name: 'Devices', exact: true }).waitFor();
    await page.waitForTimeout(1500);

    const cdp = await page.context().newCDPSession(page);
    try {
      await cdp.send('Network.enable', { maxPostDataSize: 65536 });
      const started = performance.now();
      const gatewayRequests = [];
      const observationRequests = [];
      cdp.on('Network.requestWillBeSent', ({ request }) => {
        if (request.method !== 'POST') return;
        const path = new URL(request.url).pathname;
        if (path === '/gateway_request') {
          let command = null;
          try { command = JSON.parse(request.postData).request?.command ?? null; } catch {}
          gatewayRequests.push({ elapsedMs: Math.round(performance.now() - started), command });
        } else if (path.startsWith('/observation_')) {
          observationRequests.push(path);
        }
      });
      await page.waitForTimeout(60_100);
      const elapsedMs = Math.round(performance.now() - started);
      console.log(JSON.stringify({ view: 'Devices', elapsedMs, gatewayRequests, observationRequests }));
      assert.ok(elapsedMs >= 60_000);
      assert.deepEqual(gatewayRequests.filter(request => request.command !== 'workspace_client_renew'), []);
    } finally {
      await cdp.detach();
    }
  } finally {
    await browser.close();
  }
});
