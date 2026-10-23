import test from 'node:test';
import assert from 'node:assert/strict';
import { newDocument, createDesignerHistory, applyDesignerCommand, createWidgetCatalog } from '../../companion-designer/src/index.ts';
import { createPreviewController } from './controller.ts';
import { componentDefinition, componentSamples, componentPresentation, widgetLayerText, widgetGaugeFraction, widgetIndicatorActive, radialGaugePath } from './widget-presentation.ts';

function panel(index = 0, definitionPatch) {
  const definition = createWidgetCatalog()[index]; definitionPatch?.(definition);
  const document = applyDesignerCommand(createDesignerHistory(newDocument()), { type: 'add-component', definition }).present;
  return { document, widget: document.presentation.widgets.at(-1), definition };
}
function feedback(document, widget, value = true, patch = {}) {
  const output = document.ui.feedbackWidgets.find(f => f.id === widget.feedbackIds[0]);
  const port = componentDefinition(document, widget).outputs[0];
  return { [output.channelId]: { channelId: output.channelId, outputId: output.id, status: 'fresh', value, provenance: 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: 0, receivedAtMs: 100, unit: port.unit ?? 'ratio', ...(port.range ? { range: port.range } : {}), ...patch } };
}

test('capsule toggles latch their requested input while artwork follows confirmed OFF/ON feedback',()=>{
  for(const index of [12,13]){
    const {document,widget}=panel(index),engine=createPreviewController(document,{clock:()=>100});engine.arm();
    assert.ok(engine.begin(1,widget.id));assert.ok(engine.release(1));
    assert.equal(engine.snapshot().values[widget.controlIds[0]],true);
    assert.equal(componentPresentation(document,widget,{},100).state,'unknown');
    const off=componentPresentation(document,widget,feedback(document,widget,false),100);
    assert.ok(off.layers.find(l=>l.id==='off-thumb').visible);assert.equal(off.layers.find(l=>l.id==='on-thumb').visible,false);
    const confirmed=feedback(document,widget,true);
    assert.ok(engine.begin(2,widget.id));assert.ok(engine.release(2));assert.equal(engine.snapshot().values[widget.controlIds[0]],false);
    assert.equal(componentPresentation(document,widget,confirmed,100).state,'active');
    assert.equal(componentPresentation(document,widget,confirmed,1100).state,'stale');engine.disarm();
  }
});

test('requested toggle artwork follows input immediately and simulator confirmation remains independent',()=>{
 for(const index of [12,13]){
  const {document,widget}=panel(index),engine=createPreviewController(document,{clock:()=>100});engine.arm();
  const view=(samples={},now=100,extra={})=>componentPresentation(document,widget,samples,now,{requested:engine.snapshot().values[widget.controlIds[0]],...extra});
  assert.equal(view().confirmation,'Game: unknown');
  assert.ok(view().layers.find(l=>l.id==='off-thumb').visible);
  engine.begin(1,widget.id);
  assert.ok(view({},100,{pressed:true}).layers.find(l=>l.id==='on-thumb').visible);
  engine.release(1);
  assert.ok(view().layers.find(l=>l.id==='on-thumb').visible);
  const off=feedback(document,widget,false),on=feedback(document,widget,true);
  assert.equal(view(off).confirmation,'Game: OFF');
  assert.ok(view(off).layers.find(l=>l.id==='on-thumb').visible);
  assert.equal(view(on).confirmation,'Game: ON');
  assert.equal(view(on,1100).confirmation,'Game: stale');
  assert.equal(view(feedback(document,widget,true,{provenance:'requested',sourceKind:'host'})).confirmation,'Game: unknown');
  assert.equal(view(feedback(document,widget,true,{status:'stale'})).confirmation,'Game: stale');
  assert.equal(view(feedback(document,widget,true,{status:'disconnected'})).confirmation,'Game: disconnected');
  assert.equal(view({},100,{connected:false}).confirmation,'Game: disconnected');
  engine.begin(2,widget.id);engine.release(2);
  assert.ok(view(on).layers.find(l=>l.id==='off-thumb').visible);
  assert.equal(view(on).confirmation,'Game: ON');
  engine.begin(3,widget.id);engine.release(3);engine.disarm();
  assert.ok(view().layers.find(l=>l.id==='off-thumb').visible);
 }
 const {document,widget}=panel(0);
 assert.equal(componentPresentation(document,widget,{},100,{requested:true}).confirmation,undefined);
});

test('one shared composition projection preserves identities and confirmed-state semantics', () => {
  const { document, widget, definition } = panel();
  const samples = feedback(document, widget);
  assert.equal(componentDefinition(document, widget).id, definition.id);
  assert.equal(componentDefinition(document, document.presentation.widgets[0]), undefined);
  assert.equal(componentPresentation(document, document.presentation.widgets[0], {}, 100), undefined);
  const view = componentPresentation(document, widget, samples, 100);
  assert.equal(view.state, 'active'); assert.equal(view.interactionEnabled, true);
  assert.deepEqual(view.layers.map(l => l.id), definition.layers.map(l => l.id));
  assert.equal(view.layers[0].stroke, '#63dfd0');
  assert.equal(componentPresentation(document, widget, samples, 100, { pressed: true }).state, 'pressed');
  assert.equal(componentPresentation(document, widget, {}, 100, { pressed: true }).state, 'pressed');
  assert.equal(view.properties.label, definition.properties.find(p => p.name === 'label').default);
  assert.equal(componentPresentation(document, widget, samples, 100, { connected: false }).state, 'disconnected');
  assert.equal(componentPresentation(document, widget, samples, 100, { enabled: false }).state, 'disabled');
  assert.equal(componentPresentation(document, widget, samples, 1100).state, 'stale');
  assert.equal(componentPresentation(document, widget, feedback(document, widget, true, { provenance: 'requested', sourceKind: 'host' }), 100).state, 'normal');
});

test('feedback projection refuses another output, bad types, units and ranges', () => {
  const { document, widget, definition } = panel();
  for (const patch of [{ outputId: 'different' }, { channelId: 'different' }, { value: 1 }, { range: { min: 0, max: 1 } }]) {
    assert.equal(componentSamples(document, widget, definition, feedback(document, widget, true, patch)).active, undefined);
  }
  for (const status of ['unknown', 'disconnected']) assert.equal(componentSamples(document, widget, definition, feedback(document, widget, undefined, { status })).active.status, status);
  const gauge = panel(5);
  assert.equal(componentSamples(gauge.document, gauge.widget, gauge.definition, feedback(gauge.document, gauge.widget, 50, { unit: '%' })).value.value, 50);
  assert.equal(componentSamples(gauge.document, gauge.widget, gauge.definition, feedback(gauge.document, gauge.widget, 50, { unit: 'ratio' })).value, undefined);
  assert.equal(componentSamples(gauge.document, gauge.widget, gauge.definition, feedback(gauge.document, gauge.widget, 50, { unit: '%', range: { min: 0, max: 10 } })).value, undefined);
  const text = panel(4, d => d.outputs[0] = { name: 'value', label: 'Value', kind: 'text' });
  assert.equal(componentSamples(text.document, text.widget, text.definition, feedback(text.document, text.widget, 'hello')).value.value, 'hello');
});

test('only a fresh explicit enable channel permits interaction when a gate is declared', () => {
  const { document, widget } = panel(0, d => d.state.enabledPort = 'active');
  assert.equal(componentPresentation(document, widget, {}, 100).interactionEnabled, false);
  assert.equal(componentPresentation(document, widget, feedback(document, widget, false), 100).interactionEnabled, false);
  assert.equal(componentPresentation(document, widget, feedback(document, widget, true), 100).interactionEnabled, true);
  assert.equal(componentPresentation(document, widget, feedback(document, widget, true), 1100).interactionEnabled, false);
});

test('text, indicator and gauge values never promote stale or requested state to confirmation', () => {
  const { document, widget } = panel(5);
  const view = componentPresentation(document, widget, feedback(document, widget, 50, { unit: '%' }), 100);
  const text = view.layers.find(l => l.kind === 'text' && l.outputPort), gauge = view.layers.find(l => l.kind === 'gauge');
  assert.equal(widgetLayerText(text, view.outputs, 100), '50.0 %');
  assert.equal(widgetLayerText(text, view.outputs, 1100), 'STALE');
  assert.equal(widgetLayerText(text, {}, 100), '—');
  for (const [value, expected] of [['a'.repeat(256), 'a'.repeat(120)], ['é'.repeat(256), 'é'.repeat(120)], ['😀'.repeat(128), '😀'.repeat(60)], ['a'.repeat(119) + '😀tail', 'a'.repeat(119)]]) {
    assert.equal(widgetLayerText(text, { value: { ...view.outputs.value, value } }, 100), expected);
  }
  assert.equal(widgetLayerText(view.layers[1], view.outputs, 100), 'LEVEL');
  assert.equal(widgetLayerText(gauge, view.outputs, 100), '');
  assert.equal(widgetGaugeFraction(gauge, view.outputs, 100), 0.5);
  assert.equal(widgetGaugeFraction(gauge, view.outputs, 1100), 0);
  assert.equal(widgetGaugeFraction(text, view.outputs, 100), 0);
  assert.equal(widgetGaugeFraction(gauge, { value: { ...view.outputs.value, value: 200 } }, 100), 1);
  const status = panel(); const indicator = status.definition.layers.find(l => l.kind === 'indicator');
  assert.equal(widgetIndicatorActive(indicator, { active: { ...Object.values(feedback(status.document, status.widget))[0] } }, 100), true);
  assert.equal(widgetIndicatorActive(indicator, { active: { ...Object.values(feedback(status.document, status.widget))[0], provenance: 'requested' } }, 100), false);
  assert.equal(widgetIndicatorActive(indicator, {}, 100), false); assert.equal(widgetIndicatorActive(text, {}, 100), false);
  for (const fraction of [0, 0.5, 1]) assert.match(radialGaugePath(10, 10, 200, 200, fraction), /^M .* A /);
  assert.ok(!radialGaugePath(0, 0, 100, 100, 1).includes('NaN'));
});

test('component momentary, toggle and selector inputs use existing cancellation and ownership', () => {
  for (const index of [0, 1, 2]) {
    const { document, widget } = panel(index); const transitions = [];
    const engine = createPreviewController(document, { clock: () => 0, onchange: changes => transitions.push(...changes) });
    engine.arm();
    assert.equal(engine.begin(1, widget.id, index === 2 ? { value: 0.5 } : {}), true);
    assert.deepEqual(engine.snapshot().pressedWidgetIds, [widget.id]);
    assert.equal(engine.begin(2, widget.id), false);
    assert.equal(engine.release(1), true); assert.deepEqual(engine.snapshot().pressedWidgetIds, []);
    assert.equal(engine.snapshot().values[widget.controlIds[0]], index === 0 ? false : index === 1 ? true : 0.5);
    engine.setWidgetEnabled(widget.id, false);
    assert.equal(engine.snapshot().values[widget.controlIds[0]], index === 2 ? 0 : false);
    assert.equal(engine.begin(3, widget.id), false);
    engine.setWidgetEnabled(widget.id, true); engine.setWidgetEnabled(widget.id, true);
    assert.equal(engine.begin(3, widget.id, index === 2 ? { value: 1 } : {}), true);
    engine.release(3, 'Cancelled', true);
    assert.equal(engine.snapshot().values[widget.controlIds[0]], index === 2 ? 0 : false);
    assert.ok(transitions.length >= 4);
    engine.disarm('Lost session'); assert.deepEqual(engine.snapshot().pressedWidgetIds, []);
  }
  const display = panel(4), engine = createPreviewController(display.document); engine.arm();
  assert.equal(engine.begin(1, display.widget.id), false, 'presentation cannot create input');
});
