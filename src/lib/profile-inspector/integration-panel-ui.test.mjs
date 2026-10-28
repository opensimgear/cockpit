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

test('game inspector places Settings immediately before Remove in its actions footer',()=>{
 const html=body(),actions=footer(html);
 assert.match(actions.replace(/<!--[\s\S]*?-->/g,''),/>Settings<\/button>\s*<button/);
 assert.equal((html.match(/>Settings<\/button>/g)??[]).length,1);
 assert.ok(actions.indexOf('Settings')<actions.indexOf('Remove'));
 assert.doesNotMatch(html.slice(0,html.indexOf('<footer')),/settings-link|>Settings<\/button>/);
});

test('game integrations omit metadata, legacy ports and catalogue Details blocks',()=>{
 const html=body();assert.doesNotMatch(html,/metadata-section|ports-section|catalogue-section|<summary\b[^>]*>Details<\/summary>|Resource attributes|Available ports/);
});

test('configured unassigned integration retains Settings through the saved game context',()=>{
 const html=body({node:node('game',false)}),actions=footer(html);assert.match(actions,/>Settings<\/button>/);assert.match(actions,/>Remove<\/button>/);
});

test('companion inspectors retain their Details and ports sections',()=>{
 const html=body({node:node('companion')});assert.match(html,/metadata-section/);assert.match(html,/<summary\b[^>]*>Details<\/summary>/);assert.match(html,/ports-section/);assert.doesNotMatch(footer(html),/>Settings<\/button>/);
});

test('read-only game inspector disables Remove while retaining available Settings',()=>{
 const actions=footer(body({readOnly:true})),buttons=[...actions.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)];
 const settings=buttons.find(button=>button[2].includes('Settings')),remove=buttons.find(button=>button[2].includes('Remove'));
 assert.ok(settings);assert.ok(remove);assert.doesNotMatch(settings[1],/\bdisabled\b/);assert.match(remove[1],/\bdisabled\b/);
});
