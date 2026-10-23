import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createCompiledPanel, verifyCompiledPanelPublication, validateCompiledPanelDocument,
  compiledPanelResources, compiledPanelCapabilityProjection, panelBehaviorSemantics,
  panelControlValue, panelFeedbackEvents, bundleForWidget,
} from '../src/panel.ts';
import { BROWSER_WIDGET_CAPABILITIES, canonicalWidget, widgetDigest } from '../src/artifact.ts';
import { compileWidget } from '../../widget-compiler/src/compiler.ts';
import { createToggleSource, createLinearGaugeFixture, createThreeStateFixture } from '../../widget-compiler/src/seeds.ts';

const viewport = { pageId: 'page', title: 'Page', width: 640, height: 400 };
let toggleBundle, togglePanel;
async function fixture() {
  toggleBundle ??= await compileWidget(createToggleSource(), { width: 128, height: 64, motion: false, audio: false });
  togglePanel ??= await createCompiledPanel({ id: 'test-panel', title: 'Test', viewports: [viewport] }, [
    { id: 'switch', pageId: 'page', label: 'Toggle', bounds: { x: .1, y: .1, width: .2, height: .16 }, bundle: toggleBundle },
  ]);
  return structuredClone(togglePanel);
}
async function resign(publication) {
  publication.publicationHash = 'sha256:' + await widgetDigest(new TextEncoder().encode(canonicalWidget(publication.document)));
  publication.capabilityFingerprint = 'sha256:' + await widgetDigest(new TextEncoder().encode(canonicalWidget(compiledPanelCapabilityProjection(publication.document))));
  return publication;
}
test('compiled panel is source-free, deduplicates content and verifies an entire closure before activation', async () => {
  const panel = await fixture();
  assert.equal(panel.document.schemaVersion, 4);
  assert.equal(panel.document.presentation.format, 'compiled-raster-v1');
  assert.equal(panel.document.presentation.artifacts[0].artifact.scene, undefined);
  assert.deepEqual(await verifyCompiledPanelPublication(panel), panel);
  assert.equal(bundleForWidget(panel, 'switch').artifactHash, toggleBundle.artifactHash);
  const duplicate = await createCompiledPanel({ id: 'two', title: 'Two', viewports: [viewport] }, [
    ...['one', 'two'].map((id, i) => ({ id, label: id, pageId: 'page', bounds: { x: i * .3, y: .1, width: .2, height: .16 }, bundle: toggleBundle })),
  ]);
  assert.equal(duplicate.document.presentation.artifacts.length, 1);
  assert.equal(duplicate.assets.length, panel.assets.length);
  assert.equal(duplicate.document.ui.controls.length, 2);
  const resources = compiledPanelResources(duplicate.document);
  assert.equal(resources.activeCandidateBytes, resources.residentBytes * 2);
  assert.ok(resources.residentBytes > resources.assetBytes + resources.manifestBytes);
  assert.throws(() => bundleForWidget(panel, 'missing'), /Unknown/);
  const longId = 'w'.repeat(64);
  const long = await createCompiledPanel({ id: 'long', title: 'Long IDs', viewports: [viewport] }, [{ id: longId, pageId: 'page', label: 'Long', bounds: { x: 0, y: 0, width: .2, height: .16 }, bundle: toggleBundle }]);
  assert.ok(long.document.ui.controls[0].id.length <= 64);
  assert.ok(long.document.ui.feedbackWidgets[0].id.length <= 64);
  assert.equal(panelControlValue(long, longId, toggleBundle.artifact.inputs[0].id, true).value, true);
});
test('input and feedback ports bind independently, including immediate requested input', async () => {
  const panel = await fixture(), widget = panel.document.presentation.widgets[0], artifact = toggleBundle.artifact;
  const port = artifact.inputs[0];
  assert.deepEqual(panelControlValue(panel, 'switch', port.id, true), { controlId: widget.inputBindings[0].controlId, value: true });
  assert.throws(() => panelControlValue(panel, 'missing', port.id, true), /Unknown/);
  assert.throws(() => panelControlValue(panel, 'switch', 'missing', true), /Unknown/);
  assert.throws(() => panelControlValue(panel, 'switch', port.id, 1), /domain/);
  const feedbackPort = artifact.outputs[0], channel = panel.document.ui.feedbackWidgets[0].channelId;
  assert.deepEqual(panelFeedbackEvents(panel, 'switch', {}), [{ event: artifact.behavior.events.find(e => e.updates?.includes(feedbackPort.signal)).id, feedback: { [feedbackPort.signal]: { status: 'unknown' } } }]);
  const sample = { status: 'fresh', value: true, provenance: 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: 25 };
  assert.deepEqual(panelFeedbackEvents(panel, 'switch', { [channel]: sample })[0].feedback[feedbackPort.signal], sample);
  assert.throws(() => panelFeedbackEvents(panel, 'missing', {}), /Unknown/);
});
test('generic enum input normalizes declared values and numeric gauge feedback needs no input endpoint', async () => {
  const bundle = await compileWidget(createThreeStateFixture(), { width: 128, height: 64, motion: false });
  const panel = await createCompiledPanel({ id: 'three', title: 'Three', viewports: [viewport] }, [{ id: 'selector', pageId: 'page', label: 'Selector', bounds: { x: 0, y: 0, width: .2, height: .16 }, bundle }]);
  const port = bundle.artifact.inputs[0];
  assert.deepEqual(port.values.map(value => panelControlValue(panel, 'selector', port.id, value).value), [0, .5, 1]);
  assert.equal(panel.document.targets[0].axes.length, 1);
  const gauge = await compileWidget(createLinearGaugeFixture(), { width: 128, height: 64, motion: false });
  const readOnly = await createCompiledPanel({ id: 'gauge', title: 'RPM', viewports: [viewport] }, [{ id: 'rpm', pageId: 'page', label: 'RPM', bounds: { x: 0, y: 0, width: .2, height: .16 }, bundle: gauge }]);
  assert.equal(readOnly.document.ui.controls.length, 0);
  assert.equal(readOnly.document.targets.length, 0);
  assert.equal(readOnly.document.ui.feedbackWidgets[0].kind, 'gauge');
});
test('presentation and cue changes keep capability identity while behavior changes alter it', async () => {
  const original = await fixture(), presentation = structuredClone(original);
  presentation.document.ui.title = 'Renamed';
  presentation.document.presentation.widgets[0].label = 'Power';
  presentation.document.presentation.widgets[0].bounds.x = .5;
  presentation.document.ui.controls[0].label = 'Power request';
  await resign(presentation);
  assert.equal(presentation.capabilityFingerprint, original.capabilityFingerprint);
  assert.notEqual(presentation.publicationHash, original.publicationHash);
  const a = original.document.presentation.artifacts[0].artifact;
  const b = structuredClone(a.behavior);
  const transition = b.regions[0].transitions[0];
  transition.actions ??= [];
  transition.actions.push({ type: 'cue', cue: 'another-sound' });
  assert.deepEqual(panelBehaviorSemantics(b), panelBehaviorSemantics(a.behavior));
  transition.priority = 11;
  assert.notDeepEqual(panelBehaviorSemantics(b), panelBehaviorSemantics(a.behavior));
  assert.ok(validateCompiledPanelDocument(presentation.document).ok);
});
test('admission rejects corrupt bytes, payload and content mismatches atomically', async () => {
  const original = await fixture();
  for (const mutate of [
    p => p.publicationHash = 'sha256:' + '0'.repeat(64),
    p => p.capabilityFingerprint = 'sha256:' + '0'.repeat(64),
    p => p.assets.pop(),
    p => p.assets.push(p.assets[0]),
    p => p.assets[0].assetId = 'unknown',
    p => p.assets[0].base64 = 'AAAA',
    p => p.assets[0].extra = true,
    p => p.extra = true,
  ]) { const p = structuredClone(original); mutate(p); await assert.rejects(verifyCompiledPanelPublication(p)); }
  assert.deepEqual(await verifyCompiledPanelPublication(original), original);
  await assert.rejects(verifyCompiledPanelPublication(original, { ...BROWSER_WIDGET_CAPABILITIES, maxTotalBytes: 768 }), /budget/);
});
test('panel validation rejects malformed structure, dangling ports, incorrect dimensions and resource overflow', async () => {
  const panel = await fixture();
  const cases = [
    d => d.schemaVersion = 3,
    d => d.ui.schemaVersion = 1,
    d => d.ui.extra = true,
    d => d.ui.title = '',
    d => d.ui.id = 'bad id',
    d => d.ui.controls[0].id = d.ui.pages[0].id,
    d => d.ui.controls[0].kind = 'hat',
    d => d.ui.controls[0].style = {},
    d => d.ui.feedbackWidgets[0].kind = 'image',
    d => d.ui.feedbackWidgets.push(d.ui.feedbackWidgets[0]),
    d => d.ui.pages = [],
    d => d.presentation.viewports[0].pageId = 'missing',
    d => d.presentation.viewports[0].width = 1,
    d => d.presentation.format = 'source',
    d => d.presentation.artifacts[0].hash = 'invalid',
    d => d.presentation.artifacts.push(d.presentation.artifacts[0]),
    d => d.presentation.artifacts[0].artifact.extra = true,
    d => d.ui.assets = [],
    d => d.ui.assets[0].sha256 = '0'.repeat(64),
    d => d.presentation.widgets[0].pageId = 'missing',
    d => d.presentation.widgets[0].bounds.x = 2,
    d => d.presentation.widgets[0].bounds.width = 0,
    d => d.presentation.widgets[0].bounds.width = .21,
    d => d.presentation.widgets[0].artifactHash = '0'.repeat(64),
    d => d.presentation.widgets[0].inputBindings[0].portId = 'missing',
    d => d.presentation.widgets[0].inputBindings[0].controlId = 'missing',
    d => d.presentation.widgets[0].outputBindings[0].feedbackId = 'missing',
    d => d.presentation.widgets[0].outputBindings[0].portId = 'missing',
    d => d.ui.controls.push({ id: 'unbound', label: 'Unbound', kind: 'button' }),
    d => d.ui.feedbackWidgets[0].kind = 'text',
    d => d.targets[0].hats = 1,
    d => d.targets[0].axes = ['bipolar'],
    d => d.targets[0].buttons = 0,
    d => d.bindings[0].targetId = 'missing',
    d => d.bindings[0].index = 128,
    d => d.bindings[0].kind = 'axis',
    d => d.bindings[0].controlId = 'missing',
    d => d.ui.title = 'x'.repeat(1024 * 1024),
  ];
  for (const mutate of cases) { const d = structuredClone(panel.document); mutate(d); assert.equal(validateCompiledPanelDocument(d).ok, false, mutate.toString()); }
  assert.equal(validateCompiledPanelDocument(null).ok, false);
  assert.equal(validateCompiledPanelDocument(panel.document, { ...BROWSER_WIDGET_CAPABILITIES, maxResidentBytes: 768 }).ok, false);
});
