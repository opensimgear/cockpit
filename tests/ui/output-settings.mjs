// Exercise the actual output settings in the running dev cockpit with native IPC fixtures.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const origin = process.env.OSG_SETTINGS_UI_URL ?? 'http://127.0.0.1:1420';
const artifacts = resolve('.research/output-settings');

test('output cards preserve native setup, enablement, error states and compact responsive layout', { timeout: 60_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1200, height: 1100 } });
  const compiled = await (await fetch(origin + '/src/lib/OutputSettings.svelte')).text();
  const runtime = compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)[1];
  const requests = [], invocations = [], errors = [];
  const visibility = { enabled: true, qualified: true, settingsError: null, selfAccess: { ready: true, message: 'Capture access configured.' } };
  const desktop = { enabled: false, qualified: true, state: 'stopped', reason: '', scope: { kind: 'application' } };
  const drivers = { supported: true, vjoy: { installed: true, version: '2.2.2.0', error: null }, hidhide: { installed: true, version: '1.5.230.0', error: null } };
  let revision = 1, rejectWrite = false;
  const version = () => `sha256:${String(revision).padStart(64, '0')}`;
  const snapshot = () => ({ scope: 'desktop', gameId: null, revision: version(), settings: { enabled: desktop.enabled, scope: desktop.scope }, profileSession: { owner: null } });
  page.on('pageerror', error => errors.push(error.message));
  await page.exposeFunction('__outputInvoke', async (command, args) => {
    invocations.push({ command, args });
    if (command === 'optional_dependencies') return { supported: true, dependencies: [
      { id: 'vjoy', name: 'vJoy', version: '2.2', purpose: 'Virtual controller driver.' },
      { id: 'hidhide', name: 'HidHide', version: '1.5', purpose: 'Physical controller visibility.' },
    ] };
    if (command === 'optional_dependency_install') {
      drivers[args.id] = { installed: true, version: args.id === 'vjoy' ? '2.2.2.0' : '1.5.230.0', error: null };
      return 'Installer finished.';
    }
    if (['open_vjoy', 'open_hidhide'].includes(command)) return;
    assert.equal(command, 'gateway_request');
    const request = args.request; requests.push(request);
    let state;
    if (request.command === 'system_inventory') state = { systemInventory: { drivers, diagnostics: [], virtualDevices: [
      { id: 'vjoy:1', name: 'vJoy Device 1', status: 'free' }, { id: 'vjoy:2', name: 'vJoy Device 2', status: 'busy', owner: 'Flight simulator' },
    ] } };
    else if (request.command === 'controller_visibility_status') state = { controllers: { visibility } };
    else if (request.command === 'controller_visibility_enable') { visibility.enabled = request.enabled; state = { controllers: { visibility } }; }
    else if (request.command === 'profile_desktop_status') state = { desktop };
    else if (request.command === 'profile_local_settings_get') state = { localSettings: snapshot() };
    else if (request.command === 'profile_local_settings_save') {
      if (rejectWrite) return { ok: false, error: 'Local settings revision conflict. Refresh before saving.' };
      assert.equal(request.expectedSettingsRevision, version()); assert.equal(request.expectedActiveOwner, null);
      desktop.enabled = request.settings.enabled; revision++; state = { localSettings: snapshot() };
    } else throw Error('Unexpected output settings request: ' + request.command);
    return { ok: true, state };
  });
  await page.addInitScript(() => { window.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.__outputInvoke(command, args) }; });
  await page.route(origin + '/__output_test', route => route.fulfill({ contentType: 'text/html', body: `<html><body><div class="cockpit"><main style="height:100dvh;max-width:940px;margin:auto;padding:20px"><div id="test"></div></main></div><script type="module">import {mount} from '${runtime}';import OutputSettings from '/src/lib/OutputSettings.svelte';import '/src/app.css';mount(OutputSettings,{target:document.querySelector('#test')});</script></body></html>` }));
  try {
    await page.goto(origin + '/__output_test');
    const setup = page.getByRole('region', { name: 'Controller and desktop output settings', exact: true });
    const virtual = setup.getByRole('region', { name: 'Virtual controllers', exact: true });
    const hiding = setup.getByRole('region', { name: 'Device hiding', exact: true });
    const keyboard = setup.getByRole('region', { name: 'Keyboard and mouse', exact: true });
    const desktopToggle = keyboard.getByRole('switch', { name: 'Enable desktop output on this computer', exact: true });
    await expect(virtual.getByRole('status')).toHaveText('Enabled');
    await expect(virtual).toContainText('busy · Flight simulator');
    await expect(virtual.locator('.card-title')).toContainText('vJoy · 2.2.2.0');
    await expect(virtual.getByRole('button', { name: 'Set up', exact: true })).toHaveCount(0);
    await expect(setup.getByText('Driver installers', { exact: true })).toHaveCount(0);
    assert.equal(invocations.filter(call => call.command === 'optional_dependencies').length, 0, 'Installed drivers do not load installers.');
    await expect(hiding.getByRole('switch')).toHaveCount(0); await expect(desktopToggle).toBeEnabled();
    await expect(hiding.getByRole('status')).toHaveText('Enabled');
    await expect(hiding).not.toContainText('Capture access configured.');
    await expect(keyboard.locator('.setup-status')).toHaveCount(0);
    assert.ok(requests.every(request => !request.command.endsWith('_save') && request.command !== 'controller_visibility_enable'));
    await desktopToggle.check(); await expect(desktopToggle).toBeChecked();
    await virtual.getByRole('button', { name: 'Configure', exact: true }).click();
    await hiding.getByRole('button', { name: 'Manage', exact: true }).click();
    assert.ok(invocations.some(call => call.command === 'open_vjoy')); assert.ok(invocations.some(call => call.command === 'open_hidhide'));
    await page.screenshot({ path: resolve(artifacts, 'outputs-desktop.png'), fullPage: true });
    drivers.vjoy = { installed: false, version: null, error: null };
    drivers.hidhide = { installed: false, version: null, error: null };
    await setup.getByRole('button', { name: 'Refresh', exact: true }).click();
    await expect(virtual.getByRole('button', { name: 'Set up', exact: true })).toBeVisible();
    await expect(hiding.getByRole('button', { name: 'Set up', exact: true })).toBeVisible();
    await expect(virtual.getByRole('status')).toHaveText('Not installed');
    await expect(hiding.getByRole('status')).toHaveText('Not installed');
    await expect(virtual.getByRole('button', { name: 'Configure', exact: true })).toHaveCount(0);
    await expect(virtual.locator('.driver-version')).toHaveCount(0);
    await expect(setup.locator('details')).toHaveCount(0);
    await page.screenshot({ path: resolve(artifacts, 'outputs-missing-drivers.png'), fullPage: true });
    await virtual.getByRole('button', { name: 'Set up', exact: true }).click();
    assert.ok(invocations.some(call => call.command === 'optional_dependency_install' && call.args.id === 'vjoy'));
    await expect(virtual.locator('.card-title')).toContainText('vJoy · 2.2.2.0');
    await expect(virtual.getByRole('button', { name: 'Set up', exact: true })).toHaveCount(0);
    await expect(virtual.getByRole('button', { name: 'Configure', exact: true })).toBeEnabled();
    rejectWrite = true;
    await desktopToggle.click();
    await expect(setup.getByRole('alert')).toContainText('revision conflict');
    await expect(desktopToggle).toBeChecked();
    rejectWrite = false;
    visibility.qualified = false; visibility.reason = 'HidHide is unavailable on this computer.';
    desktop.qualified = false; desktop.reason = 'Desktop output is unavailable on this platform.';
    await setup.getByRole('button', { name: 'Refresh', exact: true }).click();
    await expect(hiding.getByRole('switch')).toHaveCount(0); await expect(desktopToggle).toBeDisabled();
    await expect(hiding).toContainText(visibility.reason); await expect(keyboard).toContainText(desktop.reason);
    for (const width of [520, 375]) {
      await page.setViewportSize({ width, height: 1100 });
      assert.ok(await setup.evaluate(node => node.scrollWidth <= node.clientWidth), 'Output settings do not overflow at narrow widths.');
      for (const card of [virtual, hiding, keyboard]) {
        const bounds = await card.boundingBox(); assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= width);
      }
      await page.screenshot({ path: resolve(artifacts, `outputs-${width}.png`), fullPage: true });
    }
    assert.deepEqual(errors, []);
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    throw error;
  } finally { await browser.close(); }
});
