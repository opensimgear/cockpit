import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';

const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');

test('native destination overview shares source lanes across arrows and keeps AND fan-in in one lane', { timeout: 180_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP ?? 'http://127.0.0.1:9222');
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev cockpit.');
  const request = value => page.evaluate(async request => (await import('/src/lib/gateway.ts')).gatewayRequest(request), value);
  const name = `Destination overview ${randomUUID()}`;
  let profileId;
  const saved = async () => (await request({ command: 'profile_assignments_get', id: profileId })).profiles;
  const save = async () => {
    const button = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await button.isEnabled()) await button.click();
    await expect(button).toBeDisabled();
    return saved();
  };
  const rows = page.locator('.signal-lane:not(.pending-connection)');
  const overview = page.getByRole('region', { name: 'Signal lanes', exact: true });
  const openArrow = async index => {
    const arrow = page.locator('.signal-edge-hit').nth(index);
    await expect(arrow).toBeVisible();
    await arrow.focus();
    await arrow.press('Enter');
    await expect(overview).toBeVisible();
  };
  const choose = async (label, value) => {
    const field = overview.getByRole('combobox', { name: label, exact: true });
    await field.fill(value);
    await page.locator('button[role="option"]:visible:not([data-value=""])').first().click();
  };

  try {
    const current = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
    if (await current.count() && await current.isEnabled()) {
      await page.getByRole('button', { name: 'Back to Profiles', exact: true }).click();
      await page.getByRole('button', { name: 'Save and continue', exact: true }).click();
      await expect(current).toHaveCount(0);
    }
    const fixture = await page.evaluate(async name => {
      const { gatewayRequest } = await import('/src/lib/gateway.ts');
      const { blankProfile, addProfileResources } = await import('/src/lib/profile-editor.ts');
      const { graphSelectProviderPort, graphSetVirtual, graphConnect } = await import('/src/lib/profile-graph.ts');
      const inventory = (await gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 64 })).profileResources.resources;
      const sources = inventory.filter(resource => resource.resourceType === 'host-controller' && resource.availability === 'online');
      const selected = [];
      for (const source of sources) {
        const detail = (await gatewayRequest({ command: 'profile_resources_inspect', resourceId: source.id, offset: 0, limit: 64 })).profileResources;
        const button = detail.ports.find(port => port.descriptor.direction === 'output' && port.descriptor.contract.delivery === 'state' && port.descriptor.contract.schema.type === 'boolean');
        if (button) selected.push({ source, button });
        if (selected.length === 2) break;
      }
      if (selected.length < 2 || selected[0].source.id === selected[1].source.id) throw Error('Two distinct online physical controllers with Boolean outputs are required.');
      let draft = addProfileResources(blankProfile(crypto.randomUUID(), name), [], selected.map(item => item.source));
      const sourceNodes = selected.map(item => {
        const assignment = draft.assignments.find(row => row.sourceId === item.source.id);
        return draft.document.resources.find(resource => resource.id === assignment.resourceId);
      });
      for (let index = 0; index < selected.length; index++) {
        draft = graphSelectProviderPort(draft, sourceNodes[index].id, selected[index].source.id, selected[index].button);
      }
      draft = graphSetVirtual(draft, { id: 'virtual-target', name: 'Shared virtual output', required: false, layout: { buttons: ['button-1', 'button-2'], axes: [], hats: [] } });
      draft.document.editor.positions = [
        { nodeId: sourceNodes[0].id, x: 0, y: 80 },
        { nodeId: sourceNodes[1].id, x: 0, y: 360 },
        { nodeId: 'virtual-target', x: 510, y: 190 },
      ];
      draft = graphConnect(draft, { nodeId: sourceNodes[0].id, portId: draft.document.resources.find(row => row.id === sourceNodes[0].id).ports[0].id }, { nodeId: 'virtual-target', portId: 'button-1' });
      draft = graphConnect(draft, { nodeId: sourceNodes[1].id, portId: draft.document.resources.find(row => row.id === sourceNodes[1].id).ports[0].id }, { nodeId: 'virtual-target', portId: 'button-2' });
      await gatewayRequest({ command: 'profile_editor_save', ...draft, expectedRevision: null });
      return { id: draft.document.id, sources: selected.map(item => item.source.name), sourceIds: sourceNodes.map(node => node.id), signals: selected.map(item => item.button.descriptor.id) };
    }, name);
    profileId = fixture.id;
    await page.reload();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Profiles', exact: true }).click();
    const back = page.getByRole('button', { name: 'Back to Profiles', exact: true });
    const edit = page.getByRole('button', { name: `Edit ${name}`, exact: true });
    await expect.poll(async () => await back.isVisible() || await edit.isVisible()).toBe(true);
    if (await back.isVisible()) await back.click();
    await edit.click();

    for (const arrowIndex of [0, 1]) {
      await openArrow(arrowIndex);
      await expect(rows).toHaveCount(2);
      const header = overview.locator('header .source-device');
      await expect(header).toContainText(fixture.sources[0]);
      await expect(header).toContainText(fixture.sources[1]);
      await page.keyboard.press('Escape');
      await expect(overview).toHaveCount(0);
    }

    // Add a Boolean AND to the first existing destination route and select
    // the other controller as its second source. The existing second route
    // remains on button-2, so no destination input receives a second writer.
    await openArrow(0);
    const laneIds = await rows.evaluateAll(items => items.map(item => item.dataset.laneId));
    await rows.first().locator('.insert-transform').click();
    const settings = overview.locator('.lane-settings');
    const types = settings.getByRole('combobox', { name: 'Transformation type', exact: true });
    await expect(types).toBeVisible();
    await types.selectOption('all');
    const incomplete = overview.getByLabel('Incomplete connection', { exact: true });
    const secondInput = incomplete.getByRole('combobox', { name: 'Source for Boolean AND second', exact: true });
    await expect(secondInput).toBeVisible();
    await choose('Source for Boolean AND second', fixture.sources[1]);
    await expect(incomplete).toHaveCount(0);
    await expect(rows).toHaveCount(2);
    assert.deepEqual(await rows.evaluateAll(items => items.map(item => item.dataset.laneId)), laneIds, 'AND fan-in stays within the existing destination lane.');

    const withAnd = await save();
    const andNode = withAnd.document.processingNodes.find(node => node.operation.kind === 'all');
    assert.ok(andNode, 'The route persists a Boolean AND node.');
    assert.equal(andNode.inputs.length, 2);
    assert.equal(withAnd.document.bindings.filter(binding => binding.to.nodeId === andNode.id).length, 2);
    const andSourceIds = withAnd.document.bindings.filter(binding => binding.to.nodeId === andNode.id).map(binding => binding.from.nodeId);
    assert.deepEqual([...new Set(andSourceIds)].sort(), [...fixture.sourceIds].sort(), 'Both physical controller nodes feed the AND.');
    assert.equal(withAnd.document.bindings.length, 4, 'The two-input route and the independent button-2 route are saved once each.');
    assert.equal(withAnd.document.virtualOutputs[0].ports.length, 2);

    await page.reload();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Profiles', exact: true }).click();
    await expect.poll(async () => await back.isVisible() || await edit.isVisible()).toBe(true);
    if (await back.isVisible()) await back.click();
    await edit.click();
    for (const arrowIndex of [0, 1]) {
      await openArrow(arrowIndex);
      await expect(rows).toHaveCount(2);
      assert.deepEqual(await rows.evaluateAll(items => items.map(item => item.dataset.laneId)), laneIds);
      const header = overview.locator('header .source-device');
      await expect(header).toContainText(fixture.sources[0]);
      await expect(header).toContainText(fixture.sources[1]);
      await page.keyboard.press('Escape');
    }
    const reloaded = await saved();
    assert.deepEqual(reloaded.document.processingNodes, withAnd.document.processingNodes);
    assert.deepEqual(reloaded.document.bindings, withAnd.document.bindings);
  } finally {
    if (profileId) {
      const state = (await request({ command: 'profile_get', id: profileId })).profiles.document;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: state.revision });
      await page.reload();
    }
    await browser.close();
  }
});
