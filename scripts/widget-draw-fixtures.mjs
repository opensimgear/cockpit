import { readFile, writeFile } from 'node:fs/promises';
import { importBundle, resolveWidgetLayers, resolveWidgetState } from '../packages/companion-designer/src/index.ts';
import { widgetLayerText } from '../packages/companion-ui/src/widget-presentation.ts';

const { document } = await importBundle(await readFile('examples/companion/shared-systems-complete.bundle.json', 'utf8'));
const position=index=>({x:.025+(index%4)*.24,y:.025+Math.floor(index/4)*.24,width:.22,height:.21});
const widgets = document.presentation.widgets.map((widget,index) => ({ definition: document.presentation.definitions.find(d => d.id === widget.config.definitionId && d.version === widget.config.definitionVersion), properties: widget.config.properties, bounds: position(index) }));
widgets.push({ ...widgets[0], properties: { label: 'SECOND', accent: '#ff765e' }, bounds: position(widgets.length) });
const allStates = structuredClone(widgets[0]);
allStates.definition.id = 'all-device-states'; allStates.properties = { label: 'ALL STATES' }; allStates.bounds = position(widgets.length);
allStates.definition.outputs.push({ name: 'warning', label: 'Warning', kind: 'indicator' }, { name: 'enabled', label: 'Enabled', kind: 'indicator' });
allStates.definition.state = { confirmedPort: 'active', warningPort: 'warning', enabledPort: 'enabled' }; widgets.push(allStates);

export const referenceViews = {};
const frames = ['normal', 'pressed', 'active', 'requested', 'warning', 'disabled', 'unknown', 'stale', 'disconnected'].map(name => {
  const contexts = widgets.map(({ definition, properties, bounds }, index) => {
    const status = ['unknown', 'stale', 'disconnected'].includes(name) ? name : 'fresh';
    const samples = Object.fromEntries(definition.outputs.map(port => [port.name, {
      status, value: status === 'unknown' || status === 'disconnected' ? null : port.kind === 'indicator' ? port.name === 'enabled' || port.name === 'warning' && name === 'warning' || port.name === 'active' && ['active', 'requested', 'disabled'].includes(name) : port.kind === 'gauge' ? 50 : 'Sample',
      provenance: name === 'requested' ? 'requested' : 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: name === 'stale' ? 1000 : 0, receivedAtMs: 100,
    }]));
    const context = { samples, now: 100, connected: name !== 'disconnected', enabled: name !== 'disabled', pressed: name === 'pressed' && definition.interaction.kind !== 'none' };
    const state = resolveWidgetState(definition, context), layers = resolveWidgetLayers(definition, properties, state);
    (referenceViews[name] ??= []).push({ widget: { id: `draw-${index}`, bounds }, definition, layers, state, samples, now: context.now });
    return { ...context, state, texts: layers.map(layer => layer.kind === 'text' ? widgetLayerText(layer, samples, context.now) : null) };
  });
  return { name, contexts };
});
const numberLayer = widgets[4].definition.layers.find(layer => layer.kind === 'text' && layer.outputPort);
const textCases = [];
function textCase(value, precision = 0, unit = '', status = 'fresh', age = 0) {
  const sample = { value, status, provenance: 'game-confirmed', sourceKind: 'simulator', receivedAtMs: 100, sourceAgeMs: age };
  const layer = { ...numberLayer, precision, unit };
  textCases.push({ value, precision, unit, status, age, text: widgetLayerText(layer, { [layer.outputPort]: sample }, 100) });
}
const numbers = [0, -0, .5, -.5, 2.5, 2.675, 1.005, .125, -.125, 999999999.99995, -999999999.99995, 999999999999999900000, Number.MIN_VALUE];
let seed = 112n; const bytes = new DataView(new ArrayBuffer(8));
for (let i = 0; i < 256; i++) {
  seed = BigInt.asUintN(64, seed * 6364136223846793005n + 1442695040888963407n);
  const bits = (seed & ((1n << 52n) - 1n)) | ((900n + seed % 193n) << 52n) | (seed & (1n << 63n)); bytes.setBigUint64(0, bits);
  const value = bytes.getFloat64(0); if (Math.abs(value) < 1e21) numbers.push(value);
}
for (const value of numbers) for (let precision = 0; precision <= 4; precision++) textCase(value, precision, precision % 2 ? '%' : '');
for (const value of ['Sample', '', true, false, 'a'.repeat(256), 'é'.repeat(256), '😀'.repeat(128), 'a'.repeat(119) + '😀tail']) textCase(value);
for (const status of ['unknown', 'stale', 'disconnected']) textCase(status === 'stale' ? 50 : null, 1, '%', status);
textCase(50, 1, '%', 'fresh', 1000);
const fixture = JSON.stringify({ formatVersion: 1, widgets, frames, textCases }) + '\n', path = 'apps/companion-esp32/test/widget-draw.json';
if (process.argv.includes('--check')) { if ((await readFile(path, 'utf8')).replaceAll('\r\n', '\n') !== fixture) throw Error('Widget draw source/reference fixture differs'); }
else if (process.argv[1]?.endsWith('widget-draw-fixtures.mjs')) await writeFile(path, fixture);
