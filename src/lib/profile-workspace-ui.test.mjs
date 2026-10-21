import {webcrypto} from 'node:crypto';
import assert from 'node:assert/strict';import test from 'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import * as server from'svelte/internal/server';import * as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profile-discovery','./profile-game-context','./profile-retired-recovery','./profile-desktop-preview','./profile-desktop-runtime','./runtime-config','./panel-input-metadata','./games','./game-settings-navigation','./cockpit-history','./profiles','./profile-library','./profile-inventory','./profile-graph','./profile-companion-authoring','./profile-graph-history','./profile-processing','./profile-learning','./profile-learning-controller','./profile-automatic','./profile-automatic-controller','./profile-compilation','./profile-runtime','./profile-runtime-controller','./profile-inspection','./profile-inspection-controller','./profile-activity','./profile-activity-controller','./profile-editor','./profile-canvas','./profile-port-browser','./profile-vehicle-conflicts']);
modules.set('./notifications',{notify:()=>{throw new Error('Rendering must not replay save notifications.');}});
modules.set('./profile-palette',(await loadModules(['./profile-palette'])).get('./profile-palette'));
const noRequest=()=>{throw new Error('Rendering/navigation must not send a native request.');};
const componentCache=new Map();async function component(name){if(componentCache.has(name))return componentCache.get(name);const filename=new URL(name,import.meta.url);const{js}=compile(await readFile(filename,'utf8'),{filename:filename.pathname,generate:'server'});const dependencies=new Map();for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(dependencies.has(name))return dependencies.get(name);if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected dependency ${name}`);}},{filename:filename.pathname});componentCache.set(name,exports.default);return exports.default;}
const ProfilesPanel=await component('./ProfilesPanel.svelte'),ProfileCanvas=await component('./ProfileCanvas.svelte');
const{DEFAULT_PROFILE_ID}=modules.get('./profiles');const{ProfileEditorStore,blankProfile,seedProfile}=modules.get('./profile-editor');const hash=`sha256:${'1'.repeat(64)}`;const uid=n=>`11111111-1111-4111-8111-${String(n).padStart(12,'0')}`;const plain=value=>JSON.parse(JSON.stringify(value));
const resource=(type,n)=>({id:`native:${type}:${n}`,name:`${type} ${n}`,resourceType:type,availability:type==='game-integration'?'unavailable':'online',generation:'1',capabilityRevision:hash,portCount:type==='game-integration'?20000:10,cachedPortCount:10,connections:[{scope:'fixture',key:String(n),route:{id:`route${n}`,transport:type==='osg-module'?'can':type==='game-integration'?'integration':'hid',providerGeneration:'boot',attributes:{gateway:'Gateway-1',owner:'fixture-owner'},baselineReady:false,writable:false,priority:1}}],attributes:{gameId:type==='game-integration'?'dcs':''},diagnostics:type==='game-integration'?['Game closed; waiting for matching context']:[]});
const inventory=['host-controller','osg-module','companion','game-integration'].flatMap(type=>[resource(type,1),resource(type,2)]);
function harness(){const docs=new Map([[DEFAULT_PROFILE_ID,{...blankProfile(uid(1),'Default'),id:DEFAULT_PROFILE_ID}]]);const calls=[];let id=10;const summary=d=>({id:d.id,name:d.name,revision:d.revision,isDefault:d.id===DEFAULT_PROFILE_ID,semanticHash:hash});const session={selectedProfileId:DEFAULT_PROFILE_ID,selectionMode:'manual',state:'stopped',appliedSnapshot:{profileId:DEFAULT_PROFILE_ID,revision:1,semanticHash:hash},fault:null};const api={async list(){return{profiles:[...docs.values()].map(summary),session};},async getAssignments(id){calls.push(['get',id]);return{document:plain(docs.get(id)),assignments:[],session};},async editorSave(document,expectedRevision,assignments){calls.push(['save',expectedRevision]);const saved={...plain(document),revision:expectedRevision===null?1:expectedRevision+1};docs.set(saved.id,saved);return{document:saved,assignments:plain(assignments),profiles:[...docs.values()].map(summary),session};}};const client={listAll:async()=>({resources:inventory}),inspect:noRequest};return{docs,calls,client,store:new ProfileEditorStore(api,client,()=>uid(id++))};}
const html=(store,client)=>render(ProfilesPanel,{props:{editorStore:store,resourceClient:client,openGameSettings:noRequest}}).body;

test('component palette offers paired companions by device name, not retained panel instances or transformations',async()=>{
 const Palette=await component('./ProfileComponentPalette.svelte');
 const paired={...resource('companion',1),name:'Browser tablet',portCount:0,attributes:{componentKind:'paired-companion',companionId:'tablet'}};
 const offline={...paired,id:'offline-tablet',name:'Offline tablet',availability:'offline',attributes:{...paired.attributes,companionId:'offline'}};
 const panel={...paired,id:'old-panel',name:'Flight instruments panel',attributes:{companionId:'tablet',publicationHash:hash}};
 const body=render(Palette,{props:{inventory:[...inventory.filter(resource=>resource.resourceType!=='companion'),paired,offline,panel],assignedIds:new Set(),resourceCount:0,virtualCount:0,desktopAdded:false,refresh:noRequest,addResource:noRequest,addVirtual:noRequest,addDesktop:noRequest}}).body;
 for(const name of ['Component palette','Search components','Add Browser tablet','Add Offline tablet','Add virtual controller','Add keyboard and mouse','Controllers','OSG devices','Groups'])assert.ok(body.includes(name),name);
 assert.ok(!body.includes('Flight instruments panel'));assert.ok(!body.includes('Add transformation'));assert.ok(!body.includes('Visible transformation'));assert.ok(!body.includes('type="checkbox"'));
 assert.ok(!/<button[^>]*aria-label="Add Offline tablet"[^>]*disabled/.test(body),'A paired offline companion remains addable.');
 assert.match(body,/<button[^>]*aria-label="Add Offline tablet"[^>]*draggable="true"/);
 for(const label of ['Add virtual controller','Add keyboard and mouse'])assert.match(body,new RegExp(`<button[^>]*aria-label="${label}"[^>]*draggable="true"`));
});

test('palette prevents duplicate physical companions across legacy assignments and respects each component capacity',async()=>{
 const Palette=await component('./ProfileComponentPalette.svelte'),paired={...resource('companion',1),name:'Browser tablet',attributes:{componentKind:'paired-companion',companionId:'tablet'}},legacy={...paired,id:'old-panel',attributes:{companionId:'tablet'}};
 const props={inventory:[paired,legacy,resource('host-controller',1)],assignedIds:new Set([legacy.id]),resourceCount:64,virtualCount:8,desktopAdded:true,refresh:noRequest,addResource:noRequest,addVirtual:noRequest,addDesktop:noRequest};
 const body=render(Palette,{props}).body;
 for(const label of ['Add Browser tablet','Add host-controller 1','Add virtual controller','Add keyboard and mouse'])assert.match(body,new RegExp(`<button[^>]*aria-label="${label}"[^>]*disabled`),label);
 const readOnly=render(Palette,{props:{...props,resourceCount:0,virtualCount:0,desktopAdded:false,assignedIds:new Set(),disabled:true}}).body;
 for(const label of ['Add Browser tablet','Add host-controller 1','Add virtual controller','Add keyboard and mouse'])assert.match(readOnly,new RegExp(`<button[^>]*aria-label="${label}"[^>]*disabled`),label);
 assert.ok(!readOnly.includes('draggable="true"'));
});

test('drop addition places one component without shifting implicit existing blocks or changing viewport and is one Undo edit',()=>{
 const {profilePlaceComponent}=modules.get('./profile-palette'),{canvasNodes}=modules.get('./profile-canvas'),graph=modules.get('./profile-graph'),{ProfileGraphHistory}=modules.get('./profile-graph-history');
 const value=seedProfile(uid(4),inventory.filter(item=>item.resourceType==='host-controller'));value.document.editor.positions=[];value.document.editor.viewport={x:-174,y:88,zoom:.6};const before=plain(value),positions=canvasNodes(value.document,value.assignments,inventory).map(node=>({nodeId:node.id,...node.position}));
 const next=graph.graphSetVirtual(value,{id:'dropped',name:'Virtual controller',required:false,layout:{buttons:['fire'],axes:[],hats:[]}});next.document=profilePlaceComponent(value.document,next.document,'dropped',{x:-128.5,y:472.25});
 assert.deepEqual(plain(next.document.editor.positions),[...positions,{nodeId:'dropped',x:-128.5,y:472.25}]);assert.deepEqual(plain(next.document.editor.viewport),before.document.editor.viewport);assert.deepEqual(plain(value),before);
 const history=new ProfileGraphHistory(value);history.commit(next,'Add component');assert.deepEqual(plain(history.undo()),before);assert.deepEqual(plain(history.redo()),plain(next));
 for(const position of [{x:NaN,y:1},{x:0,y:Infinity},{x:1e6+1,y:0}])assert.throws(()=>profilePlaceComponent(value.document,next.document,'dropped',position),/bounded canvas/);
});

test('drop admission rechecks inventory, capacities and companion aliases after a drag starts',()=>{
 const {profilePaletteCanAdd}=modules.get('./profile-palette'),document=blankProfile(uid(5)),paired={...resource('companion',1),attributes:{componentKind:'paired-companion',companionId:'tablet'}},legacy={...paired,id:'old-panel',attributes:{companionId:'tablet'}},assignments=[{resourceId:'other',sourceId:legacy.id,ports:[]}];
 const component={kind:'resource',sourceId:paired.id};assert.equal(profilePaletteCanAdd(component,document,[],[paired,legacy]),true);assert.equal(profilePaletteCanAdd(component,document,assignments,[paired,legacy]),false);assert.equal(profilePaletteCanAdd(component,document,[],[]),false);assert.equal(profilePaletteCanAdd({kind:'resource',sourceId:legacy.id},document,[],[paired,legacy]),false);
 document.resources=Array.from({length:64},(_,i)=>({id:`resource-${i}`,name:'Resource',kind:'device',required:false,ports:[]}));assert.equal(profilePaletteCanAdd(component,document,[],[paired]),false);
 document.virtualOutputs=Array.from({length:8},(_,i)=>({id:`virtual-${i}`}));assert.equal(profilePaletteCanAdd({kind:'virtual'},document,[],[]),false);document.desktopOutputs=[{id:'desktop'}];assert.equal(profilePaletteCanAdd({kind:'desktop'},document,[],[]),false);
});

test('palette additions join the drop group in the same undoable edit for resources and outputs',()=>{
 const {profilePlaceComponent}=modules.get('./profile-palette'),{canvasCreateGroup}=modules.get('./profile-canvas'),{ProfileGraphHistory}=modules.get('./profile-graph-history');
 const before=canvasCreateGroup(blankProfile(uid(6)),'Controls'),after=plain(before);after.resources=[{id:'wheel',name:'Wheel',kind:'device',required:false,ports:[]}];
 const placed=profilePlaceComponent(before,after,'wheel',{x:40,y:60});assert.deepEqual(plain(placed.editor.groups[0].members),['wheel']);
 const virtual=modules.get('./profile-graph').graphSetVirtual({document:placed,assignments:[]},{id:'vjoy',name:'Output',required:false,layout:{buttons:['fire'],axes:[],hats:[]}});
 const second=profilePlaceComponent(placed,virtual.document,'vjoy',{x:100,y:80}),history=new ProfileGraphHistory({document:placed,assignments:[]});history.commit({document:second,assignments:[]},'Add output');assert.deepEqual(plain(history.current.document.editor.groups[0].members),['wheel','vjoy']);assert.deepEqual(plain(history.undo().document.editor.groups[0].members),['wheel']);assert.deepEqual(plain(history.redo().document.editor.groups[0].members),['wheel','vjoy']);
});

test('draft controller effect summary survives save and reopen without claiming applied publication or changing its revision',async()=>{
 const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const session=plain(h.store.state.session),{ProfileEditorUndo}=modules.get('./profile-graph-history'),graph=modules.get('./profile-graph'),undo=new ProfileEditorUndo(h.store),devices=h.store.state.editor.document.resources.filter(resource=>resource.kind==='device');
 let value={document:plain(h.store.state.editor.document),assignments:plain(h.store.state.editor.assignments)};value.document.name='Effect choices';value=graph.graphSetRuntimeOverride(value,devices[0].id,true);value=graph.graphSetRuntimeOverride(value,devices[1].id,false);value=graph.graphSetVisibility(value,devices[1].id,'hidden-while-active');value=graph.graphSetVirtual(value,{id:'virtual-one',name:'Requested output',required:false,layout:{buttons:['fire'],axes:[],hats:[]}});undo.edit(value.document,value.assignments,'Edit controller choices');
 const check=()=>{const body=html(h.store,h.client);for(const label of ['Draft controller effects','1 enable while active','1 disable while active','2 keep configured','1 hide-while-active requests','1 requested virtual controllers','Draft requests','Activation checks device support and ownership','Save applies changes when this profile is active.'])assert.ok(body.includes(label),label);assert.deepEqual(plain(h.store.state.session),session);};
 check();assert.deepEqual(h.calls,[]);await h.store.save();const id=h.store.state.editor.document.id;h.store.back();await h.store.open(id);check();assert.deepEqual(h.calls.map(call=>call[0]),['save','get']);
});

test('publication policy follows saved-revision Undo/Redo, reopen, copy and portable export independently of runtime ownership',async()=>{
 const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const first=h.store.state.editor.document.resources.find(resource=>resource.kind==='device').id,{ProfileEditorUndo}=modules.get('./profile-graph-history'),{graphSetRuntimeOverride}=modules.get('./profile-graph'),undo=new ProfileEditorUndo(h.store),session=plain(h.store.state.session);
 let value={document:plain(h.store.state.editor.document),assignments:plain(h.store.state.editor.assignments)};value.document.name='Publication policy';value=graphSetRuntimeOverride(value,first,false);undo.edit(value.document,value.assignments,'Disable publication');await h.store.save();const id=h.store.state.editor.document.id;assert.deepEqual(plain(h.docs.get(id).overrides),[{resourceId:first,enabled:false}]);assert.equal(undo.step('undo'),true);assert.deepEqual(plain(h.store.state.editor.document.overrides),[]);assert.equal(undo.step('redo'),true);assert.equal(h.store.state.editor.document.revision,1);h.store.back();await h.store.open(id);assert.deepEqual(plain(h.store.state.editor.document.overrides),[{resourceId:first,enabled:false}]);
 const portable=modules.get('./profiles').parseProfileDocument(modules.get('./profiles').exportProfileDocument(plain(h.store.state.editor.document)));assert.deepEqual(plain(portable.overrides),[{resourceId:first,enabled:false}]);h.store.back();await h.store.beginCopy(id);assert.deepEqual(plain(h.store.state.editor.document.overrides),[{resourceId:first,enabled:false}]);assert.deepEqual(plain(h.store.state.session),session);assert.deepEqual(h.calls.map(call=>call[0]),['save','get','get']);
});

test('group and collapsed-port authoring survives atomic Save, reopen, copy, export and saved-revision Undo/Redo',async()=>{
 const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const {ProfileEditorUndo}=modules.get('./profile-graph-history'),{canvasSaveGroup,canvasTogglePorts}=modules.get('./profile-canvas'),undo=new ProfileEditorUndo(h.store),first=h.store.state.editor.document.resources[0].id;
 const grouped=canvasSaveGroup(h.store.state.editor.document,'Controls',[first]);grouped.name='Grouped rig';undo.edit(grouped,undefined,'Group controls');undo.edit(canvasTogglePorts(h.store.state.editor.document,first,'outputs'),undefined,'Collapse outputs');const expected=plain(h.store.state.editor.document.editor);await h.store.save();const id=h.store.state.editor.document.id;assert.equal(h.store.state.editor.document.revision,1);assert.deepEqual(plain(h.docs.get(id).editor),expected);
 assert.equal(undo.step('undo'),true);assert.equal(h.store.state.editor.document.editor.collapsedPortGroups,undefined);assert.equal(h.store.state.editor.document.revision,1);assert.equal(undo.step('redo'),true);assert.deepEqual(plain(h.store.state.editor.document.editor),expected);h.store.back();await h.store.open(id);assert.deepEqual(plain(h.store.state.editor.document.editor),expected);
 const exported=modules.get('./profiles').exportProfileDocument(plain(h.store.state.editor.document));assert.deepEqual(JSON.parse(exported).editor,expected);h.store.back();await h.store.beginCopy(id);assert.deepEqual(plain(h.store.state.editor.document.editor),expected);assert.notEqual(h.store.state.editor.document.id,id);assert.deepEqual(plain(h.store.state.editor.assignments),[]);assert.deepEqual(h.calls.map(call=>call[0]),['save','get','get']);
});

test('Profiles enters a Default-only list with visible Add and native keyboard-operable exact-profile rows',async()=>{
 const h=harness();await h.store.refresh();const body=html(h.store,h.client),plainHtml=body.replace(/<!--[\s\S]*?-->/g,'');
 assert.ok(/<h1[^>]*>Profiles<\/h1>/.test(body));assert.match(body,/<button[^>]*>[^]*?Add profile/);
 const actions=plainHtml.match(/<div\b[^>]*class="[^"]*\bpage-actions\b[^"]*"[^>]*>([^]*?)<\/div><\/div>/)?.[1];
 assert.ok(actions);assert.match(actions,/<button\b[^>]*>\s*<svg\b[^]*?<\/svg>\s*Import profile<\/button>\s*<button\b[^>]*>[^]*?Add profile<\/button>/);
 assert.match(plainHtml,/<div\b[^>]*class="[^"]*\blist-tools\b[^"]*"[^>]*>\s*<span>1 profile<\/span>\s*<\/div>/);
 assert.ok(!body.includes('Refresh profiles'));assert.ok(!body.includes(' components'));
 const row=body.match(/<button[^>]*class="[^"]*\bprofile-row\b[^"]*"[^>]*>/)?.[0];assert.ok(row);assert.match(row,/aria-label="Edit Default"/);assert.match(row,/title="Default"/);
 assert.match(body,/<button[^>]*title="Activate saved profile \(hardware only\)"[^>]*aria-label="Activate Default"[^>]*disabled/);
 assert.match(plainHtml,/<strong[^>]*>Default<\/strong>\s*<(span|small)\b[^>]*>Built-in<\/\1>/);
 assert.ok(!body.includes('Built-in · editable'));assert.doesNotMatch(plainHtml,/>Saved profile<\/(?:span|small)>/);
 assert.ok(body.includes('aria-label="Profile selection mode"'));assert.ok(body.includes('Runtime profile status'));assert.ok(!body.includes('Saved revision'));assert.ok(body.includes('Debug'));assert.ok(!body.includes('Profile canvas'));assert.deepEqual(h.calls,[]);
});

test('tile activation applies only the explicit saved target without editor navigation and respects current native admission',async()=>{
 const h=harness();h.docs.set(uid(2),{...blankProfile(uid(2),'Named rig'),revision:7});await h.store.refresh();const row=h.store.state.profiles.find(profile=>profile.id===uid(2)),other=h.store.state.profiles.find(profile=>profile.id===DEFAULT_PROFILE_ID),requests=[],visits=[];
 const stopped={state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null};let status=stopped,statusError=false,finishApply;
 const {ProfileRuntimeClient}=modules.get('./profile-runtime'),client=new ProfileRuntimeClient(async request=>{requests.push(plain(request));if(request.command==='profile_status'){if(statusError)throw new Error('Status disconnected');return status;}if(request.command==='profile_apply')return new Promise(resolve=>{finishApply=resolve;});throw new Error(`Unexpected runtime command ${request.command}`);});
 const filename=new URL('./ProfilesPanel.svelte',import.meta.url),source=(await readFile(filename,'utf8')).replace('</script>','captureActivation({activateProfile,activationDisabled,runtime});</script>'),{js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};let handlers;
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureActivation(value){handlers=value;},require(name){
  if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onDestroy(){}};
  // Preserve the production onState binding while omitting child markup in SSR.
  if(name==='./ProfileRuntimeControls.svelte')return{default:(_payload,props)=>props.controller.subscribe(props.onState)};
  if(name.endsWith('.svelte'))return{default:()=>{}};if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected activation dependency ${name}`);
 }},{filename:filename.pathname});
 render(exports.default,{props:{editorStore:h.store,resourceClient:h.client,runtimeClient:client,onNavigate:id=>visits.push(id)}}).body;
 assert.deepEqual(requests,[]);assert.equal(handlers.activationDisabled(row),true);await handlers.activateProfile(row);assert.deepEqual(requests,[]);
 await handlers.runtime.refresh();assert.equal(handlers.activationDisabled(row),false);const applying=handlers.activateProfile(row);
 const {activationId,...applyRequest}=requests.at(-1);assert.match(activationId,/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/);assert.equal(activationId,handlers.runtime.state.activation.id);assert.deepEqual(applyRequest,{command:'profile_apply',id:row.id,expectedRevision:7,expectedActiveOwner:null});assert.equal(handlers.activationDisabled(row),true);assert.equal(handlers.activationDisabled(other),true);const pendingCount=requests.length;await handlers.activateProfile(other);assert.equal(requests.length,pendingCount);
 const applied={state:'running',appliedSnapshot:{profileId:row.id,revision:7,semanticHash:hash},owner:{session:uid(80),generation:1,profileId:row.id,profileRevision:7,gameId:null},degradation:[],fault:null};finishApply(applied);await applying;
 assert.equal(handlers.activationDisabled(row),true);assert.equal(handlers.activationDisabled(other),false);assert.equal(h.store.state.view,'list');assert.equal(h.store.state.editor,null);assert.deepEqual(h.calls,[]);assert.deepEqual(visits,[]);
 // A retained saved snapshot after Stop must not make that profile look active.
 status={...stopped,appliedSnapshot:applied.appliedSnapshot};await handlers.runtime.refresh();assert.equal(handlers.activationDisabled(row),false);
 for(const state of ['recovery-needed','transitioning','awaiting-game']){
  status=state==='awaiting-game'?{...applied,state}:{...stopped,state};await handlers.runtime.refresh();assert.equal(handlers.activationDisabled(other),true,state);const count=requests.length;await handlers.activateProfile(other);assert.equal(requests.length,count,state);
 }
 status=stopped;await handlers.runtime.refresh();assert.equal(handlers.activationDisabled(row),false);statusError=true;assert.equal(await handlers.runtime.refresh(),false);assert.equal(handlers.activationDisabled(row),true);const staleCount=requests.length;await handlers.activateProfile(row);assert.equal(requests.length,staleCount);handlers.runtime.dispose();
});

test('actual library list exposes Import and saved Export/Delete while corrupt read-only metadata protects every mutation and Default stays undeletable',async()=>{
 const rows=[{id:DEFAULT_PROFILE_ID,name:'Default',revision:1,isDefault:true,semanticHash:hash},{id:uid(2),name:'Named rig',revision:3,isDefault:false,semanticHash:hash}],client={listAll:noRequest,inspect:noRequest},store=new ProfileEditorStore({list:async()=>({profiles:rows,readOnly:true,diagnostics:['Corrupt library preserved.']}),getAssignments:noRequest,editorSave:noRequest},client);
 await store.refresh();const body=html(store,client);assert.ok(body.includes('Corrupt library preserved.'));assert.match(body,/<button[^>]*disabled[^>]*>\s*<svg\b[^]*?<\/svg>[^]*?Import profile<\/button>/);assert.match(body,/<button[^>]*aria-label="Delete Named rig"[^>]*disabled/);assert.ok(!body.includes('aria-label="Delete Default"'));assert.match(body,/<button[^>]*title="Export profile"[^>]*aria-label="Export Default"[^>]*>[\s\S]*?<svg\b/);assert.ok(!/<button[^>]*aria-label="Export Default"[^>]*disabled/.test(body));
});
test('Add opens a complete seeded editor with Save/Cancel/Back, zero routes, and every category including closed games',async()=>{const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const body=html(h.store,h.client);for(const label of ['NEW PROFILE DRAFT','Save profile','Cancel creation','Back to Profiles','Blank profile','Copy saved profile','Profile canvas','Fit all','Auto-layout','Search profile blocks','0 signal routes','0 transformations','0 requested virtual controllers','0 hide-while-active requests'])assert.ok(body.includes(label),label);for(const r of inventory)assert.ok(body.includes(r.name),r.name);assert.equal((body.match(/class="resource-block /g)||[]).length,8);assert.ok(!body.includes('aria-label="Profile selection mode"'));assert.ok(body.includes('Game integration'));assert.ok(body.includes('OSG peripheral'));assert.ok(body.includes('Physical controller'));assert.ok(body.includes('Companion'));assert.ok(body.includes('matching verified game'));assert.ok(body.includes('20,000')||body.includes('20000')||body.includes('Unavailable / dormant'));assert.deepEqual(h.calls,[]);h.store.back(true);assert.equal(h.docs.size,1);assert.ok(/<h1[^>]*>Profiles<\/h1>/.test(html(h.store,h.client)));});
test('Save adds the named row; exact reopening and unsaved Back preserve saved layout without runtime effects',async()=>{const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const next=plain(h.store.state.editor.document);next.name='Rally rig';next.editor.viewport={x:25,y:80,zoom:.6};h.store.update(next);await h.store.save();h.store.back();let body=html(h.store,h.client);assert.ok(body.includes('Edit Rally rig'));assert.ok(body.includes('Edit Default'));await h.store.open(next.id);body=html(h.store,h.client);assert.match(body,/<h1[^>]*aria-label="Rally rig"[^>]*>/);assert.ok(body.includes('aria-label="Edit profile name"'));assert.ok(!/<input[^>]*aria-label="Profile name"/.test(body));assert.ok(body.includes('translate(25px,80px) scale(0.6)'));const changed=plain(h.store.state.editor.document);changed.name='Unsaved';h.store.update(changed);assert.equal(h.store.back(),false);assert.match(html(h.store,h.client),/<h1[^>]*aria-label="Unsaved"[^>]*>/);h.store.back(true);assert.equal(h.docs.get(next.id).name,'Rally rig');assert.equal(h.store.state.session.selectedProfileId,DEFAULT_PROFILE_ID);assert.deepEqual(h.calls.map(c=>c[0]),['save','get']);});
test('resource inspector keeps CAN+USB ownership metadata off graph wires and shows exact typed port groups',()=>{const r=resource('osg-module',1);r.portCount=20000;r.connections.push({...r.connections[0],key:'usb',route:{...r.connections[0].route,id:'usb',transport:'usb'}});const seeded=seedProfile(uid(1),[r]);const node=seeded.document.resources[0];node.ports=[{id:'altitude',name:'Altitude',direction:'output',contract:{delivery:'state',schema:{type:'number',min:0,max:100000,unit:'ft'},freshnessMs:500,fallback:null}},{id:'light',name:'Warning light',direction:'input',contract:{delivery:'action',parameters:[{id:'value',name:'Value',schema:{type:'boolean'}}],semantics:'setter',endpoint:'indicator',readable:true,display:null,ownership:null}}];const body=render(ProfileCanvas,{props:{document:seeded.document,assignments:seeded.assignments,inventory:[r],inventoryClient:{inspect:noRequest},selectedId:node.id,change:noRequest}}).body;for(const label of ['CAN','USB','Connection metadata (2)','Gateway-1','fixture-owner','Selected graph ports · 2','Altitude','ft','fallback unknown','Warning light','setter','Large catalogues stay paged'])assert.ok(body.includes(label),label);assert.ok(!body.includes('class="signal-edge"'));assert.ok(body.includes('20000 advertised'));});
test('saved offline blocks retain identity, placement and signal routing; closed-game settings link is game-scoped',()=>{const seeded=seedProfile(uid(1),inventory);const game=seeded.document.resources.find(r=>r.kind==='game');const body=render(ProfileCanvas,{props:{document:seeded.document,assignments:seeded.assignments,inventory,inventoryClient:{inspect:noRequest},selectedId:game.id,change:noRequest,openGameSettings:noRequest}}).body;assert.ok(body.includes('Open game settings'));assert.ok(body.includes('Game closed; waiting for matching context'));const offline=render(ProfileCanvas,{props:{document:seeded.document,assignments:seeded.assignments,inventory:[],inventoryClient:{inspect:noRequest},selectedId:game.id,change:noRequest}}).body;assert.ok(offline.includes('saved resource is currently absent'));assert.ok(offline.includes('Offline · saved block retained'));assert.ok(offline.includes(`>${game.id}</code>`));});
test('Default editor name is protected; render, navigation and Save do not call runtime control commands',async()=>{const h=harness();await h.store.refresh();await h.store.open(DEFAULT_PROFILE_ID);const body=html(h.store,h.client);assert.match(body,/<h1[^>]*aria-label="Default"[^>]*>/);assert.ok(!body.includes('aria-label="Edit profile name"'));assert.ok(!/<input[^>]*aria-label="Profile name"/.test(body));const source=await readFile(new URL('./ProfilesPanel.svelte',import.meta.url),'utf8');for(const path of ['profile_select','profile_apply','integration_start','dcs_start','game_activate'])assert.ok(!source.includes(path));assert.ok(source.includes('showModal()'));assert.ok(source.includes('store.beginBlank([])'));assert.ok(!source.includes('fitOnMount={editor.baseRevision===null}'));});
test('copy editor preserves real mixed graph/layout, distinguishes transformations/virtual outputs and leaves machine resources unassigned',async()=>{
  const fixture=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/mixed.json',import.meta.url),'utf8'));
  const h=harness();h.docs.set(fixture.id,fixture);await h.store.refresh();await h.store.beginCopy(fixture.id);const body=html(h.store,h.client);
  assert.ok(body.includes('Unassigned'));assert.ok(body.includes('machine assignments are excluded'));assert.ok(body.includes('Explicit transformation'));assert.ok(body.includes('Virtual controller'));assert.equal((body.match(/class="signal-edge(?: |")/g)||[]).length,fixture.bindings.length);
  assert.deepEqual(plain(h.store.state.editor.document.editor),fixture.editor);assert.equal(h.store.state.editor.assignments.length,0);assert.equal(h.docs.size,2);assert.deepEqual(h.calls,[['get',fixture.id]]);
});
test('game settings round-trip retains the same editor store, draft identity, latest edits and applied snapshot',async()=>{
  const {visitGameSettings,returnFromGameSettings}=modules.get('./game-settings-navigation');const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const id=h.store.state.editor.document.id;const before=plain(h.calls),applied=h.store.state.session.appliedSnapshot;
  const origin={view:'profiles',returnState:{editorStore:h.store,profileId:id,selection:{resourceId:h.store.state.editor.document.resources[0].id}}};const visit=visitGameSettings('dcs',origin);
  const edit=plain(h.store.state.editor.document);edit.name='Kept while settings are open';h.store.update(edit);
  const returned=returnFromGameSettings(visit);assert.equal(returned,origin);assert.equal(returned.returnState.editorStore,h.store);assert.equal(h.store.state.editor.document.id,id);assert.equal(h.store.state.editor.document.name,edit.name);assert.equal(h.store.state.session.appliedSnapshot,applied);assert.deepEqual(h.calls,before);assert.ok(html(h.store,h.client).includes(edit.name));
});
async function panelNavigation(store,client,onNavigate) {
  const filename=new URL('./ProfilesPanel.svelte',import.meta.url),source=(await readFile(filename,'utf8')).replace('</script>','capturePanelNavigation({requestNavigation,getNavigation,restoreNavigation,keepEditing,cancel,openProfile,back});</script>');
  const {js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};let navigation;
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,capturePanelNavigation(value){navigation=value;},require(name){
    if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name.endsWith('.svelte'))return{default:()=>{}};
    if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected Panel navigation dependency ${name}`);
  }},{filename:filename.pathname});
  render(exports.default,{props:{editorStore:store,resourceClient:client,onNavigate}}).body;return navigation;
}
test('profile navigation reports real page changes and Keep editing cancels history restoration without dropping the dirty editor',async()=>{
  const h=harness();await h.store.refresh();const visits=[],decisions=[];
  const panel=await panelNavigation(h.store,h.client,id=>visits.push(id));
  await panel.openProfile(DEFAULT_PROFILE_ID);assert.deepEqual(visits,[DEFAULT_PROFILE_ID]);
  const changed=plain(h.store.state.editor.document);changed.editor.viewport={x:99,y:22,zoom:.7};h.store.update(changed);const editor=h.store.state.editor;
  panel.requestNavigation(()=>decisions.push('left'),()=>decisions.push('cancel'));
  assert.equal(panel.getNavigation(),DEFAULT_PROFILE_ID);assert.deepEqual(decisions,[]);
  panel.keepEditing();assert.deepEqual(decisions,['cancel']);assert.equal(h.store.state.editor,editor);
  await panel.restoreNavigation(DEFAULT_PROFILE_ID);assert.equal(h.store.state.editor,editor);assert.deepEqual(h.calls,[['get',DEFAULT_PROFILE_ID]]);
  panel.requestNavigation(()=>decisions.push('left'),()=>decisions.push('cancel'));panel.cancel();
  assert.deepEqual(decisions,['cancel','left']);assert.equal(panel.getNavigation(),null);assert.deepEqual(visits,[DEFAULT_PROFILE_ID]);
  await panel.restoreNavigation(DEFAULT_PROFILE_ID);panel.back();assert.deepEqual(visits,[DEFAULT_PROFILE_ID,null]);
  assert.deepEqual(h.calls,[['get',DEFAULT_PROFILE_ID],['get',DEFAULT_PROFILE_ID]]);
});
test('restoring a discarded profile draft falls back to the list without saving or changing runtime selection',async()=>{
  const h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();const discardedId=h.store.state.editor.document.id,session=plain(h.store.state.session);h.store.back(true);
  const visits=[],panel=await panelNavigation(h.store,h.client,id=>visits.push(id));
  await panel.restoreNavigation(discardedId);assert.equal(panel.getNavigation(),null);assert.deepEqual(visits,[]);
  assert.equal(h.docs.size,1);assert.deepEqual(plain(h.store.state.session),session);assert.deepEqual(h.calls,[['get',discardedId]]);
});
// Expose the real App handlers inside the SSR closure; production has no test API.
async function appHarness() {
  const filename=new URL('../App.svelte',import.meta.url);
  const source=(await readFile(filename,'utf8')).replace('</script>',`captureAppNavigation({navigate,openGameSettings,returnToSettingsOrigin,readSettingsHash,setPanel(panel){profilesPanel=panel;},get state(){return{view,selectedGameId,settingsVisit};}});</script>`);
  const {js}=compile(source,{filename:filename.pathname,generate:'server'});
  const helpers=await loadModules(['./profile-automatic','./profile-retired-recovery','./profile-desktop-preview','./system-inventory','./profiles','./profile-runtime','./profile-runtime-controller','./runtime-config','./panel-input-metadata','./games','./game-settings-navigation','./cockpit-history','./gateway-capabilities','./controller-lifecycle','./devices',]);
  const exports={},events=[];let navigation,location=new URL('http://localhost/#workspace');
  const window={get location(){return location;},dispatchEvent(event){events.push(event);}};
  const history={pushState(_state,_unused,url){location=new URL(url,location);},replaceState(_state,_unused,url){location=new URL(url,location);}};
  const Icon=await component('./Icon.svelte');
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,URL,window,history,crypto:webcrypto,CustomEvent:class{constructor(type,{detail}){this.type=type;this.detail=detail;}},captureAppNavigation(value){navigation=value;},require(name){
    if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;
    if(name.endsWith('/Icon.svelte'))return{default:Icon};
    if(name.endsWith('/ProfilesPanel.svelte'))return{default:()=>{throw new Error('Profiles mounted before its section was opened.');}};
    if(name.endsWith('.svelte'))return{default:()=>{}};
    const key=name.replace('./lib/','./');if(helpers.has(key))return helpers.get(key);
    if(name.endsWith('/gateway'))return{gatewayRequest:noRequest,GatewayError:Error,releaseOnExit:noRequest,openCockpitLink:noRequest};
    if(name.endsWith('/gateway-workspace'))return{GatewayWorkspace:class{constructor(){noRequest();}}};
    if(name.endsWith('/companion-startup'))return{restoreCompanionPanels:noRequest};
    if(name.endsWith('/notifications'))return{notify:noRequest};throw new Error(`Unexpected App dependency ${name}`);
  }},{filename:filename.pathname});
  const body=render(exports.default).body;
  return{body,navigation,events,setHash(hash){const oldURL=location.href;location.hash=hash;navigation.readSettingsHash({oldURL});},get hash(){return location.hash;}};
}
test('actual App sidebar exposes Profiles without eagerly mounting its list/editor on the overview',async()=>{
  const {body}=await appHarness();assert.match(body,/<nav[^>]*aria-label="Main navigation"/);assert.match(body.replace(/<!--[\s\S]*?-->/g,''),/<button[^>]*title="Profiles"[^>]*>[^]*?Profiles<\/button>/);assert.ok(!body.includes('Profile canvas'));assert.ok(!/<h1[^>]*>Profiles<\/h1>/.test(body));
});
test('actual App game-settings handlers retain the original profile through game changes, return and guarded sidebar departure',async()=>{
  const app=await appHarness(),h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();
  const {ProfileNavigationGuard}=modules.get('./profile-editor');const guard=new ProfileNavigationGuard(h.store),nav=app.navigation;
  nav.navigate('profiles');nav.setPanel({requestNavigation:next=>guard.request(next),getNavigation:()=>h.store.state.editor?.document.id??null});const original=h.store.state.editor,applied=h.store.state.session.appliedSnapshot;
  const returnState={editorStore:h.store,profileId:original.document.id,selection:{resourceId:original.document.resources[0].id}};
  nav.openGameSettings('dcs',returnState);const origin=nav.state.settingsVisit.origin;assert.equal(nav.state.view,'games');assert.equal(origin.view,'profiles');assert.equal(origin.returnState,returnState);assert.equal(h.store.state.editor,original);
  nav.openGameSettings('x-plane-12');assert.equal(nav.state.settingsVisit.origin,origin);assert.equal(nav.state.selectedGameId,'x-plane-12');
  nav.returnToSettingsOrigin();assert.equal(nav.state.view,'profiles');assert.equal(app.events.at(-1).detail,returnState);assert.equal(h.store.state.editor,original);
  nav.navigate('games');assert.equal(nav.state.view,'profiles');assert.equal(guard.awaitingChoice,true);guard.keepEditing();assert.equal(h.store.state.editor,original);
  nav.openGameSettings('dcs',returnState);nav.navigate('devices');assert.equal(nav.state.view,'profiles');assert.equal(nav.state.settingsVisit,null);assert.equal(h.store.state.editor,original);guard.discard();assert.equal(nav.state.view,'devices');assert.equal(h.store.state.editor,null);assert.equal(h.store.state.session.appliedSnapshot,applied);assert.deepEqual(h.calls,[]);
  nav.navigate('games');assert.equal(nav.state.settingsVisit,null);assert.equal(nav.state.selectedGameId,'');
});
test('actual App hash navigation keeps the draft and saved canvas on browser Back without creating an ordinary Games return target',async()=>{
  const app=await appHarness(),h=harness();await h.store.refresh();await h.store.refreshInventory();h.store.beginBlank();
  const document=plain(h.store.state.editor.document);document.editor.viewport={x:72,y:-40,zoom:.7};h.store.update(document);const editor=h.store.state.editor;
  app.navigation.navigate('profiles');app.setHash('#games/dcs/integration');assert.equal(app.navigation.state.settingsVisit.origin.view,'profiles');assert.equal(app.navigation.state.settingsVisit.origin.hash,'#workspace');
  app.setHash('#games/x-plane-11/integration');assert.equal(app.navigation.state.settingsVisit.origin.hash,'#workspace');app.setHash('#workspace');assert.equal(app.navigation.state.view,'profiles');assert.equal(app.navigation.state.settingsVisit,null);assert.equal(h.store.state.editor,editor);assert.deepEqual(plain(h.store.state.editor.document.editor.viewport),document.editor.viewport);assert.deepEqual(h.calls,[]);
  app.navigation.navigate('games');assert.equal(app.navigation.state.settingsVisit,null);app.setHash('#games/dcs/integration');assert.equal(app.navigation.state.settingsVisit,null);
});
test('retained selection metadata never marks an inactive profile as active or selected',async()=>{const h=harness();await h.store.refresh();const body=html(h.store,h.client);assert.ok(!body.includes('Selected for runtime'));assert.ok(!body.includes('<span>Active</span>'));assert.deepEqual(h.calls,[]);});
test('profile pages keep runtime controls visible and load diagnostic views only through Debug',async()=>{
 const h=harness();await h.store.refresh();let body=html(h.store,h.client);
 const title=body.indexOf('<h1'),add=body.indexOf('Add profile'),stop=body.indexOf('aria-label="Runtime profile status"'),list=body.indexOf('aria-label="Saved profiles"');
 assert.ok(title<add&&add<stop&&stop<list);assert.ok(body.includes('Debug'));assert.ok(!body.includes('Applied port samples'));assert.ok(!body.includes('Native graph preflight'));assert.ok(!body.includes('Keyboard and mouse recording preview'));assert.ok(!body.includes('Saved revision'));assert.deepEqual(h.calls,[]);
 await h.store.open(DEFAULT_PROFILE_ID);body=html(h.store,h.client);assert.ok(body.indexOf('Back to Profiles')<body.indexOf('<h1'));assert.ok(body.indexOf('Save profile')<body.indexOf('Runtime profile status'));assert.ok(body.indexOf('Runtime profile status')<body.indexOf('Profile canvas'));assert.ok(!body.includes('Applied port samples'));assert.ok(!body.includes('Native graph preflight'));
 const source=await readFile(new URL('./ProfilesPanel.svelte',import.meta.url),'utf8');assert.match(source,/if debugOpen/);assert.ok(source.includes('<ProfileDebugView'));assert.ok(!source.includes('<ProfileLiveInspector'));assert.ok(!source.includes('<ProfileDesktopControls'));
});

test('actual Add handler opens an empty editor immediately without requiring or truncating inventory',async()=>{
 for(const loaded of [false,true]){
  const h=harness();await h.store.refresh();if(loaded)await h.store.refreshInventory();
  const filename=new URL('./ProfilesPanel.svelte',import.meta.url),source=(await readFile(filename,'utf8')).replace('</script>','captureAdd({add});</script>'),{js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={};let handlers;
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,captureAdd(value){handlers=value;},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return svelte;if(name.endsWith('.svelte'))return{default:()=>{}};if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected Add dependency ${name}`);}},{filename:filename.pathname});
  render(exports.default,{props:{editorStore:h.store,resourceClient:h.client}}).body;handlers.add();
  assert.equal(h.store.state.view,'editor');assert.equal(h.store.state.editor.baseRevision,null);assert.deepEqual(plain(h.store.state.editor.document.resources),[]);assert.deepEqual(plain(h.store.state.editor.assignments),[]);assert.deepEqual(h.calls,[]);
 }
});

test('profile mode switch shares the Debug and Add action bar and appears only once on the list',async()=>{
 const h=harness();await h.store.refresh();const body=html(h.store,h.client),actions=body.match(/<div\b[^>]*class="[^"]*\bpage-actions\b[^"]*"[^>]*>([^]*?)<\/div><\/div>/)?.[1],runtimeCard=body.match(/<section\b[^>]*aria-label="Runtime profile status"[^>]*>([^]*?)<\/section>/)?.[1];
 assert.ok(actions);assert.match(actions,/<button\b[^>]*role="switch"[^>]*aria-label="Automatic profile selection"/);assert.ok(actions.includes('Debug'));assert.ok(actions.includes('Add profile'));assert.ok(runtimeCard);assert.ok(!runtimeCard.includes('Profile selection mode'));assert.ok(!runtimeCard.includes('Activate a saved profile from its tile'));assert.equal((body.match(/aria-label="Profile selection mode"/g)??[]).length,1);assert.deepEqual(h.calls,[]);
 await h.store.open(DEFAULT_PROFILE_ID);const editorBody=html(h.store,h.client);assert.ok(!editorBody.includes('aria-label="Profile selection mode"'));assert.ok(!editorBody.includes('Activate this profile when ready. Saving it keeps the current active profile.'));
});

test('profile editor has a saved game context selector, a top history toolbar, and no activation card',async()=>{
 const h=harness();await h.store.refresh();await h.store.open(DEFAULT_PROFILE_ID);const body=html(h.store,h.client).replace(/<!--[\s\S]*?-->/g,'');
 const context=body.match(/<select[^>]*aria-label="Game context"[^>]*>([^]*?)<\/select>/)?.[1];assert.ok(context);assert.match(context,/<option value=""[^>]*>None<\/option>/);
 for(const game of modules.get('./games').supportedGames)assert.ok(context.includes(`value="${game.id}"`),game.name);
 const runtime=body.match(/<section[^>]*aria-label="Runtime profile status"[^>]*>([^]*?)<\/section>/)?.[1];assert.ok(runtime!==undefined);assert.ok(!runtime.includes('Game context'));assert.ok(!runtime.includes('Activate profile'));assert.ok(!body.includes('class="group-editor"'));
 assert.match(body,/<div class="routing-toolbar[^]*?aria-label="Edit history"[^]*?aria-label="Undo"[^]*?<svg[^]*?aria-label="Redo"[^]*?<svg/);
 assert.ok(body.indexOf('aria-label="Edit history"')<body.indexOf('aria-label="Search profile blocks"'));assert.deepEqual(h.calls.map(call=>call[0]),['get']);
});

test('fresh active ownership puts its tile first and stale or absent ownership restores ordinary ordering and tile appearance',async()=>{
 const h=harness(),activeId=uid(3);h.docs.set(activeId,{...blankProfile(activeId,'Zulu rig'),revision:3});h.docs.set(uid(2),blankProfile(uid(2),'Alpha rig'));await h.store.refresh();
 const before=plain(h.store.state.profiles),{ProfileRuntimeClient}=modules.get('./profile-runtime'),{ProfileRuntimeController}=modules.get('./profile-runtime-controller');
 const appliedSnapshot={profileId:activeId,revision:3,semanticHash:hash},stopped={state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null};
 let status={...stopped,state:'running',appliedSnapshot,owner:{session:uid(81),generation:1,profileId:activeId,profileRevision:3,gameId:null}},failStatus=false;
 const runtime=new ProfileRuntimeController(new ProfileRuntimeClient(async request=>{assert.equal(request.command,'profile_status');if(failStatus)throw new Error('Native status disconnected');return status;}));await runtime.refresh();
 const filename=new URL('./ProfilesPanel.svelte',import.meta.url),{js}=compile(await readFile(filename,'utf8'),{filename:filename.pathname,generate:'server'}),exports={};
 vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,require(name){
  if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onDestroy(){}};
  if(name==='./profile-runtime-controller')return{...modules.get(name),ProfileRuntimeController:class{constructor(){return runtime;}}};
  if(name.endsWith('.svelte'))return{default:()=>{}};if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected tile dependency ${name}`);
 }},{filename:filename.pathname});
 const tiles=()=>[...render(exports.default,{props:{editorStore:h.store,resourceClient:h.client}}).body.matchAll(/<li\b[^>]*class="([^"]*\bprofile-tile\b[^"]*)"[^>]*>([^]*?)<\/li>/g)].map(([,classes,body])=>({classes:classes.split(/\s+/),name:body.match(/aria-label="Edit ([^"]+)"/)?.[1],body}));
 let rows=tiles();assert.deepEqual(rows.map(row=>row.name),['Zulu rig','Default','Alpha rig']);assert.ok(rows[0].classes.includes('profile-active'));assert.ok(!rows[0].classes.includes('profile-muted'));assert.ok(rows[0].body.includes('Deactivate Zulu rig'));for(const row of rows.slice(1)){assert.ok(row.classes.includes('profile-muted'));assert.ok(!row.classes.includes('profile-active'));}
 failStatus=true;assert.equal(await runtime.refresh(),false);assert.equal(runtime.state.status.owner.profileId,activeId);
 rows=tiles();assert.deepEqual(rows.map(row=>row.name),['Default','Alpha rig','Zulu rig']);for(const row of rows)assert.ok(!row.classes.includes('profile-active')&&!row.classes.includes('profile-muted'));
 failStatus=false;status={...stopped,appliedSnapshot};await runtime.refresh();rows=tiles();assert.deepEqual(rows.map(row=>row.name),['Default','Alpha rig','Zulu rig']);for(const row of rows){assert.ok(!row.classes.includes('profile-active')&&!row.classes.includes('profile-muted'));assert.ok(!row.body.includes('<span>Active</span>'));}
 status={...status,state:'degraded',degradation:['Optional resource unavailable'],owner:{session:uid(82),generation:2,profileId:activeId,profileRevision:3,gameId:null}};await runtime.refresh();rows=tiles();assert.equal(rows[0].name,'Zulu rig');assert.ok(rows[0].classes.includes('profile-active'));assert.ok(rows[1].classes.includes('profile-muted'));
 assert.deepEqual(plain(h.store.state.profiles),before,'Active-first presentation must not reorder saved profile metadata.');assert.deepEqual(h.calls,[]);runtime.dispose();
});
