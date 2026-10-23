import { readFile } from 'node:fs/promises';
import test from 'node:test';
import assert from 'node:assert/strict';
import { exampleBundles, flightButtonBox, axisRacing } from './layouts.mjs';
import { importBundle, exportBundle, publishDocument, createLibrary, saveDraft, openDraft, compareDesignerPublications } from '../../../packages/companion-designer/src/index.ts';
import { neutralValues, validateFeedbackValues } from '../../../packages/companion-contracts/src/index.ts';
import { createPreviewController } from '../../../packages/companion-ui/src/controller.ts';
import { feedbackPresentation } from '../../../packages/companion-ui/src/presentation.ts';

const widget = (document, key) => document.presentation.widgets.find(item => item.id === `${document.ui.id}-${key}-widget`);
const input = (document, key) => `${document.ui.id}-${key}`;

test('retired transport fixtures exactly reproduce deterministic generators', async () => {
  const first = await exampleBundles(), second = await exampleBundles();
  assert.deepEqual(first, second);
  for (const [name, serialized] of first) {
    assert.equal(await readFile(new URL(name, import.meta.url), 'utf8'), serialized);
    const bundle = await importBundle(serialized);
    assert.equal(bundle.bundleVersion, 1);
    assert.equal(bundle.document.schemaVersion, 2);
    assert.deepEqual(bundle.assets, []);
    assert.ok(Buffer.byteLength(serialized) < 32 * 1024);
  }
});

test('both examples survive real importer, library, export and immutable publication round trips', async () => {
  let library = createLibrary();
  for (const [name, serialized] of await exampleBundles()) {
    const bundle = await importBundle(serialized);
    library = await saveDraft(library, { id: name.slice(0, -5), name: bundle.document.ui.title, document: bundle.document, assets: bundle.assets });
    const saved = openDraft(library, name.slice(0, -5));
    const reopened = await importBundle(await exportBundle(saved.document, saved.assets));
    const publication = await publishDocument(reopened.document, reopened.assets);
    const original = await publishDocument(bundle.document, bundle.assets);
    assert.equal(publication.publicationHash, original.publicationHash);
    assert.equal(publication.capabilityFingerprint, original.capabilityFingerprint);
    assert.deepEqual(reopened.document, bundle.document);
    assert.ok(Object.isFrozen(publication.document.presentation.widgets[0]));
  }
  assert.equal(library.drafts.length, 2);
});

test('published target shapes and stable bindings preserve every example input without local assignments', () => {
  for (const [document, axes, buttons, hats] of [[flightButtonBox(), ['unipolar', 'bipolar'], 7, 1], [axisRacing(), ['bipolar', 'unipolar', 'unipolar', 'unipolar'], 2, 0]]) {
    assert.equal(document.targets.length, 1);
    assert.deepEqual(document.targets[0].axes, axes);
    assert.equal(document.targets[0].buttons, buttons);
    assert.equal(document.targets[0].hats, hats);
    assert.equal(document.ui.pages.length, 2);
    assert.equal(document.bindings.length, document.ui.controls.length);
    const occupied = new Set();
    for (const control of document.ui.controls) {
      const binding = document.bindings.find(row => row.controlId === control.id);
      assert.ok(binding);
      assert.equal(binding.kind, control.kind);
      const identity = `${binding.targetId}:${binding.kind}:${binding.index}`;
      assert.ok(!occupied.has(identity)); occupied.add(identity);
      if (control.kind === 'axis') assert.equal(document.targets[0].axes[binding.index], control.axisMode);
    }
    assert.deepEqual(document.ui.assets, []);
    assert.ok(!Object.hasOwn(document.targets[0], 'slot'));
  }
  assert.notEqual(flightButtonBox().ui.id, axisRacing().ui.id);
});

test('flight button, toggle, pulse, encoder, selector and hat semantics match the guide', () => {
  const document = flightButtonBox(); let now = 0;
  const engine = createPreviewController(document, { clock: () => now });
  assert.equal(engine.begin(1, widget(document, 'transmit').id), false);
  engine.arm();
  assert.equal(engine.begin(1, widget(document, 'transmit').id), true);
  assert.equal(engine.snapshot().values[input(document, 'transmit')], true);
  engine.release(1);
  assert.equal(engine.snapshot().values[input(document, 'transmit')], false);
  engine.begin(2, widget(document, 'gear').id); engine.release(2);
  assert.equal(engine.snapshot().values[input(document, 'gear')], true);
  engine.begin(3, widget(document, 'ack').id); engine.release(3);
  assert.equal(engine.snapshot().values[input(document, 'ack')], true);
  now = 100; engine.tick(now);
  assert.equal(engine.snapshot().values[input(document, 'ack')], false);
  engine.begin(4, widget(document, 'radio').id, { direction: -1 }); engine.release(4);
  assert.equal(engine.snapshot().values[input(document, 'radio-down')], true);
  now = 150; engine.tick(now);
  assert.equal(engine.snapshot().values[input(document, 'radio-down')], false);
  engine.begin(5, widget(document, 'flaps').id, { value: 0.5 }); engine.release(5);
  assert.equal(engine.snapshot().values[input(document, 'flaps')], 0.5);
  engine.begin(6, widget(document, 'view').id, { value: 3 });
  assert.equal(engine.snapshot().values[input(document, 'view')], 3);
  engine.release(6, 'Pointer cancelled', true);
  assert.equal(engine.snapshot().values[input(document, 'view')], -1);
  engine.disarm('Page changed');
  assert.deepEqual(engine.snapshot().values, neutralValues(document.ui));
  now = 1000; engine.tick(now);
  assert.equal(engine.snapshot().armed, false);
  assert.deepEqual(engine.snapshot().values, neutralValues(document.ui));
});

test('racing axes accept simultaneous pointers and cancellation/Stop use declared fallbacks', () => {
  const document = axisRacing(), engine = createPreviewController(document);
  engine.arm();
  assert.equal(engine.begin(1, widget(document, 'steering').id, { x: 1 }), true);
  assert.equal(engine.begin(2, widget(document, 'throttle').id, { y: 1 }), true);
  assert.equal(engine.begin(3, widget(document, 'brake').id, { y: 0.5 }), true);
  assert.equal(engine.begin(4, widget(document, 'shift-up').id), true);
  assert.equal(engine.snapshot().activePointers, 4);
  assert.equal(engine.snapshot().values[input(document, 'steering')], 1);
  assert.equal(engine.snapshot().values[input(document, 'throttle')], 1);
  assert.ok(engine.snapshot().values[input(document, 'brake')] > 0);
  engine.release(2, 'Pointer cancelled', true);
  assert.equal(engine.snapshot().values[input(document, 'throttle')], 0);
  assert.equal(engine.snapshot().values[input(document, 'steering')], 1);
  assert.equal(engine.snapshot().values[input(document, 'shift-up')], true);
  engine.disarm('Stop');
  assert.equal(engine.snapshot().activePointers, 0);
  assert.deepEqual(engine.snapshot().values, neutralValues(document.ui));
});

test('styling and behavior edits produce explicit immutable revision differences', async () => {
  const document = axisRacing();
  const original = await publishDocument(document, []);
  const styled = structuredClone(document); styled.presentation.widgets[0].style.accent = '#FFCC55';
  const restyled = await publishDocument(styled, []);
  assert.equal(compareDesignerPublications(document, styled), 'styling');
  assert.notEqual(restyled.publicationHash, original.publicationHash);
  assert.equal(restyled.capabilityFingerprint, original.capabilityFingerprint);
  const behavior = structuredClone(document); behavior.ui.controls.find(control => control.id === input(document, 'handbrake')).fallback = 0.2;
  const semantic = await publishDocument(behavior, []);
  assert.equal(compareDesignerPublications(document, behavior), 'capability');
  assert.notEqual(semantic.capabilityFingerprint, original.capabilityFingerprint);
  assert.equal(original.document.ui.controls.find(control => control.id === input(document, 'handbrake')).fallback, 0);
});

test('example feedback starts unknown, preserves types/provenance and ages without controller input', () => {
  const document = flightButtonBox(), engine = createPreviewController(document);
  const before = engine.snapshot();
  const radio = widget(document, 'radio-text'), rpm = widget(document, 'rpm');
  assert.equal(feedbackPresentation(radio, document.ui, {}, 0).available, false);
  const output = document.ui.feedbackWidgets.find(value => value.id === radio.feedbackIds[0]);
  const sample = { channelId: output.channelId, outputId: output.id, status: 'fresh', value: '251.025', provenance: 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: 0, unit: 'text' };
  assert.equal(validateFeedbackValues([sample], document.ui).ok, true);
  assert.equal(feedbackPresentation(radio, document.ui, { [sample.channelId]: { ...sample, receivedAtMs: 0 } }, 10).confirmed, true);
  assert.equal(feedbackPresentation(radio, document.ui, { [sample.channelId]: { ...sample, receivedAtMs: 0 } }, 1001).confirmed, false);
  const numeric = document.ui.feedbackWidgets.find(value => value.id === rpm.feedbackIds[0]);
  assert.equal(validateFeedbackValues([{ ...sample, channelId: numeric.channelId, outputId: numeric.id, unit: 'rpm', value: 6000, range: { min: 0, max: 100000 } }], document.ui).ok, true);
  assert.equal(validateFeedbackValues([{ ...sample, value: true }], document.ui).ok, false);
  assert.deepEqual(engine.snapshot(), before);
});

test('portable examples reject injected host authority and machine-specific assignments', async () => {
  const bundle = JSON.parse((await exampleBundles()).get('axis-racing.json'));
  bundle.document.ui.companionId = 'not-authority';
  await assert.rejects(importBundle(JSON.stringify(bundle)));
  delete bundle.document.ui.companionId;
  bundle.document.targets[0].slot = 2;
  await assert.rejects(importBundle(JSON.stringify(bundle)));
});
