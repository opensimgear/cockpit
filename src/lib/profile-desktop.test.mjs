import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile}from'node:fs/promises';
import{loadModules}from'./test-module-loader.mjs';
import {desktopStatus} from './profile-desktop-runtime.ts';
const modules=await loadModules(['./profiles','./profile-desktop','./profile-graph','./profile-processing','./profile-canvas','./profile-desktop-runtime']);
const {parseProfileDocument,exportProfileDocument,profileDocumentError}=modules.get('./profiles');
const {desktopActionValid,desktopActionContract,desktopPreset,DESKTOP_PRESETS}=modules.get('./profile-desktop');
const {graphSetDesktop,graphSetProcessing,graphConnect,graphRemoveNode,graphPorts}=modules.get('./profile-graph');
const {processingNodeError}=modules.get('./profile-processing');
const {canvasNodes,canvasMove}=modules.get('./profile-canvas');
const fixture=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/desktop-only.json',import.meta.url)));
const blank=()=>({...structuredClone(fixture),bindings:[],processingNodes:[],desktopOutputs:[]});
test('desktop-only portable document roundtrips exact actions without joystick or local enablement',()=>{
 const document=parseProfileDocument(JSON.stringify(fixture));assert.deepEqual(JSON.parse(exportProfileDocument(document)),fixture);assert.equal(document.virtualOutputs.length,0);assert.equal(document.resources.length,0);
 assert.equal(profileDocumentError({...fixture,enabled:true}).length>0,true);assert.equal(profileDocumentError({...fixture,scope:{kind:'desktop-wide'}}).length>0,true);
 const nodes=canvasNodes(document,[],[]);assert.equal(nodes.find(node=>node.kind==='desktop').name,'Keyboard and mouse');assert.equal(graphPorts(document,'desktop').length,2);assert.equal(canvasMove(document,'desktop',{x:50,y:70}).editor.positions[0].nodeId,'desktop');
});
test('desktop presets keep exact action contracts and every conversion visible',()=>{
 for(const[kind]of DESKTOP_PRESETS){const preset=desktopPreset(kind,'desktop');let graph=graphSetDesktop({document:blank(),assignments:[]},preset.output);for(const node of preset.nodes){assert.equal(processingNodeError(node),'',`${kind}: ${node.id}`);graph=graphSetProcessing(graph,node);}for(const route of preset.routes)graph=graphConnect(graph,route.from,route.to);assert.equal(profileDocumentError(graph.document),'');assert.equal(graph.document.virtualOutputs.length,0);assert.equal(graph.document.desktopOutputs.length,1);for(const definition of preset.output.actions)assert.deepEqual(preset.output.ports.find(port=>port.id===definition.portId).contract,desktopActionContract(definition.action));
 if(['hat-arrows','stick-pointer','encoder-tap','mouse-click','scroll'].includes(kind))assert.ok(graph.document.processingNodes.length>0);
 }
});
test('desktop validation rejects unsupported identities, hidden contract changes and multiple endpoints',()=>{
 for(const key of [0,3,72,102,200,232,65535])assert.equal(desktopActionValid({kind:'key-hold',key}),false);
 for(const key of [4,44,79,101,224,231])assert.equal(desktopActionValid({kind:'key-hold',key}),true);
 assert.equal(desktopActionValid({kind:'chord',keys:[224,224]}),false);assert.equal(desktopActionValid({kind:'pointer',axis:'x',speed:100,curve:Infinity}),false);
 const invalid=structuredClone(fixture);invalid.desktopOutputs[0].ports[0].contract.delivery='event';assert.ok(profileDocumentError(invalid));
 assert.ok(profileDocumentError({...fixture,desktopOutputs:null}));
 const graph={document:structuredClone(fixture),assignments:[]};assert.throws(()=>graphSetDesktop(graph,desktopPreset('key-hold','second').output),/one logical/);
 const nativeOrder=structuredClone(graph);for(const port of nativeOrder.document.desktopOutputs[0].ports)port.contract=Object.fromEntries(Object.entries(port.contract).sort(([a],[b])=>a.localeCompare(b)));
 const retarget=structuredClone(nativeOrder.document.desktopOutputs[0]);retarget.actions[0].action.key=79;const updated=graphSetDesktop(nativeOrder,retarget);assert.equal(updated.document.desktopOutputs[0].actions[0].action.key,79);assert.equal(updated.document.bindings.length,3);
 const changed=structuredClone(fixture.desktopOutputs[0]);changed.actions[0].action={kind:'key-tap',key:44,downMs:30,upMs:30};assert.throws(()=>graphSetDesktop(graph,changed),/Disconnect routes/);
 const removed=graphRemoveNode(graph,'desktop');assert.equal(removed.document.desktopOutputs,undefined);assert.equal(removed.document.bindings.length,1);
});
test('keyboard scope roundtrips per endpoint without native identities and rejects forged scope fields',()=>{
 const source=structuredClone(fixture),reference='01234567-89ab-4cde-8fab-0123456789ab';
 assert.equal(source.desktopOutputs[0].scope,undefined,'old documents retain the safe application default without rewriting their saved shape');
 for(const scope of [{kind:'selected-game'},{kind:'desktop-wide'},{kind:'application'},{kind:'application',applicationReference:reference}]){
  source.desktopOutputs[0].scope=scope;assert.equal(profileDocumentError(source),'');assert.deepEqual(JSON.parse(exportProfileDocument(source)).desktopOutputs[0].scope,scope);
 }
 for(const scope of [null,{kind:'process',pid:42},{kind:'desktop-wide',applicationReference:reference},{kind:'application',applicationReference:'not-native'},{kind:'application',pid:42,path:'forged.exe'}]){source.desktopOutputs[0].scope=scope;assert.ok(profileDocumentError(source));}
});
test('native desktop status requires local enablement and bounded tagged scope',()=>{
 const status={enabled:false,qualified:false,state:'stopped',reason:'Enablement required',scope:{kind:'application'},runtime:null};assert.deepEqual(JSON.parse(JSON.stringify(desktopStatus(status))),status);assert.throws(()=>desktopStatus({...status,enabled:'yes'}));assert.throws(()=>desktopStatus({...status,scope:{kind:'process',pid:123}}));
 for(const invalid of [null,undefined,42,'stopped',{}, {...status,qualified:1}, {...status,reason:7}, {...status,scope:null}])assert.throws(()=>desktopStatus(invalid));
 const captured=desktopStatus(status);captured.scope.kind='desktop-wide';assert.equal(status.scope.kind,'application');
});
