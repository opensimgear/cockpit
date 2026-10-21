import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import { loadModules } from './test-module-loader.mjs';

const modules = await loadModules(['./profile-editor', './profile-canvas', './profile-diagram', './profile-processing', './profile-port-browser','./profile-wire-layout','./profile-input-availability','./canvas-styling','./canvas-navigation','./profile-canvas-render']);
const plain = value => JSON.parse(JSON.stringify(value));
const port = (id, direction, name = id) => ({ id, direction, name, contract: { delivery: 'state', schema: { type: 'boolean' }, freshnessMs: 1000, fallback: null } });
const resource = (id, ports = []) => ({ id, name: id, kind: 'device', required: false, ports });
const profile = () => ({
  schemaVersion: 2, id: '11111111-1111-4111-8111-111111111111', name: 'Wiring gestures', revision: 1,
  eligibility: { gameIds: [] }, resources: [resource('source', [port('saved-fire', 'output')]), resource('target', [port('saved-led', 'input')]), resource('empty')],
  bindings: [], processingNodes: [], virtualOutputs: [], visibility: [], overrides: [], companionAssignments: [],
  editor: { positions: [{ nodeId: 'source', x: 20, y: 40 }, { nodeId: 'target', x: 400, y: 40 }, { nodeId: 'empty', x: 20, y: 350 }], viewport: { x: 0, y: 0, zoom: 1 } }
});
const noRequest = () => { throw new Error('Canvas gestures must not issue inventory or native requests.'); };
const transpile = source => ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const iconSource = await readFile(new URL('./Icon.svelte', import.meta.url), 'utf8');
const iconCode = transpile(compile(iconSource, { filename: 'Icon.svelte', generate: 'server' }).js.code);
let canvasSource = await readFile(new URL('./ProfileCanvas.svelte', import.meta.url), 'utf8');
canvasSource = canvasSource.replace('</script>', `captureCanvas({begin,beginWire,beginReconnect,beginSegment,addWireHandle,beginHandle,handleKeyboard,inputState,wireClick,move,scheduleMove,wheel,finish,keyboard,lostPointer,choose,chooseConnection,syncSelectionFocus,blockPorts,activeHandle,overPalette,dropPalette,setStage(value){stage=value;},setView(value){view=value;},setInspector(value){inspectorPanel=value;},setSelection(value){selectedId=value;},get state(){return{wire,wireTargetId,wireTargetPort,wireRejectedId,wireRejectedPort,wireRejectedReason,drag,view,selectedId,selectionView,focusedSelectionId,suppressWireClick,paletteConnectionId,segmentDrag,segmentPreview,segmentBlocked,routes};}});</script>`);
const canvasCode = transpile(compile(canvasSource, { filename: 'ProfileCanvas.svelte', generate: 'server' }).js.code);

// Invoke the production Svelte closures with explicit pointer-capture and hit-test fixtures.
// Actual browser dispatch and hit testing are covered by the running cockpit UI checks.
function harness(props = {}, arrowStyle='smooth',cornerRadius=24,wheelAction='pan') {
  const styling=modules.get('./canvas-styling').canvasStyling;styling.setArrowStyle(arrowStyle);styling.setCornerRadius(cornerRadius);
  modules.get('./canvas-navigation').canvasNavigation.setWheelAction(wheelAction);
  const calls = [], edits = [], captured = new Set(), focuses=[];
  const frames=new Map();let frameId=0;
  let handlers, hit = element();
  const context = {
    requestAnimationFrame:callback=>{const id=++frameId;frames.set(id,callback);return id;},cancelAnimationFrame:id=>frames.delete(id),
    TextEncoder, captureCanvas: value => handlers = value,
    window: { document: { elementFromPoint: () => hit } },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return svelte;
      if (name === './Icon.svelte') return { default: Icon };
      const key = name.replace(/\.ts$/, '');
      if (modules.has(key)) return modules.get(key);
      throw new Error(`Unexpected Canvas dependency ${name}`);
    }
  };
  const iconExports = {};
  vm.runInNewContext(iconCode, { ...context, exports: iconExports }, { filename: 'Icon.svelte' });
  const Icon = iconExports.default, canvasExports = {};
  vm.runInNewContext(canvasCode, { ...context, exports: canvasExports }, { filename: 'ProfileCanvas.svelte' });
  const document = props.document ?? profile(), before = plain(document);
  const body = render(canvasExports.default, { props: {
    document, assignments: [], inventory: [], inventoryClient: { inspect: noRequest },
    change: (...args) => edits.push(args), connectBlocks: (...args) => calls.push(['connect', ...args]),
    connectionFrom: id => calls.push(['from', id]), connectionTo: id => calls.push(['to', id]),
    closeConnectionPanel: () => calls.push(['close']), ...props
  } }).body;
  handlers.setStage({
    clientWidth:1000,clientHeight:800,
    focus:options=>focuses.push(options),
    setPointerCapture: id => captured.add(id), hasPointerCapture: id => captured.has(id), releasePointerCapture: id => captured.delete(id),
    getBoundingClientRect: () => ({ left: 100, top: 50, right: 1100, bottom: 850 }), contains: target => target.inside !== false
  });
  return { handlers, body, calls, edits, captured, focuses, document, before, frames,flushFrames(){const queued=[...frames.values()];frames.clear();for(const callback of queued)callback(0);}, setHit(value) { hit = value; } };
}
function element({ nodeId, input, output, portId, disabled = false, inside = true, panel = false } = {}) {
  const value = { disabled, inside, dataset: { connectionTarget: input, connectionSource: output, connectionPort: portId } };
  value.closest = selector => {
    if (selector === '[data-connection-target]') return input ? value : null;
    if (selector === '[data-connection-source]') return output ? value : null;
    return nodeId || panel ? value : null;
  };
  return value;
}
const pointer = (x = 100, y = 100, extra = {}) => ({ button: 0, pointerId: 7, clientX: x, clientY: y, preventDefault() {}, stopPropagation() {}, ...extra });
const click = (detail = 1) => ({ detail, stopPropagation() {} });
const button = (body, attribute, id) => {
  const tag = body.match(new RegExp(`<button[^>]*${attribute}="${id}"[^>]*>`));
  assert.ok(tag, `Missing ${attribute} handle for ${id}`);
  return tag[0];
};
const namedButton = (body, nodeId, portId) => {
  const tag = [...body.matchAll(/<button\b[^>]*>/g)].map(([tag]) => tag).find(tag => (tag.includes(`data-connection-source="${nodeId}"`) || tag.includes(`data-connection-target="${nodeId}"`)) && tag.includes(`data-connection-port="${portId}"`));
  assert.ok(tag, `Missing ${nodeId}/${portId} named handle`);
  return tag;
};
function namedTransforms() {
  const document=profile(),processing=modules.get('./profile-processing'),contract=port('fixture','output').contract;
  const gate=processing.processingTemplate('gate','gate',contract);gate.inputs[0].name='Enable';gate.inputs[1].name='Signal';gate.outputs[0].name='Result';
  const script=processing.processingTemplate('script','script',contract);script.outputs[0].name='Primary';script.outputs.push({...plain(script.outputs[0]),id:'alternate',name:'Alternate'});
  document.processingNodes=[gate,script];document.editor.positions.push({nodeId:'gate',x:600,y:250},{nodeId:'script',x:20,y:250});
  return document;
}

test('both wire directions preserve output-to-input routing, world coordinates and the exact profile', () => {
  for (const direction of ['output', 'input']) {
    const h = harness(), origin = direction === 'output' ? 'source' : 'target';
    h.handlers.setView({ x: 20, y: 30, zoom: .5 });
    h.handlers.beginWire(pointer(), origin, direction);
    assert.equal(h.captured.has(7), true);
    h.handlers.move(pointer(320, 180));
    assert.deepEqual(plain(h.handlers.state.wire.end), { x: 400, y: 200 });
    assert.equal(h.handlers.state.wire.moved, true);
    h.setHit(direction === 'output' ? element({ nodeId: 'target', input: 'target' }) : element({ nodeId: 'source', output: 'source' }));
    h.handlers.finish(pointer(320, 180));
    assert.deepEqual(plain(h.calls), [['connect', 'source', 'target', {x:320,y:180}]]);
    assert.equal(h.handlers.state.wire, null);
    assert.equal(h.captured.size, 0);
    h.handlers.wireClick(click(), origin, direction);
    assert.deepEqual(plain(h.calls), [['connect', 'source', 'target', {x:320,y:180}]], 'The native follow-up click must not replace the paired mapping.');
    assert.equal(h.edits.length, 0);
    assert.deepEqual(h.document, h.before, 'Dragging only opens the chooser; exact ports are saved later.');
  }
});

test('pointer bursts render once per frame and pointer release flushes the latest position',()=>{
  const h=harness();h.handlers.begin(pointer(100,100,{target:element()}));
  h.handlers.scheduleMove(pointer(120,110));h.handlers.scheduleMove(pointer(160,130));
  assert.equal(h.frames.size,1);assert.equal(h.handlers.state.view.x,0);assert.deepEqual(h.edits,[]);
  h.flushFrames();assert.deepEqual(plain(h.handlers.state.view),{x:60,y:30,zoom:1});assert.equal(h.edits.length,0);
  h.handlers.scheduleMove(pointer(180,140));h.handlers.finish(pointer(180,140));
  assert.deepEqual(plain(h.handlers.state.view),{x:80,y:40,zoom:1});assert.equal(h.frames.size,0);assert.deepEqual(h.edits,[]);assert.deepEqual(plain(h.document),h.before);
  const cancelled=harness();cancelled.handlers.begin(pointer(100,100,{target:element()}));cancelled.handlers.scheduleMove(pointer(180,140));cancelled.handlers.finish(pointer(180,140),true);cancelled.flushFrames();assert.deepEqual(plain(cancelled.handlers.state.view),{x:0,y:0,zoom:1});assert.deepEqual(cancelled.edits,[]);
});

test('wheel panning stays local and never schedules a profile update',()=>{
  const h=harness(),before=plain(h.document),event={deltaX:0,deltaY:12,clientX:200,clientY:100,ctrlKey:false,metaKey:false,preventDefault(){}};
  h.handlers.wheel(event);h.handlers.wheel(event);h.handlers.wheel(event);
  assert.equal(h.frames.size,0);assert.equal(h.edits.length,0);assert.equal(h.handlers.state.view.y,-36);
  h.flushFrames();assert.deepEqual(h.edits,[]);assert.deepEqual(plain(h.document),before);
});

test('wheel preferences choose pan or zoom and Ctrl or Meta reverses the action without profile edits',()=>{
  for(const action of ['pan','zoom'])for(const modifier of [null,'ctrlKey','metaKey']){
    const h=harness({},'smooth',24,action),event={deltaX:8,deltaY:-120,clientX:200,clientY:100,ctrlKey:false,metaKey:false,preventDefault(){},...(modifier?{[modifier]:true}:{})};
    h.handlers.wheel(event);
    if((action==='zoom')!==!!modifier){assert.ok(Math.abs(h.handlers.state.view.zoom-1.1)<1e-10);assert.ok(Math.abs(h.handlers.state.view.x+10)<1e-10);assert.ok(Math.abs(h.handlers.state.view.y+5)<1e-10);}
    else assert.deepEqual(plain(h.handlers.state.view),{x:-8,y:120,zoom:1});
    assert.deepEqual(h.edits,[]);assert.deepEqual(plain(h.document),h.before);
  }
});

test('keyboard navigation and a later graph edit never write the local viewport into the profile',()=>{
  const h=harness(),target={closest:()=>null};
  for(const key of ['ArrowLeft','ArrowUp','=','-','0'])h.handlers.keyboard({key,target,preventDefault(){}});
  assert.deepEqual(h.edits,[]);assert.deepEqual(plain(h.document),h.before);
  h.handlers.setView({x:300,y:200,zoom:1.5});h.handlers.begin(pointer(100,100,{target}),'source');h.handlers.move(pointer(145,130));h.handlers.finish(pointer(145,130));
  assert.equal(h.edits.length,1);assert.equal(h.edits[0][1],'Move component');assert.deepEqual(plain(h.edits[0][0].editor.viewport),h.before.editor.viewport);assert.deepEqual(plain(h.handlers.state.view),{x:300,y:200,zoom:1.5});
});

test('selecting blocks focuses the unobscured left canvas and deselection restores one original viewport',()=>{
  for(const readOnly of [false,true]){
    const h=harness({readOnly}),overview={x:77,y:-33,zoom:.42};h.handlers.setView(overview);h.handlers.setInspector({offsetWidth:400});
    h.handlers.choose('source');h.handlers.syncSelectionFocus();let focused=plain(h.handlers.state.view);
    assert.ok(Math.abs(focused.x+(20+144)*focused.zoom-300)<1e-8);assert.ok(Math.abs(focused.y+(40+94)*focused.zoom-400)<1e-8);assert.ok(Math.abs(288*focused.zoom-300)<1e-8);
    assert.deepEqual(plain(h.handlers.state.selectionView),overview);
    h.handlers.choose('target');h.handlers.syncSelectionFocus();focused=plain(h.handlers.state.view);assert.ok(Math.abs(focused.x+(400+144)*focused.zoom-300)<1e-8);assert.deepEqual(plain(h.handlers.state.selectionView),overview);
    const manual={x:200,y:80,zoom:1.3};h.handlers.setView(manual);h.handlers.syncSelectionFocus();assert.deepEqual(plain(h.handlers.state.view),manual,'The same selection must not recenter after a view or property change.');
    h.handlers.setSelection('');h.handlers.syncSelectionFocus();assert.deepEqual(plain(h.handlers.state.view),overview);assert.equal(h.handlers.state.selectionView,null);assert.equal(h.handlers.state.focusedSelectionId,'');assert.deepEqual(h.edits,[]);assert.deepEqual(plain(h.document),h.before);
  }
});

test('selection focus waits for active gestures and wire actions use the restored viewport',async()=>{
  const selected=[],h=harness({selectConnection:id=>selected.push(id)}),overview={x:-125,y:48,zoom:.6};h.handlers.setView(overview);h.handlers.setInspector({offsetWidth:400});
  h.handlers.beginWire(pointer(),'source','output');h.handlers.setSelection('target');h.handlers.syncSelectionFocus();assert.deepEqual(plain(h.handlers.state.view),overview);assert.equal(h.handlers.state.selectionView,null);
  h.handlers.finish(pointer(),true);h.handlers.syncSelectionFocus();assert.equal(h.handlers.state.focusedSelectionId,'target');selected.length=0;
  h.handlers.chooseConnection('route');assert.deepEqual(plain(h.handlers.state.view),overview);assert.deepEqual(selected,[],'Wait for the restored transform to render before measuring wire actions.');await svelte.tick();assert.deepEqual(selected,['route']);assert.equal(h.handlers.state.selectedId,'');assert.deepEqual(h.edits,[]);
});

test('wrong-side, same-block, disabled and off-canvas wire releases remain inert', () => {
  for (const direction of ['output', 'input']) {
    const origin = direction === 'output' ? 'source' : 'target';
    const invalid = [
      element({ nodeId: 'other', ...(direction === 'output' ? { output: 'other' } : { input: 'other' }) }),
      element({ nodeId: origin, ...(direction === 'output' ? { input: origin } : { output: origin }) }),
      element({ nodeId: 'other', disabled: true, ...(direction === 'output' ? { input: 'other' } : { output: 'other' }) }),
      element({ inside: false }), element({ panel: true })
    ];
    for (const target of invalid) {
      const h = harness();
      h.handlers.beginWire(pointer(), origin, direction);h.handlers.move(pointer(300, 100));h.setHit(target);h.handlers.finish(pointer(300, 100));
      h.handlers.wireClick(click(), origin, direction);
      assert.deepEqual(h.calls, []);
      assert.equal(h.captured.size, 0);
      assert.equal(h.handlers.state.wire, null);
      assert.equal(h.edits.length, 0);
    }
  }
});

test('pointer cancellation, Escape and lost capture never open the connection chooser', () => {
  for (const cancel of ['pointer', 'escape', 'capture']) {
    const h = harness();h.handlers.beginWire(pointer(), 'target', 'input');h.handlers.move(pointer(300, 100));
    h.setHit(element({ nodeId: 'source', output: 'source' }));
    if (cancel === 'pointer') h.handlers.finish(pointer(300, 100), true);
    if (cancel === 'escape') h.handlers.keyboard({ key: 'Escape', preventDefault() {} });
    if (cancel === 'capture') { h.captured.delete(7);h.handlers.lostPointer(pointer()); }
    h.handlers.finish(pointer(300, 100));h.handlers.wireClick(click(), 'target', 'input');
    assert.deepEqual(h.calls, []);assert.equal(h.handlers.state.wire, null);assert.equal(h.captured.size, 0);
  }
  const h = harness();h.handlers.beginWire(pointer(0, 0, { button: 2 }), 'source', 'output');
  assert.equal(h.handlers.state.wire, null);assert.equal(h.captured.size, 0);
});

test('handle clicks retain keyboard access while empty-canvas clicks and moved wire releases cancel the chooser', () => {
  for (const direction of ['output', 'input']) {
    const origin = direction === 'output' ? 'source' : 'target', callback = direction === 'output' ? 'from' : 'to', h = harness();
    h.handlers.beginWire(pointer(), origin, direction);h.handlers.finish(pointer());h.handlers.wireClick(click(), origin, direction);
    assert.deepEqual(h.calls, [[callback, origin]]);
    h.handlers.wireClick(click(0), origin, direction);
    assert.deepEqual(h.calls, [[callback, origin], [callback, origin]], 'Keyboard activation remains available after a wire gesture.');
    h.handlers.choose('empty');assert.deepEqual(h.calls.at(-1), ['close']);assert.equal(h.handlers.state.selectedId, 'empty');

    const cancelled = harness({ selectedId: origin, connectionOpen: true });
    cancelled.handlers.beginWire(pointer(), origin, direction);cancelled.handlers.move(pointer(300, 100));cancelled.handlers.finish(pointer(300, 100));
    cancelled.handlers.wireClick(click(), origin, direction);
    assert.deepEqual(cancelled.calls, [['close']], 'A moved wire dropped on the background cancels instead of opening a single-ended chooser.');
    assert.equal(cancelled.handlers.state.selectedId, '');assert.equal(cancelled.handlers.state.wire, null);assert.equal(cancelled.captured.size, 0);
    assert.deepEqual(cancelled.edits, []);assert.deepEqual(cancelled.document, cancelled.before);
  }
  const h = harness({ selectedId: 'source', connectionOpen: true });
  h.handlers.begin(pointer(300, 100, { target: element() }));h.handlers.finish(pointer(300, 100));
  assert.deepEqual(h.calls, [['close']]);assert.equal(h.handlers.state.selectedId, '');assert.equal(h.handlers.state.drag, null);assert.equal(h.captured.size, 0);
  assert.deepEqual(h.edits, []);assert.deepEqual(h.document, h.before, 'Dismissing the chooser does not edit the profile.');

  const pan = harness({ selectedId: 'source', connectionOpen: true });
  pan.handlers.begin(pointer(300, 100, { target: element() }));pan.handlers.move(pointer(350, 120));pan.handlers.finish(pointer(350, 120));
  assert.deepEqual(plain(pan.handlers.state.view), { x: 50, y: 20, zoom: 1 });assert.equal(pan.captured.size, 0);
  assert.deepEqual(pan.calls, [], 'Background panning does not dismiss the chooser.');
  assert.deepEqual(pan.edits, [], 'Background panning never changes the profile.');
  assert.deepEqual(plain(pan.document), pan.before);
});

test('blocks show discovered input and output counts while loading/error handles and read-only state remain clear', () => {
  const discoveredPorts = { source: [port('fire', 'output', 'Fire trigger'), port('axis', 'output', 'Brake axis'), port('switch', 'output', 'Third output'), port('led', 'input', 'Backlight')], target: [] };
  const h = harness({ discoveredPorts });
  assert.match(button(h.body, 'data-connection-source', 'source'), /Outputs for source \(3\)/);
  assert.match(button(h.body, 'data-connection-target', 'source'), /Inputs \/ actions for source \(1\)/);
  for (const name of ['Fire trigger', 'Brake axis', 'Third output', 'Backlight', '+1 more']) assert.ok(!h.body.includes(name));
  assert.ok(!h.body.includes('saved-fire'));assert.ok(!h.body.includes('saved-led'), 'A completed empty discovery replaces the saved preview.');
  assert.match(button(h.body, 'data-connection-target', 'target'), /\sdisabled(?:\s|>|=)/);assert.match(button(h.body, 'data-connection-source', 'empty'), /\sdisabled(?:\s|>|=)/);
  assert.match(button(h.body, 'data-connection-target', 'target'), /Inputs \/ actions for target \(0\)/);
  assert.match(button(h.body, 'data-connection-source', 'empty'), /Outputs for empty \(0\)/);assert.deepEqual(h.document, h.before);
  const waiting = harness({ discoveryBusy: ['empty'] });
  assert.ok(waiting.body.includes('Discovering inputs and outputs'));assert.doesNotMatch(button(waiting.body, 'data-connection-target', 'empty'), /\sdisabled(?:\s|>|=)/);
  const failed = harness({ discoveryErrors: { empty: 'Provider unavailable' } });
  assert.ok(failed.body.includes('Port discovery failed'));assert.doesNotMatch(button(failed.body, 'data-connection-source', 'empty'), /\sdisabled(?:\s|>|=)/, 'Failed discovery must allow opening its retry UI.');
  const locked = harness({ readOnly: true, discoveredPorts });
  for (const id of ['source', 'target', 'empty']) for (const attribute of ['data-connection-source', 'data-connection-target']) assert.match(button(locked.body, attribute, id), /\sdisabled(?:\s|>|=)/);
  locked.handlers.beginWire(pointer(), 'source', 'output');locked.handlers.wireClick(click(0), 'source', 'output');
  assert.equal(locked.handlers.state.wire, null);assert.deepEqual(locked.calls, []);assert.equal(locked.captured.size, 0);
});

test('port discovery falls back to intrinsic processing, virtual and desktop contracts, and the connection panel replaces node inspection', () => {
  const h = harness(), input = port('value', 'input'), output = port('result', 'output');
  assert.deepEqual(plain(h.handlers.blockPorts({ processing: { inputs: [input], outputs: [output] } }, 'output')), [output]);
  assert.deepEqual(plain(h.handlers.blockPorts({ processing: { inputs: [input], outputs: [output] } }, 'input')), [input]);
  for (const kind of ['virtual', 'desktop']) {
    assert.deepEqual(plain(h.handlers.blockPorts({ [kind]: { ports: [input] } }, 'input')), [input]);
    assert.deepEqual(plain(h.handlers.blockPorts({ [kind]: { ports: [input] } }, 'output')), []);
  }
  const panel = harness({ selectedId: 'source', connectionOpen: true, connectionPanel: svelte.createRawSnippet(() => ({ render: () => '<p>Exact port choices</p>' })) });
  assert.ok(panel.body.includes('aria-label="Exact connection editor"'));assert.ok(panel.body.includes('Exact port choices'));assert.ok(!panel.body.includes('aria-label="Selected block inspector"'));
  const inspected = harness({ selectedId: 'source' });assert.ok(inspected.body.includes('aria-label="Selected block inspector"'));assert.ok(!inspected.body.includes('aria-label="Exact connection editor"'));
});

test('compact Transform cards show named border ports instead of aggregate input and output boxes',()=>{
  const document=namedTransforms(),h=harness({document}),articles=[...h.body.matchAll(/<article\b[^>]*data-node-id="([^"]+)"[^>]*>[\s\S]*?<\/article>/g)];
  for(const id of ['gate','script']){
    const article=articles.find(([,nodeId])=>nodeId===id)?.[0];assert.ok(article,id);assert.match(article,/width:224px;height:142px/);assert.match(article,/class="block-category[^"]*">Transform/);assert.ok(!article.includes('Processing'));assert.ok(!article.includes('block-port-groups'));assert.ok(!article.includes('Inputs / actions'));assert.ok(!article.includes('port-group-heading'));
    assert.equal((article.match(/class="port-dot[^"]*"/g)??[]).length,3,'Each real named port has one visible border dot.');
  }
  for(const [nodeId,portId,name,direction]of [['gate','gate','Enable','input'],['gate','value','Signal','input'],['gate','out','Result','output'],['script','alternate','Alternate','output']]){
    assert.match(namedButton(h.body,nodeId,portId),new RegExp(`aria-label="${name} ${direction} for `));assert.ok(new RegExp(`class="transform-port-name[^"]*">${name}</span>`).test(h.body));
  }
  const locked=harness({document,readOnly:true});for(const [nodeId,portId]of [['gate','gate'],['gate','value'],['gate','out'],['script','alternate']])assert.match(namedButton(locked.body,nodeId,portId),/\sdisabled(?:\s|>|=)/);
  assert.deepEqual(plain(document),h.before);assert.deepEqual(h.edits,[]);
});

test('named-port drags preserve both exact identities in either direction and highlight only the intended rows',()=>{
  const document=namedTransforms(),{canvasNodes,canvasNodeAnchor}=modules.get('./profile-canvas'),nodes=canvasNodes(document,[],[]);
  for(const direction of ['output','input']){
    const h=harness({document}),origin=direction==='output'?'script':'gate',originPort=direction==='output'?'alternate':'value',target=direction==='output'?'gate':'script',targetPort=direction==='output'?'value':'alternate';
    h.handlers.beginWire(pointer(),origin,direction,originPort);assert.equal(h.handlers.state.wire.portId,originPort);assert.deepEqual(plain(h.handlers.state.wire.end),plain(canvasNodeAnchor(nodes.find(node=>node.id===origin),direction,originPort)),'The pending wire starts at its exact row border.');
    assert.equal(h.handlers.activeHandle(origin,direction,originPort),true);assert.equal(h.handlers.activeHandle(origin,direction,direction==='output'?'out':'gate'),false);
    h.setHit(element({nodeId:target,...(direction==='output'?{input:target}:{output:target}),portId:targetPort}));h.handlers.move(pointer(550,300));
    assert.equal(h.handlers.state.wireTargetId,target);assert.equal(h.handlers.state.wireTargetPort,targetPort);assert.equal(h.handlers.activeHandle(target,direction==='output'?'input':'output',targetPort),true);assert.equal(h.handlers.activeHandle(target,direction==='output'?'input':'output',direction==='output'?'gate':'out'),false);
    h.handlers.finish(pointer(550,300));assert.deepEqual(plain(h.calls),[['connect','script','gate',{x:550,y:300},'alternate','value']]);assert.equal(h.captured.size,0);assert.deepEqual(h.edits,[]);assert.deepEqual(plain(document),h.before);
  }
});

test('named-port keyboard activation uses the exact row anchor and keeps an existing opposite endpoint pending',()=>{
  const document=namedTransforms(),calls=[],h=harness({document,connectionOpen:true,connectionSourceId:'script',connectionSourcePort:'alternate',connectionFrom:(...args)=>calls.push(['from',...args]),connectionTo:(...args)=>calls.push(['to',...args]),selectConnection:id=>calls.push(['select',id])});
  h.handlers.setView({x:20,y:30,zoom:.5});h.handlers.wireClick(click(0),'gate','input','value');h.handlers.wireClick(click(0),'script','output','alternate');
  assert.deepEqual(plain(calls),[['to','gate',{x:420,y:265},'value'],['from','script',{x:242,y:265},'alternate']]);assert.deepEqual(h.calls,[],'Starting the other endpoint must not clear the route through closeConnectionPanel.');
  assert.match(namedButton(h.body,'script','alternate'),/data-connection-active="true"/);assert.match(namedButton(h.body,'script','out'),/data-connection-active="false"/);
});

test('selected wires highlight only their exact named ports and reconnect either endpoint to another exact row',()=>{
  const document=namedTransforms();document.bindings=[{id:'route',from:{nodeId:'script',portId:'alternate'},to:{nodeId:'gate',portId:'value'}}];
  const {canvasNodes,canvasNodeAnchor}=modules.get('./profile-canvas'),nodes=canvasNodes(document,[],[]);
  for(const side of ['from','to']){
    const reconnected=[],h=harness({document,selectedEdgeId:'route',reconnectConnection:(...args)=>reconnected.push(args)}),fixedNode=side==='from'?'gate':'script',fixedPort=side==='from'?'value':'alternate',movingNode=side==='from'?'script':'gate',movingDirection=side==='from'?'output':'input',oldPort=side==='from'?'alternate':'value',newPort=side==='from'?'out':'gate';
    for(const [nodeId,portId,active]of [['script','alternate',true],['script','out',false],['gate','value',true],['gate','gate',false]])assert.match(namedButton(h.body,nodeId,portId),new RegExp(`data-connection-active="${active}"`));
    h.handlers.beginReconnect(pointer(),'route',side);assert.equal(h.handlers.state.wire.nodeId,fixedNode);assert.equal(h.handlers.state.wire.portId,fixedPort);assert.deepEqual(plain(h.handlers.state.wire.end),plain(canvasNodeAnchor(nodes.find(node=>node.id===movingNode),movingDirection,oldPort)));
    h.setHit(element({nodeId:movingNode,...(side==='from'?{output:movingNode}:{input:movingNode}),portId:newPort}));h.handlers.move(pointer(500,350));h.handlers.finish(pointer(500,350));assert.deepEqual(plain(reconnected),[['route',side,movingNode,{x:500,y:350},newPort]]);assert.equal(h.handlers.state.wire,null);assert.equal(h.captured.size,0);assert.deepEqual(h.edits,[]);assert.deepEqual(h.calls,[]);
  }
});

test('selected wires expose endpoint labels, highlighted boxes, reconnect handles and delete access',()=>{
  const document=profile();document.bindings=[{id:'route',from:{nodeId:'source',portId:'saved-fire'},to:{nodeId:'target',portId:'saved-led'}}];
  const selected=[],removed=[],h=harness({document,selectedEdgeId:'route',selectConnection:id=>selected.push(id),disconnectConnection:id=>removed.push(id),reconnectConnection(){}});
  assert.match(h.body,/data-reconnect-side="from"/);assert.match(h.body,/data-reconnect-side="to"/);
  assert.match(h.body,/class="connection-port-label[^\"]*"[^>]*>saved-fire/);
  assert.match(button(h.body,'data-connection-source','source'),/data-connection-active="true"/);
  assert.match(button(h.body,'data-connection-target','target'),/data-connection-active="true"/);
  h.handlers.chooseConnection('route');assert.deepEqual(selected,['route']);assert.equal(h.handlers.state.selectedId,'');
  h.handlers.keyboard({key:'Delete',target:{closest:()=>null},preventDefault(){}});assert.deepEqual(removed,['route']);
  h.handlers.keyboard({key:'Escape',target:{closest:()=>null},preventDefault(){}});assert.deepEqual(selected,['route','']);
  const locked=harness({document,selectedEdgeId:'route',readOnly:true,reconnectConnection(){}});assert.ok(!locked.body.includes('data-reconnect-side='));
});

test('fully connected named and aggregate inputs expose disabled drop targets while outputs remain available',()=>{
  const document=namedTransforms();document.bindings=[
    {id:'target-route',from:{nodeId:'source',portId:'saved-fire'},to:{nodeId:'target',portId:'saved-led'}},
    {id:'gate-value',from:{nodeId:'script',portId:'alternate'},to:{nodeId:'gate',portId:'value'}},
    {id:'gate-enable',from:{nodeId:'source',portId:'saved-fire'},to:{nodeId:'gate',portId:'gate'}}
  ];
  const h=harness({document});
  assert.match(button(h.body,'data-connection-target','target'),/\sdisabled(?:\s|>|=)/,'A full input group must not offer a new connection drop.');
  assert.match(namedButton(h.body,'gate','value'),/\sdisabled(?:\s|>|=)/);assert.match(namedButton(h.body,'gate','gate'),/\sdisabled(?:\s|>|=)/);
  assert.doesNotMatch(namedButton(h.body,'gate','out'),/\sdisabled(?:\s|>|=)/,'A fully connected transform can continue to fan out its output.');
  assert.match(h.body,/All inputs are connected\./);assert.match(h.body,/This input is already connected\./);
  const partial=plain(document);partial.bindings=partial.bindings.filter(binding=>binding.id!=='gate-enable');
  const p=harness({document:partial});assert.match(namedButton(p.body,'gate','value'),/\sdisabled(?:\s|>|=)/);assert.doesNotMatch(namedButton(p.body,'gate','gate'),/\sdisabled(?:\s|>|=)/,'One free input does not free another occupied exact input.');
  assert.deepEqual(plain(h.document),h.before);assert.deepEqual(h.edits,[]);
});

test('occupied named inputs reject a new gesture but remain available to the reconnecting owner',()=>{
  const document=namedTransforms();document.bindings=[{id:'route',from:{nodeId:'script',portId:'alternate'},to:{nodeId:'gate',portId:'value'}}];
  const h=harness({document,reconnectConnection(){}});
  assert.equal(h.handlers.inputState('gate','value').full,true);assert.equal(h.handlers.inputState('gate','gate').full,false);
  h.handlers.beginWire(pointer(),'gate','input','value');h.handlers.wireClick(click(0),'gate','input','value');assert.equal(h.handlers.state.wire,null);assert.deepEqual(h.calls,[]);
  h.handlers.beginWire(pointer(),'script','output','alternate');h.setHit(element({nodeId:'gate',input:'gate',portId:'value',disabled:true}));h.handlers.move(pointer(500,350));assert.equal(h.handlers.state.wireTargetId,'');assert.equal(h.handlers.state.wireRejectedId,'gate');assert.equal(h.handlers.state.wireRejectedReason,'This input is already connected.');h.handlers.finish(pointer(500,350));assert.deepEqual(h.calls,[['close']]);assert.deepEqual(h.edits,[]);
  h.handlers.beginReconnect(pointer(),'route','to');assert.equal(h.handlers.inputState('gate','value').full,false,'Only the binding being reconnected is excluded from occupied input checks.');
  h.handlers.finish(pointer(),true);assert.equal(h.handlers.inputState('gate','value').full,true);assert.equal(h.captured.size,0);
});

test('toolbar reconnection frees only its own input and a new drag cannot borrow that exemption',()=>{
  const document=namedTransforms();document.bindings=[{id:'route',from:{nodeId:'script',portId:'alternate'},to:{nodeId:'gate',portId:'value'}},{id:'other',from:{nodeId:'script',portId:'first'},to:{nodeId:'gate',portId:'gate'}}];
  const h=harness({document,reconnectingBindingId:'route'});
  assert.equal(h.handlers.inputState('gate','value').full,false);assert.equal(h.handlers.inputState('gate','gate').full,true);
  h.handlers.beginWire(pointer(),'script','output','alternate');assert.equal(h.handlers.inputState('gate','value').full,true,'An unrelated fresh connection must still see the occupied input.');h.handlers.finish(pointer(),true);
  assert.equal(h.handlers.inputState('gate','value').full,false);assert.deepEqual(h.edits,[]);
});

test('losing capture over an occupied input clears the rejected drop feedback',()=>{
  const document=namedTransforms();document.bindings=[{id:'route',from:{nodeId:'script',portId:'alternate'},to:{nodeId:'gate',portId:'value'}}];
  const h=harness({document});h.handlers.beginWire(pointer(),'script','output','alternate');
  h.setHit(element({nodeId:'gate',input:'gate',portId:'value',disabled:true}));h.handlers.move(pointer(500,350));
  assert.equal(h.handlers.state.wireRejectedReason,'This input is already connected.');
  assert.equal(h.handlers.state.wireRejectedPort,'value');h.captured.delete(7);h.handlers.lostPointer(pointer());
  for(const field of ['wireTargetId','wireTargetPort','wireRejectedId','wireRejectedPort','wireRejectedReason'])assert.equal(h.handlers.state[field],'',field);
  assert.equal(h.handlers.state.wire,null);assert.deepEqual(h.edits,[]);assert.deepEqual(h.calls,[]);
});

function orthogonalDocument(){const document=profile();document.bindings=[{id:'route',from:{nodeId:'source',portId:'saved-fire'},to:{nodeId:'target',portId:'saved-led'}}];document.editor.arrowStyle='orthogonal';return document;}

test('app styling controls rendered arrows without profile edits or a canvas style selector',()=>{
  const document=orthogonalDocument(),h=harness({document,selectedEdgeId:'route'},'orthogonal');
  assert.doesNotMatch(h.body,/aria-label="Arrow style"/);assert.match(h.body,/data-segment-binding="route"/);assert.match(h.body,/Move horizontal segment/);
  const smooth=harness({document,selectedEdgeId:'route'});assert.ok(!smooth.body.includes('data-segment-binding='),'Global styling overrides old profile appearance metadata');assert.doesNotMatch(smooth.handlers.state.routes[0].path,/\s[LQ]/);assert.match(smooth.handlers.state.routes[0].path,/\sC/);
  assert.deepEqual(h.edits,[]);assert.deepEqual(smooth.edits,[]);assert.deepEqual(plain(document),h.before);
  const locked=harness({document,selectedEdgeId:'route',readOnly:true},'orthogonal');assert.ok(!locked.body.includes('data-segment-binding='));locked.handlers.beginSegment(pointer(),'route',0);assert.equal(locked.handlers.state.segmentDrag,null);assert.deepEqual(locked.edits,[]);
});

test('horizontal and vertical segment gestures preview without edits, keep endpoints fixed and commit once',()=>{
  const document=orthogonalDocument(),h=harness({document,selectedEdgeId:'route'},'orthogonal'),before=plain(h.handlers.state.routes[0].points);
  assert.equal(before.length,2,'The fixture starts with a straight arrow so dragging adds useful bends.');
  h.handlers.setView({x:0,y:0,zoom:.5});h.handlers.beginSegment(pointer(500,300),'route',0);assert.equal(h.captured.has(7),true);assert.deepEqual(plain(h.focuses),[{preventScroll:true}],'Segment gestures focus the canvas so Escape and Undo work after using toolbar controls.');h.handlers.move(pointer(500,335));assert.equal(h.edits.length,0);assert.equal(h.handlers.state.segmentBlocked,false);assert.ok(h.handlers.state.segmentPreview);
  const preview=plain(h.handlers.state.segmentPreview),points=preview.editor.wireRoutes[0].points;assert.equal(points[2].y,before[0].y+70,'Segment movement compensates for the canvas zoom.');assert.deepEqual(points[0],before[0]);assert.deepEqual(points.at(-1),before.at(-1));
  h.handlers.finish(pointer(500,335));assert.equal(h.edits.length,1);assert.equal(h.edits[0][1],'Move wire segment');assert.deepEqual(plain(h.edits[0][0]),preview);assert.equal(h.handlers.state.segmentDrag,null);assert.equal(h.captured.size,0);assert.deepEqual(document,h.before);
  const vertical=harness({document:preview,selectedEdgeId:'route'},'orthogonal'),index=points.findIndex((point,index)=>index<points.length-1&&point.x===points[index+1].x);
  assert.ok(index>=0);assert.match(vertical.body,/Move vertical segment/);vertical.handlers.beginSegment(pointer(500,300),'route',index);vertical.handlers.move(pointer(512,300));assert.equal(vertical.edits.length,0);assert.equal(vertical.handlers.state.segmentBlocked,false);vertical.handlers.finish(pointer(512,300));assert.equal(vertical.edits.length,1);const moved=plain(vertical.edits[0][0].editor.wireRoutes[0].points);assert.equal(moved[index].x,points[index].x+12);assert.deepEqual(moved[0],points[0]);assert.deepEqual(moved.at(-1),points.at(-1));assert.deepEqual(plain(vertical.edits[0][0].bindings),document.bindings);
});

test('segment cancellation, lost capture, insignificant movement and blocked moves never create history',()=>{
  for(const cancel of ['pointer','escape','capture','undo','tiny','blocked']){
    const document=orthogonalDocument();if(cancel==='blocked'){const obstacle=document.editor.positions.find(position=>position.nodeId==='empty');obstacle.x=310;obstacle.y=260;}
    const h=harness({document,selectedEdgeId:'route',undo(){}},'orthogonal');h.handlers.beginSegment(pointer(500,300),'route',0);
    h.handlers.move(pointer(500,cancel==='tiny'?302:cancel==='blocked'?370:335));
    if(cancel==='pointer')h.handlers.finish(pointer(500,335),true);
    else if(cancel==='escape')h.handlers.keyboard({key:'Escape',preventDefault(){}});
    else if(cancel==='capture'){h.captured.delete(7);h.handlers.lostPointer(pointer());}
    else if(cancel==='undo'){h.handlers.scheduleMove(pointer(500,360));h.handlers.keyboard({key:'z',ctrlKey:true,target:{closest:()=>null},preventDefault(){}});assert.equal(h.frames.size,0);h.flushFrames();}
    else{if(cancel==='blocked')assert.equal(h.handlers.state.segmentBlocked,true);h.handlers.finish(pointer(500,cancel==='tiny'?302:370));}
    assert.deepEqual(h.edits,[],cancel);assert.equal(h.handlers.state.segmentDrag,null,cancel);assert.equal(h.handlers.state.segmentPreview,null,cancel);assert.equal(h.captured.size,0,cancel);assert.deepEqual(h.document,h.before,cancel);
  }
});
test('both arrow styles add, drag and cancel geometric reroute points without selecting blocks or extra history',()=>{
  for(const style of ['smooth','orthogonal']){
    const document=orthogonalDocument(),h=harness({document,selectedEdgeId:'route'},style),route=h.handlers.state.routes[0],point={x:(route.points[0].x+route.points.at(-1).x)/2,y:route.points[0].y};
    h.handlers.addWireHandle(pointer(point.x+100,point.y+50),'route');assert.equal(h.edits.length,1);assert.equal(h.edits[0][1],'Add reroute point');
    const inserted=plain(h.edits[0][0]);assert.deepEqual(inserted.editor.wireRoutes[0].handles,[point]);
    const editor=harness({document:inserted,selectedEdgeId:'route'},style);assert.match(editor.body,/data-reroute-binding="route"/);
    editor.handlers.beginHandle(pointer(500,300),'route',0);editor.handlers.scheduleMove(pointer(505,315));assert.equal(editor.frames.size,1);assert.equal(editor.edits.length,0);editor.flushFrames();assert.equal(editor.handlers.state.segmentBlocked,false);assert.ok(editor.handlers.state.segmentPreview);
    editor.handlers.finish(pointer(505,315));assert.equal(editor.edits.length,1);assert.equal(editor.edits[0][1],'Move reroute point');assert.deepEqual(plain(editor.edits[0][0].editor.wireRoutes[0].handles),[{x:point.x+5,y:point.y+15}]);assert.deepEqual(plain(editor.edits[0][0].bindings),document.bindings);assert.equal(editor.handlers.state.selectedId,'');
    const cancelled=harness({document:inserted,selectedEdgeId:'route'},style);cancelled.handlers.beginHandle(pointer(500,300),'route',0);cancelled.handlers.move(pointer(505,315));cancelled.handlers.keyboard({key:'Escape',preventDefault(){}});assert.equal(cancelled.edits.length,0);assert.equal(cancelled.handlers.state.segmentPreview,null);assert.equal(cancelled.captured.size,0);
    cancelled.handlers.handleKeyboard({key:'Delete',preventDefault(){},stopPropagation(){}},'route',0);assert.equal(cancelled.edits.length,1);assert.equal(cancelled.edits[0][1],'Remove reroute point');assert.equal(cancelled.edits[0][0].bindings.length,1);assert.equal(cancelled.edits[0][0].editor.wireRoutes[0].handles,undefined);
    assert.deepEqual(document,h.before);assert.deepEqual(inserted,editor.before);
  }
});

test('dragging either selected endpoint opens only the matching reconnection operation',()=>{
  for(const side of ['from','to']){
    const document=profile();document.bindings=[{id:'route',from:{nodeId:'source',portId:'saved-fire'},to:{nodeId:'target',portId:'saved-led'}}];
    const reconnected=[],h=harness({document,reconnectConnection:(...args)=>reconnected.push(args)});
    h.handlers.beginReconnect(pointer(),'route',side);assert.equal(h.captured.has(7),true);
    const target=side==='from'?element({nodeId:'other-source',output:'other-source'}):element({nodeId:'other-target',input:'other-target'});
    h.setHit(target);h.handlers.move(pointer(550,280));assert.equal(h.handlers.state.wireTargetId,side==='from'?'other-source':'other-target');
    h.handlers.finish(pointer(550,280));assert.deepEqual(plain(reconnected),[['route',side,side==='from'?'other-source':'other-target',{x:550,y:280}]]);
    assert.equal(h.handlers.state.wire,null);assert.equal(h.captured.size,0);assert.deepEqual(h.edits,[]);assert.deepEqual(h.calls,[]);
    h.handlers.beginReconnect(pointer(),'route',side);h.handlers.finish(pointer(600,300),true);assert.equal(reconnected.length,1);
  }
});

test('dropping a transformation on a wire inserts there and ordinary canvas drops retain graph coordinates',()=>{
  const component={kind:'transformation',preset:'scale'},insertions=[],drops=[],ended=[],h=harness({paletteDrag:{component,name:'Multiply',category:'Transformation'},dropComponent:(...args)=>drops.push(args),dropOnConnection:(...args)=>insertions.push(args),endPaletteDrag:()=>ended.push(true)});
  const target={closest:selector=>selector==='[data-connection-id]'?{dataset:{connectionId:'route'}}:null};
  const event={clientX:500,clientY:300,target,dataTransfer:{},preventDefault(){},stopPropagation(){}};
  h.handlers.overPalette(event);assert.equal(h.handlers.state.paletteConnectionId,'route');h.handlers.dropPalette(event);
  assert.deepEqual(plain(insertions),[[component,'route',{x:288,y:193}]],'A single-row transform is centered using its compact 224 × 114 bounds.');
  assert.deepEqual(drops,[]);assert.equal(ended.length,1);
  h.handlers.dropPalette({...event,target:{closest:()=>null}});assert.equal(drops.length,1);assert.equal(insertions.length,1);assert.equal(ended.length,2);
});

test('selected right-angle segment handles retain transformation insertion targets',()=>{
  const component={kind:'transformation',preset:'scale'},insertions=[],drops=[];
  const h=harness({document:orthogonalDocument(),selectedEdgeId:'route',paletteDrag:{component,name:'Multiply',category:'Transformation'},dropComponent:(...args)=>drops.push(args),dropOnConnection:(...args)=>insertions.push(args)},'orthogonal');
  const segments=[...h.body.matchAll(/<path\b[^>]*data-segment-binding="route"[^>]*>/g)].map(([tag])=>tag);
  assert.ok(segments.length,'The selected right-angle wire exposes draggable segments.');
  for(const tag of segments){
    const connectionId=tag.match(/data-connection-id="([^"]+)"/)?.[1];assert.equal(connectionId,'route');
    const target={closest:selector=>selector==='[data-connection-id]'?{dataset:{connectionId}}:null};
    const event={clientX:500,clientY:300,target,dataTransfer:{},preventDefault(){},stopPropagation(){}};
    h.handlers.overPalette(event);assert.equal(h.handlers.state.paletteConnectionId,'route');h.handlers.dropPalette(event);
  }
  assert.equal(insertions.length,segments.length);assert.deepEqual(drops,[]);
  for(const insertion of insertions)assert.deepEqual(plain(insertion),[component,'route',{x:288,y:193}]);
});
