import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules,libModuleName} from './test-module-loader.mjs';

const fixture=JSON.parse(await readFile(new URL('../../examples/dcs-bios/feature-lab.profile.json',import.meta.url),'utf8'));
const plain=value=>JSON.parse(JSON.stringify(value));
const modules=await loadModules(['./profile-canvas']);
const noRequest=()=>{throw Error('Rendering the canvas must not call native services.');};
async function components(capture){
 const cache=new Map();
 async function component(name,parent=import.meta.url){
  const filename=new URL(name,parent),key=libModuleName(name,parent);if(cache.has(key))return cache.get(key);
  let source=await readFile(filename,'utf8');
  if(key==='./ProfileCanvas.svelte')source=source.replace('</script>','captureCanvas({moveBlock,begin,scopedCanvasEdit});</script>');
  const {js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(libModuleName(match[1],filename),{default:await component(match[1],filename)});
  const code=ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  for(const match of code.matchAll(/require\(["']([^"']+)["']\)/g)){
   const key=libModuleName(match[1],filename).replace(/\.ts$/,'');
   if(!key.startsWith('./')||dependencies.has(key)||key.endsWith('.svelte')||key==='./profile-elk-runtime'||modules.has(key))continue;
   for(const [name,exports] of await loadModules([key]))modules.set(name,exports);
  }
  const exports={};vm.runInNewContext(code,{exports,TextEncoder,captureCanvas:capture,require(specifier){
   const dependency=libModuleName(specifier,filename);
   if(dependency==='svelte/internal/server')return server;if(dependency==='svelte')return svelte;
   if(dependencies.has(dependency))return dependencies.get(dependency);
   if(dependency==='./profile-elk-runtime')return {layoutProfileWithElk:noRequest};
   const key=dependency.replace(/\.ts$/,'');if(modules.has(key))return modules.get(key);
   throw Error(`Unexpected component dependency ${specifier}`);
  }},{filename:filename.pathname});cache.set(key,exports.default);return exports.default;
 }
 return component('./ProfileCanvas.svelte');
}
async function harness({selectedId='',run=()=>{}}={}){
 const changes=[],document=plain(fixture),before=plain(document),Canvas=await components(handlers=>run(handlers,changes));
 const body=render(Canvas,{props:{document,assignments:[],inventory:[],inventoryClient:{inspect:noRequest},selectedId,fitOnMount:false,change:next=>changes.push(next),removeNode:()=>{throw Error('Rendering must not remove components.');}}}).body;
 assert.deepEqual(document,before,'Rendering and reference interaction must preserve the source graph');
 return {body,changes,before};
}

test('the example renders Common, both aircraft and shared hardware once on the same canvas',async()=>{
 const {body}=await harness();
 for(const id of ['ah6-dcs','hornet-dcs','game-context-dcs-1','aux-panel','right-stick'])assert.equal((body.match(new RegExp('data-node-id="'+id+'"','g'))??[]).length,1,id);
 assert.doesNotMatch(body,/data-base-reference|Defined in Base|Edit in Base|· Base/);
 for(const icon of ['link','monitor','gamepad'])assert.ok(body.includes('data-block-icon="'+icon+'"'),icon);
 assert.ok((body.match(/class="signal-edge-hit(?:\s|\")/g)??[]).length>=2);
});

test('shared companion settings remain directly editable from the unified canvas',async()=>{
 const {body}=await harness({selectedId:'aux-panel'}),inspector=body.slice(body.indexOf('aria-label="Selected block inspector"'));
 assert.match(inspector,/Block settings/);assert.doesNotMatch(inspector,/Defined in Base|Edit in Base/);
 assert.match(inspector,/Remove/);
});

test('moving shared hardware and either aircraft edits one graph and preserves other positions',async()=>{
 for(const id of ['aux-panel','ah6-dcs','hornet-dcs']){
  const {changes,before}=await harness({run({moveBlock}){moveBlock({altKey:true,key:'ArrowRight',shiftKey:false,preventDefault(){},stopPropagation(){}},id);}});
  assert.equal(changes.length,1);const after=plain(changes[0]),original=before.editor.positions.find(position=>position.nodeId===id);
  assert.equal(after.editor.positions.find(position=>position.nodeId===id).x,original.x+16);
  for(const position of before.editor.positions.filter(position=>position.nodeId!==id))assert.deepEqual(after.editor.positions.find(row=>row.nodeId===position.nodeId),position);
  assert.deepEqual(after.bindings,before.bindings);assert.deepEqual(after.vehicleTabs,before.vehicleTabs);
 }
});
