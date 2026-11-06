// Requires a running Windows Cockpit native dev bridge and supported HidHide.
// Startup may configure the capture executable. Set OSG_TEST_VISIBILITY_SAVE=1
// only with an isolated database to also exercise a saved preference change.
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
  const visibilityOpens = [], visibilityCloses = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => {
    if (request.method() !== 'POST') return;
    const path = new URL(request.url()).pathname;
    if (path === '/api/gateway/') commands.push(request.postDataJSON().command);
    if (path === '/api/observe/subscribe' && request.postDataJSON().topic === 'controllers.visibility') {
      const { connectionId, requestId } = request.postDataJSON();
      visibilityOpens.push(JSON.stringify([connectionId, requestId]));
    }
  });
  page.on('response', response => {
    if (response.ok() && new URL(response.url()).pathname === '/api/observe/unsubscribe') {
      const { connectionId, requestId } = response.request().postDataJSON();
      visibilityCloses.push(JSON.stringify([connectionId, requestId]));
    }
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
    const visibilityReads = () => commands.filter(command => command === 'controller_visibility_status').length;
    const beforeOpen = visibilityReads();
    const navigation = page.getByRole('navigation', { name: 'Main navigation' });
    await navigation.getByRole('button', { name: /^Devices(?:\s|$)/ }).click();
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
    // Opening the panel reads once after the inventory baseline. Allow any
    // subsequent native inventory changes to settle before checking idle work.
    assert.equal(visibilityReads() - beforeOpen, 1);
    await page.waitForTimeout(800);
    const settledReads = visibilityReads();
    await page.waitForTimeout(2200);
    assert.equal(visibilityReads(), settledReads);
    const repeated = await native({ command: 'controller_visibility_status', sourceId: source.id });
    assert.deepEqual(repeated.visibility.selfAccess, access);
    assert.deepEqual(repeated.visibility.effective, initial.visibility.effective);
    assert.deepEqual(errors, []);
    const writes = commands.filter(command => /^(?:controller_visibility_(?:save|enable|restore|apply|launch)|controller_(?:forget|reassign|update)|profile_apply)$/.test(command));
    assert.deepEqual(writes, []);
    if (process.env.OSG_TEST_VISIBILITY_SAVE === '1') {
      const savedPolicy = page.getByRole('combobox', { name: /^Saved policy/ });
      const original = initial.visibility.intent;
      const next = original.mode === 'visible' ? 'hide-all' : 'visible';
      await expect(savedPolicy).toHaveValue(original.mode);
      const firstReader = visibilityOpens.at(-1);
      assert.ok(firstReader);
      let restored = false;
      try {
        const saved = await native({ command: 'controller_visibility_save', sourceId: source.id, intent: { mode: next } });
        assert.deepEqual(saved.visibility.effective, initial.visibility.effective);
        await expect(savedPolicy).toHaveValue(next);
        await page.getByRole('button', { name: 'Back to Devices' }).click();
        await expect(savedPolicy).toHaveCount(0);
        await expect.poll(() => visibilityCloses.includes(firstReader)).toBe(true);
        const whileClosed = visibilityReads();
        await native({ command: 'controller_visibility_save', sourceId: source.id, intent: original });
        restored = true;
        await expect(tile).toBeVisible();
        assert.equal(visibilityReads(), whileClosed);
        await tile.click();
        await expect(savedPolicy).toHaveValue(original.mode);
        assert.notEqual(visibilityOpens.at(-1), firstReader);
        assert.equal(visibilityReads() - whileClosed, 1);
      } finally {
        if (!restored) await native({ command: 'controller_visibility_save', sourceId: source.id, intent: original });
      }
    }
    await page.screenshot({ path: resolve(artifacts, 'startup-access.png'), fullPage: true });
    await status.screenshot({ path: resolve(artifacts, 'startup-access-status.png') });
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ observedAt: new Date().toISOString(), endpoint,
      selfAccess: access, sourceId: source.id, effective: initial.visibility.effective, commands, writes, errors,
      visibilityOpens, visibilityCloses }, null, 2) + '\n');
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    throw error;
  } finally { await browser.close(); }
});
