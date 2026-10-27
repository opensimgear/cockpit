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
  const assertSharedInputEndpoints = async () => {
    const geometry = await page.locator('.overview-edge').evaluateAll(paths => {
      const target = document.querySelector('.resource-block[data-node-id="virtual-target"] [data-connection-target] .port-dot');
      if (!target) throw Error('The shared destination Inputs/actions port dot is missing.');
      const rect = target.getBoundingClientRect(), center = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
      const branches = paths.map(path => {
        const length = path.getTotalLength(), point = path.getPointAtLength(length), matrix = path.getScreenCTM();
        if (!matrix) throw Error('An overview arrow has no screen transform.');
        const junction = JSON.parse(path.dataset.junction ?? 'null');
        return {
          binding: path.dataset.bindingId,
          mergeId: path.dataset.mergeId,
          junction,
          end: { x: point.x, y: point.y },
          hasMarker: !!path.getAttribute('marker-end') && path.getAttribute('marker-end') !== 'none',
        };
      });
      const trunks = [...document.querySelectorAll('.overview-merge')].map(path => {
        const length = path.getTotalLength(), end = path.getPointAtLength(length), before = path.getPointAtLength(Math.max(0, length - 80)), start = path.getPointAtLength(0), matrix = path.getScreenCTM();
        if (!matrix) throw Error('The overview merge stem has no screen transform.');
        return {
          id: path.dataset.mergeId,
          targetNode: path.dataset.targetNode,
          routeCount: Number(path.dataset.routeCount),
          hasMarker: !!path.getAttribute('marker-end') && path.getAttribute('marker-end') !== 'none',
          start: { x: start.x, y: start.y },
          beforeEnd: { x: before.x, y: before.y },
          worldEnd: { x: end.x, y: end.y },
          end: { x: matrix.a * end.x + matrix.c * end.y + matrix.e, y: matrix.b * end.x + matrix.d * end.y + matrix.f },
        };
      });
      return { branches, trunks, target: center };
    });
    assert.equal(geometry.branches.length, 2, 'Both source branches enter the merge.');
    assert.equal(geometry.trunks.length, 1, 'The shared destination has one final arrow stem.');
    const [trunk] = geometry.trunks;
    assert.ok(trunk.id, 'The final stem exposes its merge id.');
    assert.equal(trunk.targetNode, 'virtual-target');
    assert.ok(trunk.routeCount >= 2, 'The final stem represents both incoming routes.');
    assert.ok(trunk.hasMarker, 'The shared final stem carries the arrowhead.');
    assert.ok(Math.abs(trunk.worldEnd.x - trunk.beforeEnd.x) >= 79 && Math.abs(trunk.worldEnd.y - trunk.beforeEnd.y) <= 1.5,
      'The roomy fixture has at least 80 canvas units of horizontal final stem.');
    assert.ok(Math.hypot(trunk.end.x - geometry.target.x, trunk.end.y - geometry.target.y) <= 1.5,
      `Merge stem ends at (${trunk.end.x.toFixed(1)}, ${trunk.end.y.toFixed(1)}), away from the shared input dot (${geometry.target.x.toFixed(1)}, ${geometry.target.y.toFixed(1)}).`);
    for (const branch of geometry.branches) {
      assert.ok(branch.mergeId, `Branch ${branch.binding} exposes its merge id.`);
      assert.equal(branch.mergeId, trunk.id, `Branch ${branch.binding} joins the shared stem.`);
      assert.equal(branch.hasMarker, false, `Branch ${branch.binding} has no arrowhead.`);
      assert.ok(branch.junction && Number.isFinite(branch.junction.x) && Number.isFinite(branch.junction.y),
        `Branch ${branch.binding} exposes its junction coordinates.`);
      assert.ok(Math.hypot(branch.end.x - branch.junction.x, branch.end.y - branch.junction.y) <= 1.5,
        `Branch ${branch.binding} does not end at its declared merge junction.`);
      assert.ok(Math.hypot(trunk.start.x - branch.junction.x, trunk.start.y - branch.junction.y) <= 1.5,
        `Stem ${trunk.id} does not start at branch ${branch.binding}'s junction.`);
    }
  };
  const openArrow = async index => {
    const arrow = page.locator('.signal-edge-hit').nth(index);
    await expect(arrow).toBeVisible();
    await arrow.focus();
    await arrow.press('Enter');
    await expect(overview).toBeVisible();
  };
  const openMerge = async () => {
    const merge = page.locator('.overview-merge-hit');
    await expect(merge).toHaveCount(1);
    await merge.focus();
    await merge.press('Enter');
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
    await expect(page.locator('.overview-edge')).toHaveCount(2);
    await assertSharedInputEndpoints();

    for (const arrowIndex of [0, 1]) {
      await openArrow(arrowIndex);
      await expect(rows).toHaveCount(2);
      const header = overview.locator('header .source-device');
      await expect(header).toContainText(fixture.sources[0]);
      await expect(header).toContainText(fixture.sources[1]);
      await page.keyboard.press('Escape');
      await expect(overview).toHaveCount(0);
    }
    await openMerge();
    await expect(rows).toHaveCount(2);
    await expect(overview.locator('header .source-device')).toContainText(fixture.sources[0]);
    await expect(overview.locator('header .source-device')).toContainText(fixture.sources[1]);
    await page.keyboard.press('Escape');

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

    await expect.poll(() => page.locator('.overview-edge').evaluateAll(paths => paths.map(path => Number(path.dataset.routeCount)).sort())).toEqual([1, 2]);
    await assertSharedInputEndpoints();
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
    await assertSharedInputEndpoints();
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
