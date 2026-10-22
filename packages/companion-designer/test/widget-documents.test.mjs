import test from 'node:test';
import assert from 'node:assert/strict';
import {
  newDocument, createDesignerHistory, applyDesignerCommand as apply, upgradeWidgetDocument,
  createWidgetCatalog, validateWidgetDefinition, validateDesignerDocument, canonical,
  compareDesignerPublications, publishDocument, exportBundle, importBundle,
  undoDesigner, redoDesigner, createLibrary, saveDraft, duplicateDraft,
} from '../src/index.ts';

const catalog = createWidgetCatalog();
function seed() {
  let h = createDesignerHistory(newDocument('Shared systems'));
  return apply(h, { type: 'add-component', definition: catalog[0], properties: { label: 'ENGINE' } });
}
const component = doc => doc.presentation.widgets.find(w => w.kind === 'component');

test('all original catalog entries validate with shared portable layers and interactions', () => {
  assert.equal(catalog.length, 14);
  for (const definition of catalog) assert.equal(validateWidgetDefinition(definition).ok, true, definition.id);
  const again = createWidgetCatalog(); again[0].properties[0].default = 'Different';
  assert.equal(createWidgetCatalog()[0].properties[0].default, 'ENGINE');
  assert.equal(catalog.find(d=>d.id==='osg-text-instrument').outputs[0].kind,'text');
  const icon=catalog.find(d=>d.id==='osg-power-icon');
  assert.equal(icon.state.confirmedPort,'active');assert.equal(icon.interaction.kind,'none');
  assert.ok(icon.layers.some(layer=>layer.kind==='ellipse'));assert.equal(icon.inputs.length,0);
});

test('schema 2 upgrade is explicit, detached and preserves legacy publications', async () => {
  const old = newDocument('Existing'), before = canonical(old), hash = (await publishDocument(old, [])).publicationHash;
  const upgraded = upgradeWidgetDocument(old);
  assert.equal(old.schemaVersion, 2); assert.equal(canonical(old), before);
  assert.equal(upgraded.schemaVersion, 3); assert.deepEqual(upgraded.presentation.definitions, []);
  assert.deepEqual(upgraded.ui, old.ui); assert.deepEqual(upgraded.bindings, old.bindings);
  assert.equal((await publishDocument(old, [])).publicationHash, hash);
  const wrong = structuredClone(old); wrong.presentation.definitions = [];
  assert.equal(validateDesignerDocument(wrong).ok, false, 'schema 2 must not gain new fields under old hashes');
  delete upgraded.presentation.definitions; assert.equal(validateDesignerDocument(upgraded).ok, false);
});

test('instances create independent stable controls and confirmed feedback with one shared definition', () => {
  const first = seed(), second = apply(first, { type: 'add-component', definition: catalog[0], properties: { label: 'LIGHTS' } });
  const doc = second.present, widgets = doc.presentation.widgets.filter(w => w.kind === 'component');
  assert.equal(doc.schemaVersion, 3); assert.equal(doc.presentation.definitions.length, 1);
  assert.equal(widgets.length, 2); assert.notDeepEqual(widgets[0].controlIds, widgets[1].controlIds);
  assert.notDeepEqual(widgets[0].feedbackIds, widgets[1].feedbackIds);
  assert.equal(doc.ui.feedbackWidgets.length, 2);
  assert.equal(new Set(doc.ui.feedbackWidgets.map(f => f.channelId)).size, 2);
  assert.deepEqual(doc.presentation.definitions[0], catalog[0]);
  assert.equal(validateDesignerDocument(doc).ok, true);
  const duplicated = apply(second, { type: 'duplicate-widget', id: widgets[0].id });
  assert.equal(duplicated.present.presentation.definitions.length, 1);
  assert.equal(validateDesignerDocument(duplicated.present).ok, true);
  const removed = apply(duplicated, { type: 'delete-widget', id: widgets[0].id });
  assert.equal(removed.present.ui.feedbackWidgets.length, 2);
  assert.equal(undoDesigner(removed).present.presentation.widgets.length, duplicated.present.presentation.widgets.length);
  assert.equal(redoDesigner(undoDesigner(removed)).present.ui.feedbackWidgets.length, 2);
});

test('every catalog family can be instantiated without losing the source panel', () => {
  let history = createDesignerHistory(newDocument('Catalog'));
  for (const definition of catalog) history = apply(history, { type: 'add-component', definition, properties: definition.interaction.kind === 'navigate' ? { page: history.present.ui.pages[0].id } : {} });
  assert.equal(history.present.presentation.definitions.length, catalog.length);
  assert.equal(validateDesignerDocument(history.present).ok, true);
});

test('invalid definition, property, primitive and navigation references fail atomically', () => {
  const history = seed(), before = canonical(history.present);
  for (const mutate of [
    d => d.presentation.definitions.push(d.presentation.definitions[0]),
    d => component(d).config.definitionVersion = 99,
    d => component(d).config.properties.extra = 'missing',
    d => component(d).feedbackIds = [],
    d => component(d).controlIds = [],
    d => d.ui.feedbackWidgets[0].kind = 'gauge',
    d => d.presentation.definitions[0].layers[0].radius = -1,
    d => component(d).config.extra = true,
    d => component(d).config.definitionId = 'absent',
  ]) { const doc = structuredClone(history.present); mutate(doc); assert.equal(validateDesignerDocument(doc).ok, false); }
  assert.throws(() => apply(history, { type: 'add-component', definition: catalog[9], properties: { page: 'missing' } }), /navigation/);
  assert.throws(() => apply(history, { type: 'add-component', definition: catalog[0], viewportId: 'missing' }), /viewport/);
  const changed = structuredClone(catalog[0]); changed.layers[0].fill = '#ffffff';
  assert.throws(() => apply(history, { type: 'add-component', definition: changed }), /immutable/);
  assert.equal(canonical(history.present), before);
});

test('library version adoption is explicit and preserves bindings and instance overrides', async () => {
  const history = seed(), widget = component(history.present), beforeBindings = structuredClone(history.present.bindings);
  const next = structuredClone(catalog[0]); next.version = 2; next.layers[0].radius = 0;
  const updated = apply(history, { type: 'update-component', id: widget.id, definition: next });
  assert.equal(component(history.present).config.definitionVersion, 1);
  assert.equal(component(updated.present).config.definitionVersion, 2);
  assert.deepEqual(component(updated.present).controlIds, widget.controlIds);
  assert.deepEqual(component(updated.present).feedbackIds, widget.feedbackIds);
  assert.deepEqual(updated.present.bindings, beforeBindings);
  assert.deepEqual(component(updated.present).config.properties, { label: 'ENGINE' });
  assert.equal(compareDesignerPublications(history.present, updated.present), 'styling');
  assert.equal((await publishDocument(history.present, [])).capabilityFingerprint, (await publishDocument(updated.present, [])).capabilityFingerprint);
  assert.notEqual((await publishDocument(history.present, [])).publicationHash, (await publishDocument(updated.present, [])).publicationHash);
  const behavior = structuredClone(next); behavior.version = 3; behavior.interaction.kind = 'toggle';
  const toggled = apply(updated, { type: 'update-component', id: widget.id, definition: behavior });
  assert.equal(compareDesignerPublications(updated.present, toggled.present), 'capability');
  const invalid = structuredClone(next); invalid.version = 4; invalid.inputs[0].name = 'renamed'; invalid.interaction.port = 'renamed';
  assert.throws(() => apply(updated, { type: 'update-component', id: widget.id, definition: invalid }), /rebinding/);
  assert.throws(() => apply(updated, { type: 'update-component', id: widget.id, definition: catalog[1] }), /same widget/);
  assert.throws(() => apply(updated, { type: 'update-component', id: updated.present.presentation.widgets[0].id }), /reusable widget/);
  const props = apply(updated, { type: 'update-component', id: widget.id, properties: { label: 'POWER', accent: '#ffaaaa' } });
  assert.equal(compareDesignerPublications(updated.present, props.present), 'styling');
});

test('selector caption revisions preserve contracts while option values require reconciliation', async () => {
  const h = apply(seed(), { type: 'add-component', definition: catalog[2] });
  const selected = h.present.presentation.widgets.at(-1), revision = structuredClone(catalog[2]);
  revision.version = 2; revision.interaction.options.forEach((option, index) => option.label = 'Mode ' + index);
  const styled = apply(h, { type: 'update-component', id: selected.id, definition: revision });
  assert.equal(compareDesignerPublications(h.present, styled.present), 'styling');
  assert.deepEqual(styled.present.bindings, h.present.bindings);
  assert.deepEqual(styled.present.ui.controls, h.present.ui.controls);
  assert.equal((await publishDocument(styled.present, [])).capabilityFingerprint, (await publishDocument(h.present, [])).capabilityFingerprint);
  const changed = structuredClone(styled.present);
  changed.presentation.definitions.find(definition => definition.id === revision.id && definition.version === 2).interaction.options[1].value = .75;
  assert.equal(validateDesignerDocument(changed).ok, true);
  assert.equal(compareDesignerPublications(styled.present, changed), 'capability');
});

test('typed output unit and range changes require semantic reconciliation', () => {
  const h = apply(seed(), { type: 'add-component', definition: catalog[4] });
  const changed = structuredClone(h.present);
  changed.presentation.definitions.find(d => d.id === catalog[4].id).outputs[0].range.max = 200;
  assert.equal(compareDesignerPublications(h.present, changed), 'capability');
  changed.presentation.definitions.find(d => d.id === catalog[4].id).outputs[0].range.min = 300;
  assert.equal(validateDesignerDocument(changed).ok, false);
});

test('component publications and bundles round-trip with exact identity', async () => {
  const doc = seed().present, publication = await publishDocument(doc, []);
  const imported = await importBundle(await exportBundle(doc, []));
  const after = await publishDocument(imported.document, imported.assets);
  assert.equal(after.publicationHash, publication.publicationHash);
  assert.equal(after.capabilityFingerprint, publication.capabilityFingerprint);
  assert.ok(Object.isFrozen(publication.document.presentation.definitions[0].layers));
  const saved = await saveDraft(createLibrary(), { id: 'draft', name: 'Shared', document: doc, assets: [] });
  const duplicate = duplicateDraft(saved, 'draft').drafts[1].document;
  assert.equal(validateDesignerDocument(duplicate).ok, true);
  assert.deepEqual(duplicate.presentation.definitions, doc.presentation.definitions);
  assert.notEqual(duplicate.ui.id, doc.ui.id); assert.notDeepEqual(component(duplicate).controlIds, component(doc).controlIds);
});

test('copied navigation instances target copied pages without changing their library recipes', async () => {
  const original = seed();
  const history = apply(original, { type: 'add-component', definition: catalog[9], properties: { page: original.present.ui.pages[0].id } });
  const saved = await saveDraft(createLibrary(), { id: 'draft', name: 'Navigation', document: history.present, assets: [] });
  const copied = duplicateDraft(saved, 'draft').drafts[1].document;
  const navigation = copied.presentation.widgets.find(w => w.kind === 'component' && w.config.definitionId === catalog[9].id);
  assert.equal(navigation.config.properties.page, copied.ui.pages[0].id);
  assert.notEqual(navigation.config.properties.page, history.present.ui.pages[0].id);
  assert.deepEqual(copied.presentation.definitions, history.present.presentation.definitions);
  assert.equal(validateDesignerDocument(copied).ok, true);
});

test('component axis ports and resolved gauge ranges are checked beyond primitive arity', () => {
  const h = apply(seed(), { type: 'add-component', definition: catalog[2] });
  const selector = h.present.presentation.widgets.find(w => w.kind === 'component' && w.config.definitionId === catalog[2].id);
  const wrong = structuredClone(h.present); wrong.ui.controls.find(c => c.id === selector.controlIds[0]).axisMode = 'bipolar';
  assert.match(validateDesignerDocument(wrong).error, /axis contract/);
  const fallback = structuredClone(h.present); fallback.ui.controls.find(c => c.id === selector.controlIds[0]).fallback = 0.5;
  assert.match(validateDesignerDocument(fallback).error, /axis contract/);
  assert.throws(() => apply(h, { type: 'add-component', definition: catalog[5], properties: { minimum: 100, maximum: 10 } }), /gauge range/);
});
