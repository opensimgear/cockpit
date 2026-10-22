import { readFile, writeFile } from 'node:fs/promises';
import { importBundle, createWidgetCatalog, validateWidgetDefinition, resolveWidgetProperties, resolveWidgetLayers, WIDGET_STATES } from '../packages/companion-designer/src/index.ts';

// Independent TypeScript oracle for device-side source admission and resolution.
const { document } = await importBundle(await readFile('examples/companion/shared-systems.bundle.json', 'utf8'));
const bounds = { x: .1, y: .1, width: .8, height: .8 };
const custom = {
  formatVersion: 1, id: 'all-source-fields', version: 7, title: 'All source fields', size: { width: 320, height: 240 },
  properties: [
    { name: 'caption', label: 'Caption', type: 'text', default: 'Aé 😃' },
    { name: 'tint', label: 'Tint', type: 'color', default: '#ABCdef' },
    { name: 'art', label: 'Artwork', type: 'asset', default: 'artwork' },
    { name: 'visible', label: 'Visible', type: 'boolean', default: true },
    { name: 'radius', label: 'Radius', type: 'number', default: 3, min: 0, max: 8 },
    { name: 'opacity', label: 'Opacity', type: 'number', default: .8, min: 0, max: 1 },
    { name: 'minimum', label: 'Minimum', type: 'number', default: -20, min: -100, max: -1 },
  ],
  inputs: [], outputs: [
    { name: 'active', label: 'Active', kind: 'indicator' },
    { name: 'warning', label: 'Warning', kind: 'indicator', unit: 'bool' },
    { name: 'enabled', label: 'Enabled', kind: 'indicator' },
    { name: 'caption', label: 'Caption', kind: 'text', unit: '' },
    { name: 'value', label: 'Value', kind: 'gauge', unit: '%', range: { min: -100, max: 100 } },
  ], interaction: { kind: 'none' }, hitArea: bounds, state: { confirmedPort: 'active', warningPort: 'warning', enabledPort: 'enabled' },
  layers: [
    { id: 'rect', label: 'Rectangle', kind: 'rectangle', bounds, radius: { property: 'radius' }, fill: { property: 'tint' }, stroke: '#123456', strokeWidth: { property: 'radius' }, visible: { property: 'visible' }, opacity: { property: 'opacity' }, states: { normal: { fill: '#102030', strokeWidth: 2 }, pressed: { fill: '#ffffff', visible: false }, warning: { stroke: '#ff0000' } } },
    { id: 'oval', label: 'Ellipse', kind: 'ellipse', bounds },
    { id: 'line', label: 'Line', kind: 'line', bounds, fill: '#ffffff' },
    { id: 'text', label: 'Text', kind: 'text', bounds, text: { property: 'caption' }, font: 'mono', fontSize: 128, align: 'right', outputPort: 'caption', precision: 4, unit: 'km', states: { normal: { text: 'NORMAL' }, active: { text: { property: 'caption' } }, stale: { text: 'STALE' }, unknown: { text: '—' }, disconnected: { opacity: 0 } } },
    { id: 'art', label: 'Image', kind: 'image', bounds, assetId: { property: 'art' } },
    { id: 'lamp', label: 'Indicator', kind: 'indicator', bounds, outputPort: 'active' },
    ...['horizontal', 'vertical', 'radial'].map((orientation, index) => ({ id: `gauge-${index}`, label: 'Gauge', kind: 'gauge', bounds, outputPort: 'value', min: { property: 'minimum' }, max: 100, orientation })),
  ],
};
const assets = [{ id: 'artwork' }];
const sourceCases = document.presentation.widgets.map(widget => ({ definition: document.presentation.definitions.find(d => d.id === widget.config.definitionId && d.version === widget.config.definitionVersion), properties: widget.config.properties }));
sourceCases.push({ definition: sourceCases[0].definition, properties: { label: 'SECOND', accent: '#ff0000' } });
sourceCases.push({ definition: custom, properties: {} }, { definition: custom, properties: { caption: '独立', tint: '#123456', art: '', visible: false, radius: 8, opacity: .1, minimum: -99 } });
const bipolar = structuredClone(sourceCases.find(c => c.definition.interaction.kind === 'selector'));
bipolar.definition.id = 'bipolar-selector'; bipolar.definition.inputs[0].axisMode = 'bipolar'; bipolar.definition.inputs[0].fallback = -1;
bipolar.definition.interaction.options = [{ label: 'Reverse', value: -1 }, { label: 'Forward', value: 1 }];
sourceCases.push(bipolar);
// Empty text/asset values and all three text alignments are legitimate source.
const empty = structuredClone(custom); empty.id = 'empty-literals'; empty.title = 'Empty'; empty.layers = [empty.layers[3], empty.layers[4]];
empty.layers[0].text = ''; empty.layers[0].align = 'left'; delete empty.layers[0].states; empty.layers[1].assetId = '';
sourceCases.push({ definition: empty, properties: {} });
sourceCases.push(...createWidgetCatalog().filter(d=>d.id.startsWith('osg-capsule-toggle-')).map(definition=>({definition,properties:{}})));

function valid(definition, properties) {
  const checked = validateWidgetDefinition(definition, assets.map(a => a.id));
  if (!checked.ok) return false;
  try { resolveWidgetProperties(checked.value, properties, assets.map(a => a.id)); for (const state of WIDGET_STATES) resolveWidgetLayers(checked.value, properties, state); return true; }
  catch { return false; }
}
const cases = sourceCases.map(({ definition, properties }) => {
  if (!valid(definition, properties)) throw Error('Invalid source fixture ' + definition.id);
  return { definition, properties, resolved: WIDGET_STATES.map(state => resolveWidgetLayers(definition, properties, state)) };
});

// Mutations stay compact, while the C harness independently applies them and
// compares its decision against the shared source validator for every case.
const mutations = [];
function add(index, root, path, value, remove = false) {
  const source = structuredClone(sourceCases[index]); let parent = source[root];
  for (const key of path.slice(0, -1)) parent = parent[key];
  if (remove) delete parent[path.at(-1)]; else parent[path.at(-1)] = value;
  mutations.push({ index, root, path, ...(remove ? { remove: true } : { value }), valid: valid(source.definition, source.properties) });
}
for (const index of [0, 2, 10, 11, 13]) {
  function visit(value, path = []) {
    if (!value || typeof value !== 'object') return;
    for (const [key, child] of Object.entries(value)) {
      const next = [...path, Array.isArray(value) ? Number(key) : key];
      for (const bad of [null, false, -1, 1e10, '', 'unknown', {}, []]) add(index, 'definition', next, bad);
      if (!Array.isArray(value)) add(index, 'definition', next, undefined, true);
      visit(child, next);
    }
    if (!Array.isArray(value)) add(index, 'definition', [...path, 'unexpected'], true);
  }
  visit(sourceCases[index].definition);
}
for (const [key, value] of Object.entries({ unknown: 1, caption: false, tint: '#fff', art: 'missing', visible: 0, radius: 9, opacity: -.1, minimum: 1 })) add(11, 'properties', [key], value);
for (const title of ['   ', '\u00a0\u1680\u2000\u200a\u2028\u2029\u202f\u205f\u3000\ufeff', ' Å ', '界', '😀']) add(11, 'definition', ['title'], title);
add(11, 'definition', ['layers', 6, 'min'], 101);
add(11, 'definition', ['properties', 6, 'default'], -99);
add(11, 'definition', ['layers', 0, 'states', 'normal', 'opacity'], { property: 'radius' });
add(11, 'definition', ['layers', 0, 'states', 'normal', 'visible'], { property: 'visible', unexpected: true });
const path = 'apps/companion-esp32/test/widget-source.json';
const result = JSON.stringify({ formatVersion: 1, assets, states: WIDGET_STATES, cases, mutations }) + '\n';
if (process.argv.includes('--check')) { if ((await readFile(path, 'utf8')).replaceAll('\r\n', '\n') !== result) throw Error('Widget source admission fixture differs from the portable model'); }
else await writeFile(path, result);
console.log(`${cases.length} admitted examples and ${mutations.length} differential source mutations`);
