import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { newDocument, createDesignerHistory, applyDesignerCommand as apply, createWidgetCatalog, normalizeToPortableUi, publishDocument, exportBundle, canonical } from '../packages/companion-designer/src/index.ts';

// Production authoring APIs produce fixtures consumed independently by Rust.
let history = createDesignerHistory(newDocument('Shared systems'));
history = apply(history, { type: 'delete-widget', id: history.present.presentation.widgets[0].id });
history = apply(history, { type: 'set-viewport', pageId: history.present.ui.pages[0].id, width: 800, height: 480 });
for (const [index, definition] of createWidgetCatalog().entries()) {
  history = apply(history, { type: 'add-component', definition, properties: definition.interaction.kind === 'navigate' ? { page: history.present.ui.pages[0].id } : {}, bounds: { x: .025 + (index % 4) * .24, y: .025 + Math.floor(index / 4) * .32, width: .22, height: .28 } });
}
const document = structuredClone(history.present), ids = new Map();
const remap = (old, replacement) => { ids.set(old, replacement); return replacement; };
document.ui.id = 'shared-systems';
document.ui.pages.forEach((p, i) => p.id = remap(p.id, `page-${i}`));
document.ui.controls.forEach((c, i) => c.id = remap(c.id, `input-${i}`));
document.ui.feedbackWidgets.forEach((f, i) => { f.id = remap(f.id, `output-${i}`); f.channelId = `channel-${i}`; });
document.targets.forEach((t, i) => t.id = remap(t.id, `target-${i}`));
document.presentation.viewports.forEach(v => v.pageId = ids.get(v.pageId));
document.presentation.widgets.forEach((w, i) => {
  w.id = `widget-${i}`; w.pageId = ids.get(w.pageId); w.controlIds = w.controlIds.map(id => ids.get(id)); w.feedbackIds = w.feedbackIds.map(id => ids.get(id));
  if (w.config.properties.page) w.config.properties.page = ids.get(w.config.properties.page);
});
document.bindings.forEach(b => { b.controlId = ids.get(b.controlId); b.targetId = ids.get(b.targetId); });
document.ui = normalizeToPortableUi(document);
history = createDesignerHistory(document);
const first = document.presentation.widgets[0], styledDefinition = structuredClone(document.presentation.definitions[0]);
styledDefinition.version = 2; styledDefinition.layers[0].radius = 0;
const styled = apply(history, { type: 'update-component', id: first.id, definition: styledDefinition, properties: { label: 'REACTOR', accent: '#ffb570' } });
const behaviorDefinition = structuredClone(styledDefinition); behaviorDefinition.version = 3; behaviorDefinition.interaction.kind = 'toggle';
const behavior = apply(styled, { type: 'update-component', id: first.id, definition: behaviorDefinition });
const outputs = [];
for (const [name, doc] of [['base', document], ['styling', styled.present], ['behavior', behavior.present]]) {
  const p = await publishDocument(doc, []);
  outputs.push([new URL(`../crates/companion-service/tests/fixtures/widgets-${name}.json`, import.meta.url), JSON.stringify({ canonicalDocument: canonical(p.document), publicationHash: p.publicationHash, capabilityFingerprint: p.capabilityFingerprint, assets: p.assets }, null, 2) + '\n']);
}
outputs.push([new URL('../examples/companion/shared-systems.bundle.json', import.meta.url), await exportBundle(document, []) + '\n']);
for (const [url, contents] of outputs) {
  if (process.argv.includes('--check')) {
    if (readFileSync(url, 'utf8') !== contents) throw new Error(`Widget fixture differs: ${fileURLToPath(url)}`);
  } else { mkdirSync(new URL('.', url), { recursive: true }); writeFileSync(url, contents); }
}
console.log(`${outputs.length} shared widget publication fixtures ${process.argv.includes('--check') ? 'verified' : 'written'}.`);
