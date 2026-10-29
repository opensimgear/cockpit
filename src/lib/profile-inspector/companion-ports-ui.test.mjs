import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules,libModuleName} from '../test-module-loader.mjs';

const modules=await loadModules(['./profile-canvas']),cache=new Map();
const transpile=source=>ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
async function component(name,parent=import.meta.url){
 const filename=new URL(name,parent),key=libModuleName(name,parent);if(cache.has(key))return cache.get(key);
 const source=key.endsWith('/ControlConnectionBadge.svelte')?'':await readFile(filename,'utf8');
 const {js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();
 for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(libModuleName(match[1],filename),{default:await component(match[1],filename)});
 const exports={};vm.runInNewContext(transpile(js.code),{exports,TextEncoder,require(specifier){
  const dependency=libModuleName(specifier,filename);
  if(dependency==='svelte/internal/server')return server;if(dependency==='svelte')return svelte;
  if(dependencies.has(dependency))return dependencies.get(dependency);
  if(dependency==='./profile-resource-state')return {profileResourceState:()=> 'unassigned'};
  const key=dependency.replace(/\.ts$/,'');if(modules.has(key))return modules.get(key);
  throw Error(`Unexpected component dependency ${specifier}`);
 }},{filename:filename.pathname});cache.set(key,exports.default);return exports.default;
}
const Inspector=await component('./ProfileBlockInspector.svelte');
const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1500,fallback:false};
const port={id:'signal',name:'Saved signal',direction:'output',contract};
const document={id:'profile',eligibility:{gameIds:['dcs']},resources:[],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}};
function node(kind='game',observed=true){return {id:'integration',name:'Integration',kind:'resource',category:'Integration',position:{x:0,y:0},resource:{id:'integration',name:'Integration',kind,required:false,ports:[port]},...(observed?{assignment:{resourceId:'integration',sourceId:'native',ports:[]},observation:{id:'native',name:'Native',resourceType:kind==='game'?'game-integration':'companion',attributes:kind==='game'?{gameId:'dcs',aircraftModule:'CommonData'}:{companionId:'panel'},portCount:1,connections:[],diagnostics:[]}}:{})};}
function body(props={}){return render(Inspector,{props:{node:node(),document,removeNode:()=>{},openGameSettings:()=>{},...props}}).body;}
function footer(html){const match=/<footer\b[^>]*class="[^"]*controller-actions[^"]*"[^>]*>([\s\S]*?)<\/footer>/.exec(html);assert.ok(match,'Inspector actions must be in the footer');return match[1];}


test('selected companion ports immediately use compact Inputs and Outputs tiles without opening Inspect',()=>{
 const companion=node('companion');companion.resource.ports=[{...port,id:'button-1',name:'Virtual A'},{...port,id:'feedback:active',name:'Active lamp',direction:'input'}];
 const html=body({node:companion,companionAuthoring:true,removeProviderPort:()=>{}});
 assert.match(html,/aria-label="Companion inputs"/);assert.match(html,/aria-label="Companion outputs"/);
 assert.match(html,/>Inputs<\/h4>/);assert.match(html,/>Outputs<\/h4>/);
 assert.match(html,/class="port-tile[^>]*data-port-id="button-1"/);assert.match(html,/class="port-tile[^>]*data-port-id="feedback:active"/);
 assert.match(html,/Filter inputs/);assert.match(html,/Filter outputs/);assert.match(html,/Remove Virtual A port and routes/);
 assert.doesNotMatch(html,/ports-section|port-category-toggle|Available ports|<code>|<p[^>]*>state · boolean/);
 assert.match(html,/metadata-section/);
});

test('selected companion port removal stays available and respects read-only mode',()=>{
 for(const readOnly of [false,true]){
  const html=body({node:node('companion'),companionAuthoring:true,readOnly,removeProviderPort:()=>{}});
  const remove=html.match(/<button[^>]*aria-label="Remove Saved signal port and routes"[^>]*>/)?.[0];assert.ok(remove);
  if(readOnly)assert.match(remove,/disabled/);else assert.doesNotMatch(remove,/disabled/);
 }
});

test('non-companion resource ports retain their existing category controls',()=>{
 const html=body({node:node('other')});assert.match(html,/ports-section/);assert.match(html,/port-category-toggle/);assert.doesNotMatch(html,/Companion inputs|Companion outputs/);
});
