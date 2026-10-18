// Requires a running Windows Cockpit native dev bridge and supported HidHide.
// Startup may configure the capture executable; every request here is read-only.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const endpoint = process.env.OSG_HIDHIDE_NATIVE_UI_URL ?? 'http://127.0.0.1:5173';
const artifacts = resolve(process.env.OSG_HIDHIDE_NATIVE_ARTIFACTS ?? '.research/hidhide-self-access/native-ui');

test('native startup grants HidHide self-access and Devices displays the same passive result', { timeout: 120_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1500, height: 1100 } });
  const commands = [], errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => {
    if (request.method() === 'POST' && request.url().includes('/api/gateway/')) commands.push(request.postDataJSON().command);
  });
  const native = request => page.evaluate(async request => {
    const response = await fetch('/api/gateway/', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-OSG-Client': '1' },
      body: JSON.stringify({ ...request, clientId: crypto.randomUUID() }) });
    const result = await response.json();
    if (!response.ok || !result.ok) throw Error(result.error ?? 'Native observation failed.');
    return result.state.controllers;
  }, request);
  try {
    await page.goto(endpoint);
    const inventory = await native({ command: 'controller_list', offset: 0, limit: 32 });
    const access = inventory.selfAccess;
    assert.ok(access && ['configured', 'unchanged'].includes(access.status), JSON.stringify(access));
    assert.equal(access.ready, true);
    assert.equal(access.changed, access.status === 'configured');
    assert.match(access.executable, /^\\Device\\.+\.exe$/i);
    const source = inventory.sources.find(source => source.online) ?? inventory.sources[0];
    assert.ok(source, 'A remembered native controller is required to inspect visibility details.');
    const initial = await native({ command: 'controller_visibility_status', sourceId: source.id });
    assert.deepEqual(initial.visibility.selfAccess, access);
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: /^Devices(?:\s|$)/ }).click();
    const devices = page.getByRole('region', { name: 'System devices', exact: true });
    const tile = devices.locator(`[data-component-id="controller:${source.id}"]`);
    await expect(tile).toBeVisible({ timeout: 90_000 });
    await tile.click();
    const status = page.getByRole('status', { name: 'Startup HidHide access', exact: true });
    await expect(status).toBeVisible();
    await expect(status).toHaveAttribute('data-self-access-status', access.status);
    await expect(status).toContainText(access.executable);
    await expect(status).toContainText('Access was verified at startup.');
    await expect(devices.locator('.system-list')).not.toBeVisible();
    // Wait for actual timer-driven status requests, rather than clicking refresh.
    const observed = commands.filter(command => command === 'controller_visibility_status').length;
    await expect.poll(() => commands.filter(command => command === 'controller_visibility_status').length).toBeGreaterThan(observed);
    const repeated = await native({ command: 'controller_visibility_status', sourceId: source.id });
    assert.deepEqual(repeated.visibility.selfAccess, access);
    assert.deepEqual(repeated.visibility.effective, initial.visibility.effective);
    assert.deepEqual(errors, []);
    const writes = commands.filter(command => /^(?:controller_visibility_(?:save|enable|restore|apply|launch)|controller_(?:forget|reassign|update)|profile_apply)$/.test(command));
    assert.deepEqual(writes, []);
    await page.screenshot({ path: resolve(artifacts, 'startup-access.png'), fullPage: true });
    await status.screenshot({ path: resolve(artifacts, 'startup-access-status.png') });
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ observedAt: new Date().toISOString(), endpoint,
      selfAccess: access, sourceId: source.id, effective: initial.visibility.effective, commands, writes, errors }, null, 2) + '\n');
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    throw error;
  } finally { await browser.close(); }
});
