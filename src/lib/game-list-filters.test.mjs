import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import * as svelte from 'svelte';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./games']),{emptyGameListFilters,filterGameList,gameListLibraries,UNTAGGED_TAG_FILTER}=modules.get('./games');
const librarySource=client=>({client,appId:client==='steam'?'123':'fixture',installPath:'C:/Games/Fixture'});
const games=[
  {id:'sim',name:'Integrated simulator',tagIds:['simulation','flight'],provider:'dcs-bios',source:'steam',installations:[{source:'steam',sources:[librarySource('steam'),librarySource('disk')]},{source:'disk'}]},
  {id:'racer',name:'Racing game',tagIds:['racing'],source:'epic'},
  {id:'local',name:'Local game',source:'local'},
  {id:'other',name:'Other integrated game',provider:'x-plane',source:'steam'},
];
const ids=filters=>filterGameList(games,{...emptyGameListFilters(),...filters}).map(game=>game.id).join(',');

test('empty game filters include all games and selections combine within and across groups',()=>{
  assert.equal(ids({}),'sim,racer,local,other');
  assert.equal(ids({tags:['simulation','racing']}),'sim,racer');
  assert.equal(ids({tags:['flight']}),'sim');
  assert.equal(ids({tags:['flight',UNTAGGED_TAG_FILTER]}),'sim,local,other');
  assert.equal(ids({tags:[UNTAGGED_TAG_FILTER]}),'local,other');
  assert.equal(ids({libraries:['epic','disk']}),'sim,racer,local');
  assert.equal(ids({integrations:['available']}),'sim,other');
  assert.equal(ids({integrations:['unavailable']}),'racer,local');
  assert.equal(ids({integrations:['available','unavailable']}),'sim,racer,local,other');
  assert.equal(ids({tags:['simulation','racing'],libraries:['steam'],integrations:['available']}),'sim');
  assert.equal(ids({libraries:['epic'],integrations:['available']}),'');
  assert.equal(filterGameList([{id:'empty',name:'Empty tags',tagIds:[]}],{...emptyGameListFilters(),tags:[UNTAGGED_TAG_FILTER]}).length,1);
  const first=emptyGameListFilters(),second=emptyGameListFilters();first.tags.push('simulation');assert.equal(second.tags.length,0);
});

test('library options cover every installation without treating a supplemental disk observation as a local copy',()=>{
  const list=[...games,{id:'missing',name:'No source'},{id:'shared',name:'Shared path',installations:[{source:'steam',sources:[librarySource('steam'),librarySource('epic'),librarySource('disk')]}]},{id:'manual',name:'Manual',source:'manual'},{id:'standalone',name:'Standalone',source:'standalone'}];
  assert.equal(gameListLibraries(list).map(option=>option.label).join(','),'Epic Games,Local,Steam');
  const shared=list.slice(5,6);assert.equal(filterGameList(shared,{...emptyGameListFilters(),libraries:['disk']}).length,0);
  assert.equal(filterGameList(shared,{...emptyGameListFilters(),libraries:['epic']}).length,1);
  assert.equal(filterGameList(list.slice(4,5),{...emptyGameListFilters(),libraries:['disk']}).length,1);
  assert.equal(gameListLibraries([]).length,0);
});

async function component(name){
  const file=new URL(name,import.meta.url),source=await readFile(file,'utf8'),{js}=compile(source,{filename:file.pathname,generate:'server'}),dependencies=new Map();
  for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});
  const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,require(name){
    if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);throw Error(name);
  }});return exports.default;
}
test('game filter panel uses three multiselect groups with accessible controls',async()=>{
  const Panel=await component('./GameListFilters.svelte');
  const html=render(Panel,{props:{games,tags:{simulation:'Simulation',racing:'Racing'},filters:{tags:['simulation'],libraries:['steam'],integrations:['available']},manageTags(){throw Error('Passive render');}}}).body;
  assert.match(html,new RegExp('<label[^>]*>Tags</label>'));assert.match(html,/role="combobox"/);assert.match(html,/aria-label="Remove Simulation"/);
  for(const label of ['Library','Integration available'])assert.match(html,new RegExp('<legend[^>]*>'+label+'</legend>'));
  assert.equal((html.match(/type="checkbox"/g)??[]).length,5);
  assert.equal((html.match(/<input[^>]*checked/g)??[]).length,2);
  assert.ok(html.includes('Manage tags'));assert.ok(html.includes('Clear'));
});
test('game list hides the filter panel initially and has an icon-only toggle in its actions',async()=>{
  const source=await readFile(new URL('./GamesPanel.svelte',import.meta.url),'utf8');
  assert.ok(source.includes('filtersOpen=$state(false)'));assert.ok(source.includes('{#if filtersOpen}'));
  const button=source.match(/<button class="secondary-button filter-button"[^]*?<\/button>/)?.[0];assert.ok(button);assert.ok(button.includes('aria-label="Filter games"'));assert.ok(button.includes('<Icon name="sliders"'));
  assert.ok(button.endsWith('/></button>'));assert.ok(!source.includes('category-filter'));
});


test('tag selector renders controlled removable badges inside its accessible search field',async()=>{
  const Input=await component('./GameTagInput.svelte');
  const html=render(Input,{props:{tags:{racing:'Racing',sim:'Simulation'},values:['racing'],label:'Game tags',onchange(){throw Error('Passive render');}}}).body;
  assert.match(html,/role="combobox"/);assert.match(html,/aria-expanded="false"/);
  assert.match(html,/aria-label="Remove Racing"/);assert.match(html,/class="tag-chip/);
  assert.equal((html.match(/class="tag-chip/g)??[]).length,1);
  const disabled=render(Input,{props:{tags:{racing:'Racing'},values:['racing'],disabled:true,onchange(){}}}).body;
  assert.match(disabled,/<input[^>]*disabled/);assert.match(disabled,/<button[^>]*disabled/);
});

test('tag manager preserves IDs and captures the taxonomy for conflict detection',async()=>{
  const Dialog=await component('./GameTagsDialog.svelte');
  const tags=Object.fromEntries(Array.from({length:47},(_,index)=>['tag-'+index,'Genre '+index]));
  const html=render(Dialog,{props:{tags,request(){throw Error('Passive render');},close(){},saved(){}}}).body;
  assert.equal((html.match(/class="tag-row/g)??[]).length,47);
  assert.match(html,/data-tag-id="tag-0"/);assert.match(html,/aria-label="Search tags"/);
  for(const label of ['Add','Delete','Cancel','Save'])assert.ok(html.includes(label));
});


test('tag facets distinguish the real untagged ID from the virtual no-tags choice',()=>{
  const catalogue=[{id:'tagged',tagIds:['untagged']},{id:'empty',tagIds:[]}];
  assert.equal(filterGameList(catalogue,{...emptyGameListFilters(),tags:['untagged']}).map(game=>game.id).join(','),'tagged');
  assert.equal(filterGameList(catalogue,{...emptyGameListFilters(),tags:[UNTAGGED_TAG_FILTER]}).map(game=>game.id).join(','),'empty');
});
