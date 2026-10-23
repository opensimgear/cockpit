import { readFile, writeFile } from 'node:fs/promises';
import { importBundle, publishDocument } from '../packages/companion-designer/src/assets.ts';
import { normalizeToPortableUi, checked, canonical } from '../packages/companion-designer/src/model.ts';
import { componentDefinition, componentPresentation } from '../packages/companion-ui/src/widget-presentation.ts';
import { createPreviewController } from '../packages/companion-ui/src/controller.ts';

const bundle = await importBundle(await readFile('tests/fixtures/legacy-companion/shared-systems.bundle.json', 'utf8'));
let document = structuredClone(bundle.document);
document.ui.id = 'shared-input-traces'; document.ui.title = 'Shared input traces';
document.ui.pages.push({ id: 'page-1', title: 'Second page', items: [] });
document.presentation.viewports.push({ pageId: 'page-1', width: 800, height: 480 });
document.presentation.widgets[9].config.properties.page = 'page-1';
const toggle = componentDefinition(document, document.presentation.widgets[1]);
function add(definition, index, pageId, x) {
  if (!document.presentation.definitions.some(d => d.id === definition.id)) document.presentation.definitions.push(definition);
  const controlIds = definition.inputs.map((port, p) => {
    const { name: _, ...descriptor } = port; const id = `trace-input-${index}-${p}`;
    document.ui.controls.push({ ...descriptor, id }); return id;
  });
  const feedbackIds = definition.outputs.map((port, p) => {
    const id = `trace-output-${index}-${p}`; document.ui.feedbackWidgets.push({ id, label: port.label, kind: port.kind, channelId: `trace-channel-${index}-${p}` }); return id;
  });
  document.presentation.widgets.push({ id: `widget-${index}`, pageId, label: definition.title, kind: 'component', bounds: { x, y: pageId === 'page-0' ? .665 : .025, width: .22, height: .28 }, controlIds, feedbackIds, config: { definitionId: definition.id, definitionVersion: definition.version, properties: {} } });
}
const gated = structuredClone(toggle); gated.id = 'gated-toggle'; gated.hitArea = { x: .1, y: .1, width: .8, height: .8 };
gated.outputs.push({ name: 'enabled', label: 'Enabled', kind: 'indicator' }); gated.state.enabledPort = 'enabled';
add(gated, 10, 'page-0', .505); add(toggle, 11, 'page-0', .745); add(toggle, 12, 'page-1', .025);
const bipolar = structuredClone(componentDefinition(document, document.presentation.widgets[2])); bipolar.id = 'bipolar-selector';
bipolar.inputs[0].axisMode = 'bipolar'; bipolar.inputs[0].fallback = -1;
bipolar.interaction.options = [{ label: 'Reverse', value: -1 }, { label: 'Idle', value: 0 }, { label: 'Forward', value: 1 }]; add(bipolar, 13, 'page-1', .265);
const text = structuredClone(componentDefinition(document, document.presentation.widgets[4])); text.id = 'portable-text'; text.outputs = [{ name: 'value', label: 'Text', kind: 'text', unit: 'é'.repeat(120) }];
add(text, 14, 'page-1', .505);
document.ui = normalizeToPortableUi(document); document = checked(document);
const target = { id: 'trace-target', label: 'Logical trace controller', axes: [], buttons: 0, hats: 0 };
document.targets = [target]; document.bindings = document.ui.controls.map(control => {
  const index = control.kind === 'button' ? target.buttons++ : control.kind === 'hat' ? target.hats++ : target.axes.push(control.axisMode) - 1;
  return { controlId: control.id, targetId: target.id, kind: control.kind, index };
});
const publication = await publishDocument(document, []);
let now = 0, page = 'page-0', navigation;
const feedback = {}, controller = createPreviewController(document, { clock: () => now });
const refresh = () => { for (const widget of document.presentation.widgets) controller.setWidgetEnabled(widget.id, componentPresentation(document, widget, feedback, now)?.interactionEnabled ?? false); };
refresh();
const down = index => ({ type: 'down', widgetId: `widget-${index}` });
const up = { type: 'up' }, out = { type: 'out' }, arm = { type: 'arm' }, disarm = { type: 'disarm' };
const enabled = value => ({ type: 'feedback', widgetId: 'widget-10', value });
const actions = [up, arm, down(0), up, down(1), up, down(11), up, down(1), out, up, down(1), up,
  down(2), up, down(2), out, up, down(10), up, enabled(true), down(10), up, enabled(false), enabled(true), down(10), up,
  { type: 'tick', now: 1000 }, enabled(true), down(10), out, up, down(0), disarm, up, arm,
  down(9), up, up, arm, down(12), up, down(13), up, down(13), up, down(13), out, up, disarm, up];
let feedbackSequence = 0;
const steps = actions.map((action, index) => {
  let ack;
  if (action.type === 'arm') controller.arm();
  else if (action.type === 'disarm') controller.disarm();
  else if (action.type === 'tick') { now = action.now; refresh(); }
  else if (action.type === 'feedback') {
    const widget = document.presentation.widgets.find(w => w.id === action.widgetId), definition = componentDefinition(document, widget);
    const port = definition.outputs.findIndex(p => p.name === definition.state.enabledPort), output = document.ui.feedbackWidgets.find(o => o.id === widget.feedbackIds[port]);
    const sample = { outputId: output.id, channelId: output.channelId, value: action.value, status: 'fresh', unit: 'ratio', provenance: 'local', sourceKind: 'host', sourceAgeMs: 0 };
    feedback[output.channelId] = { ...sample, receivedAtMs: now }; refresh();
    ack = { ok: true, type: 'ack', clientId: 'client', sessionId: 'session', generation: '1', sequence: String(index + 1), ackStage: 'source-accepted', submitted: false,
      feedback: { version: 1, type: 'feedback', clientId: 'client', sessionId: 'session', generation: '1', publicationHash: publication.publicationHash, controllerInstanceId: 'instance', sequence: String(++feedbackSequence), bindingGeneration: '1', mode: 'snapshot', bindings: [{ outputId: output.id, channelId: output.channelId }], values: [sample] } };
  } else if (action.type === 'down') {
    const widget = document.presentation.widgets.find(w => w.id === action.widgetId), definition = componentDefinition(document, widget);
    if (definition.interaction.kind === 'navigate') navigation = definition.interaction.pageProperty ? widget.config.properties[definition.interaction.pageProperty] : undefined;
    else {
      let data = {};
      if (definition.interaction.kind === 'selector') { const options = definition.interaction.options, current = controller.snapshot().values[widget.controlIds[0]], selected = options.findIndex(o => o.value === current); data = { value: options[(selected + 1) % options.length].value }; }
      controller.begin(0, widget.id, data);
    }
  } else if (action.type === 'out') { controller.release(0, 'Pointer cancelled', true); navigation = undefined; }
  else if (action.type === 'up') {
    if (navigation) { controller.disarm('Page changed'); page = navigation; navigation = undefined; }
    else controller.release(0);
  }
  const state = controller.snapshot();
  const states = Object.fromEntries(document.presentation.widgets.map(widget => [widget.id, componentPresentation(document, widget, feedback, now, { connected: true, enabled: widget.controlIds.length === 0 || state.armed, pressed: state.pressedWidgetIds.includes(widget.id) }).state]));
  return { ...action, now, ...(ack ? { ack } : {}), expected: { page, armed: state.armed, values: state.values, pressed: state.pressedWidgetIds, states, changes: state.events.flatMap(event => event.changes) } };
});
const result = JSON.stringify({ formatVersion: 1, document, canonicalDocument: canonical(document), publicationHash: publication.publicationHash, steps }) + '\n';
const path = 'apps/companion-esp32/test/widget-input.json';
if (process.argv.includes('--check')) { if ((await readFile(path, 'utf8')).replaceAll('\r\n', '\n') !== result) throw Error('Shared input fixture differs from actual controller behavior'); }
else await writeFile(path, result);
console.log(`${steps.length} shared momentary/toggle/selector/gate/cancellation/navigation trace steps`);
