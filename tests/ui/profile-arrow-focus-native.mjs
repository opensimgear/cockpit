import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';

const require = createRequire(import.meta.url), module = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(module), { expect } = require(module + '/test');

test('native arrow selection focuses connected routes and restores the original canvas view', { timeout: 180_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP ?? 'http://127.0.0.1:9222');
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev cockpit.');
  const request = value => page.evaluate(async request => (await import('/src/lib/gateway.ts')).gatewayRequest(request), value);
  const name = `Arrow focus ${randomUUID()}`;
  let profileId;
  const save = page.locator('.editor-save-actions').getByRole('button', { name: 'Save', exact: true });
  const stage = page.locator('.canvas-stage');
  const world = page.locator('.canvas-world');
  const overview = page.locator('.canvas-lanes-panel');
  const viewport = () => world.evaluate(element => {
    const matrix = new DOMMatrixReadOnly(getComputedStyle(element).transform);
    return { x: matrix.e, y: matrix.f, zoom: matrix.a };
  });
  const settleViewport = () => stage.evaluate(element => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const openArrow = async (bundleId, mouse = false) => {
    const arrow = page.locator(`.signal-edge-hit:not(.overview-merge-hit)[data-connection-id='${bundleId}']`);
    await expect(arrow).toBeVisible();
    if (mouse) {
      const point = await arrow.evaluate(path => {
        const rect = document.querySelector('.canvas-stage').getBoundingClientRect(), matrix = path.getScreenCTM();
        for (const fraction of [.5, .25, .75, .1, .9]) {
          const point = path.getPointAtLength(path.getTotalLength() * fraction), screen = new DOMPoint(point.x, point.y).matrixTransform(matrix);
          if (screen.x > rect.left && screen.x < rect.right && screen.y > rect.top && screen.y < rect.bottom && document.elementFromPoint(screen.x, screen.y) === path) return { x: screen.x, y: screen.y };
        }
        return null;
      });
      assert.ok(point, 'A source arrow must have an actual mouse hit target.');
      await page.mouse.click(point.x, point.y);
    } else {
      await arrow.focus();
      await arrow.press('Enter');
    }
    await expect(overview).toBeVisible();
    await settleViewport();
  };
  const assertRouteFitsAboveLanes = async (nodeIds, bindingIds, panelTop) => {
    const stageRect = await stage.boundingBox();
    const bounds = await page.evaluate(({ nodeIds, bindingIds }) => {
      const related = [...document.querySelectorAll('.resource-block.route-related')].filter(node => nodeIds.includes(node.dataset.nodeId));
      const blockBounds = related.map(node => {
        const rect = node.getBoundingClientRect();
        return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, id: node.dataset.nodeId };
      });
      const routeBounds = [...document.querySelectorAll('.overview-edge,.overview-merge')].filter(path => bindingIds.includes(path.dataset.bindingId) || nodeIds.includes(path.dataset.targetNode)).map(path => {
        const box = path.getBBox(), matrix = path.getScreenCTM();
        if (!matrix) throw Error('A selected route has no screen transform.');
        const points = [
          new DOMPoint(box.x, box.y).matrixTransform(matrix),
          new DOMPoint(box.x + box.width, box.y).matrixTransform(matrix),
          new DOMPoint(box.x, box.y + box.height).matrixTransform(matrix),
          new DOMPoint(box.x + box.width, box.y + box.height).matrixTransform(matrix),
        ];
        return { left: Math.min(...points.map(point => point.x)), top: Math.min(...points.map(point => point.y)), right: Math.max(...points.map(point => point.x)), bottom: Math.max(...points.map(point => point.y)) };
      });
      return { blockBounds, routeBounds };
    }, { nodeIds, bindingIds });
    assert.ok(nodeIds.every(id => bounds.blockBounds.some(block => block.id === id)), 'Every connected source and destination is marked route-related.');
    for (const item of [...bounds.blockBounds, ...bounds.routeBounds]) {
      assert.ok(item.left >= stageRect.x && item.right <= stageRect.x + stageRect.width, 'Focused geometry stays inside the stage horizontally.');
      assert.ok(item.top >= stageRect.y, 'Focused geometry stays below the top of the stage.');
      assert.ok(item.bottom <= panelTop - 2, `Focused geometry ends above the lanes panel (${item.bottom.toFixed(1)}px <= ${panelTop.toFixed(1)}px).`);
    }
    assert.ok(bounds.routeBounds.length >= 1, 'At least one route segment is included in the focused geometry.');
  };

  try {
    if (await save.count() && await save.isEnabled()) {
      await page.getByRole('button', { name: 'Back to Profiles', exact: true }).click();
      await page.getByRole('button', { name: 'Save and continue', exact: true }).click();
      await expect(save).toHaveCount(0);
    }

    const fixture = await page.evaluate(async name => {
      const { gatewayRequest } = await import('/src/lib/gateway.ts');
      const { blankProfile, addProfileResources } = await import('/src/lib/profile-editor.ts');
      const { graphSelectProviderPort, graphSetVirtual, graphConnect } = await import('/src/lib/profile-graph.ts');
      const inventory = (await gatewayRequest({ command: 'profile_resources_list', offset: 0, limit: 64 })).profileResources.resources;
      const selected = [];
      for (const source of inventory.filter(resource => resource.resourceType === 'host-controller' && resource.availability === 'online')) {
        const detail = (await gatewayRequest({ command: 'profile_resources_inspect', resourceId: source.id, offset: 0, limit: 64 })).profileResources;
        const ports = [...detail.ports];
        while (ports.length < detail.total) {
          const next = (await gatewayRequest({ command: 'profile_resources_inspect', resourceId: source.id, offset: ports.length, limit: 64 })).profileResources;
          if (!next.ports.length) throw Error('The controller catalogue ended before its total.');
          ports.push(...next.ports);
        }
        const button = ports.find(port => port.descriptor.direction === 'output' && port.descriptor.contract.delivery === 'state' && port.descriptor.contract.schema.type === 'boolean');
        if (button) selected.push({ source, button });
        if (selected.length === 2) break;
      }
      if (selected.length < 2 || selected[0].source.id === selected[1].source.id) throw Error('Two distinct online physical controllers with Boolean outputs are required.');
      let draft = addProfileResources(blankProfile(crypto.randomUUID(), name), [], selected.map(item => item.source));
      const sourceNodes = selected.map(item => draft.document.resources.find(resource => resource.id === draft.assignments.find(row => row.sourceId === item.source.id).resourceId));
      for (let index = 0; index < selected.length; index++) draft = graphSelectProviderPort(draft, sourceNodes[index].id, selected[index].source.id, selected[index].button);
      for (const [id, outputName, buttons] of [
        ['virtual-primary', 'Primary output', ['button-1', 'button-2']],
        ['virtual-secondary', 'Secondary output', ['button-1']],
        ['virtual-unrelated', 'Unrelated output', ['button-1']],
      ]) draft = graphSetVirtual(draft, { id, name: outputName, required: false, layout: { buttons, axes: [], hats: [] } });
      draft.document.editor.positions = [
        { nodeId: sourceNodes[0].id, x: -800, y: 180 },
        { nodeId: sourceNodes[1].id, x: -520, y: 760 },
        { nodeId: 'virtual-primary', x: 350, y: 430 },
        { nodeId: 'virtual-secondary', x: 1420, y: 700 },
        { nodeId: 'virtual-unrelated', x: 3050, y: 2200 },
      ];
      const sourcePorts = sourceNodes.map(node => draft.document.resources.find(row => row.id === node.id).ports[0].id);
      draft = graphConnect(draft, { nodeId: sourceNodes[0].id, portId: sourcePorts[0] }, { nodeId: 'virtual-primary', portId: 'button-1' });
      draft = graphConnect(draft, { nodeId: sourceNodes[1].id, portId: sourcePorts[1] }, { nodeId: 'virtual-primary', portId: 'button-2' });
      draft = graphConnect(draft, { nodeId: sourceNodes[0].id, portId: sourcePorts[0] }, { nodeId: 'virtual-secondary', portId: 'button-1' });
      const { signalOverview } = await import('/src/lib/profile-signal-overview.ts');
      const arrows = signalOverview(draft.document);
      const primaryArrowIds = arrows.filter(bundle => bundle.binding.to.nodeId === 'virtual-primary').map(bundle => bundle.id);
      const secondaryArrowId = arrows.find(bundle => bundle.binding.to.nodeId === 'virtual-secondary')?.id;
      if (primaryArrowIds.length !== 2 || !secondaryArrowId) throw Error('The overview fixture did not create two source-pair arrows and one other destination arrow.');
      await gatewayRequest({ command: 'profile_editor_save', ...draft, expectedRevision: null });
      return { id: draft.document.id, sourceIds: sourceNodes.map(node => node.id), primaryArrowIds, secondaryArrowId };
    }, name);
    profileId = fixture.id;

    await page.reload();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Profiles', exact: true }).click();
    const back = page.getByRole('button', { name: 'Back to Profiles', exact: true });
    const edit = page.getByRole('button', { name: `Edit ${name}`, exact: true });
    await expect.poll(async () => await back.isVisible() || await edit.isVisible()).toBe(true);
    if (await back.isVisible()) await back.click();
    await edit.click();
    await expect(page.locator('.signal-edge-hit:not(.overview-merge-hit)')).toHaveCount(3);
    await expect(save).toBeDisabled();

    // Establish a deliberate starting camera so selection must visibly focus the route.
    for (let index = 0; index < 3; index++) await page.getByRole('button', { name: 'Zoom out', exact: true }).click();
    const original = await viewport();
    await openArrow(fixture.primaryArrowIds[0], true);
    const focused = await viewport();
    assert.notDeepEqual(focused, original, 'Selecting an arrow changes the canvas view.');
    const lanesTop = (await overview.boundingBox()).y;
    await assertRouteFitsAboveLanes([...fixture.sourceIds, 'virtual-primary'], fixture.primaryArrowIds, lanesTop);
    await expect(save).toBeDisabled();

    // A second source arrow to the same destination preserves a user camera adjustment.
    await page.getByRole('button', { name: 'Zoom in', exact: true }).click();
    const manual = await viewport();
    await openArrow(fixture.primaryArrowIds[1]);
    assert.deepEqual(await viewport(), manual, 'Switching to another arrow with the same destination keeps the manual camera.');
    await expect(save).toBeDisabled();

    // A different destination reframes, while the original pre-selection view remains the restore target.
    await openArrow(fixture.secondaryArrowId);
    assert.notDeepEqual(await viewport(), manual, 'Selecting another destination frames its route.');
    await expect(save).toBeDisabled();
    await openArrow(fixture.primaryArrowIds[0]);
    await assertRouteFitsAboveLanes([...fixture.sourceIds, 'virtual-primary'], fixture.primaryArrowIds, (await overview.boundingBox()).y);
    await page.keyboard.press('Escape');
    await expect(overview).toHaveCount(0);
    assert.deepEqual(await viewport(), original, 'Escape restores the exact original canvas transform.');
    await expect(save).toBeDisabled();

    // Selecting and dismissing through blank canvas uses the same restoration path.
    await openArrow(fixture.secondaryArrowId);
    await stage.click({ position: { x: 12, y: 12 } });
    await expect(overview).toHaveCount(0);
    assert.deepEqual(await viewport(), original, 'Dismissing on the blank canvas restores the exact original transform.');
  } finally {
    if (profileId) {
      const state = (await request({ command: 'profile_get', id: profileId })).profiles.document;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: state.revision });
      await page.reload();
    }
    await browser.close();
  }
});
