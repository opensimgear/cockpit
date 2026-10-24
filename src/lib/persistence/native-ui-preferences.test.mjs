import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import ts from 'typescript';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {UiPreferences,UI_KEYS} from './ui-preferences.ts';
const filename=new URL('./native-ui-preferences.ts',import.meta.url);
const sourcePath=fileURLToPath(filename);
const code=ts.transpileModule(await readFile(filename,'utf8'),{fileName:sourcePath,compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,inlineSourceMap:true,inlineSources:true}}).outputText;
function harness({native=true,readError=false,listenError=false}={}){
 const store=new UiPreferences(),entries=Object.fromEntries(UI_KEYS.map(key=>[key,{revision:0,value:null}])),calls=[],windowEvents=new Map();let event;
 const exports={};vm.runInNewContext(code,{exports,window:{addEventListener:(name,callback)=>windowEvents.set(name,callback)},require(name){
  if(name==='@tauri-apps/api/core')return{isTauri:()=>native,invoke:async command=>{calls.push(command);if(readError)throw Error('SQL unavailable');return {entries:structuredClone(entries)};}};
  if(name==='@tauri-apps/api/event')return{listen:async(name,callback)=>{calls.push(name);if(listenError)throw Error('event unavailable');event=callback;}};
  if(name==='../gateway')return{gatewayRequest:async request=>{calls.push(request);entries[request.key]={revision:request.expectedRevision+1,value:request.value};return {uiPreferences:entries[request.key]};}};
  if(name==='./ui-preferences')return{uiPreferences:store,UI_KEYS};throw Error('Unexpected bootstrap dependency '+name);
 }},{filename:sourcePath});return {store,entries,calls,windowEvents,bootstrap:exports.bootstrapUiPreferences,event:()=>event};
}
test('native bootstrap hydrates before returning and uses fixed commands without browser storage',async()=>{
 const h=harness();h.entries.theme={revision:4,value:true};await h.bootstrap();assert.equal(h.store.get('theme').value,true);assert.equal(h.store.get('theme').ready,true);assert.deepEqual(h.calls,['ui-preferences-changed','persistence_ui_read']);
 await h.store.save('theme',false);assert.equal(h.calls.at(-1).command,'ui_preferences_save');assert.equal(h.calls.at(-1).key,'theme');assert.equal(h.calls.at(-1).expectedRevision,4);
 h.entries.theme={revision:6,value:true};h.event()({payload:{key:'theme',revision:6}});await new Promise(setImmediate);assert.equal(h.store.get('theme').revision,6);assert.equal(h.store.get('theme').value,true);
 const reads=h.calls.length;h.event()({payload:{key:'secrets',revision:100}});await new Promise(setImmediate);assert.equal(h.calls.length,reads);
});
test('read failure leaves an explicit diagnostic and does not enable native saves',async()=>{
 const h=harness({readError:true});await h.bootstrap();assert.equal(h.store.get('theme').ready,false);assert.match(h.store.get('theme').error,/SQL unavailable/);await assert.rejects(h.store.save('theme',true),/Refresh/);assert.equal(h.calls.length,2);
});
test('focus reconciles durable state if event subscription is unavailable and preview never invokes native',async()=>{
 const h=harness({listenError:true});await h.bootstrap();h.entries.theme={revision:1,value:true};h.windowEvents.get('focus')();await new Promise(setImmediate);assert.equal(h.store.get('theme').value,true);
 const preview=harness({native:false});await preview.bootstrap();assert.deepEqual(preview.calls,[]);await preview.store.save('theme',true);assert.equal(preview.store.get('theme').value,true);
});
