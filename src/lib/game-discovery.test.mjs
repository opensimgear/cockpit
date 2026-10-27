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
const {discoveryActions,discoverySelectAll,discoveryPathError,discoveryMatchOptions,discoveryImage}=modules.get('./game-discovery');
const plain=value=>JSON.parse(JSON.stringify(value));
test('discovery defaults to add and update with removal opt in, preserving explicit decisions',()=>{
 const entries=[{id:'add',action:'add'},{id:'update',action:'update'},{id:'remove',action:'remove'}];
 assert.deepEqual(plain(discoveryActions(entries,{})),[{id:'add',selected:true},{id:'update',selected:true},{id:'remove',selected:false}]);
 const drafts={add:{selected:false},update:{executable:'C:/Games/game.exe',gameId:'existing',replaceInstallationId:'old'},remove:{selected:true}};
 assert.deepEqual(plain(discoveryActions(entries,drafts)),[{id:'add',selected:false},{id:'update',selected:true,...drafts.update},{id:'remove',selected:true}]);
 assert.equal(discoveryActions([...entries,{id:'new',action:'add'}],drafts).at(-1).selected,true);
});

test('bulk selection affects only one library and preserves executable and match choices',()=>{
 const entries=[{id:'steam-add',action:'add'},{id:'steam-remove',action:'remove'},{id:'disk-add',action:'add'}];
 const library=entries.slice(0,2),drafts={'steam-add':{selected:false,executable:'C:/Game/game.exe',gameId:'matched',replaceInstallationId:'existing'},'disk-add':{selected:false}};
 const selected=discoverySelectAll(library,drafts,true);
 assert.deepEqual(plain(discoveryActions(entries,selected)).map(action=>action.selected),[true,true,false]);
 assert.deepEqual(plain(selected['steam-add']),{...drafts['steam-add'],selected:true});
 assert.deepEqual(plain(drafts),{'steam-add':{selected:false,executable:'C:/Game/game.exe',gameId:'matched',replaceInstallationId:'existing'},'disk-add':{selected:false}});
 const cleared=discoverySelectAll(library,selected,false);
 assert.deepEqual(plain(discoveryActions(entries,cleared)).map(action=>action.selected),[false,false,false]);
 assert.deepEqual(plain(discoverySelectAll([],drafts,true)),drafts);
});

const image='data:image/jpeg;base64,/9j/AA==';
test('discovery artwork accepts native raster previews and saved native backgrounds only',()=>{
 assert.equal(discoveryImage({image}),image);
 assert.equal(discoveryImage({image:null},{background:{image}}),image);
 assert.equal(discoveryImage({}, {defaultBackground:{image}}),image);
 for(const image of ['https://example.com/image.jpg','file:///C:/image.png','data:image/svg+xml;base64,PHN2Zz4=','data:image/jpeg;base64,','data:image/png;base64,AA==\n',undefined])assert.equal(discoveryImage({image}),'');
 assert.equal(discoveryImage({image:'https://example.com/image.jpg'},{defaultBackground:{image}}),image);
});

const components=new Map();
async function component(name){
 if(components.has(name))return components.get(name);
 const file=new URL(name,import.meta.url),{js}=compile(await readFile(file,'utf8'),{filename:file.pathname,generate:'server'}),dependencies=new Map();
 for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});
 const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,require(name){
  if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;
  if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);
  if(name==='./gateway')return{pickGameExecutable(){throw Error('Rendering must not open native dialogs.');}};throw Error(name);
 }},{filename:file.pathname});components.set(name,exports.default);return exports.default;
}
test('review rows show artwork, name and executable choice, then the action checkbox',async()=>{
 const View=await component('./GameDiscoveryView.svelte');
 const entries=[{id:'add',action:'add',gameId:'new',name:'New title',image,installPath:'C:/Games/New',executableCandidates:['C:/Games/New/bin/game.exe']},{id:'update',action:'update',name:'Changed title',executableCandidates:[]},{id:'remove',action:'remove',name:'Gone title',executableCandidates:[]}].map(entry=>({library:'steam',installPath:'C:/Install root hidden',previousPath:'C:/Previous root hidden',...entry}));
 const html=render(View,{props:{gameState:{catalogue:[],discovery:{scanId:'fixture',status:'ready',entries,libraries:[{id:'steam',name:'Steam',status:'complete',found:3}],diagnostics:[]}},busy:false,request(){throw Error('Passive render');},close(){}}}).body;
 const rows=[...html.matchAll(/<article\b[^>]*data-entry-id="([^"]+)"[^>]*>([^]*?)<\/article>/g)];assert.equal(rows.length,3);
 const add=rows[0][2];assert.ok(add.includes('src="'+image+'"'));assert.ok(add.indexOf('entry-art')<add.indexOf('entry-details'));assert.ok(add.indexOf('entry-details')<add.indexOf('entry-choice'));assert.match(add,/<select[^>]*aria-label="New title executable"/);assert.ok(add.includes('value="C:/Games/New/bin/game.exe"'));assert.ok(add.includes('>bin\\game.exe</option>'));assert.match(add,/<input[^>]*checked[^>]*>/);
 assert.ok(!html.includes('Install root hidden'));assert.ok(!html.includes('Previous root hidden'));
 assert.ok(!rows[1][2].includes('<img'));assert.ok(rows[1][2].includes('No executable found'));
 assert.ok(!rows[2][2].includes('<select'));assert.match(rows[2][2],/aria-label="Remove Gone title"/);assert.ok(!/<input[^>]*checked/.test(rows[2][2]));
 assert.ok(!html.includes('Match and installation'));assert.match(html,/<details[^>]*data-library="steam"[^>]*open/);
 assert.match(html,/<input[^>]*aria-label="Select all games in Steam"[^>]*>/);
 const source=await readFile(new URL('./GameDiscoveryView.svelte',import.meta.url),'utf8');assert.ok(source.includes('grid-template-columns:144px minmax(0,1fr) auto'));assert.ok(source.includes('align-items:center;gap:14px'));assert.ok(source.includes('font-size:var(--ui-heading-size)'));assert.ok(source.includes('.entry-details{min-width:0;align-self:stretch}'));assert.ok(source.includes('min-height:80px;max-height:112px'));assert.ok(source.includes('object-position:center'));assert.ok(!source.includes('padding-top:3px'));assert.ok(source.includes('failedImages={...failedImages,[image]:true}'));assert.ok(source.includes('openLibraries[library.id]??'));assert.ok(!source.includes('gameDisplayPath(entry.installPath)'));
});
test('local discovery accepts absolute folders and executables and rejects unsafe paths',()=>{
 for(const path of ['C:/Games','/home/pilot/games',String.raw`\\server\games`])assert.equal(discoveryPathError(path),'');
 for(const path of ['', '  ', 'relative/game.exe','C:/Games\nBad'])assert.ok(discoveryPathError(path));
});
test('new catalogue starts empty and arbitrary discovery ids validate independently of membership',()=>{
 const games=modules.get('./games');assert.equal(games.getGameCatalogue().length,0);
 for(const id of ['steam-123','local-flight-sim','dcs','a'.repeat(96)])assert.equal(games.isGameId(id),true);
 for(const id of ['Bad','bad/id','a'.repeat(97),'',null])assert.equal(games.isGameId(id),false);
});

test('first scan can explicitly merge two pending unknown titles before either is saved',()=>{
 const entries=[{id:'steam',gameId:'steam-flight',name:'Flight Simulator',action:'add'},{id:'disk',gameId:'disk-flight',name:'FlightSim',action:'add'},{id:'duplicate',gameId:'steam-flight',name:'Duplicate source',action:'update'},{id:'removed',gameId:'removed-flight',name:'Removed',action:'remove'}];
 assert.deepEqual(plain(discoveryMatchOptions([],entries,entries[1])),[{id:'steam-flight',name:'Flight Simulator'}]);
 assert.deepEqual(plain(discoveryMatchOptions([{id:'saved-flight',name:'Saved title'},{id:'steam-flight',name:'Saved name'}],entries,entries[1])),[{id:'saved-flight',name:'Saved title'},{id:'steam-flight',name:'Saved name'}]);
 const actions=discoveryActions(entries,{disk:{gameId:'steam-flight'}});
 assert.equal(actions.find(action=>action.id==='disk').gameId,'steam-flight');
 assert.equal(actions.find(action=>action.id==='disk').selected,true);
 assert.equal(actions.find(action=>action.id==='steam').selected,true);
});
