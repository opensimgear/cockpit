import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');

const views = ['Overview', 'Devices', 'Profiles', 'Games', 'Companions', 'Activity', 'Debug', 'Settings'];
const selected = process.env.OSG_IDLE_VIEWS?.split(',').map(view => view.trim()) ?? views;
assert.ok(selected.length && selected.every(view => views.includes(view)), 'Select known Cockpit views.');

const browserUrl = process.env.OSG_IDLE_BROWSER_URL;
const transport = browserUrl ? 'browser development' : 'native';
const browserWarmupMs = browserUrl ? Number(process.env.OSG_IDLE_BROWSER_WARMUP_MS ?? 45_000) : 0;
assert.ok(Number.isSafeInteger(browserWarmupMs) && browserWarmupMs >= 0 && browserWarmupMs <= 120_000,
  'Browser warmup must be between 0 and 120,000 ms.');

test(`${transport} Cockpit stays idle without periodic gateway reads`, { timeout: selected.length * 85_000 + browserWarmupMs }, async t => {
  const browser = browserUrl
    ? await chromium.launch({ headless: true, channel: process.env.OSG_BROWSER_CHANNEL })
    : await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222');
  try {
    const page = browserUrl
      ? await browser.newPage()
      : browser.contexts().flatMap(context => context.pages())
        .find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
    assert.ok(page, 'Start the development Cockpit first.');
    if (browserUrl) {
      await page.goto(browserUrl);
      await page.getByRole('navigation', { name: 'Main navigation' }).waitFor();
      // A broken observation transport can also produce an idle network trace.
      await page.evaluate(async () => {
        const { observeWorkspaceState } = await import('/src/lib/workspace-read-models/shared.ts');
        let stop, timer;
        try {
          await new Promise((resolve, reject) => {
            timer = setTimeout(() => reject(Error('Browser workspace observation did not become ready.')), 10_000);
            stop = observeWorkspaceState(state => {
              if (state.status === 'ready') resolve();
              if (state.status === 'error' || state.status === 'lost') reject(Error(state.error));
            });
          });
        } finally {
          clearTimeout(timer);
          stop?.();
        }
      });
      if (browserWarmupMs) await page.waitForTimeout(browserWarmupMs);
    }
    for (const view of selected) await t.test(view, async () => traceView(page, view));
  } finally {
    await browser.close();
  }
});

async function traceView(page, view) {
  const navigation = page.getByRole('navigation', { name: 'Main navigation' });
  const target = view === 'Settings'
    ? page.locator('.sidebar-bottom').getByRole('button', { name: view, exact: true })
    : navigation.getByRole('button', { name: new RegExp(`^${view}(?:\\s|$)`) });
  await target.click();
  await page.getByRole('navigation', { name: 'Breadcrumb' }).getByText(view, { exact: true }).waitFor();
  await page.waitForTimeout(2500);

  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send('Network.enable', { maxPostDataSize: 65536 });
    const started = performance.now();
    const gatewayRequests = [];
    const observationRequests = [];
    cdp.on('Network.requestWillBeSent', ({ request }) => {
      if (!['POST', 'GET'].includes(request.method)) return;
      const path = new URL(request.url).pathname;
      if (request.method === 'POST' && (path === '/gateway_request' || path === '/api/gateway/')) {
        let command = null;
        try {
          const body = JSON.parse(request.postData);
          command = (body.request ?? body).command ?? null;
        } catch {}
        gatewayRequests.push({ elapsedMs: Math.round(performance.now() - started), command });
      } else if (path.startsWith('/observation_') || path.startsWith('/api/observe/')) {
        observationRequests.push(path);
      }
    });
    await page.waitForTimeout(60_100);
    const elapsedMs = Math.round(performance.now() - started);
    console.log(JSON.stringify({ transport, view, elapsedMs, gatewayRequests, observationRequests }));
    assert.ok(elapsedMs >= 60_000);
    assert.deepEqual(gatewayRequests.filter(request => request.command !== 'workspace_client_renew'), []);
  } finally {
    await cdp.detach();
  }
}
