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
  await page.emulateMedia({ reducedMotion: 'no-preference' });
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
  const settleViewport = async () => {
    await stage.evaluate(element => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await expect(world).toHaveAttribute('data-camera-animating', 'false');
    await stage.evaluate(element => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  };
  const sampleCameraFrames = count => page.evaluate(async count => {
    const world = document.querySelector('.canvas-world');
    const sample = () => { const matrix = new DOMMatrixReadOnly(getComputedStyle(world).transform); return { x: matrix.e, y: matrix.f, zoom: matrix.a, animating: world.dataset.cameraAnimating }; };
    const values = [];
    for (let index = 0; index < count; index++) { await new Promise(resolve => requestAnimationFrame(resolve)); values.push(sample()); }
    return values;
  }, count);
  const cameraAnimation = () => page.evaluate(() => new Promise(resolve => {
    const world = document.querySelector('.canvas-world'), started = performance.now(), samples = [];
    let observed = false, settledFrames = 0;
    const frame = () => {
      const matrix = new DOMMatrixReadOnly(getComputedStyle(world).transform), active = world.dataset.cameraAnimating === 'true';
      const view = { x: matrix.e, y: matrix.f, zoom: matrix.a };
      if (active) { observed = true; settledFrames = 0; samples.push(view); }
      else if (observed) { samples.push(view); if (++settledFrames >= 2) { resolve({ observed, samples, final: view }); return; } }
      if (performance.now() - started > 5000) { resolve({ observed, samples, final: view }); return; }
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }));
  const sameViewport = (left, right) => Math.abs(left.x-right.x)<.05 && Math.abs(left.y-right.y)<.05 && Math.abs(left.zoom-right.zoom)<.0001;
  const assertAnimated = (result, start, label) => {
    assert.ok(result.observed, `${label} sets the camera animation state.`);
    assert.ok(result.samples.some(value => !sameViewport(value, start)), `${label} moves the rendered camera.`);
    assert.ok(result.samples.some(value => !sameViewport(value, start) && !sameViewport(value, result.final)), `${label} exposes an intermediate rendered camera frame.`);
  };
  const openArrow = async (bundleId, { mouse = false, settle = true } = {}) => {
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
    if (settle) await settleViewport();
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

    // Toolbar zoom/fit and keyboard pan use the same rendered camera animation.
    const initial = await viewport(), zoomOut = page.getByRole('button', { name: 'Zoom out', exact: true });
    let animation = cameraAnimation();
    await zoomOut.click();
    let transition = await animation;
    assertAnimated(transition, initial, 'Toolbar zoom');
    await settleViewport();
    const zoomed = await viewport();
    animation = cameraAnimation();
    await page.getByRole('button', { name: 'Fit all', exact: true }).click();
    transition = await animation;
    assertAnimated(transition, zoomed, 'Fit all');
    await settleViewport();
    const fitted = await viewport();
    animation = cameraAnimation();
    await stage.press('ArrowRight');
    transition = await animation;
    assertAnimated(transition, fitted, 'Keyboard pan');
    await settleViewport();
    const panned = await viewport();
    animation = cameraAnimation();
    await stage.press('ArrowLeft');
    transition = await animation;
    assertAnimated(transition, panned, 'Reverse keyboard pan');
    await settleViewport();
    assert.deepEqual(await viewport(), fitted, 'Opposing keyboard pans return to the fitted viewport.');
    for (let index = 0; index < 3; index++) { await zoomOut.click(); await settleViewport(); }
    const original = await viewport();
    const arrowAnimation = cameraAnimation();
    await openArrow(fixture.primaryArrowIds[0], { mouse: true, settle: false });
    const arrowTransition = await arrowAnimation;
    assertAnimated(arrowTransition, original, 'Arrow selection');
    await settleViewport();
    const focused = await viewport();
    assert.notDeepEqual(focused, original, 'Selecting an arrow changes the canvas view.');
    const lanesTop = (await overview.boundingBox()).y;
    await assertRouteFitsAboveLanes([...fixture.sourceIds, 'virtual-primary'], fixture.primaryArrowIds, lanesTop);
    await expect(save).toBeDisabled();

    // A second source arrow to the same destination preserves a user camera adjustment.
    const beforeManualZoom = await viewport();
    animation = cameraAnimation();
    await page.getByRole('button', { name: 'Zoom in', exact: true }).click();
    transition = await animation;
    assertAnimated(transition, beforeManualZoom, 'Manual toolbar zoom while an arrow is focused');
    await settleViewport();
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
    const arrowRestoreStart = await viewport(), arrowRestoreAnimation = cameraAnimation();
    await page.keyboard.press('Escape');
    await expect(overview).toHaveCount(0);
    const arrowRestoreTransition = await arrowRestoreAnimation;
    assertAnimated(arrowRestoreTransition, arrowRestoreStart, 'Arrow overview dismissal');
    await settleViewport();
    assert.deepEqual(await viewport(), original, 'Escape restores the exact original canvas transform.');
    await expect(save).toBeDisabled();

    // Selecting a physical block animates its own focus and Escape restores the same baseline.
    const block = page.locator(`.resource-block[data-node-id="${fixture.sourceIds[1]}"] .block-heading`);
    const blockAnimation = cameraAnimation();
    await block.focus();
    await block.press('Enter');
    const blockTransition = await blockAnimation;
    assertAnimated(blockTransition, original, 'Physical block selection');
    await settleViewport();
    assert.notDeepEqual(await viewport(), original, 'Selecting a connected controller focuses its block.');
    const blockRestoreStart = await viewport(), blockRestoreAnimation = cameraAnimation();
    await page.keyboard.press('Escape');
    const blockRestoreTransition = await blockRestoreAnimation;
    assertAnimated(blockRestoreTransition, blockRestoreStart, 'Physical block dismissal');
    await settleViewport();
    assert.deepEqual(await viewport(), original, 'Escape from block selection restores the exact original transform.');
    await expect(save).toBeDisabled();

    // Reduced motion snaps to the same focused transform, then snaps back without an intermediate camera frame.
    await page.emulateMedia({ reducedMotion: 'reduce' });
    const reducedStart = await viewport();
    await openArrow(fixture.primaryArrowIds[0], { settle: false });
    const reducedFocusFrames = await sampleCameraFrames(3);
    assert.ok(reducedFocusFrames.every(frame => frame.animating === 'false'), 'Reduced-motion arrow focus never starts a camera animation.');
    assert.ok(reducedFocusFrames.every(frame => sameViewport(frame, focused)), 'Reduced-motion arrow focus lands directly on the same target transform.');
    assert.notDeepEqual(reducedStart, focused, 'Reduced-motion selection still applies the focus target.');
    await page.keyboard.press('Escape');
    await expect(overview).toHaveCount(0);
    await settleViewport();
    const reducedRestoreFrames = await sampleCameraFrames(3);
    assert.ok(reducedRestoreFrames.every(frame => frame.animating === 'false' && sameViewport(frame, original)), 'Reduced-motion restoration snaps directly to the original transform.');
    await page.emulateMedia({ reducedMotion: 'no-preference' });

    // Selecting and dismissing through blank canvas uses the same restoration path.
    await openArrow(fixture.secondaryArrowId);
    await stage.click({ position: { x: 12, y: 12 } });
    await expect(overview).toHaveCount(0);
    await settleViewport();
    assert.deepEqual(await viewport(), original, 'Dismissing on the blank canvas restores the exact original transform.');

    // A background pointer press interrupts a camera transition at its rendered frame;
    // subsequent dragging pans from that frame without the canceled animation resuming.
    const blankPoint = await page.evaluate(() => {
      const stage = document.querySelector('.canvas-stage'), rect = stage.getBoundingClientRect();
      for (const [dx, dy] of [[12, 12], [12, 32], [32, 12], [44, 18]]) {
        const x = rect.left + dx, y = rect.top + dy, hit = document.elementFromPoint(x, y);
        if (hit && stage.contains(hit) && !hit.closest('.resource-block,svg,button,input,select,a,.canvas-lanes-panel')) return { x, y };
      }
      return null;
    });
    assert.ok(blankPoint, 'The stage top-left has a blank background point for the interruption drag.');
    await page.getByRole('button', { name: 'Zoom in', exact: true }).click();
    await expect(world).toHaveAttribute('data-camera-animating', 'true');
    let pointerHeld = false;
    try {
      await page.mouse.move(blankPoint.x, blankPoint.y, { steps: 1 });
      await expect(world).toHaveAttribute('data-camera-animating', 'true');
      await page.mouse.down();
      pointerHeld = true;
      await expect(world).toHaveAttribute('data-camera-animating', 'false');
      const interrupted = await viewport(), heldFrames = await sampleCameraFrames(4);
      assert.ok(heldFrames.every(frame => frame.animating === 'false' && sameViewport(frame, interrupted)), 'The interrupted camera remains fixed while the pointer is held.');
      await page.mouse.move(blankPoint.x + 20, blankPoint.y + 10, { steps: 1 });
      await sampleCameraFrames(2);
      const dragged = await viewport();
      assert.ok(Math.abs(dragged.x - interrupted.x - 20) < .1, 'Background drag pans exactly 20 screen pixels horizontally from the interrupted frame.');
      assert.ok(Math.abs(dragged.y - interrupted.y - 10) < .1, 'Background drag pans exactly 10 screen pixels vertically from the interrupted frame.');
      assert.ok(Math.abs(dragged.zoom - interrupted.zoom) < .0001, 'Background drag preserves the interrupted zoom.');
      await page.mouse.up();
      pointerHeld = false;
      const afterRelease = await sampleCameraFrames(4);
      assert.ok(afterRelease.every(frame => frame.animating === 'false' && sameViewport(frame, dragged)), 'The canceled animation does not resume after pointer release.');
      await expect(save).toBeDisabled();
    } finally {
      if (pointerHeld) await page.mouse.up();
    }
  } finally {
    await page.emulateMedia({ reducedMotion: null });
    if (profileId) {
      const state = (await request({ command: 'profile_get', id: profileId })).profiles.document;
      await request({ command: 'profile_delete', id: profileId, expectedRevision: state.revision });
      await page.reload();
    }
    await browser.close();
  }
});
