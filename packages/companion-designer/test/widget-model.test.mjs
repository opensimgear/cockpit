import test from 'node:test';
import assert from 'node:assert/strict';
import {
  validateWidgetDefinition as validate, resolveWidgetProperties, resolveWidgetValue,
  resolveWidgetState, currentWidgetSample, resolveWidgetLayers, widgetSemantics,
  widgetRequiredFeatures, widgetCompatibility, validateWidgetLibrary, saveWidgetDefinition,
  WIDGET_LIMITS,
} from '../src/index.ts';

const box = () => ({ x: 0, y: 0, width: 1, height: 1 });
function seed() {
  return {
    formatVersion: 1, id: 'illuminated-switch', version: 1, title: 'Illuminated switch',
    size: { width: 200, height: 120 }, hitArea: box(),
    properties: [
      { name: 'label', label: 'Label', type: 'text', default: 'ENGINE' },
      { name: 'accent', label: 'Accent', type: 'color', default: '#62e8da' },
      { name: 'art', label: 'Artwork', type: 'asset', default: '' },
      { name: 'enabled', label: 'Show border', type: 'boolean', default: true },
      { name: 'opacity', label: 'Opacity', type: 'number', default: 1, min: 0, max: 1 },
    ],
    inputs: [{ name: 'press', label: 'Press', kind: 'button' }],
    outputs: [{ name: 'active', label: 'Confirmed status', kind: 'indicator' }],
    interaction: { kind: 'momentary', port: 'press' }, state: { confirmedPort: 'active' },
    layers: [
      { id: 'bezel', label: 'Bezel', kind: 'rectangle', bounds: box(), fill: '#10222b', stroke: { property: 'accent' }, strokeWidth: 2, radius: 12, visible: { property: 'enabled' }, opacity: { property: 'opacity' }, states: { normal: { opacity: 0.8 }, pressed: { fill: '#244957', opacity: 1 }, active: { strokeWidth: 4 } } },
      { id: 'label', label: 'Label', kind: 'text', bounds: { x: 0.1, y: 0.25, width: 0.8, height: 0.5 }, text: { property: 'label' }, fill: '#eafafa', font: 'sans', fontSize: 24, align: 'center', states: { disabled: { text: 'DISABLED' } } },
      { id: 'led', label: 'Status', kind: 'indicator', bounds: { x: 0.1, y: 0.1, width: 0.1, height: 0.1 }, fill: { property: 'accent' }, outputPort: 'active' },
    ],
  };
}
const sample = (patch = {}) => ({ channelId: 'channel', outputId: 'output', status: 'fresh', value: true, provenance: 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: 0, receivedAtMs: 100, unit: 'ratio', ...patch });
const context = (patch = {}) => ({ connected: true, enabled: true, pressed: false, now: 100, samples: { active: sample() }, ...patch });

test('portable definitions validate and detach without renderer-specific fields', () => {
  const source = seed(), result = validate(source);
  assert.equal(result.ok, true); source.layers[0].fill = '#ffffff';
  assert.equal(result.value.layers[0].fill, '#10222b');
  assert.equal(validate({ ...seed(), lvglObject: 12 }).ok, false);
  assert.equal(validate(null).ok, false);
  const cycle = seed(); cycle.loop = cycle; assert.equal(validate(cycle).ok, false);
  const huge = seed(); huge.title = 'a'.repeat(WIDGET_LIMITS.bytes); assert.match(validate(huge).error, /budget/);
});

test('strict definition validation rejects invalid geometry, versions, ranges and references', () => {
  const invalid = [
    d => d.formatVersion = 2, d => d.id = 'invalid/id', d => d.version = 0, d => d.version = 1.2,
    d => d.title = '', d => d.size.width = 1, d => d.size.height = Infinity,
    d => d.hitArea.width = 0, d => d.hitArea.x = 1, d => d.hitArea.extra = 1,
    d => d.properties.push(d.properties[0]), d => d.properties[0].type = 'script',
    d => d.properties[0].default = '\n', d => d.properties[1].default = 'red',
    d => d.properties[2].default = 'missing', d => d.properties[3].default = 1,
    d => d.properties[4].default = 2, d => d.properties[4].min = 1, d => d.properties[4].max = NaN,
    d => d.inputs.push(d.inputs[0]), d => d.outputs[0].name = 'press', d => d.inputs[0].kind = 'keyboard',
    d => d.outputs[0].kind = 'javascript', d => d.outputs[0].unit = '\t',
    d => d.outputs[0].range = { min: 0, max: 1 },
    d => d.interaction.port = 'missing', d => d.interaction.kind = 'repeat',
    d => d.interaction = { kind: 'none' }, d => d.interaction = { kind: 'navigate', pageProperty: 'missing' },
    d => d.state.warningPort = 'absent', d => d.state.extra = true,
    d => d.layers = [], d => d.layers.push(d.layers[0]), d => d.layers[0].kind = 'webview',
    d => d.layers[0].bounds.width = -1, d => d.layers[0].radius = 80,
    d => d.layers[0].stroke = { property: 'label' }, d => d.layers[0].opacity = { property: 'missing' },
    d => d.layers[0].strokeWidth = 17, d => d.layers[0].visible = 1, d => d.layers[0].fill = 'url(x)',
    d => d.layers[0].states.javascript = {}, d => d.layers[0].states.active = { bounds: box() },
    d => d.layers[0].states.active = { text: 'wrong layer' }, d => d.layers[0].states.active = { fill: 'red' },
    d => d.layers[1].text = { property: 'accent' }, d => d.layers[1].font = 'remote-url',
    d => d.layers[1].fontSize = 0, d => d.layers[1].align = 'justify',
    d => d.layers[1].outputPort = 'missing', d => d.layers[1].precision = 5,
    d => d.layers[1].unit = '\n', d => d.layers[2].outputPort = 'missing',
    d => d.layers = Array.from({ length: WIDGET_LIMITS.layers + 1 }, () => d.layers[0]),
    d => d.properties = Array.from({ length: WIDGET_LIMITS.properties + 1 }, () => d.properties[0]),
    d => d.inputs = Array.from({ length: WIDGET_LIMITS.ports + 1 }, () => d.inputs[0]),
  ];
  for (const [index, mutate] of invalid.entries()) { const d = seed(); mutate(d); assert.equal(validate(d, []).ok, false, `invalid case ${index}`); }
});

test('layers support ordered shapes, prepared assets, fonts, typed gauges and property references', () => {
  const d = seed(); d.outputs.push({ name: 'level', label: 'Level', kind: 'gauge', unit: '%' });
  d.layers.push(
    { id: 'line', label: 'Line', kind: 'line', bounds: box(), stroke: '#ffffff' },
    { id: 'ellipse', label: 'Ellipse', kind: 'ellipse', bounds: box() },
    { id: 'image', label: 'Image', kind: 'image', bounds: box(), assetId: { property: 'art' } },
    { id: 'gauge', label: 'Gauge', kind: 'gauge', bounds: box(), outputPort: 'level', min: 0, max: 100, orientation: 'radial' },
    { id: 'number', label: 'Number', kind: 'text', bounds: box(), text: '—', font: 'mono', fontSize: 20, align: 'right', outputPort: 'level', precision: 2, unit: '%' },
  );
  d.properties[2].default = 'image-asset'; assert.equal(validate(d, ['image-asset']).ok, true);
  assert.equal(validate(d, []).ok, false);
  const gauge = d.layers[6]; gauge.min = 100; assert.match(validate(d).error, /gauge range/);
  gauge.min = 0; gauge.outputPort = 'active'; assert.match(validate(d).error, /layer output/);
  gauge.outputPort = 'level'; gauge.orientation = 'unknown'; assert.match(validate(d).error, /orientation/);
  gauge.orientation = 'vertical'; gauge.min = { property: 'opacity' }; assert.equal(validate(d).ok, true);
  d.layers[0].opacity = { property: 'opacity' }; d.properties[4].max = 10;
  assert.match(validate(d).error, /range exceeds layer/);
});

test('interaction ports are typed and selector fallback is explicit', () => {
  const d = seed(); d.inputs = [{ name: 'select', label: 'Mode', kind: 'axis', axisMode: 'unipolar', fallback: 0 }];
  d.interaction = { kind: 'selector', port: 'select', options: [{ label: 'Off', value: 0 }, { label: 'On', value: 1 }] };
  assert.equal(validate(d).ok, true);
  for (const mutate of [x => x.inputs[0].axisMode = 'invalid', x => x.inputs[0].fallback = -1, x => x.interaction.options[1].value = 0, x => x.interaction.options.pop(), x => x.inputs[0].fallback = 0.5, x => x.interaction.options[1].value = 2]) {
    const bad = structuredClone(d); mutate(bad); assert.equal(validate(bad).ok, false);
  }
  d.inputs[0].axisMode = 'bipolar'; d.inputs[0].fallback = -1; d.interaction.options[0].value = -1; assert.equal(validate(d).ok, true);
  d.interaction = { kind: 'momentary', port: 'select' }; assert.equal(validate(d).ok, false);
  d.inputs = []; d.interaction = { kind: 'navigate', pageProperty: 'label' }; assert.equal(validate(d).ok, true);
  d.interaction = { kind: 'none' }; assert.equal(validate(d).ok, true);
  d.inputs = [{ name: 'hat', label: 'Hat', kind: 'hat', fallback: 0 }]; assert.equal(validate(d).ok, false);
  d.inputs[0].fallback = -1; assert.equal(validate(d).ok, false, 'no interaction claims unhandled inputs');
  d.inputs = [{ name: 'press', label: 'Press', kind: 'button' }]; d.interaction = { kind: 'toggle', port: 'press' }; assert.equal(validate(d).ok, true);
});

test('properties resolve independently, reject unknown overrides and preserve definitions', () => {
  const d = seed(), overrides = { label: 'POWER', accent: '#ff0000', enabled: false, opacity: 0.5 };
  const result = resolveWidgetProperties(d, overrides);
  assert.equal(result.label, 'POWER'); assert.equal(result.art, ''); assert.equal(d.properties[0].default, 'ENGINE');
  assert.equal(resolveWidgetProperties(d, { art: 'image' }, ['image']).art, 'image');
  assert.throws(() => resolveWidgetProperties(d, { art: 'missing' }, []), /asset/);
  for (const invalid of [{ extra: 'x' }, { label: 1 }, { accent: 'blue' }, { opacity: 2 }, { enabled: 'true' }]) assert.throws(() => resolveWidgetProperties(d, invalid));
  assert.equal(resolveWidgetValue({ property: 'label' }, result), 'POWER'); assert.equal(resolveWidgetValue(3, result), 3);
  const normal = resolveWidgetLayers(d, overrides, 'normal'), pressed = resolveWidgetLayers(d, overrides, 'pressed');
  assert.equal(normal[0].stroke, '#ff0000'); assert.equal(normal[0].visible, false); assert.equal(normal[0].opacity, 0.8);
  assert.equal(pressed[0].fill, '#244957'); assert.equal(pressed[0].opacity, 1); assert.equal(pressed[1].text, 'POWER');
  assert.equal(resolveWidgetLayers(d, {}, 'disabled')[1].text, 'DISABLED');
});

test('state precedence separates local press from fresh game-confirmed status', () => {
  const d = seed(); assert.equal(resolveWidgetState(d, context()), 'active');
  assert.equal(resolveWidgetState(d, context({ pressed: true })), 'pressed');
  assert.equal(resolveWidgetState(d, context({ connected: false, pressed: true })), 'disconnected');
  assert.equal(resolveWidgetState(d, context({ enabled: false, pressed: true })), 'disabled');
  assert.equal(resolveWidgetState(d, context({ samples: {} })), 'unknown');
  assert.equal(resolveWidgetState(d, context({ samples: {}, pressed: true })), 'pressed');
  assert.equal(resolveWidgetState(d, context({ now: 1100, pressed: true })), 'pressed');
  assert.equal(resolveWidgetState(d, context({ pressed: true, samples: { active: sample({ status: 'disconnected' }) } })), 'disconnected');
  for (const provenance of ['requested', 'local', 'host-accepted', 'host-submitted']) assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ provenance }) } })), 'normal');
  assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ value: false }) } })), 'normal');
  assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ sourceKind: 'host' }) } })), 'normal');
  assert.equal(resolveWidgetState(d, context({ now: 1100 })), 'stale');
  assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ status: 'stale' }) } })), 'stale');
  assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ status: 'unknown' }) } })), 'unknown');
  assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ value: 1 }) } })), 'unknown');
  assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ status: 'disconnected' }) } })), 'disconnected');
  d.state = {}; assert.equal(resolveWidgetState(d, context({ samples: {} })), 'normal');
  d.state = { warningPort: 'active' }; assert.equal(resolveWidgetState(d, context({ pressed: true })), 'warning');
  d.state = { enabledPort: 'active' }; assert.equal(resolveWidgetState(d, context({ samples: { active: sample({ value: false }) } })), 'disabled');
});

test('source age cannot become fresh by retransmission or an invalid clock', () => {
  assert.ok(currentWidgetSample(sample({ sourceAgeMs: 800 }), 299));
  assert.equal(currentWidgetSample(sample({ sourceAgeMs: 800 }), 300), undefined);
  for (const [s, now] of [[undefined, 100], [sample({ receivedAtMs: 200 }), 100], [sample({ receivedAtMs: NaN }), 100], [sample({ sourceAgeMs: -1 }), 100], [sample(), NaN]]) assert.equal(currentWidgetSample(s, now), undefined);
});

test('semantics exclude styling and include input behavior and navigation destination', () => {
  const a = seed(), b = seed(); b.title = 'New title'; b.version = 2; b.layers.reverse(); b.layers[0].fill = '#ffffff';
  b.properties[0].default = 'POWER'; b.size.width = 300; b.hitArea.width = 0.8;
  assert.deepEqual(widgetSemantics(a), widgetSemantics(b, { label: 'Different' }));
  b.interaction.kind = 'toggle'; assert.notDeepEqual(widgetSemantics(a), widgetSemantics(b));
  a.inputs = []; a.interaction = { kind: 'navigate', pageProperty: 'label' };
  assert.notDeepEqual(widgetSemantics(a, { label: 'page-a' }), widgetSemantics(a, { label: 'page-b' }));
});

test('compatibility reports exact unsupported features, layer and touch budgets', () => {
  const d = seed(), features = widgetRequiredFeatures(d);
  const profile = { id: 'browser', features, maxLayers: 32, maxDefinitions: 64, maxDocumentBytes: 1048576, minTouchPixels: 44, maxImagePixels: 33554432 };
  assert.deepEqual(widgetCompatibility(d, profile, 'one', 200, 120), []);
  const small = { ...profile, id: 'small-board', features: [], maxLayers: 1 };
  const errors = widgetCompatibility(d, small, 'one', 30, 30);
  assert.equal(errors.length, features.length + 2); assert.ok(errors.every(e => e.widgetId === 'one' && e.message.includes('small-board')));
  assert.ok(errors.some(e => e.feature === 'touchPixels' && e.required === 44 && e.available === 30));
  d.inputs = []; d.interaction = { kind: 'none' }; assert.ok(!widgetCompatibility(d, small, 'one', 10, 10).some(e => e.feature === 'touchPixels'));
});

test('library revisions are bounded, immutable and detached from callers', () => {
  const first = seed(); let library = saveWidgetDefinition({ formatVersion: 1, definitions: [] }, first);
  first.title = 'Changed'; assert.equal(library.definitions[0].title, 'Illuminated switch');
  assert.throws(() => saveWidgetDefinition(library, seed()), /immutable/);
  const next = seed(); next.version = 2; library = saveWidgetDefinition(library, next);
  assert.deepEqual(library.definitions.map(d => d.version), [1, 2]);
  assert.equal(validateWidgetLibrary({ formatVersion: 2, definitions: [] }).ok, false);
  assert.equal(validateWidgetLibrary({ formatVersion: 1, definitions: [seed(), seed()] }).ok, false);
  assert.equal(validateWidgetLibrary({ formatVersion: 1, definitions: [{ bad: true }] }).ok, false);
  assert.equal(validateWidgetLibrary({ formatVersion: 1, definitions: Array.from({ length: 65 }, seed) }).ok, false);
  assert.equal(validateWidgetLibrary({ formatVersion: 1, definitions: [], extra: 'x'.repeat(WIDGET_LIMITS.libraryBytes) }).ok, false);
  assert.throws(() => saveWidgetDefinition({ formatVersion: 2, definitions: [] }, seed()));
  assert.throws(() => saveWidgetDefinition(library, { ...seed(), formatVersion: 2 }));
});
