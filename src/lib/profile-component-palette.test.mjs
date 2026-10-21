import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import test from 'node:test';
import {compile} from 'svelte/compiler';
import {render} from 'svelte/server';
import * as server from 'svelte/internal/server';
import ts from 'typescript';

const filename=new URL('./ProfileComponentPalette.svelte',import.meta.url);
const {js}=compile(await readFile(filename,'utf8'),{filename:filename.pathname,generate:'server'});
const exports={};
vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,require(name){
  if(name==='svelte/internal/server')return server;
  if(name==='./Icon.svelte')return {default:()=>{}};
  if(name==='./profile-editor')return {profileCategory:resource=>resource.resourceType};
  if(name==='./profile-palette')return {PROFILE_PALETTE_MIME:'application/x-osg-profile-component'};
  throw new Error(`Unexpected palette import ${name}`);
}},{filename:filename.pathname});
const Palette=exports.default;
const never=()=>{throw new Error('Rendering a palette must not change the profile.');};
const game=(id,name)=>({id:`integration:${id}`,name,resourceType:'game-integration',availability:'online',attributes:{gameId:id}});
const controller={id:'controller:1',name:'Stick',resourceType:'host-controller',availability:'online',attributes:{}};
const props={inventory:[game('dcs','DCS World'),game('x-plane-12','X-Plane 12'),controller],assignedIds:new Set(),resourceCount:0,virtualCount:0,desktopAdded:false,refresh:never,addResource:never,addVirtual:never,addDesktop:never,addGroup:never,focusGroup:never,renameGroup:never,deleteGroup:never};
const body=patch=>render(Palette,{props:{...props,...patch}}).body;

test('game integrations are managed by the saved context and never offered as palette components',()=>{
  const html=body({});assert.ok(!html.includes('aria-label="Integrations"'));assert.ok(!html.includes('Add DCS World'));assert.ok(!html.includes('Add X-Plane 12'));
});

test('all independent component accordions start closed, with groups leading outputs and devices',()=>{
  const html=body({inventory:[...props.inventory,{id:'module:1',name:'Module',resourceType:'osg-module',availability:'online',attributes:{}},{id:'companion:1',name:'Display',resourceType:'companion',availability:'online',attributes:{componentKind:'paired-companion'}}]}),labels=[...html.matchAll(/<details\b[^>]*aria-label="([^"]+)"/g)].map(match=>match[1]);assert.deepEqual(labels,['Profile groups','Output components','Controllers','OSG devices','Companions']);assert.ok(!/<details[^>]*\bname=/.test(html));assert.ok(!/<details\b[^>]*\sopen(?:[\s=>])/.test(html));
});

test('groups list focuses and renames each frame, with removal explicitly preserving components',()=>{
  const editorGroups=[{id:'group-1',name:'Flight controls',members:['stick','throttle']},{id:'group-2',name:'Empty group',members:[]}];
  const html=body({editorGroups});
  for(const label of ['Focus Flight controls','Rename Flight controls','Remove Flight controls group','Focus Empty group','Remove Empty group group'])assert.ok(html.includes(`aria-label="${label}"`),label);
  assert.ok(html.includes('Remove group; keep its components'));
  assert.ok(html.includes('Add group'));
  assert.deepEqual(editorGroups[0].members,['stick','throttle']);
});

test('read-only libraries and the group limit prevent palette group mutations without blocking focus',()=>{
  const editorGroups=Array.from({length:32},(_,index)=>({id:`group-${index}`,name:`Group ${index}`,members:[]}));
  const full=body({editorGroups});
  assert.match(full,/<button[^>]*class="add-group\b[^>]*disabled/);
  const readonly=body({editorGroups:editorGroups.slice(0,1),disabled:true});
  for(const label of ['Rename Group 0','Remove Group 0 group'])assert.match(readonly,new RegExp(`<button[^>]*aria-label="${label}"[^>]*disabled`));
  assert.ok(!/<button[^>]*aria-label="Focus Group 0"[^>]*disabled/.test(readonly));
});
