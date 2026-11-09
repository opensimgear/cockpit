import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

const require = createRequire(import.meta.url);

const views = ['Overview', 'Devices', 'Profiles', 'Games', 'Companions', 'Activity', 'Debug', 'Settings'];
const selected = process.env.OSG_IDLE_VIEWS?.split(',').map(view => view.trim()) ?? views;
assert.ok(selected.length && selected.every(view => views.includes(view)), 'Select known Cockpit views.');

const browserUrl = process.env.OSG_IDLE_BROWSER_URL;
const transport = browserUrl ? 'browser development' : 'native';
const browserWarmupMs = browserUrl ? Number(process.env.OSG_IDLE_BROWSER_WARMUP_MS ?? 0) : 0;
assert.ok(Number.isSafeInteger(browserWarmupMs) && browserWarmupMs >= 0 && browserWarmupMs <= 120_000,
  'Browser warmup must be between 0 and 120,000 ms.');

test(`${transport} Cockpit stays idle without periodic gateway reads`, { timeout: selected.length * 85_000 + browserWarmupMs + (browserUrl ? 120_000 : 0) }, async t => {
  if (!browserUrl) {
    const page = await nativePage();
    try {
      await page.call('Network.enable', { maxPostDataSize: 65536 });
      for (const view of selected) await t.test(view, () => traceNativeView(page, view));
    } finally {
      page.close();
    }
    return;
  }
  const { chromium } = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
  const browser = await chromium.launch({ headless: true, channel: process.env.OSG_BROWSER_CHANNEL });
  try {
    const page = await browser.newPage();
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
            if (state.status === 'error' || (state.status === 'lost' && !state.loading)) reject(Error(state.error));
          });
        });
      } finally {
        clearTimeout(timer);
        stop?.();
      }
    });
    const startup = await page.evaluate(async () => {
      const { observeGamesState, observeGameDiscovery, observeGameJobs } = await import('/src/lib/game-read-models/shared.ts');
      let games, discovery, jobs, timer;
      const stops = [];
      try {
        return await new Promise((resolve, reject) => {
          timer = setTimeout(() => reject(Error(`Browser startup did not settle: ${JSON.stringify({
            games: games?.status, discovery: discovery?.status, scan: discovery?.value?.status,
            jobs: jobs?.value?.jobs.map(({ key, phase, terminal }) => ({ key, phase, terminal })),
          })}`)), 110_000);
          const check = () => {
            const failure = [games, discovery, jobs].find(state => state?.status === 'error' && !state.resolvingPage);
            if (failure) { reject(Error(failure.error)); return; }
            if (games?.status !== 'ready' || discovery?.status !== 'ready' || jobs?.status !== 'ready') return;
            const scan = discovery.value;
            // Ready awaits review. The scan has finished even though its job
            // stays open until the user applies the results.
            if (scan.status !== 'scanning' && jobs.value.jobs.every(job => job.terminal
              || (job.key === 'discovery' && job.phase === 'ready' && job.run === scan.scanId))) {
              resolve({ discovery: scan.status, jobs: jobs.value.jobs.length });
            }
          };
          stops.push(observeGamesState(state => { games = state; check(); }));
          stops.push(observeGameDiscovery(state => { discovery = state; check(); }));
          stops.push(observeGameJobs(state => { jobs = state; check(); }));
        });
      } finally {
        clearTimeout(timer);
        for (const stop of stops) stop();
      }
    });
    console.log(JSON.stringify({ transport, startup }));
    if (browserWarmupMs) await page.waitForTimeout(browserWarmupMs);
    for (const view of selected) await t.test(view, async () => traceView(page, view));
  } finally {
    await browser.close();
  }
});

async function traceNativeView(page, view) {
  const selector = view === 'Settings' ? '.sidebar-bottom button' : 'nav[aria-label="Main navigation"] button';
  const label = JSON.stringify(view);
  await until(() => page.evaluate(`(() => {
    const button = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find(item => (item.getAttribute('aria-label') ?? item.textContent.trim()).startsWith(${label}));
    if (!button) return false;
    button.click();
    return true;
  })()`));
  const visible = () => page.evaluate(`!document.hidden &&
    [...document.querySelectorAll('[aria-label="Breadcrumb"] [aria-current="page"]')].some(item =>
      item.textContent.trim() === ${label} && item.getBoundingClientRect().width > 0)`);
  await until(visible);
  await new Promise(resolve => setTimeout(resolve, 2500));
  const listen = listener => page.listen(event => {
    if (event.method === 'Network.requestWillBeSent') listener(event.params);
  });
  let checked = false;
  const stop = listen(({ request }) => {
    if (request.postData?.includes('reactive-idle-instrumentation')) checked = true;
  });
  try {
    await page.evaluate("window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'profile_status',clientId:'reactive-idle-instrumentation'}})");
    assert.ok(checked, 'The native gateway instrumentation request was not observed.');
  } finally {
    stop();
  }
  await traceTraffic(view, listen);
  assert.ok(await visible(), `The ${view} view changed or became hidden during the trace.`);
}

async function traceView(page, view) {
  const navigation = page.getByRole('navigation', { name: 'Main navigation' });
  const target = view === 'Settings'
    ? page.locator('.sidebar-bottom').getByRole('button', { name: view, exact: true })
    : navigation.getByRole('button', { name: new RegExp(`^${view}(?:\\s|$)`) });
  await target.click();
  const breadcrumb = page.getByRole('navigation', { name: 'Breadcrumb' }).getByText(view, { exact: true });
  await breadcrumb.waitFor();
  const visible = async () => await breadcrumb.isVisible() && await page.evaluate(() => !document.hidden);
  await page.waitForTimeout(2500);
  assert.ok(await visible(), `The ${view} view is hidden before the trace.`);

  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send('Network.enable', { maxPostDataSize: 65536 });
    await traceTraffic(view, listener => {
      cdp.on('Network.requestWillBeSent', listener);
      return () => cdp.off('Network.requestWillBeSent', listener);
    });
    assert.ok(await visible(), `The ${view} view changed or became hidden during the trace.`);
  } finally {
    await cdp.detach();
  }
}

async function traceTraffic(view, listen) {
  const started = performance.now();
  const gatewayRequests = [];
  const observationRequests = [];
  const stop = listen(({ request }) => {
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
  try {
    await new Promise(resolve => setTimeout(resolve, 60_100));
    const elapsedMs = Math.round(performance.now() - started);
    console.log(JSON.stringify({ transport, view, elapsedMs, gatewayRequests, observationRequests }));
    assert.ok(elapsedMs >= 60_000);
    assert.deepEqual(gatewayRequests.filter(request => request.command !== 'workspace_client_renew'), []);
    assert.deepEqual(observationRequests, [], 'The settled view must not make observation protocol requests while idle.');
  } finally {
    stop();
  }
}
