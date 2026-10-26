// Real Devices list, filters and topology through the running development cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const origin = process.env.OSG_DEVICES_UI_URL ?? 'http://127.0.0.1:1420';
const artifacts = resolve('.research/device-filters');

test('Devices combines multi-select groups, search and presentation without dropdowns or count strips', { timeout: 60_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const page = await browser.newPage({ viewport: { width: 1400, height: 1100 } });
  const compiled = await (await fetch(origin + '/src/lib/SystemDevices.svelte')).text();
  const runtime = compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)[1];
  const sources = [{ id: 'stick', name: 'Flight stick', online: true }, { id: 'pedals', name: 'Pedals', online: false }]
    .map(source => ({ ...source, role: 'controller', backend: 'windows-raw-input', controlCount: 4, baselineReady: true }));
  const requests = [], errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.exposeFunction('__filtersInvoke', async (command, args) => {
    assert.equal(command, 'gateway_request'); const request = args.request; requests.push(request);
    let state;
    if (request.command === 'controller_list') state = { controllers: { session: 'filters', backendStatus: 'ready', offset: 0, total: sources.length, limit: 32, sources } };
    else if (request.command === 'module_usb_list') state = { moduleUsb: { offset: 0, total: 0, modules: [] } };
    else if (request.command === 'system_inventory') state = { systemInventory: { observedAt: 1, offset: 0, total: 2, diagnostics: [], visibilityInspected: true,
      virtualDevices: [{ id: 'vjoy:1', name: 'Virtual joystick', available: true, status: 'free', diagnostics: [] },
        { id: 'vjoy:2', name: 'Virtual backup', available: false, status: 'missing', diagnostics: [] },
        { id: 'vjoy:3', name: 'Virtual unknown', available: null, status: 'unknown', diagnostics: [] }],
      hiddenDevices: [{ id: 'stick-pnp', name: 'Stick USB', instanceId: 'USB\\VID_STICK', controllerIds: ['stick'], present: true, hidden: true, configured: true, diagnostics: [] },
        { id: 'pedals-pnp', name: 'Pedals Bluetooth', instanceId: 'BTHENUM\\DEV_PEDALS', controllerIds: ['pedals'], present: false, hidden: false, configured: false, diagnostics: [] }],
    } };
    else if (request.command === 'companion_inventory') state = { companions: { host: { clients: [{ companionId: 'tablet', name: 'Tablet', kind: 'browser', connected: true, peer: '192.0.2.1' }], instances: [] }, usb: { devices: [] } } };
    else if (request.command === 'profile_status') state = { profiles: { profileSession: { state: 'stopped' } } };
    else if (request.command === 'game_get_state') state = { games: { catalogue: [] } };
    else if (request.command === 'profile_resources_list') state = { profileResources: { session: 'filters', revision: '1', offset: 0, total: 0, resources: [] } };
    else if (request.command === 'profile_list') state = { profiles: { profiles: [] } };
    else throw Error('Unexpected filter request: ' + request.command);
    return { ok: true, state };
  });
  await page.addInitScript(() => { window.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.__filtersInvoke(command, args) }; });
  await page.route(origin + '/__device_filters_test', route => route.fulfill({ contentType: 'text/html', body: `<html><body><div class="cockpit"><main style="height:100dvh;max-width:1200px;margin:auto;padding:24px"><div id="test"></div></main></div><script type="module">import {mount} from '${runtime}';import Devices from '/src/lib/SystemDevices.svelte';import '/src/app.css';mount(Devices,{target:document.querySelector('#test'),props:{active:true,entries:[],openModule(){},openCompanions(){},openIntegration(){},oninventory(){}}});</script></body></html>` }));
  try {
    await page.goto(origin + '/__device_filters_test');
    const devices = page.getByRole('region', { name: 'System devices', exact: true });
    const tiles = devices.locator('.system-list [data-component-id]');
    const filterButton = devices.getByRole('button', { name: /^Filter(?:\s|$)/ });
    const panel = devices.getByRole('region', { name: 'Device filters', exact: true });
    const type = panel.getByRole('group', { name: 'Type', exact: true });
    const connection = panel.getByRole('group', { name: 'Connection type', exact: true });
    const status = panel.getByRole('group', { name: 'Status', exact: true });
    const visibility = panel.getByRole('group', { name: 'Visibility', exact: true });
    const search = devices.getByRole('textbox', { name: 'Search components', exact: true });
    const select = (group, name) => group.getByRole('checkbox', { name, exact: true }).check();
    await expect(tiles).toHaveCount(6);
    await expect(devices.getByRole('combobox')).toHaveCount(0);
    await expect(devices.getByLabel('Component counts', { exact: true })).toHaveCount(0);
    await expect(filterButton).toHaveAttribute('aria-expanded', 'false');
    await expect(filterButton).toHaveAttribute('title', 'Filter devices');
    await expect(filterButton).toHaveText('');
    await page.evaluate(() => document.fonts.ready);
    const searchBounds = await search.boundingBox(), filterBounds = await filterButton.boundingBox();
    assert.ok(filterBounds.x >= searchBounds.x + searchBounds.width, 'The icon-only filter action sits to the right of search.');
    assert.ok(Math.abs(filterBounds.y + filterBounds.height / 2 - searchBounds.y - searchBounds.height / 2) < 2);
    await expect(panel).toHaveCount(0);
    await filterButton.press('Enter');
    await expect(panel).toBeVisible(); await expect(filterButton).toHaveAttribute('aria-expanded', 'true');
    await select(type, 'Physical · Third-party'); await select(type, 'Virtual');
    await expect(tiles).toHaveCount(5);
    await select(connection, 'USB'); await select(connection, 'Bluetooth');
    await expect(tiles).toHaveCount(2);
    await select(connection, 'Virtual'); await expect(tiles).toHaveCount(5);
    await select(status, 'Offline'); await select(status, 'Unavailable');
    await expect(tiles).toHaveCount(2);
    await select(visibility, 'Visible'); await expect(tiles).toHaveCount(1);
    await expect(tiles.first()).toHaveAttribute('data-component-id', 'controller:pedals');
    await page.screenshot({ path: resolve(artifacts, 'filters-selected.png'), fullPage: true });
    await filterButton.click(); await expect(panel).toHaveCount(0);
    await expect(tiles).toHaveCount(1);
    await filterButton.click(); await expect(visibility.getByRole('checkbox', { name: 'Visible', exact: true })).toBeChecked();
    await panel.getByRole('button', { name: 'Clear', exact: true }).click(); await expect(tiles).toHaveCount(6);
    await select(type, 'Virtual');
    await devices.getByRole('button', { name: 'Topology', exact: true }).click();
    const graph = devices.getByRole('region', { name: 'System topology', exact: true });
    await expect(graph.locator('[data-component-id^="virtual:"]')).toHaveCount(3);
    await expect(type.getByRole('checkbox', { name: 'Virtual', exact: true })).toBeChecked();
    await devices.getByRole('button', { name: 'List', exact: true }).click();
    await expect(tiles).toHaveCount(3);
    await tiles.filter({ hasText: 'Virtual joystick' }).click();
    await devices.getByRole('button', { name: 'Back to Devices', exact: true }).click();
    await expect(tiles).toHaveCount(3); await expect(type.getByRole('checkbox', { name: 'Virtual', exact: true })).toBeChecked();
    await search.fill('Tablet'); await expect(tiles).toHaveCount(0);
    await devices.getByRole('button', { name: 'Clear filters', exact: true }).click();
    await expect(search).toHaveValue(''); await expect(tiles).toHaveCount(6);
    await search.fill('Tablet'); await select(type, 'Companion');
    await panel.getByRole('button', { name: 'Clear', exact: true }).click();
    await expect(search).toHaveValue('Tablet'); await expect(tiles).toHaveCount(1);
    await search.fill('');
    await page.setViewportSize({ width: 375, height: 1000 });
    await expect(panel).toBeVisible();
    assert.ok(await devices.evaluate(node => node.scrollWidth <= node.clientWidth), 'Filter controls stay within a narrow device list.');
    await page.screenshot({ path: resolve(artifacts, 'filters-mobile.png'), fullPage: true });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await filterButton.click(); await expect(panel).toHaveCount(0);
    await filterButton.click(); await expect(panel).toBeVisible();
    assert.deepEqual(errors, []);
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    throw error;
  } finally { await browser.close(); }
});
