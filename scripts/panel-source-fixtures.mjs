import { readFile, writeFile } from 'node:fs/promises';
import { importBundle, publishDocument, validateDesignerDocument, normalizeToPortableUi, canonical } from '../packages/companion-designer/src/index.ts';

const { document: catalog } = await importBundle(await readFile('examples/companion/shared-systems.bundle.json', 'utf8'));
const input = JSON.parse(await readFile('apps/companion-esp32/test/widget-input.json', 'utf8')).document;
const legacy = JSON.parse(JSON.parse(await readFile('apps/companion-esp32/test/live-publication.json', 'utf8')).canonicalDocument);
const mixed = structuredClone(legacy); mixed.schemaVersion = 3; mixed.presentation.definitions = structuredClone(catalog.presentation.definitions);
const button = structuredClone(catalog.presentation.widgets[0]); button.pageId = mixed.ui.pages[1].id; button.bounds = { x: .1, y: .15, width: .8, height: .4 };
mixed.presentation.widgets.push(button); mixed.ui.controls.push(structuredClone(catalog.ui.controls[0])); mixed.ui.feedbackWidgets.push(structuredClone(catalog.ui.feedbackWidgets[0]));
mixed.targets[0].buttons = 2; mixed.bindings.push({ controlId: button.controlIds[0], targetId: mixed.targets[0].id, kind: 'button', index: 1 });
mixed.ui = normalizeToPortableUi(mixed);

const editorial = structuredClone(catalog); editorial.presentation.groups = [{ id: 'editorial-group', pageId: editorial.ui.pages[0].id, label: 'Cockpit controls' }];
editorial.presentation.widgets[0].groupId = 'editorial-group'; editorial.presentation.widgets[0].style = { foreground: '#ABCdef', background: '#123456', accent: '#ff0000' };
editorial.ui.controls[0].style = { foreground: '#ABCdef' }; editorial.ui.feedbackWidgets[0].style = { accent: '#ABCdef' };
editorial.presentation.viewports[0].safeInsets = { top: .01, right: .01, bottom: .01, left: .01 };
editorial.presentation.viewports[0].regions = [
  { id: 'region-left', label: 'Left', bounds: { x: 0, y: 0, width: .5, height: 1 } },
  { id: 'region-right', label: 'Right', bounds: { x: .5, y: 0, width: .5, height: 1 } },
];
editorial.ui.assets = [{ id: 'artwork', sha256: 'a'.repeat(64), sizeBytes: 100, mime: 'image/png' }];
const art = structuredClone(editorial.presentation.definitions.find(d => d.id === editorial.presentation.widgets[0].config.definitionId)); art.id = 'artwork-button';
art.layers.push({ id: 'artwork', label: 'Artwork', kind: 'image', bounds: { x: .1, y: .1, width: .5, height: .5 }, assetId: 'artwork', opacity: .5 });
editorial.presentation.definitions.push(art); editorial.presentation.widgets[0].config.definitionId = art.id;
const revision = structuredClone(art); revision.version++; revision.title = 'Unused newer revision'; editorial.presentation.definitions.push(revision);
const image = { id: 'legacy-image', pageId: editorial.ui.pages[0].id, label: 'Legacy artwork', kind: 'image', bounds: { x: .1, y: .1, width: .1, height: .1 }, controlIds: [], feedbackIds: [], config: { assetId: 'artwork' } };
editorial.presentation.widgets.push(image); editorial.ui = normalizeToPortableUi(editorial);
// No source payload is needed for admission: assets retain content hashes and
// their prepared pixels follow the existing authenticated image-cache protocol.
const display = structuredClone(mixed); display.ui.controls = []; display.bindings = []; display.targets = [];
display.presentation.widgets = display.presentation.widgets.filter(w => w.controlIds.length === 0);
display.ui.feedbackWidgets = display.ui.feedbackWidgets.filter(f => display.presentation.widgets.some(w => w.feedbackIds.includes(f.id))); display.ui = normalizeToPortableUi(display);

const documents = [catalog, input, mixed, editorial, display, legacy];
const cases = [];
for (const document of documents) {
  const result = validateDesignerDocument(document); if (!result.ok) throw Error(result.error);
  const canonicalDocument = canonical(document);
  // Reuse actual publication identity for every case without artwork payloads.
  const publicationHash = 'sha256:' + Buffer.from(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(canonicalDocument))).toString('hex');
  if (!document.ui.assets.length && (await publishDocument(document, [], { optimizeAssets: false })).publicationHash !== publicationHash) throw Error('Source identity drift');
  cases.push({ document, publication: { canonicalDocument, publicationHash, assets: [] } });
}

const mutations = [];
function add(index, path, value, remove = false) {
  const document = structuredClone(documents[index]); let parent = document;
  for (const key of path.slice(0, -1)) parent = parent[key];
  if (remove) delete parent[path.at(-1)]; else parent[path.at(-1)] = value;
  const result = validateDesignerDocument(document);
  const valid = result.ok && document.bindings.length === document.ui.controls.length && document.ui.pages.every(page => document.presentation.widgets.some(w => w.pageId === page.id));
  mutations.push({ index, path, ...(remove ? { remove: true } : { value }), valid });
}
// Definitions already have 5,965 independent source mutations. Exercise the
// complete panel shape, identity maps, page layout, contracts and metadata here.
function visit(index, value, path = []) {
  if (!value || typeof value !== 'object') return;
  for (const [key, child] of Object.entries(value)) {
    const next = [...path, Array.isArray(value) ? Number(key) : key];
    if (next[0] === 'presentation' && next[1] === 'definitions' && next.length > 2) continue;
    for (const bad of [null, false, -1, 1e10, '', 'unknown', {}, []]) add(index, next, bad);
    if (!Array.isArray(value)) add(index, next, undefined, true);
    visit(index, child, next);
  }
  if (!Array.isArray(value)) add(index, [...path, 'unexpected'], true);
}
visit(0, documents[0]);
// Cover fields absent from the catalog without multiplying identical cases.
for (const path of [ ['presentation','groups',0], ['presentation','viewports',0,'safeInsets'], ['presentation','viewports',0,'regions'], ['ui','assets',0], ['targets',0], ['presentation','widgets',10] ]) {
  let value = documents[3]; for (const key of path) value = value[key]; visit(3, value, path);
}
for (const kind of ['button', 'indicator', 'text', 'numeric', 'gauge']) {
  const index = documents[2].presentation.widgets.findIndex(w => w.kind === kind);
  visit(2, documents[2].presentation.widgets[index], ['presentation','widgets',index]);
}
add(0,['presentation','definitions'], [...catalog.presentation.definitions, catalog.presentation.definitions[0]]);
add(0,['presentation','definitions',0,'layers',0,'unexpected'], true);
add(0,['presentation','widgets',0,'config','definitionVersion'], 2);
add(0,['presentation','widgets',0,'config','properties','unknown'], true);
add(0,['ui','controls',0,'id'], catalog.ui.controls[1].id);
add(0,['ui','pages',0,'items',0,'refId'], catalog.ui.pages[0].items[1].refId);
add(0,['bindings',1,'index'], catalog.bindings[0].index);
add(3,['presentation','viewports',0,'regions',1,'id'], 'region-left');
add(3,['presentation','viewports',0,'regions',1,'bounds','x'], .49);
add(3,['presentation','viewports',0,'regions',1,'bounds','width'], .49);
add(3,['presentation','viewports',0,'backgroundAssetId'], 'artwork');
add(3,['presentation','widgets',0,'config','definitionId'], 'osg-illuminated-button');
add(1,['ui','controls',2,'axisMode'], 'bipolar');
add(1,['ui','controls',2,'fallback'], .5);
add(1,['presentation','widgets',9,'config','properties','page'], 'missing');
add(1,['targets',0,'axes'], ['unipolar','unipolar']);
add(5,['schemaVersion'],3);
add(5,['presentation','definitions'],[]);
add(5,['presentation','widgets',0,'kind'],'component');
add(5,['presentation','widgets',0,'config'],{ definitionId:'missing',definitionVersion:1,properties:{} });
add(5,['presentation','widgets',0],{ ...legacy.presentation.widgets[0],kind:'component',config:{ definitionId:'missing',definitionVersion:1,properties:{} } });
const path = 'apps/companion-esp32/test/panel-source.json';
const result = JSON.stringify({ formatVersion: 1, cases, mutations }) + '\n';
if (process.argv.includes('--check')) { if ((await readFile(path, 'utf8')).replaceAll('\r\n','\n') !== result) throw Error('Panel source fixture differs from the shared model'); }
else await writeFile(path, result);
console.log(`${cases.length} complete panel cases and ${mutations.length} shared validation mutations`);
