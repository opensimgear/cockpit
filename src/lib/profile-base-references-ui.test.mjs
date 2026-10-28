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
 const body=render(Canvas,{props:{document,assignments:[],inventory:[],inventoryClient:{inspect:noRequest},activeVehicleTabId:'vehicle-1',selectedId,fitOnMount:false,change:next=>changes.push(next),removeNode:()=>{throw Error('Reference removal must be unavailable.');}}}).body;
 assert.deepEqual(document,before,'Rendering and reference interaction must preserve the source graph');
 return {body,changes,before};
}

test('actual aircraft overview renders its Base endpoint and both complete signal arrows',async()=>{
 const {body}=await harness();
 assert.match(body,/data-node-id="ah6-dcs"/);assert.match(body,/data-node-id="aux-panel"/);
 const reference=body.match(/<article\b[^>]*data-node-id="aux-panel"[^>]*>/)?.[0];assert.ok(reference);assert.match(reference,/data-base-reference="true"/);
 assert.match(body.replace(/<!--[\s\S]*?-->/g,''),/· Base/);assert.doesNotMatch(body,/data-node-id="(?:hornet-dcs|right-stick|game-context-dcs-1)"/);
 assert.equal((body.match(/class="signal-edge-hit(?:\s|\")/g)??[]).length,2);
 assert.match(body,/overview:\[&quot;aux-panel&quot;,&quot;ah6-dcs&quot;\]/);assert.match(body,/overview:\[&quot;ah6-dcs&quot;,&quot;aux-panel&quot;\]/);
});

test('selecting a Base reference explains its scope and offers no Remove action',async()=>{
 const {body}=await harness({selectedId:'aux-panel'}),inspector=body.slice(body.indexOf('aria-label="Selected block inspector"'));
 assert.match(inspector,/Defined in Base\. Edit this component from the Base tab\./);assert.doesNotMatch(inspector,/>Remove<\/button>/);
});

test('keyboard movement cannot change a Base reference',async()=>{
 const {changes}=await harness({run({moveBlock}){moveBlock({altKey:true,key:'ArrowRight',shiftKey:false,preventDefault(){},stopPropagation(){}},'aux-panel');}});assert.equal(changes.length,0);
});

test('moving an aircraft block preserves every Base node position',async()=>{
 const {changes,before}=await harness({run({moveBlock}){moveBlock({altKey:true,key:'ArrowRight',shiftKey:false,preventDefault(){},stopPropagation(){}},'ah6-dcs');}});
 assert.equal(changes.length,1);const after=changes[0],scoped=new Set(before.vehicleTabs.flatMap(tab=>tab.nodeIds));
 for(const position of before.editor.positions.filter(position=>!scoped.has(position.nodeId)))assert.deepEqual(plain(after.editor.positions.find(row=>row.nodeId===position.nodeId)),position);
 const original=before.editor.positions.find(position=>position.nodeId==='ah6-dcs');assert.equal(after.editor.positions.find(position=>position.nodeId==='ah6-dcs').x,original.x+16);
 assert.deepEqual(plain(after.bindings),before.bindings);
});
