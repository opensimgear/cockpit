import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./games','./game-discovery']);
const components=new Map();
async function component(name){
  if(components.has(name))return components.get(name);
  const file=new URL(name,import.meta.url),{js}=compile(await readFile(file,'utf8'),{filename:file.pathname,generate:'server'}),dependencies=new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});
  const exports={};
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,require(name){
    if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;
    if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);
    if(name==='./gateway')return{pickGameExecutable(){throw Error('Passive render');}};
    throw Error(name);
  }});
  components.set(name,exports.default);return exports.default;
}
const View=await component('./GameDiscoveryView.svelte');
function html(status,libraries,entries=[]){
  return render(View,{props:{gameState:{catalogue:[],discovery:{scanId:'scan',status,libraries,entries,diagnostics:[]}},busy:false,
    request(){throw Error('Passive render');},close(){}}}).body;
}
const library=(id,status)=>({id,name:id==='steam'?'Steam':'Local paths',status,found:1});
const entry={id:'example',gameId:'example',name:'Example',action:'add',selected:true,library:'steam',installPath:'C:/Example',executableCandidates:['C:/Example/Game.exe']};

test('enumerated libraries keep a preparing indicator while other libraries are scanned',()=>{
  const output=html('scanning',[library('steam','preparing'),library('disk','scanning')]);
  assert.ok(output.includes('Scanning libraries…'));
  assert.match(output,/<progress[^>]*aria-label="Steam scan progress"/);
  assert.ok(!output.includes('Results appear as each library completes.'));
});

test('matching and artwork preparation have honest progress labels with rows visible before completion',()=>{
  const libraries=[library('steam','preparing'),library('disk','preparing')];
  const matching=html('scanning',libraries);
  assert.ok(matching.includes('Matching discovered installations…'));
  const preparing=html('scanning',libraries,[entry]);
  assert.ok(preparing.includes('Reading platform metadata and preparing artwork… Discovered games are shown below.'));
  assert.match(preparing,/<article[^>]*data-entry-id="example"/);
  const checkbox=preparing.match(/<input[^>]*aria-label="Add Example"[^>]*>/)?.[0];
  assert.ok(checkbox?.includes('disabled'),'Apply selections stay locked until the review is ready');
});

test('completed reviews show the prepared rows without lingering scan progress',()=>{
  const output=html('ready',[library('steam','complete'),library('disk','complete')],[entry]);
  assert.ok(!output.includes('class="scan-status'));
  assert.ok(!output.includes('<progress'));
  assert.ok(!output.match(/<input[^>]*aria-label="Add Example"[^>]*>/)?.[0].includes('disabled'));
  assert.match(output,/<article[^>]*data-entry-id="example"/);
});

test('ready reviews hide empty result groups and empty libraries',()=>{
  const output=html('ready',[library('steam','complete'),{...library('disk','complete'),found:0}],[entry]);
  assert.ok(output.includes('Steam New and changed installations'));
  assert.ok(!output.includes('Removed installations'));
  assert.ok(!output.includes('Other detected installations'));
  assert.ok(!output.includes('data-library="disk"'));
  assert.ok(!output.includes('Match and installation'));
  const removed=html('ready',[library('steam','complete')],[{...entry,action:'remove'}]);
  assert.ok(removed.includes('Steam Removed installations'));
  assert.ok(!removed.includes('New and changed installations'));
});

test('empty libraries remain visible during scanning and when they have diagnostics',()=>{
  const checking=html('scanning',[{...library('disk','scanning'),found:0}]);
  assert.ok(checking.includes('data-library="disk"'));
  const failed=html('ready',[{...library('disk','failed'),found:0,message:'Folder is inaccessible'}]);
  assert.ok(failed.includes('Folder is inaccessible'));
});
