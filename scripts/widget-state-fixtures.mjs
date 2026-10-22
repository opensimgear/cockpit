import { readFile, writeFile } from 'node:fs/promises';
import { resolveWidgetState, resolveWidgetLayers, currentWidgetSample } from '../packages/companion-designer/src/index.ts';
import { widgetIndicatorActive, widgetGaugeFraction } from '../packages/companion-ui/src/widget-presentation.ts';

const source = JSON.parse(await readFile('apps/companion-esp32/test/widget-source.json', 'utf8'));
const definitions = source.cases.map(c => c.definition);
const states = ['fresh', 'unknown', 'stale', 'disconnected'];
const provenances = ['requested', 'local', 'host-accepted', 'host-submitted', 'game-confirmed'];
const sources = ['host', 'script', 'simulator'];
// [status, value, provenance, source, receivedAtMs, sourceAgeMs]. Compact cases
// keep the independent shared-code oracle reviewable and cheap to store.
const fresh = (value = false) => [0, value, 4, 2, 100, 0];
const cases = [];
function add(index, specs, { connected = true, enabled = true, pressed = false, now = 100 } = {}) {
  const definition = definitions[index], layers = resolveWidgetLayers(definition, source.cases[index].properties, 'normal');
  const samples = Object.fromEntries(definition.outputs.map((port, i) => [port.name, specs[i] ? {
    status: states[specs[i][0]], value: specs[i][1], provenance: provenances[specs[i][2]], sourceKind: sources[specs[i][3]], receivedAtMs: specs[i][4], sourceAgeMs: specs[i][5],
    outputId: port.name, channelId: port.name, unit: port.unit ?? 'ratio',
  } : undefined]));
  const gate = definition.state.enabledPort ? currentWidgetSample(samples[definition.state.enabledPort], now)?.value === true : true;
  cases.push({ index, specs, connected, enabled, pressed, now,
    state: resolveWidgetState(definition, { connected, enabled, pressed, now, samples }), gate,
    indicators: layers.map(layer => widgetIndicatorActive(layer, samples, now)),
    gauges: layers.map(layer => widgetGaugeFraction(layer, samples, now)),
  });
}
for (let index = 0; index < definitions.length; index++) {
  const baseline = definitions[index].outputs.map(port => fresh(port.name === 'enabled' ? true : port.kind === 'indicator' ? false : port.kind === 'text' ? 'Text' : 50));
  for (const connected of [false, true]) for (const enabled of [false, true]) for (const pressed of [false, true]) add(index, baseline, { connected, enabled, pressed });
}
const baseline = [fresh(false), fresh(false), fresh(true), fresh('Text'), fresh(50)];
const variants = [null, fresh(false), fresh(true), [1, null, 4, 2, 100, 0], [2, true, 4, 2, 100, 0], [3, null, 4, 2, 100, 0], fresh(42), fresh('true')];
for (const active of variants) for (const warning of variants) for (const enabled of variants) for (const pressed of [false, true]) add(11, [active, warning, enabled, baseline[3], baseline[4]], { pressed });
for (let port = 0; port < 5; port++) {
  for (let provenance = 0; provenance < provenances.length; provenance++) for (let origin = 0; origin < sources.length; origin++) {
    const specs = structuredClone(baseline); specs[port] = [0, port < 3 ? true : port === 3 ? 'Text' : 50, provenance, origin, 100, 0]; add(11, specs);
  }
  for (const [now, received, age] of [[100, 100, 0], [100, 101, 0], [1099, 100, 0], [1100, 100, 0], [100, 100, 999], [100, 100, 1000], [101, 100, 999], [100, 0, 900], [100, 0, 899]]) {
    const specs = structuredClone(baseline); specs[port] = [0, port < 3 ? true : port === 3 ? 'Text' : 50, 4, 2, received, age]; add(11, specs, { now });
  }
}
for (const value of [-1000, -20, 0, 40, 100, 1000, false, true, '50', null]) {
  const specs = structuredClone(baseline); specs[4] = fresh(value); add(11, specs);
}
const path = 'apps/companion-esp32/test/widget-state.json', result = JSON.stringify({ formatVersion: 1, cases }) + '\n';
if (process.argv.includes('--check')) { if ((await readFile(path, 'utf8')).replaceAll('\r\n', '\n') !== result) throw Error('Widget state fixture differs from the shared projection'); }
else await writeFile(path, result);
console.log(`${cases.length} shared state, gate, indicator and gauge projections`);
