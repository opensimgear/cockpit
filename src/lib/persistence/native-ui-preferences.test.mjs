import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import ts from 'typescript';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { UiPreferences, UI_KEYS, OBSERVED_UI_KEYS } from './ui-preferences.ts';
import { SharedObservations } from '../streams/shared.ts';
const filename = new URL('./native-ui-preferences.ts', import.meta.url);
const sourcePath = fileURLToPath(filename);
const code = ts.transpileModule(await readFile(filename, 'utf8'), {fileName:sourcePath,compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true}}).outputText;
const turn = () => new Promise(setImmediate);
function deferred() { let resolve; const promise = new Promise(yes => { resolve = yes; }); return {promise,resolve}; }
function harness({native=true, readError=false, listenError=false, label='main', readWait, onOpen, listenWait, labelError=false}={}) {
  const store = new UiPreferences();
  const entries = Object.fromEntries(UI_KEYS.map(key => [key, {revision:0,value:null}]));
  const calls = [], opens = [], windowEvents = new Map(); let event; let unlistened = 0;
  const read = async () => {
    const snapshot = {entries:structuredClone(entries)};
    if (readWait) await readWait.promise;
    if (readError) throw Error('SQL unavailable');
    return snapshot;
  };
  const exports = {};
  vm.runInNewContext(code, {exports, window:{
    addEventListener(name, callback) { windowEvents.set(name, callback); },
    removeEventListener(name, callback) { if (windowEvents.get(name) === callback) windowEvents.delete(name); },
  }, require(name) {
    if (name === '@tauri-apps/api/core') return {isTauri:()=>native, invoke:async command => { calls.push(command); return read(); }};
    if (name === '@tauri-apps/api/event') return {listen:async(name,callback) => { calls.push(name); if (listenError) throw Error('event unavailable'); event=callback; if(listenWait)await listenWait.promise; return ()=>unlistened++; }};
    if (name === '@tauri-apps/api/window') { if(labelError)throw Error('no label'); return {getCurrentWindow:()=>({label})}; }
    if (name === '../streams/index.ts') return {SharedObservations:class extends SharedObservations {
      constructor() { super({open(topic,observer,signal) { const open={topic,observer,signal,delivery:0};opens.push(open);onOpen?.(open); }}); }
    }};
    if (name === '../gateway') return {gatewayRequest:async request => {
      calls.push(request);
      if (request.command === 'ui_preferences_get') return {uiPreferences:await read()};
      entries[request.key] = {revision:request.expectedRevision+1,value:request.value};
      return {uiPreferences:entries[request.key]};
    }};
    if (name === './ui-preferences') return {uiPreferences:store,UI_KEYS,OBSERVED_UI_KEYS};
    throw Error('Unexpected bootstrap dependency '+name);
  }}, {filename:sourcePath});
  function send(key, payload, kind='snapshot') {
    const open = opens.findLast(open => open.topic === `preferences.${key}`);
    open.observer.frame({subscriptionId:'token',delivery:String(++open.delivery),frame:{
      kind, topic:open.topic, source:'ui-preferences',session:'service',generation:'1',
      revision:String(open.delivery),capturedUs:kind==='loss'?'0':'123',clockId:kind==='loss'?'none':'cockpit-state',
      continuity:kind==='snapshot'?'baseline':kind==='loss'?'lost':'continuous',
      ...(kind==='change'?{sequence:String(open.delivery-1)}:{}),
      ...(kind==='loss'?{reason:'overflow'}:{payload}),
    }}, async()=>{});
  }
  return {store,entries,calls,opens,windowEvents,send,bootstrap:exports.bootstrapUiPreferences,event:()=>event,unlistened:()=>unlistened};
}

test('native bootstrap hydrates once, streams mutations without reads and limits legacy events to remembered', async () => {
  const h=harness();h.entries.theme={revision:4,value:true};const cleanup=await h.bootstrap();
  assert.equal(h.store.get('theme').value,true);assert.equal(h.store.get('theme').ready,true);
  assert.deepEqual(h.opens.map(row=>row.topic),OBSERVED_UI_KEYS.map(key=>`preferences.${key}`));
  assert.deepEqual(h.calls,['persistence_ui_read','ui-preferences-changed']);
  h.send('theme',{ready:true,revision:'5',value:false});
  h.send('theme',{ready:true,revision:'6',value:true},'change');
  assert.equal(h.store.get('theme').revision,6);assert.equal(h.store.get('theme').value,true);assert.equal(h.calls.length,2);
  h.send('theme',{ready:true,revision:'4',value:false},'change');assert.equal(h.store.get('theme').value,true);
  h.event()({payload:{key:'theme',revision:99}});h.event()({payload:{key:'unknown',revision:99}});await turn();assert.equal(h.calls.length,2);
  h.entries.remembered={revision:1,value:{components:[],forgotten:{}}};
  h.entries.theme={revision:99,value:false};h.event()({payload:{key:'remembered',revision:1}});await turn();
  assert.equal(h.store.get('remembered').revision,1);assert.equal(h.store.get('theme').revision,6);
  await h.store.save('theme',false);assert.equal(h.calls.at(-1).command,'ui_preferences_save');assert.equal(h.calls.at(-1).expectedRevision,6);
  cleanup();cleanup();assert.equal(h.unlistened(),1);assert.equal(h.windowEvents.size,0);assert.ok(h.opens.every(row=>row.signal.aborted));
});

test('browser bootstrap reads and writes real gateway service and subscribes the same six topics', async () => {
  const h=harness({native:false});h.entries.theme={revision:2,value:true};const cleanup=await h.bootstrap();
  assert.equal(h.calls[0].command,'ui_preferences_get');assert.equal(h.store.get('theme').revision,2);assert.equal(h.opens.length,6);
  await h.store.save('theme',false);assert.equal(h.calls.at(-1).command,'ui_preferences_save');assert.equal(h.calls.at(-1).expectedRevision,2);
  h.send('theme',{ready:true,revision:'4',value:true});assert.equal(h.store.get('theme').value,true);assert.equal(h.calls.length,2);
  cleanup();
});

test('stream loss/error disables only its preference; focus explicitly reopens and hydrates', async () => {
  const h=harness({listenError:true});const cleanup=await h.bootstrap();
  h.send('theme',{ready:true,revision:'1',value:true});h.send('theme',undefined,'loss');
  assert.equal(h.store.get('theme').ready,false);assert.equal(h.store.get('progress').ready,true);
  await assert.rejects(h.store.save('theme',false),/Refresh/);
  h.opens.find(row=>row.topic==='preferences.tray').observer.error('transport ended');assert.equal(h.store.get('tray').ready,false);
  h.entries.theme={revision:2,value:false};h.windowEvents.get('focus')();await turn();
  assert.equal(h.store.get('theme').ready,true);assert.equal(h.store.get('theme').revision,2);assert.equal(h.opens.length,12);
  assert.ok(h.opens.slice(0,6).every(row=>row.signal.aborted));
  cleanup();
});

test('read failure keeps explicit errors and never enables defaults for failed topics', async () => {
  const h=harness({readError:true});const cleanup=await h.bootstrap();assert.equal(h.store.get('theme').ready,false);
  assert.match(h.store.get('theme').error,/SQL unavailable/);await assert.rejects(h.store.save('theme',true),/Refresh/);cleanup();
});

test('bootstrap snapshot race preserves later stream state and loss; pagehide fences pending callbacks', async () => {
  const readWait=deferred();const h=harness({readWait});const boot=h.bootstrap();await turn();
  h.send('theme',{ready:true,revision:'7',value:true});h.send('tray',undefined,'loss');readWait.resolve();const cleanup=await boot;
  assert.equal(h.store.get('theme').revision,7);assert.equal(h.store.get('theme').value,true);assert.equal(h.store.get('tray').ready,false);
  const focusWait=h.windowEvents.get('pagehide');focusWait();assert.equal(h.windowEvents.size,0);
  h.send('theme',{ready:true,revision:'8',value:false},'change');assert.equal(h.store.get('theme').revision,7);assert.equal(h.store.get('theme').ready,false);cleanup();
});

test('activation and unidentified windows subscribe only common topics', async () => {
  for (const options of [{label:'profile-activation'},{labelError:true}]) {
    const h=harness(options);const cleanup=await h.bootstrap();assert.deepEqual(h.opens.map(row=>row.topic),['preferences.theme','preferences.progress']);cleanup();
  }
});

test('pagehide during pending listener installation disposes future listener and pending read', async () => {
  const listenWait=deferred(),readWait=deferred();const h=harness({listenWait,readWait});const boot=h.bootstrap();await turn();
  h.windowEvents.get('pagehide')();listenWait.resolve();readWait.resolve();const cleanup=await boot;await turn();
  assert.equal(h.unlistened(),1);assert.equal(h.windowEvents.size,0);assert.ok(h.opens.every(row=>row.signal.aborted));assert.equal(h.store.get('theme').ready,false);cleanup();
});
