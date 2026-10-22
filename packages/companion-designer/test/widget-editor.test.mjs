import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createWidgetCatalog, createWidgetEditorHistory, applyWidgetEditorCommand as apply, undoWidgetEditor, redoWidgetEditor,
  canonical, validateWidgetDefinition, validateWidgetBundle, exportWidgetBundle, importWidgetBundle, mergeWidgetBundles,
  widgetAssetIds, newDocument, importAsset,
} from '../src/index.ts';

const seed = () => createWidgetEditorHistory(createWidgetCatalog()[0]);

test('layer edits, ordering, duplication and state overrides are immutable and undoable', () => {
  const original = seed(), before = canonical(original.present);
  let h = apply(original, { type: 'add-layer', kind: 'text' }), id = h.present.layers.at(-1).id;
  h = apply(h, { type: 'update-layer', id, patch: { label: 'Secondary label', text: 'PRESS' } });
  h = apply(h, { type: 'state-style', id, state: 'pressed', paint: { fill: '#ff7777', opacity: 0.5, text: 'HELD' } });
  h = apply(h, { type: 'reorder-layer', id, index: 1 });
  assert.equal(h.present.layers[1].id, id); assert.equal(h.present.layers[1].states.pressed.text, 'HELD');
  const duplicated = apply(h, { type: 'duplicate-layer', id });
  assert.notEqual(duplicated.present.layers.at(-1).id, id);
  const removed = apply(duplicated, { type: 'delete-layer', id });
  assert.equal(removed.present.layers.some(l => l.id === id), false);
  assert.equal(undoWidgetEditor(removed).present.layers.some(l => l.id === id), true);
  assert.equal(canonical(redoWidgetEditor(undoWidgetEditor(removed)).present), canonical(removed.present));
  assert.equal(canonical(original.present), before);
});

test('every supported visual layer gets a valid editable starting configuration', () => {
  for (const kind of ['rectangle', 'ellipse', 'line', 'text', 'image', 'indicator', 'gauge']) {
    const h = apply(seed(), { type: 'add-layer', kind }); assert.equal(h.present.layers.at(-1).kind, kind);
    assert.equal(validateWidgetDefinition(h.present).ok, true);
  }
  const frame = createWidgetEditorHistory(createWidgetCatalog()[7]);
  assert.equal(apply(frame, { type: 'add-layer', kind: 'indicator' }).present.outputs.length, 1);
});

test('property and definition updates preserve identity and reject inconsistent edits atomically', () => {
  let h = seed(); h = apply(h, { type: 'update', patch: { title: 'Power control', size: { width: 300, height: 160 } } });
  h = apply(h, { type: 'set-property', property: { name: 'accent', label: 'Color', type: 'color', default: '#ffcc66' } });
  h = apply(h, { type: 'set-property', property: { name: 'extra', label: 'Extra', type: 'text', default: 'Text' } });
  h = apply(h, { type: 'remove-property', name: 'extra' }); const before = canonical(h.present);
  for (const command of [
    { type: 'remove-property', name: 'absent' }, { type: 'remove-property', name: 'accent' },
    { type: 'update', patch: { id: 'changed' } }, { type: 'update', patch: { size: { width: 0, height: 1 } } },
    { type: 'update-layer', id: h.present.layers[0].id, patch: { id: 'changed' } },
    { type: 'update-layer', id: 'absent', patch: { fill: '#ffffff' } },
    { type: 'reorder-layer', id: h.present.layers[0].id, index: -1 },
    { type: 'add-layer', kind: 'javascript' },
  ]) { assert.throws(() => apply(h, command)); assert.equal(canonical(h.present), before); }
  const single = structuredClone(h.present); single.layers = [single.layers[0]];
  assert.throws(() => apply(createWidgetEditorHistory(single), { type: 'delete-layer', id: single.layers[0].id }), /visual layer/);
});

test('history is bounded, no-op edits preserve history and a new edit discards redo', () => {
  let h = seed(); assert.equal(undoWidgetEditor(h), h); assert.equal(redoWidgetEditor(h), h);
  assert.equal(apply(h, { type: 'update', patch: { title: h.present.title } }), h);
  for (let i = 0; i < 60; i++) h = apply(h, { type: 'update', patch: { title: `Widget ${i}` } });
  assert.equal(h.past.length, 50);
  h = undoWidgetEditor(h); assert.equal(h.future.length, 1);
  h = apply(h, { type: 'replace', definition: createWidgetCatalog()[1] }); assert.equal(h.future.length, 0);
});

const emptyBundle = () => ({ bundleVersion: 1, library: { formatVersion: 1, definitions: [] }, manifest: [], assets: [] });
test('portable widget bundles validate assets, round-trip and merge without overwriting immutable revisions', async () => {
  const bundle = emptyBundle(); bundle.library.definitions.push(createWidgetCatalog()[0]);
  const imported = await importWidgetBundle(await exportWidgetBundle(bundle)); assert.deepEqual(imported, bundle);
  const merged = await mergeWidgetBundles(emptyBundle(), imported); assert.deepEqual(merged, bundle);
  assert.deepEqual(await mergeWidgetBundles(merged, imported), merged);
  const conflict = structuredClone(imported); conflict.library.definitions[0].title = 'Changed';
  await assert.rejects(mergeWidgetBundles(merged, conflict), /immutable/);
  await assert.rejects(validateWidgetBundle({ ...bundle, bundleVersion: 2 }), /version/);
  await assert.rejects(importWidgetBundle('x'.repeat(13 * 1024 * 1024 + 1)), /budget/);
});

test('widget artwork travels with the library using existing manifest and digest validation', async () => {
  const png = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII='), c => c.charCodeAt(0));
  const asset = await importAsset(newDocument(), png, 'image/png');
  const definition = createWidgetCatalog()[0];
  definition.properties.push({ name: 'art', label: 'Artwork', type: 'asset', default: asset.asset.assetId });
  definition.layers.push({ id: 'art', label: 'Artwork', kind: 'image', assetId: { property: 'art' }, bounds: { x: 0, y: 0, width: 1, height: 1 } });
  const bundle = { bundleVersion: 1, library: { formatVersion: 1, definitions: [definition] }, manifest: asset.document.ui.assets, assets: [asset.asset] };
  assert.deepEqual(await importWidgetBundle(await exportWidgetBundle(bundle)), bundle);
  assert.deepEqual(widgetAssetIds(definition), [asset.asset.assetId]);
  assert.deepEqual(widgetAssetIds(definition, { art: 'another' }), ['another', asset.asset.assetId].sort());
  definition.layers[0].kind = 'image'; definition.layers[0] = { id: 'fixed-image', label: 'Fixed artwork', kind: 'image', bounds: { x: 0, y: 0, width: 1, height: 1 }, assetId: asset.asset.assetId };
  assert.deepEqual(widgetAssetIds(definition), [asset.asset.assetId]);
  const merged = await mergeWidgetBundles(emptyBundle(), bundle); assert.equal(merged.assets.length, 1);
  const conflictingAsset = structuredClone(bundle); conflictingAsset.library.definitions = []; conflictingAsset.manifest[0].id = 'new-asset'; conflictingAsset.assets[0].assetId = 'new-asset';
  assert.equal((await mergeWidgetBundles(merged, conflictingAsset)).assets.length, 2);
  const wrong = structuredClone(bundle); wrong.assets[0].base64 = wrong.assets[0].base64.slice(0, -4) + 'AAAA';
  await assert.rejects(validateWidgetBundle(wrong), /digest|size/);
  const absent = structuredClone(bundle); absent.manifest = []; absent.assets = [];
  await assert.rejects(validateWidgetBundle(absent), /asset/);
});
