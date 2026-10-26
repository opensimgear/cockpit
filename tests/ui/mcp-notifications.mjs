// Real AI settings and global notifications through the running development cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const origin = process.env.OSG_SETTINGS_UI_URL ?? 'http://127.0.0.1:1420';

test('AI saves notify once for three seconds without inline messages or polling replay', { timeout: 30_000 }, async () => {
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1200, height: 1000 } });
  const compiled = await (await fetch(origin + '/src/lib/McpSettings.svelte')).text();
  const runtime = compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)[1];
  let reads = 0;
  const status = { settingsRevision: 0, enabled: false, port: 54438, endpoint: 'http://127.0.0.1:54438/mcp',
    running: false, error: null, credentials: [], protocolVersions: ['2025-11-25'] };
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.exposeFunction('__mcpInvoke', async (command, args) => {
    if (command === 'mcp_settings_get') { reads++; return { ...status }; }
    assert.equal(command, 'mcp_settings_update');
    assert.equal(args.settings.expectedSettingsRevision, status.settingsRevision);
    status.enabled = args.settings.enabled; status.running = status.enabled; status.settingsRevision++;
    return { ...status };
  });
  await page.addInitScript(() => { window.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.__mcpInvoke(command, args) }; });
  await page.route(origin + '/__mcp_notifications_test', route => route.fulfill({ contentType: 'text/html', body: `<html><body><div class="cockpit"><main style="height:100dvh;max-width:940px;margin:auto;padding:20px"><div id="settings"></div></main><div id="notifications"></div></div><script type="module">import {mount} from '${runtime}';import McpSettings from '/src/lib/McpSettings.svelte';import Notifications from '/src/lib/Notifications.svelte';import '/src/app.css';mount(Notifications,{target:document.querySelector('#notifications')});mount(McpSettings,{target:document.querySelector('#settings')});</script></body></html>` }));
  try {
    await page.goto(origin + '/__mcp_notifications_test');
    await page.evaluate(() => {
      window.__toastStarts = [];
      const seen = new WeakSet();
      new MutationObserver(() => {
        for (const fill of document.querySelectorAll('#notifications .toast-countdown-fill')) {
          if (seen.has(fill)) continue;
          seen.add(fill);
          window.__toastStarts.push({ duration: getComputedStyle(fill).animationDuration,
            message: fill.closest('.toast').querySelector('p').textContent });
        }
      }).observe(document.querySelector('#notifications'), { childList: true, subtree: true });
    });
    const settings = page.getByRole('region', { name: 'AI integration settings' });
    const notices = page.getByRole('region', { name: 'Notifications' });
    const toggle = settings.getByRole('switch', { name: 'Enable MCP server', exact: true });
    await expect(toggle).toBeEnabled();
    await toggle.click();
    const toast = notices.getByRole('status').filter({ hasText: 'MCP settings saved.' });
    await expect(toast).toBeVisible();
    await expect(settings.getByText('MCP settings saved.', { exact: true })).toHaveCount(0);
    await expect(toast.locator('.toast-countdown-fill')).toHaveCSS('animation-duration', '3s');
    await expect(toast).toHaveCount(0, { timeout: 4500 });
    await expect.poll(() => reads, { timeout: 7000 }).toBeGreaterThanOrEqual(2);
    const first = await page.evaluate(() => window.__toastStarts);
    assert.deepEqual(first, [{ duration: '3s', message: 'MCP settings saved.' }], 'A save produces one notification; polling does not replay it.');
    await toggle.click();
    await expect(toast).toBeVisible();
    const repeated = await page.evaluate(() => window.__toastStarts);
    assert.equal(repeated.length, 2, 'Saving again produces a new notification even with the same message.');
    assert.deepEqual(repeated[1], first[0]);
    await expect(settings.getByText('MCP settings saved.', { exact: true })).toHaveCount(0);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
});
