// Running Cockpit frontend acceptance with deterministic provider replies.
// IPC fixtures cannot qualify native hardware, drivers, game delivery or pairing.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');
const endpoint = process.env.OSG_DEVICES_UI_URL ?? 'http://127.0.0.1:1420';
const artifacts = resolve(process.env.OSG_DEVICES_UI_ARTIFACTS ?? '.research/system-devices-browser');
const clone = value => JSON.parse(JSON.stringify(value));
const serial = '00112233445566778899aabb', otherSerial = '00112233445566778899aacc';
const firstGateway = `stm32:${serial}`, secondGateway = `stm32:${otherSerial}`;
const usbKey = `${serial}:COM-fixture-usb`;
const defaultId = '00000000-0000-4000-8000-000000000001';
const hash = `sha256:${'1'.repeat(64)}`;

function controller(id, name, online = true, extra = {}) {
  return { id, name, online, role: 'controller', origin: 'host-controller', backend: 'windows-raw-input', vendorId: 1, productId: 2,
    osgHid: false, generation: '1', sequence: '1', controlCount: online ? 4 : 0, baselineReady: online, identityConfidence: 'serial', ...extra };
}
function gatewayModule(id, name, extra = {}) {
  return { id, name, kind: 'panel', online: true, active: true, registered: true, enabled: true, published: true,
    descriptorIndex: 0, buttonCount: 1, axisCount: 0, hatCount: 0, buttons: [], axes: [], hats: [], profileMatch: true,
    canConfigure: false, boot: 1, configurationGeneration: 1, serialNumber: id, settings: { label: name, inverted: false, debounce: 5 }, ...extra };
}
function resource(id, connections) {
  return { id, name: id, resourceType: 'osg-module', availability: 'online', generation: '1', capabilityRevision: hash,
    portCount: 0, cachedPortCount: 0, connections: connections.map(([scope, key, transport]) => ({ scope, key, route: {
      id: `${scope}/${key}`, transport, providerGeneration: '1', attributes: {}, baselineReady: true, writable: false, priority: 0,
    } })), attributes: {}, diagnostics: [] };
}
function usb(key, name, extra = {}) {
  return { key, present: true, automatic: false, admissible: true, diagnostics: [], port: { path: key.split(':').at(-1), friendlyName: name },
    snapshot: { connected: true, port: key.split(':').at(-1), identity: { model: 'fixture-module', board: 'fixture-board', profile: name,
      moduleId: key.startsWith(serial) ? 'dual' : 'standalone-output', serial: key.split(':')[0], protocol: 3, buttonCount: 0, axisCount: 0, hatCount: 0 },
    route: null, claimedBy: null, observedAtMs: 1, error: null }, ...extra };
}
function fixtures() {
  const ports = [serial, otherSerial].map((value, index) => ({ path: `COM-fixture-gateway-${index + 1}`, manufacturer: 'OpenSimGear',
    friendlyName: 'OpenSimGear CAN gateway', serialNumber: value, backend: 'stm32' }));
  const states = new Map(ports.map((port, index) => [index ? secondGateway : firstGateway, {
    gatewayId: index ? secondGateway : firstGateway, gatewaySerial: port.serialNumber, backend: 'stm32', port: port.path,
    connected: true, running: true, needsReconnect: false, protocolVersion: 3, lifecycle: 'automatic', canProtocol: 3,
    claimedModules: [], modules: index ? [gatewayModule('dual', 'Unproven second panel')] : [
      gatewayModule('dual', 'Dual transport panel', { directUsb: true }),
      gatewayModule('can-output', 'CAN output module', { outputOnly: true, buttonCount: 0, enabled: false, published: false }),
    ],
  }]));
  return {
    ports, states,
    controllers: [controller('pedals', 'Hidden pedals', true, { role: 'pedals' }), controller('remembered-wheel', 'Remembered wheel', false),
      controller('gateway-hid', 'Gateway HID collection', true, { osgHid: true }),
      controller('remembered-throttle', 'Remembered throttle', false)],
    modules: [usb(usbKey, 'Dual USB sighting'), usb('ffeeddccbbaa:COM-fixture-output', 'Standalone output module')],
    resources: [resource('verified-dual', [[`gateway:${firstGateway}`, 'dual', 'can'], ['standalone-usb', usbKey, 'usb']]),
      resource('unproven-second', [[`gateway:${secondGateway}`, 'dual', 'can']])],
    native: { observedAt: Date.now(), virtualDevices: [
      { id: 'vjoy:3', name: 'External busy virtual joystick', available: true, status: 'busy', createdByCockpit: null, ownerPid: 9999, diagnostics: [] },
      { id: 'vjoy:4', name: 'Missing remembered virtual joystick', available: false, status: 'missing', createdByCockpit: true, diagnostics: ['Driver cannot currently expose this target.'] },
    ], controllerAssociations: [{ controllerId: 'gateway-hid', gatewayId: firstGateway, moduleId: 'dual', evidence: ['verified-gateway-serial-and-usb-identity', 'exact-hid-report-id'] }],
    hiddenDevices: [{ id: 'pedals-pnp', name: 'Hidden pedals PnP record', instanceId: 'HID\\FIXTURE_PEDALS', present: true, hidden: true,
      configured: true, scope: 'capture-exception', controllerIds: ['pedals'], diagnostics: ['Observed in the capture process; other process exceptions may differ.'] }], diagnostics: [] },
    companions: { listener: { enabled: true, endpoint: 'wss://127.0.0.1:9980' }, host: { clients: [
      { companionId: 'browser-fixture', name: 'Browser companion', kind: 'browser', connected: true, peer: '192.0.2.10' },
      { companionId: 'esp32-fixture', name: 'Known ESP32 companion', kind: 'esp32', connected: false },
    ], instances: [{ instanceId: 'assigned-fixture', companionId: 'browser-fixture', name: 'Assigned panel instance', armed: true, sessionId: 'fixture-session', publicationHash: hash }], publications: [{ publicationHash: hash }] },
    usb: { devices: [{ connected: false, port: 'COM-fixture-companion', stage: 'disconnected', device: { companionId: 'esp32-fixture', name: 'Known ESP32 companion', boardProfile: 'fixture-board' } }] } },
    games: { catalogue: [
      { id: 'dcs', name: 'DCS World', provider: 'dcs-bios', installed: true, launchSupported: false,
        integrationSetup: { state: 'ready', live: 'receiving', message: 'Fixture service receiving.', connection: { commandHost: '127.0.0.1', commandPort: 7778 } } },
      { id: 'x-plane-12', name: 'X-Plane 12', provider: 'x-plane', installed: false, launchSupported: false,
        integrationSetup: { state: 'missing-installation', live: 'unavailable', message: 'Fixture installation unavailable.', connection: {} } },
    ], cleanup: { phase: 'idle' } },
    failControllers: false, failForget: null, keepForgotten: false,
    controllerDelayMs: 0, completedControllerPages: 0, integrationBound: true,
    companionUpdateDelayMs: 0, failCompanionUpdate: false,
  };
}

test('running Devices shares complete inventory, topology, filtering, lifecycle and durable guarded cleanup', { timeout: 120_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const fixture = fixtures(), requests = [], unexpected = [], pageErrors = [], suppressedHmrMessages = [], mouseNavigation = [];
  let hidHideOpens = 0;
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const context = await browser.newContext({ viewport: { width: 1500, height: 1100 } }), page = await context.newPage();
  page.on('pageerror', error => pageErrors.push(error.message));
  // This shared dev server can receive unrelated edits during acceptance. Keep
  // the initially served app/CSS for this run; Vite errors, other socket traffic
  // and Cockpit's real polling/validation still pass through unchanged.
  await page.routeWebSocket(url => url.host === new URL(endpoint).host, socket => {
    const server = socket.connectToServer();
    if (!socket.protocols().includes('vite-hmr')) return;
    server.onMessage(message => {
      if (typeof message === 'string') {
        let payload;
        try { payload = JSON.parse(message); } catch { /* Forward non-JSON messages. */ }
        if (['update', 'full-reload', 'prune'].includes(payload?.type)) {
          suppressedHmrMessages.push(payload.type);
          return;
        }
      }
      socket.send(message);
    });
  });
  const owner = { session: defaultId, generation: 1, profileId: defaultId, profileRevision: 1, gameId: 'dcs' };
  const appliedSnapshot = { profileId: defaultId, revision: 1, semanticHash: hash };
  const session = { state: 'running', paused: false, appliedSnapshot, owner, degradation: [], fault: null };
  const profileDocument = { schemaVersion: 2, id: defaultId, name: 'Default', revision: 1,
    eligibility: { gameIds: [] }, resources: ['retained-throttle-binding', 'retained-wheel-binding', 'retained-virtual-binding']
      .map(id => ({ id, name: id, kind: 'device', required: false, ports: [] })),
    bindings: [], processingNodes: [], virtualOutputs: [], visibility: [], overrides: [], companionAssignments: [],
    editor: { positions: [], viewport: { x: 0, y: 0, zoom: 1 } } };
  const listing = { profiles: [{ id: defaultId, name: 'Default', revision: 1, isDefault: true, semanticHash: hash }],
    profileSession: session, session: { selectedProfileId: defaultId, state: 'idle', appliedSnapshot: null, fault: null },
    automatic: { schemaVersion: 1, mode: 'manual', manualProfileId: defaultId, rules: [], status: 'manual', message: 'Fixture idle.', conflicts: [], lastAttempt: null, target: null, readOnly: false }, readOnly: false, diagnostics: [] };
  const base = () => ({ connected: false, running: false, needsReconnect: false, modules: [], ports: fixture.ports,
    games: fixture.games, companions: fixture.companions, profiles: listing });
  const result = fields => ({ ok: true, state: { ...base(), ...clone(fields) } });
  await page.route('**/api/gateway/**', async route => { unexpected.push('HTTP fallback bypassed fixture IPC'); await route.abort(); });
  await page.exposeFunction('__osgNativeInvoke', async (command, args) => {
    if (command === 'optional_dependencies') return { supported: false, dependencies: [] };
    if (command === 'open_hidhide') { hidHideOpens++; return; }
    if (command !== 'gateway_request') throw Error(`Unexpected invocation: ${command}`);
    const request = clone(args.request); requests.push(request);
    const offset = request.offset ?? 0;
    if (['profile_list', 'profile_status', 'profile_automatic_status'].includes(request.command)) return result({ profiles: listing });
    if (request.command === 'profile_runtime_topology') {
      const items = request.section === 'nodes'
        ? [{ id: 'dcs', name: 'DCS', kind: 'game', summary: 'Integration' }, { id: 'panel', name: 'Panel', kind: 'companion', summary: 'Assigned panel' }]
        : fixture.integrationBound ? [{ id: 'telemetry', from: { nodeId: 'dcs', portId: 'altitude' }, to: { nodeId: 'panel', portId: 'altitude' } }] : [];
      return result({ profileTopology: { schemaVersion: 1, owner, appliedSnapshot, programHash: hash, capturedUs: '1',
        section: request.section, offset, total: items.length, items: items.slice(offset, offset + request.limit) } });
    }
    if (request.command === 'profile_assignments_get') return result({ profiles: { ...listing, document: profileDocument, assignments: [
      { resourceId: 'retained-throttle-binding', sourceId: 'remembered-throttle', ports: [] },
      { resourceId: 'retained-wheel-binding', sourceId: 'remembered-wheel', ports: [] },
      { resourceId: 'retained-virtual-binding', sourceId: 'vjoy:4', ports: [] },
    ] } });
    if (request.command === 'profile_desktop_status') return result({ desktop: {
      enabled: false, qualified: false, state: 'disabled', reason: 'Fixture observation only.', scope: { kind: 'application' },
    } });
    if (request.command === 'profile_retired_recovery_status') return result({ retiredRecovery: { required: false, outputs: [], provisioning: [], job: null, diagnostics: [], outputQualified: false, provisioningQualified: false } });
    if (request.command === 'controller_list') {
      if (fixture.controllerDelayMs) await new Promise(resolve => setTimeout(resolve, fixture.controllerDelayMs));
      fixture.completedControllerPages++;
      if (fixture.failControllers) return { ok: false, error: 'Fixture controller provider unavailable.' };
      return result({ controllers: { session: 'fixture-controllers', backendStatus: 'ready', offset, total: fixture.controllers.length,
        limit: 2, sources: fixture.controllers.slice(offset, offset + 2) } });
    }
    if (request.command === 'controller_forget') {
      if (fixture.failForget === request.sourceId) return { ok: false, error: 'Fixture removal failed once; inspect before retrying.' };
      const source = fixture.controllers.find(source => source.id === request.sourceId);
      if (source?.online) return { ok: false, error: 'Controller reconnected before cleanup.' };
      if (!fixture.keepForgotten) fixture.controllers = fixture.controllers.filter(source => source.id !== request.sourceId);
      return result({ controllers: { session: 'fixture-controllers', offset: 0, total: fixture.controllers.length, limit: 32, sources: fixture.controllers } });
    }
    if (request.command === 'controller_inspect') return result({ controllers: { source: fixture.controllers.find(source => source.id === request.sourceId),
      offset: 0, total: 0, limit: 32, controls: [], identity: { path: `fixture:${request.sourceId}`, collection: '1', serial: 'fixture', fingerprint: hash }, diagnostics: [] } });
    if (request.command === 'controller_visibility_status') return result({ controllers: { offset: 0, total: 0, limit: 32, visibility: {
      platform: 'windows', qualified: false, enabled: false, selected: [], selectedCount: 0, global: { inspected: false }, recovery: { required: false },
      intent: { mode: 'visible' }, modes: [{ mode: 'visible', label: 'Visible', supported: true, qualified: true, reason: 'Fixture inspection only.' }],
      reason: 'Fixture visibility observation only.', job: null, processes: [], nextProcess: null,
    } } });
    if (request.command === 'module_usb_list') return result({ moduleUsb: { offset, total: fixture.modules.length, modules: fixture.modules.slice(offset, offset + 1) } });
    if (request.command === 'profile_resources_list') return result({ profileResources: { session: 'fixture-evidence', revision: '1', offset,
      total: fixture.resources.length, resources: fixture.resources.slice(offset, offset + 1), inventoryDiagnostics: [], readOnly: false } });
    if (request.command === 'system_inventory') return result({ systemInventory: { ...fixture.native, offset,
      total: fixture.native.hiddenDevices.length + fixture.native.controllerAssociations.length,
      hiddenDevices: offset === 0 ? fixture.native.hiddenDevices : [], controllerAssociations: offset === 0 ? [] : fixture.native.controllerAssociations,
      nextOffset: offset === 0 ? fixture.native.hiddenDevices.length : null } });
    if (request.command === 'companion_status') return result({ companions: fixture.companions });
    if (request.command === 'companion_update') {
      const { companionId, name } = request.companion;
      const client = fixture.companions.host.clients.find(client => client.companionId === companionId);
      assert.ok(client, 'Autosave may update only an existing fixture companion.');
      assert.equal(typeof name, 'string');
      assert.ok(name.trim().length > 0 && name.length <= 80);
      if (fixture.companionUpdateDelayMs) await new Promise(resolve => setTimeout(resolve, fixture.companionUpdateDelayMs));
      if (fixture.failCompanionUpdate) return { ok: false, error: 'Fixture companion autosave failed; edit to retry.' };
      client.name = name;
      return result({ companions: fixture.companions });
    }
    if (request.command === 'game_get_state') return result({ games: fixture.games });
    if (request.command === 'ports') return result({ ports: fixture.ports });
    if (['connect', 'status', 'disconnect', 'config_read', 'events_state', 'action_describe', 'action_state'].includes(request.command) && fixture.states.has(request.gatewayId)) {
      const state = fixture.states.get(request.gatewayId);
      if (request.command === 'disconnect') state.connected = false;
      if (request.command === 'connect') state.connected = true;
      return result(state);
    }
    unexpected.push(request.command);
    return { ok: false, error: `Unexpected fixture operation: ${request.command}` };
  });
  await page.addInitScript(() => { window.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.__osgNativeInvoke(command, args) }; });
  try {
    await page.goto(endpoint);
    // Seed only this browser context's local panel library. Opening an existing
    // draft exercises editor navigation without saving or assigning native data.
    await page.evaluate(async () => {
      const { newDocument, createLibrary, saveDraft } = await import('/packages/companion-designer/src/index.ts');
      const { CompanionLibraryStore } = await import('/src/lib/companion-library.ts');
      const library = await saveDraft(createLibrary(), { id: 'fixture-panel', name: 'Fixture panel', document: newDocument('Fixture panel'), assets: [] });
      await new CompanionLibraryStore().save({ schemaVersion: 1, library, publications: {} });
    });
    const nav = page.getByRole('navigation', { name: 'Main navigation' });
    const devicesNav = nav.getByRole('button', { name: /^Devices(?:\s|$)/ });
    // Startup refresh populates the shared sidebar count before Devices opens.
    await expect(devicesNav.locator('.nav-count')).toHaveText('14');
    await expect(nav.getByRole('button', { name: 'Overview', exact: true })).toHaveClass(/\bcurrent\b/);
    await devicesNav.click();
    await page.getByRole('button', { name: 'Manage HidHide', exact: true }).click();
    assert.equal(hidHideOpens,1);
    await expect(nav.getByRole('button', { name: 'Gateways', exact: true })).toHaveCount(0);
    await expect(nav.getByRole('button', { name: 'Topology', exact: true })).toHaveCount(0);
    const devices = page.getByRole('region', { name: 'System devices', exact: true });
    const search = page.getByRole('textbox', { name: 'Search components', exact: true });
    const filter = page.getByRole('combobox', { name: 'Filter components', exact: true });
    const connections = page.getByRole('combobox', { name: 'Filter connections', exact: true });
    const list = page.getByRole('button', { name: 'List', exact: true }), topology = page.getByRole('button', { name: 'Topology', exact: true });
    await expect(page.getByRole('button', { name: 'Refresh devices', exact: true })).toHaveCount(0);
    async function waitForInventoryRefresh() {
      const previous = fixture.completedControllerPages;
      await expect.poll(() => fixture.completedControllerPages, { timeout: 8_000 }).toBeGreaterThan(previous);
    }
    await expect(devices.getByText('Dual transport panel', { exact: true })).toBeVisible();
    const expectedNames = ['Hidden pedals', 'Remembered wheel', 'Remembered throttle', 'Dual transport panel', 'Unproven second panel',
      'CAN output module', 'Standalone output module', 'External busy virtual joystick', 'Missing remembered virtual joystick',
      'DCS World · dcs-bios', 'Browser companion', 'Known ESP32 companion'];
    for (const name of expectedNames) await expect(devices.getByText(name, { exact: true }).first()).toBeVisible();
    await expect(devices.getByText('Panel: Assigned panel instance', { exact: true })).toBeVisible();
    await expect(devices.locator('[data-component-id="companion-instance:assigned-fixture"]')).toHaveCount(0);
    await expect(devices.locator('[data-component-id="integration:x-plane-12"]')).toHaveCount(0);
    await expect(devices.getByText('Gateway HID collection', { exact: true })).toHaveCount(0);
    await expect(devices.getByText('Dual USB sighting', { exact: true })).toHaveCount(0);
    await expect.poll(() => requests.filter(request => request.command === 'controller_list' && request.offset === 2).length).toBeGreaterThan(0);
    assert.ok(requests.some(request => request.command === 'module_usb_list' && request.offset === 1));
    assert.ok(requests.some(request => request.command === 'profile_resources_list' && request.offset === 1));
    assert.ok(requests.some(request => request.command === 'system_inventory' && request.offset === 1));
    const listIds = (await devices.locator('[data-component-id]').evaluateAll(nodes => nodes.map(node => node.dataset.componentId))).sort();
    assert.equal(listIds.length, 14, 'All hardware/paired companions and the bound integration appear; panels stay folded into companions.');
    assert.equal(new Set(listIds).size, listIds.length, 'HID collections and verified USB/CAN sightings share one component.');
    const tiles = devices.locator('.system-list .component-tile');
    await expect(tiles).toHaveCount(listIds.length);
    const tilePositions = await tiles.evaluateAll(nodes => nodes.map(node => {
      const bounds = node.getBoundingClientRect();
      return { x: Math.round(bounds.x), y: Math.round(bounds.y) };
    }));
    assert.ok(tilePositions.some(tile => tile.y === tilePositions[0].y && tile.x !== tilePositions[0].x),
      'The desktop inventory presents multiple component tiles per row.');
    const offlineTile = devices.locator('[data-component-id="controller:remembered-wheel"]');
    const hiddenTile = devices.locator('[data-component-id="controller:pedals"]');
    await expect(offlineTile).toHaveClass(/\boffline\b/);
    await expect(offlineTile).toHaveAccessibleName(/Offline/);
    await expect(offlineTile.locator('.component-status')).toHaveCount(0);
    await expect(offlineTile).toHaveCSS('opacity', '1');
    await expect(hiddenTile).not.toHaveClass(/\boffline\b/);
    await expect(hiddenTile).toHaveAccessibleName(/Connected · Hidden/);
    const offlineNameColor = await offlineTile.locator('strong').evaluate(node => getComputedStyle(node).color);
    const connectedNameColor = await hiddenTile.locator('strong').evaluate(node => getComputedStyle(node).color);
    assert.notEqual(offlineNameColor, connectedNameColor, 'Offline names are muted independently of their readable controls.');
    const offlineBackground = await offlineTile.evaluate(node => getComputedStyle(node).backgroundColor);
    // A retained node during a live update can still have the original card
    // class. Its basic styling must not depend on the new presentation marker.
    await offlineTile.evaluate(node => node.classList.remove('component-tile'));
    await expect(offlineTile).toHaveCSS('background-color', offlineBackground);
    await expect(offlineTile).toHaveCSS('text-align', 'left');
    await offlineTile.evaluate(node => node.classList.add('component-tile'));
    await page.screenshot({ path: resolve(artifacts, 'system-tiles.png'), fullPage: true });
    await expect(devices.locator('.type-counts')).toContainText('Physical · Third-party: 3');
    await expect(devices.locator('.type-counts')).toContainText('Physical · OSG: 4');
    await expect(devices.locator('.type-counts')).toContainText('Gateway · OSG: 2');
    fixture.controllerDelayMs = 300;
    const previousControllerPages = fixture.completedControllerPages;
    await expect.poll(() => fixture.completedControllerPages, { timeout: 8_000 }).toBeGreaterThan(previousControllerPages + 1);
    await expect(filter).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Refresh devices', exact: true })).toHaveCount(0);
    await expect(page.getByText('Refreshing…', { exact: true })).toHaveCount(0);
    fixture.controllerDelayMs = 0;
    await connections.selectOption('can');
    await expect(devices.locator('.system-list [data-component-id]')).toHaveCount(3);
    await topology.click(); await expect(connections).toHaveValue('can');
    await expect(devices.locator(`[data-component-id="gateway:${firstGateway}"]`)).toBeVisible();
    await expect(devices.locator(`[data-component-id="gateway:${secondGateway}"]`)).toBeVisible();
    await list.click(); await expect(connections).toHaveValue('can'); await connections.selectOption('all');
    await devices.locator('[data-component-id="virtual:vjoy:3"]').click();
    const details = page.getByRole('region', { name: 'Component details', exact: true });
    const backToDevices = devices.getByRole('button', { name: 'Back to Devices', exact: true });
    await expect(details).toBeVisible();
    await expect(devices.locator('.system-list')).not.toBeVisible();
    await expect(list).not.toBeVisible();
    await expect(details).toContainText('External busy virtual joystick');
    await expect(details).toContainText('Creation provenance unknown');
    await expect(details).toContainText('PID 9999');
    await expect(details.getByRole('button', { name: 'Configure device', exact: true })).toHaveCount(0);
    await expect(details.getByRole('button', { name: 'Forget device', exact: true })).toBeDisabled();
    await backToDevices.click();
    await expect(devices.locator('.system-list')).toBeVisible();
    // Sidebar navigation always opens the inventory, even when Devices is
    // already active, without discarding the current presentation or filters.
    const virtualTile = devices.locator('.system-list [data-component-id="virtual:vjoy:3"]');
    await virtualTile.click();
    await expect(details).toBeVisible();
    await devicesNav.click();
    await expect(details).not.toBeVisible();
    await expect(devices.locator('.system-list')).toBeVisible();
    await expect(list).toHaveAttribute('aria-pressed', 'true');
    await expect(virtualTile).toHaveAttribute('aria-pressed', 'true');
    await search.fill('External busy virtual joystick');
    await filter.selectOption('virtual');
    await connections.selectOption('software');
    await virtualTile.click();
    await expect(details).toBeVisible();
    await nav.getByRole('button', { name: 'Overview', exact: true }).click();
    await devicesNav.focus();
    await devicesNav.press('Enter');
    await expect(details).not.toBeVisible();
    await expect(devices.locator('.system-list')).toBeVisible();
    await expect(search).toHaveValue('External busy virtual joystick');
    await expect(filter).toHaveValue('virtual');
    await expect(connections).toHaveValue('software');
    await expect(virtualTile).toHaveAttribute('aria-pressed', 'true');
    await search.fill(''); await filter.selectOption('all'); await connections.selectOption('all');
    await topology.click();
    const graph = page.getByRole('region', { name: 'System topology', exact: true });
    await expect(graph).toBeVisible();
    const graphViewport = graph.locator('.topology-viewport');
    const graphTransform = () => graph.locator('.topology-world').evaluate(node => {
      const matrix = new DOMMatrix(getComputedStyle(node).transform);
      return { x: matrix.e, y: matrix.f, zoom: matrix.a };
    });
    const beforeScroll = await graphTransform();
    await graphViewport.hover();
    await page.mouse.wheel(0, 120);
    await expect.poll(async () => (await graphTransform()).y).toBe(beforeScroll.y - 120);
    assert.equal((await graphTransform()).zoom, beforeScroll.zoom, 'Normal wheel scrolling pans without zooming.');
    await page.mouse.wheel(0, -120);
    await expect.poll(async () => (await graphTransform()).y).toBe(beforeScroll.y);
    for (const modifier of ['Control', 'Meta']) {
      const beforeZoom = await graphTransform();
      await page.keyboard.down(modifier);
      try { await page.mouse.wheel(0, -120); }
      finally { await page.keyboard.up(modifier); }
      await expect.poll(async () => (await graphTransform()).zoom).toBeGreaterThan(beforeZoom.zoom);
    }
    await graph.getByRole('button', { name: 'Fit all components', exact: true }).click();
    const graphIds = await graph.locator('[data-component-id]').evaluateAll(nodes => nodes.map(node => node.dataset.componentId).filter(id => id !== 'host').sort());
    assert.deepEqual(graphIds, listIds);
    await expect(graph.locator('.topology-edges text')).toHaveCount(0);
    await expect(graph.locator('.topology-viewport')).toHaveCSS('background-image', /radial-gradient/);
    await expect(graph.locator('.offline-zone')).toContainText('Disconnected');
    await expect(graph.locator('[data-bus-id="host:usb"]')).toContainText('USB');
    await expect(graph.locator(`[data-bus-id="gateway:${firstGateway}:can"]`)).toContainText('CAN');
    await expect(graph.locator('.topology-edges g[data-to="controller:remembered-wheel"]')).toHaveCount(0);
    const misplacedPorts = await graph.evaluate(root => {
      const boxes = [...root.querySelectorAll('.topology-node')].map(node => ({ x: parseFloat(node.style.left), y: parseFloat(node.style.top), w: parseFloat(node.style.width), h: parseFloat(node.style.height) }));
      return [...root.querySelectorAll('.connection-port')].filter(port => {
        const x = parseFloat(port.style.left), y = parseFloat(port.style.top);
        return !boxes.some(box => ((x === box.x || x === box.x + box.w) && y >= box.y && y <= box.y + box.h) || ((y === box.y || y === box.y + box.h) && x >= box.x && x <= box.x + box.w));
      }).length;
    });
    assert.equal(misplacedPorts, 0, 'Protocol icons appear only where a connection touches a device box.');
    const connector=graph.getByRole('img',{name:'USB connector',exact:true}).first();
    await connector.hover();await expect(connector.locator('.port-tooltip')).toHaveText('USB');await expect(connector.locator('.port-tooltip')).toBeVisible();
    await expect(graph.locator('[data-component-id="virtual:vjoy:3"]')).toHaveAttribute('aria-pressed', 'true');
    await expect(graph.locator('[data-component-id="virtual:vjoy:3"]')).toHaveAttribute('aria-label', /busy.*Creation provenance unknown/);
    await expect(graph.locator('[data-component-id="virtual:vjoy:3"]')).toHaveAttribute('title', /busy.*Creation provenance unknown/);
    await expect(details).not.toBeVisible();
    // Keyboard focus reveals an offscreen node before pointer activation.
    await graph.locator('[data-component-id="virtual:vjoy:3"]').focus();
    await graph.locator('[data-component-id="virtual:vjoy:3"]').click();
    await expect(details).toBeVisible();
    await devicesNav.focus();
    await devicesNav.press('Space');
    await expect(details).not.toBeVisible();
    await expect(graph).toBeVisible();
    await expect(topology).toHaveAttribute('aria-pressed', 'true');
    await expect(graph.locator('[data-component-id="virtual:vjoy:3"]')).toHaveAttribute('aria-pressed', 'true');
    await expect(graph.getByRole('button', { name: 'Zoom in', exact: true })).toBeEnabled();
    await graph.getByRole('button', { name: 'Zoom in', exact: true }).click();
    await graph.getByRole('button', { name: 'Fit all components', exact: true }).click();
    await graph.getByRole('region', { name: /^Connection graph/ }).focus();
    await page.keyboard.press('ArrowRight'); await page.keyboard.press('Home');
    await search.fill('Dual transport panel');
    await expect(graph.locator('[data-component-id="resource:verified-dual"]')).toBeVisible();
    await expect(graph.locator('[data-component-id="gateway:stm32:00112233445566778899aabb"]')).toBeVisible();
    await list.click(); await expect(search).toHaveValue('Dual transport panel');
    await expect(devices.locator('[data-component-id]')).toHaveCount(1);
    await search.fill(''); await filter.selectOption('hidden');
    await expect(devices.locator('[data-component-id]')).toHaveCount(1);
    await expect(devices.locator('[data-component-id="controller:pedals"]')).toHaveAccessibleName(/Connected · Hidden/);
    await devices.locator('[data-component-id="controller:pedals"]').click();
    await expect(details.getByRole('button', { name: 'Configure device', exact: true })).toHaveCount(0);
    await expect(details.getByRole('button', { name: 'Forget device', exact: true })).toBeDisabled();
    const controllerDetails = details.getByRole('region', { name: 'Selected host controller', exact: true });
    const localName = controllerDetails.getByRole('textbox', { name: 'Local name', exact: true });
    const controllerRole = controllerDetails.getByRole('combobox', { name: 'Role', exact: true });
    await expect(localName).toHaveValue('Hidden pedals');
    await expect(controllerRole).toHaveValue('pedals');
    await expect(controllerDetails.getByRole('button', { name: 'Close', exact: true })).toHaveCount(0);
    // Opening a component directly must hydrate its editable metadata once.
    // A subsequent live input report must preserve the user's unsaved draft.
    await localName.fill('Pedals local draft');
    await controllerRole.selectOption('throttle');
    fixture.controllers.find(source => source.id === 'pedals').sequence = '2';
    await expect(controllerDetails).toContainText('report 2');
    await expect(localName).toHaveValue('Pedals local draft');
    await expect(controllerRole).toHaveValue('throttle');
    await page.setViewportSize({ width: 520, height: 900 });
    const narrowDetails = await details.evaluate(node => ({
      clientWidth: node.clientWidth, scrollWidth: node.scrollWidth,
      right: node.getBoundingClientRect().right, viewportWidth: window.innerWidth,
    }));
    assert.ok(narrowDetails.scrollWidth <= narrowDetails.clientWidth + 1, 'Component detail content stays inside its card at a narrow width.');
    assert.ok(narrowDetails.right <= narrowDetails.viewportWidth + 1, 'Component details stay inside the viewport.');
    await expect(localName).toBeVisible();
    await expect(controllerRole).toBeVisible();
    await page.screenshot({ path: resolve(artifacts, 'component-controller-narrow.png'), fullPage: true });
    await page.setViewportSize({ width: 1500, height: 1100 });
    await backToDevices.click();
    await topology.click(); await expect(filter).toHaveValue('hidden');
    await expect(graph.locator('[data-component-id="controller:pedals"]')).toHaveAttribute('aria-pressed', 'true');
    await list.click(); await filter.selectOption('all');
    // Provider failure keeps prior records explicitly uncertain, independently
    // of successful integration/USB/native inspection.
    fixture.failControllers = true; await waitForInventoryRefresh();
    await expect(devices).toContainText('Fixture controller provider unavailable');
    await expect(devices.getByText('Remembered wheel', { exact: true })).toBeVisible();
    await expect(devices.getByText('Standalone output module', { exact: true })).toBeVisible();
    fixture.failControllers = false; await waitForInventoryRefresh();
    await expect(devices).not.toContainText('Fixture controller provider unavailable');
    await expect(devices.getByText('Hidden pedals PnP record', { exact: true })).toHaveCount(0);
    assert.deepEqual(await devices.locator('.system-list [data-component-id]').evaluateAll(nodes => nodes.map(node => node.dataset.componentId).sort()), listIds,
      'Provider recovery must merge the correlated PnP alias without retaining a duplicate physical device.');
    // Active bindings, rather than telemetry traffic or catalogue support, decide
    // integration visibility. Reads never activate or edit the applied profile.
    fixture.games.catalogue[0].integrationSetup.live = 'waiting';
    await waitForInventoryRefresh();
    await expect(devices.locator('[data-component-id="integration:dcs"]')).toBeVisible();
    fixture.integrationBound = false;
    fixture.games.catalogue[0].integrationSetup.live = 'receiving';
    await waitForInventoryRefresh();
    await expect(devices.locator('[data-component-id="integration:dcs"]')).toHaveCount(0);
    await topology.click();
    await expect(graph.locator('[data-component-id="integration:dcs"]')).toHaveCount(0);
    await list.click();
    fixture.integrationBound = true; session.state = 'stopped'; session.owner = null; session.appliedSnapshot = null;
    await waitForInventoryRefresh();
    await expect(devices.locator('[data-component-id="integration:dcs"]')).toHaveCount(0);
    session.state = 'running'; session.owner = owner; session.appliedSnapshot = appliedSnapshot;
    await waitForInventoryRefresh();
    await expect(devices.locator('[data-component-id="integration:dcs"]')).toBeVisible();
    // Native companion transport evidence is independent from installation,
    // pairing approval and assigned input ownership. Offline network history
    // remains visible while a USB-only installation gains no network edge.
    const browserClient = fixture.companions.host.clients.find(client => client.companionId === 'browser-fixture');
    browserClient.connected = false; delete browserClient.peer;
    fixture.companions.host.instances[0].connected = false;
    fixture.companions.host.pending = [{ requestId: 'pairing-fixture', name: 'Unverified tablet', transport: 'network', approved: false }];
    fixture.companions.usb.devices[0].connected = true;
    await waitForInventoryRefresh();
    await devices.locator('[data-component-id="companion:browser-fixture"]').click();
    await expect(details).toContainText('Offline');
    const deviceConnections = details.getByRole('region', { name: 'Device connections', exact: true });
    await expect(deviceConnections).toContainText('NETWORK');
    await expect(deviceConnections).toContainText('Your computer');
    await expect(deviceConnections).toContainText('192.0.2.10');
    await expect(deviceConnections).toContainText('Last-known connection');
    // Managing a device opens that exact paired companion, including offline
    // installations. Autosave preserves newer edits while status and an earlier
    // write are in flight, without reassigning the unchanged current panel.
    const companions = page.getByRole('region', { name: 'Companions', exact: true });
    const companionsNav = nav.getByRole('button', { name: /^Companions(?:\s|$)/ });
    const companionSettings = page.getByRole('region', { name: 'Companion settings', exact: true });
    const panels = page.getByRole('region', { name: 'Panels', exact: true });
    const panelEditor = page.getByRole('region', { name: 'Panel editor', exact: true });
    await details.getByRole('button', { name: 'Manage companion', exact: true }).click();
    await expect(companionSettings).toBeVisible();
    await expect(companionSettings.getByRole('heading', { name: 'Browser companion', exact: true, level: 1 })).toBeVisible();
    await expect(companions.getByRole('heading', { name: 'Companions', exact: true })).toHaveCount(0);
    await expect(panels).toHaveCount(0);
    await expect(panelEditor).toHaveCount(0);
    const companionName = companionSettings.getByLabel('Name', { exact: true });
    await expect(companionName).toHaveValue('Browser companion');
    await expect(companionSettings).toContainText('Offline');
    await expect(companionSettings.getByRole('combobox', { name: 'Assigned panel', exact: true })).toHaveValue(hash);
    await expect(companionSettings.getByRole('button', { name: 'Save', exact: true })).toHaveCount(0);
    const companionUpdates = () => requests.filter(request => request.command === 'companion_update');
    fixture.companionUpdateDelayMs = 600;
    await companionName.fill('Browser companion first edit');
    await expect.poll(() => companionUpdates().length).toBe(1);
    browserClient.name = 'Browser companion refreshed';
    await companionName.fill('Browser companion local draft');
    await expect.poll(() => browserClient.name).toBe('Browser companion local draft');
    fixture.companionUpdateDelayMs = 0;
    const savedStatusPolls = requests.filter(request => request.command === 'companion_status').length;
    await expect.poll(() => requests.filter(request => request.command === 'companion_status').length).toBeGreaterThan(savedStatusPolls);
    await expect(companionName).toHaveValue('Browser companion local draft');
    assert.deepEqual(companionUpdates().map(request => request.companion), [
      { companionId: 'browser-fixture', name: 'Browser companion first edit' },
      { companionId: 'browser-fixture', name: 'Browser companion local draft' },
    ]);
    await page.screenshot({ path: resolve(artifacts, 'companion-full-page.png'), fullPage: true });
    // Clicking the active sidebar entry opens the main page, including when a
    // direct Manage companion link originally opened the editor.
    await companionsNav.click();
    await expect(companionSettings).toHaveCount(0);
    await expect(companions.getByRole('button', { name: 'Edit Browser companion local draft', exact: true })).toBeVisible();
    await expect(panels.getByRole('button', { name: /^Fixture panel/ })).toBeVisible();
    await companions.getByRole('button', { name: 'Edit Browser companion local draft', exact: true }).click();
    await expect(companionName).toHaveValue('Browser companion local draft');
    const beforeBlank = companionUpdates().length;
    const beforeBlankPolls = requests.filter(request => request.command === 'companion_status').length;
    await companionName.fill('   ');
    await expect.poll(() => requests.filter(request => request.command === 'companion_status').length).toBeGreaterThan(beforeBlankPolls + 1);
    assert.equal(companionUpdates().length, beforeBlank, 'A blank companion name never replaces its saved name.');
    await expect(companionName).toHaveValue('   ');
    assert.equal(browserClient.name, 'Browser companion local draft');
    fixture.failCompanionUpdate = true;
    await companionName.fill('Browser companion failed draft');
    await expect(companionSettings.getByText(/Fixture companion autosave failed/)).toBeVisible();
    await expect(companionName).toHaveValue('Browser companion failed draft');
    assert.equal(browserClient.name, 'Browser companion local draft');
    fixture.failCompanionUpdate = false;
    await companionName.fill('Browser companion');
    await expect.poll(() => browserClient.name).toBe('Browser companion');
    await expect(companionSettings.getByText(/Fixture companion autosave failed/)).toHaveCount(0);
    await companionSettings.getByRole('button', { name: 'Back to Companions', exact: true }).click();
    await expect(companions.getByRole('button', { name: 'Edit Browser companion', exact: true })).toBeVisible();
    await expect(companionSettings).toHaveCount(0);
    await devicesNav.click();
    await devices.locator('[data-component-id="companion:esp32-fixture"]').click();
    await expect(deviceConnections).toContainText('USB');
    await expect(deviceConnections).toContainText('Your computer');
    await expect(deviceConnections).toContainText('COM-fixture-companion');
    await expect(deviceConnections).toContainText('Current connection');
    await expect(deviceConnections).not.toContainText('NETWORK');
    await expect(details.getByRole('button', { name: 'Forget device', exact: true })).toBeDisabled();
    await details.getByRole('button', { name: 'Manage companion', exact: true }).click();
    await expect(companionName).toHaveValue('Known ESP32 companion');
    await expect(companionSettings.getByRole('combobox', { name: 'Assigned panel', exact: true })).toHaveValue('');
    await expect(companionSettings.getByLabel('USB port', { exact: true })).toHaveValue('COM-fixture-companion');
    // Leaving before the debounce fires still flushes the selected companion's
    // name, then the active sidebar returns directly to both inventories.
    await companionName.fill('Known ESP32 companion renamed');
    await companionsNav.click();
    await expect.poll(() => fixture.companions.host.clients.find(client => client.companionId === 'esp32-fixture').name).toBe('Known ESP32 companion renamed');
    await expect(companions.getByRole('button', { name: 'Edit Browser companion', exact: true })).toBeVisible();
    await expect(companions.getByRole('button', { name: 'Edit Known ESP32 companion renamed', exact: true })).toBeVisible();
    await expect(companionSettings).toHaveCount(0);
    // Tile edits have the same dedicated detail page as direct device links.
    // Returning from either editor restores both inventories without a save.
    await expect(panels.getByRole('button', { name: /^Fixture panel/ })).toBeVisible();
    await companions.getByRole('button', { name: 'Edit Known ESP32 companion renamed', exact: true }).click();
    await expect(companionName).toHaveValue('Known ESP32 companion renamed');
    await expect(companionSettings.getByRole('heading', { name: 'Known ESP32 companion renamed', exact: true, level: 1 })).toBeVisible();
    await expect(companions.getByRole('heading', { name: 'Companions', exact: true })).toHaveCount(0);
    await expect(panels).toHaveCount(0);
    await companionSettings.getByRole('button', { name: 'Back to Companions', exact: true }).click();
    await panels.getByRole('button', { name: /^Fixture panel/ }).click();
    await expect(panelEditor.getByRole('heading', { name: 'Fixture panel', exact: true, level: 1 })).toBeVisible();
    await expect(companions).not.toBeVisible();
    await expect(panels).toHaveCount(0);
    await expect(companionSettings).toHaveCount(0);
    await page.screenshot({ path: resolve(artifacts, 'panel-full-page.png'), fullPage: true });
    await companionsNav.click();
    await expect(panelEditor).toHaveCount(0);
    await expect(companions.getByRole('button', { name: 'Edit Browser companion', exact: true })).toBeVisible();
    await expect(panels.getByRole('button', { name: /^Fixture panel/ })).toBeVisible();
    // Chromium's trusted side-button input exercises the browser's native
    // history path. Dispatching both press and release catches double handling
    // that would otherwise skip a section or leave the app at a boundary.
    const nativeMouse = await context.newCDPSession(page);
    const browserHistory = () => nativeMouse.send('Page.getNavigationHistory');
    const settleNavigation = () => page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    async function sideButton(direction) {
      const previous = await browserHistory();
      const input = { x: 1200, y: 100, button: direction, clickCount: 1 };
      await nativeMouse.send('Input.dispatchMouseEvent', { ...input, type: 'mousePressed', buttons: direction === 'back' ? 8 : 16 });
      await nativeMouse.send('Input.dispatchMouseEvent', { ...input, type: 'mouseReleased', buttons: 0 });
      return previous;
    }
    async function historyStep(direction) {
      const previous = await sideButton(direction), expected = previous.currentIndex + (direction === 'back' ? -1 : 1);
      await expect.poll(async () => (await browserHistory()).currentIndex).toBe(expected);
      await settleNavigation();
      const current = await browserHistory();
      assert.equal(current.currentIndex, expected, 'One physical side-button click traverses exactly one history entry.');
      mouseNavigation.push({ direction, before: previous.currentIndex, after: current.currentIndex });
    }
    const companionsMainHistory = await browserHistory();
    await companionsNav.click(); await companionsNav.click();
    await settleNavigation();
    assert.deepEqual(await browserHistory(), companionsMainHistory, 'Clicking the current main section does not add duplicate history entries.');
    await devicesNav.click();
    await devices.locator('[data-component-id="companion:browser-fixture"]').click();
    await expect(details.getByRole('heading', { name: 'Browser companion', exact: true, level: 1 })).toBeVisible();
    await details.getByRole('button', { name: 'Manage companion', exact: true }).click();
    await expect(companionName).toHaveValue('Browser companion');
    await historyStep('back');
    await expect(details.getByRole('heading', { name: 'Browser companion', exact: true, level: 1 })).toBeVisible();
    await historyStep('forward');
    await expect(companionName).toHaveValue('Browser companion');
    await companionName.fill('Browser companion navigation');
    await historyStep('back');
    await expect.poll(() => browserClient.name).toBe('Browser companion navigation');
    await expect(details.getByRole('heading', { name: 'Browser companion navigation', exact: true, level: 1 })).toBeVisible();
    await historyStep('forward');
    await expect(companionName).toHaveValue('Browser companion navigation');
    await companionName.fill('Browser companion');
    await expect.poll(() => browserClient.name).toBe('Browser companion');
    await historyStep('back'); await expect(details).toBeVisible();
    await historyStep('back'); await expect(search).toBeVisible(); await expect(details).toHaveCount(0);
    await historyStep('forward'); await expect(details.getByRole('heading', { name: 'Browser companion', exact: true, level: 1 })).toBeVisible();
    await historyStep('back'); await expect(search).toBeVisible();
    await companionsNav.click();
    const branchedHistory = await browserHistory();
    assert.equal(branchedHistory.entries.length, branchedHistory.currentIndex + 1, 'A new sidebar navigation drops the obsolete forward branch.');
    await sideButton('forward'); await settleNavigation();
    assert.deepEqual(await browserHistory(), branchedHistory, 'Forward at the newest entry stays on the current app page.');
    await companions.getByRole('button', { name: 'Edit Browser companion', exact: true }).click();
    await expect(companionName).toHaveValue('Browser companion');
    await historyStep('back'); await expect(panels).toBeVisible(); await expect(companionSettings).toHaveCount(0);
    await historyStep('forward'); await expect(companionName).toHaveValue('Browser companion');
    await companionsNav.click();
    await panels.getByRole('button', { name: /^Fixture panel/ }).click();
    await expect(panelEditor.getByRole('heading', { name: 'Fixture panel', exact: true, level: 1 })).toBeVisible();
    await historyStep('back'); await expect(panels).toBeVisible(); await expect(panelEditor).toHaveCount(0);
    await historyStep('forward'); await expect(panelEditor.getByRole('heading', { name: 'Fixture panel', exact: true, level: 1 })).toBeVisible();
    const gamesNav = nav.getByRole('button', { name: /^Games(?:\s|$)/ });
    await gamesNav.click();
    await expect(page.getByRole('heading', { name: 'Games', exact: true, level: 1 })).toBeVisible();
    await historyStep('back'); await expect(panelEditor.getByRole('heading', { name: 'Fixture panel', exact: true, level: 1 })).toBeVisible();
    await historyStep('forward'); await expect(page.getByRole('heading', { name: 'Games', exact: true, level: 1 })).toBeVisible();
    await page.getByRole('button', { name: 'Open DCS World details', exact: true }).click();
    await expect(page.getByRole('region', { name: 'Game integration setup', exact: true })).toBeVisible();
    await historyStep('back'); await expect(page.getByRole('heading', { name: 'Games', exact: true, level: 1 })).toBeVisible();
    await historyStep('forward'); await expect(page.getByRole('region', { name: 'Game integration setup', exact: true })).toBeVisible();
    const profilesNav = nav.getByRole('button', { name: /^Profiles(?:\s|$)/ });
    const profiles = page.getByRole('region', { name: 'Profiles', exact: true });
    await profilesNav.click();
    await profiles.getByRole('button', { name: 'Edit Default', exact: true }).click();
    await expect(profiles.getByLabel('Profile name', { exact: true })).toHaveValue('Default');
    await historyStep('back'); await expect(profiles.getByRole('button', { name: 'Edit Default', exact: true })).toBeVisible();
    await historyStep('forward'); await expect(profiles.getByLabel('Profile name', { exact: true })).toHaveValue('Default');
    await historyStep('back');
    await profiles.getByRole('button', { name: 'Add profile', exact: true }).click();
    await profiles.getByLabel('Profile name', { exact: true }).fill('Navigation draft');
    const draftHistory = await browserHistory();
    const leaveProfile = page.getByRole('alertdialog', { name: 'Keep your unsaved changes?', exact: true });
    await sideButton('back');
    await expect(leaveProfile).toBeVisible();
    await leaveProfile.getByRole('button', { name: 'Keep editing', exact: true }).click();
    await expect.poll(async () => (await browserHistory()).currentIndex).toBe(draftHistory.currentIndex);
    await expect(profiles.getByLabel('Profile name', { exact: true })).toHaveValue('Navigation draft');
    await sideButton('back'); await expect(leaveProfile).toBeVisible();
    await leaveProfile.getByRole('button', { name: 'Discard and continue', exact: true }).click();
    await expect(profiles.getByRole('button', { name: 'Edit Default', exact: true })).toBeVisible();
    await expect.poll(async () => (await browserHistory()).currentIndex).toBe(draftHistory.currentIndex - 1);
    await sideButton('forward');
    await expect.poll(async () => (await browserHistory()).currentIndex).toBe(draftHistory.currentIndex);
    await expect(profiles.getByRole('button', { name: 'Edit Default', exact: true })).toBeVisible();
    await expect(profiles.getByLabel('Profile name', { exact: true })).toHaveCount(0);
    await devicesNav.click();
    await expect(devices.locator('[data-component-id="companion-pending:pairing-fixture"]')).toHaveCount(0);
    assert.equal(requests.filter(request => request.command === 'companion_approve').length, 0);
    await topology.click();
    await expect(graph.locator('[data-component-id="companion:browser-fixture"]')).toContainText('Offline');
    await expect(graph.locator('[data-component-id="companion-pending:pairing-fixture"]')).toHaveCount(0);
    await list.click();
    // Gateway management and existing module configuration remain reachable
    // through the shared details; returning retains selection and search.
    await search.fill('OpenSimGear CAN gateway');
    await devices.locator(`[data-component-id="gateway:${firstGateway}"]`).click();
    await expect(details.getByRole('region', { name: 'Gateways', exact: true })).toBeVisible();
    await expect(details).toContainText('CAN output module');
    await details.getByRole('button', { name: /^Disconnect gateway/ }).click();
    await expect.poll(() => requests.filter(request => request.command === 'disconnect' && request.gatewayId === firstGateway).length).toBe(1);
    await expect(details.getByRole('button', { name: /^Connect gateway/ })).toBeVisible();
    await details.getByRole('button', { name: /^Connect gateway/ }).click();
    await expect(details.getByRole('button', { name: /^Disconnect gateway/ })).toBeVisible();
    await details.locator('.gateway-device-link').filter({ hasText: 'Dual transport panel' }).click();
    await expect(page.getByRole('heading', { name: 'Dual transport panel', exact: true, level: 1 })).toBeVisible();
    await page.getByRole('button', { name: 'Back to devices', exact: true }).click();
    await expect(details).toBeVisible();
    await backToDevices.click();
    await expect(search).toHaveValue('OpenSimGear CAN gateway');
    await expect(devices.locator(`[data-component-id="gateway:${firstGateway}"]`)).toHaveAttribute('aria-pressed', 'true');
    await search.fill('');
    // The shared dialog explains metadata-only virtual cleanup. Unchanged
    // remembered snapshots remain suppressed after reload; reconnect may return.
    await devices.locator('[data-component-id="virtual:vjoy:4"]').click();
    await details.getByRole('button', { name: 'Forget device', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Cleanup devices', exact: true });
    const confirmCleanup = dialog.getByRole('button', { name: 'Confirm', exact: true });
    await expect(dialog).toContainText('OS device, vJoy layout, driver and bindings are retained');
    await expect(dialog).toContainText('retained-virtual-binding');
    await expect(confirmCleanup).toBeDisabled();
    await dialog.getByRole('checkbox', { name: 'Select Missing remembered virtual joystick', exact: true }).check();
    await expect(confirmCleanup).toBeEnabled();
    await confirmCleanup.click();
    await expect(devices.locator('[data-component-id="virtual:vjoy:4"]')).toHaveCount(0);
    await waitForInventoryRefresh(); await expect(devices.locator('[data-component-id="virtual:vjoy:4"]')).toHaveCount(0);
    // Bulk cleanup offers a selectable table. Cancel and unchecked rows never
    // mutate devices, while each checked row is revalidated before removal.
    await filter.selectOption('offline'); await search.fill('Remembered'); await topology.click();
    await page.getByRole('button', { name: 'Cleanup (2)', exact: true }).click();
    await expect(dialog).toContainText('Remembered wheel'); await expect(dialog).toContainText('Remembered throttle');
    await expect(dialog).toContainText('retained-throttle-binding');
    const cleanupTable = dialog.getByRole('table', { name: 'Devices to clean up', exact: true });
    await expect(cleanupTable.getByRole('columnheader', { name: 'Name', exact: true })).toBeVisible();
    await expect(cleanupTable.getByRole('columnheader', { name: 'Description', exact: true })).toBeVisible();
    const selectAll = dialog.getByRole('checkbox', { name: 'Select all', exact: true });
    const selectWheel = dialog.getByRole('checkbox', { name: 'Select Remembered wheel', exact: true });
    const selectThrottle = dialog.getByRole('checkbox', { name: 'Select Remembered throttle', exact: true });
    await expect(selectAll).not.toBeChecked();
    await expect(selectWheel).not.toBeChecked(); await expect(selectThrottle).not.toBeChecked();
    await expect(confirmCleanup).toBeDisabled();
    await expect(dialog).toContainText('0 of 2 selected');
    const wheelCleanupRow = cleanupTable.getByRole('row').filter({ has: page.getByRole('checkbox', { name: 'Select Remembered wheel', exact: true }) });
    await wheelCleanupRow.getByRole('rowheader', { name: 'Remembered wheel', exact: true }).click();
    await expect(selectWheel).toBeChecked();
    await expect(selectAll).toBeChecked({ indeterminate: true });
    await expect(confirmCleanup).toBeEnabled();
    await wheelCleanupRow.getByRole('cell').last().getByText('Forget this local remembered record.', { exact: false }).click();
    await expect(selectWheel).not.toBeChecked();
    await expect(confirmCleanup).toBeDisabled();
    await selectAll.check();
    await expect(selectWheel).toBeChecked(); await expect(selectThrottle).toBeChecked();
    await expect(confirmCleanup).toBeEnabled();
    await selectAll.uncheck();
    await expect(selectWheel).not.toBeChecked(); await expect(selectThrottle).not.toBeChecked();
    await expect(confirmCleanup).toBeDisabled();
    await selectAll.check(); await selectWheel.uncheck();
    await expect(selectAll).toBeChecked({ indeterminate: true });
    await expect(selectThrottle).toBeChecked(); await expect(confirmCleanup).toBeEnabled();
    const beforeCancel = requests.filter(request => request.command === 'controller_forget').length;
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    assert.equal(requests.filter(request => request.command === 'controller_forget').length, beforeCancel);
    await page.getByRole('button', { name: 'Cleanup (2)', exact: true }).click();
    await expect(selectAll).not.toBeChecked();
    await expect(selectWheel).not.toBeChecked(); await expect(selectThrottle).not.toBeChecked();
    await expect(confirmCleanup).toBeDisabled();
    await selectThrottle.check();
    fixture.failForget = 'remembered-throttle';
    await confirmCleanup.click();
    await expect(page.getByText(/Remembered throttle: Error: Fixture removal failed once/)).toBeVisible();
    assert.equal(requests.filter(request => request.command === 'controller_forget' && request.sourceId === 'remembered-wheel').length, 0);
    assert.equal(requests.filter(request => request.command === 'controller_forget' && request.sourceId === 'remembered-throttle').length, 1);
    // Failure is reported without replay. On retry a returning wheel cannot
    // be removed, and an unchanged remembered throttle stays suppressed.
    fixture.failForget = null; fixture.keepForgotten = true;
    await page.getByRole('button', { name: 'Cleanup (2)', exact: true }).click();
    await expect(confirmCleanup).toBeDisabled();
    await selectAll.check();
    await expect(selectWheel).toBeChecked(); await expect(selectThrottle).toBeChecked();
    fixture.controllers.find(source => source.id === 'remembered-wheel').online = true;
    await confirmCleanup.click();
    await expect(graph.locator('[data-component-id="controller:remembered-throttle"]')).toHaveCount(0);
    assert.equal(requests.filter(request => request.command === 'controller_forget' && request.sourceId === 'remembered-wheel').length, 0);
    assert.equal(requests.filter(request => request.command === 'controller_forget' && request.sourceId === 'remembered-throttle').length, 2);
    await filter.selectOption('all'); await search.fill(''); await list.click();
    // Mode survives remount and page reload; no inspection acquired ownership.
    await topology.click(); await page.reload(); await nav.getByRole('button', { name: /^Devices(?:\s|$)/ }).click();
    await expect(graph).toBeVisible(); await expect(search).toHaveValue('');
    await expect(graph.locator('[data-component-id="controller:remembered-throttle"]')).toHaveCount(0);
    await expect(graph.locator('[data-component-id="virtual:vjoy:4"]')).toHaveCount(0);
    fixture.controllers.find(source => source.id === 'remembered-throttle').online = true;
    fixture.native.virtualDevices.find(device => device.id === 'vjoy:4').available = true;
    await waitForInventoryRefresh();
    await expect(graph.locator('[data-component-id="controller:remembered-throttle"]')).toBeVisible();
    await expect(graph.locator('[data-component-id="virtual:vjoy:4"]')).toBeVisible();
    // Forgetting an unreachable gateway clears only its stale local routes.
    // Independently verified USB attachment remains; remote flash is untouched.
    fixture.ports = fixture.ports.filter(port => port.serialNumber !== serial);
    fixture.states.get(firstGateway).connected = false;
    await expect(graph.locator(`[data-component-id="gateway:${firstGateway}"]`)).toContainText('Offline', { timeout: 10_000 });
    await graph.locator(`[data-component-id="gateway:${firstGateway}"]`).click();
    await details.getByRole('button', { name: 'Forget device', exact: true }).click();
    await expect(dialog).toContainText('unavailable gateway cannot confirm');
    await expect(confirmCleanup).toBeDisabled();
    await dialog.getByRole('checkbox', { name: 'Select all', exact: true }).check();
    await confirmCleanup.click();
    await expect(graph.locator(`[data-component-id="gateway:${firstGateway}"]`)).toHaveCount(0);
    await expect(graph.locator(`[data-component-id="module:${firstGateway}/can-output"]`)).toHaveCount(0);
    await expect(graph.locator('[data-component-id="resource:verified-dual"]')).toBeVisible();
    await waitForInventoryRefresh();
    await expect(graph.locator(`[data-component-id="gateway:${firstGateway}"]`)).toHaveCount(0);
    await expect(graph.locator(`[data-component-id="module:${firstGateway}/can-output"]`)).toHaveCount(0);
    // Clearing external HidHide rules must retire stale hidden flags and permit local cleanup.
    fixture.native.visibilityInspected = true;
    fixture.native.hiddenDevices.push({ id: 'stale-hide', name: 'Stale HidHide device', instanceId: 'HID\\STALE', present: false, configured: true, hidden: true, diagnostics: [] });
    await waitForInventoryRefresh(); await list.click(); await search.fill('Stale HidHide');
    await devices.locator('[data-component-id="visibility:stale-hide"]').click();
    await expect(details.getByRole('button', { name: 'Forget device', exact: true })).toBeDisabled();
    await details.getByRole('button', { name: 'Manage HidHide', exact: true }).click(); assert.equal(hidHideOpens,2);
    fixture.native.hiddenDevices = fixture.native.hiddenDevices.filter(device => device.id !== 'stale-hide');
    await expect(details.getByRole('button', { name: 'Forget device', exact: true })).toBeEnabled({ timeout: 10_000 });
    await expect(details).toContainText('No configured HidHide rule');
    await details.getByRole('button', { name: 'Forget device', exact: true }).click();
    await dialog.getByRole('checkbox', { name: 'Select Stale HidHide device', exact: true }).check();
    await confirmCleanup.click();
    await expect(devices.locator('[data-component-id="visibility:stale-hide"]')).toHaveCount(0);
    await search.fill(''); await topology.click();
    const prohibited = /^(?:profile_apply|profile_game_launch|profile_editor_save|module_usb_connect|module_usb_claim|module_usb_config_apply|controller_visibility_apply|companion_usb_arm|companion_assign|companion_approve|companion_revoke|claim|register|apply|save|configure)$/;
    assert.deepEqual(requests.filter(request => prohibited.test(request.command)), []);
    assert.deepEqual(unexpected, []); assert.deepEqual(pageErrors, []);
    await page.screenshot({ path: resolve(artifacts, 'complete-system-topology.png'), fullPage: true });
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ observedAt: new Date().toISOString(), scope: 'Running dev Cockpit frontend with fixture IPC only', endpoint,
      expectedNames, listIds, graphIds, commands: requests.map(request => request.command), mouseNavigation, suppressedHmrMessages, unexpected, pageErrors }, null, 2) + '\n');
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true });
    await writeFile(resolve(artifacts, 'failure.json'), JSON.stringify({ message: String(error), requests, suppressedHmrMessages, unexpected, pageErrors }, null, 2) + '\n');
    throw error;
  } finally { await browser.close(); }
});
