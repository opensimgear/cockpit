import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import * as server from'svelte/internal/server';import * as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph','./profile-companion-authoring','./profile-graph-history','./profile-editor','./profile-canvas']),api=modules.get('./profile-companion-authoring'),graph=modules.get('./profile-graph'),plain=value=>JSON.parse(JSON.stringify(value)),hash=n=>`sha256:${String(n).repeat(64)}`;
const state={delivery:'state',schema:{type:'boolean'},freshnessMs:1500,fallback:null},panel=(n=1)=>({publicationHash:hash(n),capabilityFingerprint:hash(2),title:'Actual flight panel',controlCount:1,assetCount:0}),catalogue=()=>({schemaVersion:1,revision:hash(5),panels:[panel(),panel(6)],diagnostics:[]});
const port=(id='feedback:lamp:active',direction='input')=>({descriptor:{id,name:'Declared active state',direction,contract:plain(state)},contractHash:hash(3),routeIds:[],current:false}),page=()=>({schemaVersion:1,resourceId:'opaque-instance',publicationHash:hash(1),capabilityFingerprint:hash(2),identityFingerprint:hash(4),offset:0,total:1,ports:[port()],diagnostics:[]});
const draft=()=>({document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Companion feedback',revision:1,eligibility:{gameIds:[]},resources:[{id:'screen',name:'Paired panel',kind:'companion',required:false,ports:[]},{id:'telemetry',name:'Threshold condition',kind:'game',required:false,ports:[{id:'active',name:'Active condition',direction:'output',contract:plain(state)}]}],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[{resourceId:'screen',sourceId:'opaque-instance',ports:[]}]});
const deferred=()=>{let resolve;const promise=new Promise(yes=>resolve=yes);return{promise,resolve};};
async function view(value=draft(),seed={},props={}){
  const filename=new URL('./ProfileCompanionEditor.svelte',import.meta.url);let source=await readFile(filename,'utf8');
  source=source.replace('  const resource=$derived',"  const seeded=seedCompanion();catalogue=seeded.catalogue??catalogue;page=seeded.page??page;chosen=seeded.chosen??chosen;target=seeded.target??target;mappings=seeded.mappings??mappings;clearPorts=seeded.clearPorts??clearPorts;group=seeded.group??group;\n  const resource=$derived");
  source=source.replace('</script>',"captureCompanion({refresh,autoBrowsePanels,choose,chooseTarget,browse,capture,release,reconcile,chooseMapping,addPort,syncContext,setGraph(value){graph=value;syncContext();},get state(){return{catalogue,page,chosen,target,busy,error};}});</script>");
  const {js}=compile(source,{filename:filename.pathname,generate:'server'}),exports={},destroyers=[],calls=[],edits=[];let handlers;
  const client=new api.ProfileCompanionAuthoringClient(async request=>{calls.push(plain(request));return request.command==='profile_companion_catalogue'?catalogue():page();});
  vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,seedCompanion:()=>seed,captureCompanion(value){handlers=value;},require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return{...svelte,onDestroy(callback){destroyers.push(callback);}};if(modules.has(name))return modules.get(name);if(name==='./gateway')return{gatewayRequest(){throw new Error('Rendering cannot issue native effects.');}};throw new Error(`Unexpected companion UI dependency ${name}`);}},{filename:filename.pathname});
  const html=render(exports.default,{props:{graph:plain(value),resourceId:'screen',client,change:(next,label)=>edits.push({next:plain(next),label}),...props}}).body;
  return{html,handlers,calls,edits,destroy(){destroyers.forEach(callback=>callback());}};
}
test('actual panel selector opens passively and captures only a chosen retained revision without adding controls or routes',async()=>{
  let h=await view();assert.ok(h.html.includes('Companion panel'));assert.ok(h.html.includes('Refresh panels'));assert.doesNotMatch(h.html,/revision/i);assert.deepEqual(h.calls,[]);assert.deepEqual(h.edits,[]);await h.handlers.refresh();assert.equal(h.calls[0].command,'profile_companion_catalogue');assert.equal(h.handlers.state.catalogue.panels[0].controlCount,1);
  h=await view(draft(),{catalogue:catalogue(),chosen:hash(1)});assert.ok(h.html.includes('1 actual interactive controls'));assert.ok(h.html.includes('feedback-only graph'));assert.ok(h.html.includes('Use panel in profile'));assert.doesNotMatch(h.html,/revision/i);h.handlers.capture();assert.equal(h.edits.length,1);assert.equal(h.edits[0].next.document.resources[0].ports.length,0);assert.equal(h.edits[0].next.document.bindings.length,0);assert.deepEqual(h.edits[0].next.document.companionAssignments,[{resourceId:'screen',publicationHash:hash(1)}]);assert.deepEqual(h.calls,[]);
});

test('selector shows one latest saved state per panel and resolves old selected snapshots by stable identity',async()=>{
  const old={...panel(),panelId:'screen-panel',title:'Old panel title'},latest={...panel(6),panelId:'screen-panel',publicationHashes:[hash(1),hash(6)],title:'Current panel title'};
  const value=api.graphCaptureCompanionPanel(draft(),'screen',old);
  const h=await view(value,{catalogue:{...catalogue(),panels:[old,latest]},chosen:hash(1)});
  assert.ok(h.html.includes('Current panel title'));assert.ok(!h.html.includes('Old panel title'));
  assert.doesNotMatch(h.html,/<code>sha256:|<dt>Publication|<dt>Capabilities|Published panel/);
  assert.equal((h.html.match(/Current panel title · 1 controls/g)??[]).length,1);
  h.handlers.capture();assert.deepEqual(h.edits[0].next.document.companionAssignments,[{resourceId:'screen',panelId:'screen-panel',publicationHash:hash(6)}]);
});
test('paired component configuration automatically browses panels without selecting its current panel or exposing legacy panel names',async()=>{
  const base={resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:0,cachedPortCount:0,connections:[],diagnostics:[]};
  const paired={...base,id:'opaque-instance',name:'Paired kitchen display',attributes:{componentKind:'paired-companion',companionId:'screen',publicationHash:hash(6)}};
  const legacy={...base,id:'legacy-instance',name:'Legacy panel title',attributes:{companionId:'screen',publicationHash:hash(6)}};
  const h=await view(draft(),{},{inventory:[paired,legacy]});
  assert.ok(h.html.includes('Paired kitchen display'));assert.ok(!h.html.includes('Legacy panel title'));
  await h.handlers.autoBrowsePanels();await h.handlers.autoBrowsePanels();
  assert.deepEqual(h.calls,[{command:'profile_companion_catalogue'}]);
  assert.equal(h.handlers.state.chosen,'');assert.deepEqual(h.edits,[]);
  const old=draft();old.assignments[0].sourceId='legacy-instance';
  const retained=await view(old,{},{inventory:[paired,legacy]});assert.ok(retained.html.includes('Legacy panel title'));
});

test('an empty companion block can choose an unused paired target and saved panel in one draft edit',async()=>{
  const source={id:'unused-paired',name:'Unused paired display',resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:0,cachedPortCount:0,connections:[],attributes:{componentKind:'paired-companion',companionId:'unused'},diagnostics:[]};
  for(const unassigned of [false,true]){const value=draft();if(unassigned)value.assignments=[];const h=await view(value,{catalogue:catalogue(),chosen:hash(1),target:source.id},{inventory:[source]});assert.doesNotMatch(h.html,/<button[^>]*disabled[^>]*>Use panel in profile<\/button>/);h.handlers.capture();assert.equal(h.edits.length,1);assert.deepEqual(h.edits[0].next.assignments,[{resourceId:'screen',sourceId:source.id,ports:[]}]);assert.deepEqual(h.edits[0].next.document.companionAssignments,[{resourceId:'screen',publicationHash:hash(1)}]);assert.deepEqual(h.edits[0].next.document.resources[0].ports,[]);assert.deepEqual(h.edits[0].next.document.bindings,[]);assert.deepEqual(h.calls,[]);assert.deepEqual(value.document.companionAssignments,[]);}
});

test('a zero-control saved panel is selectable and an empty native port page creates no dummy controls',async()=>{
  const empty={...panel(),panelId:'acr-display',title:'ACR display',controlCount:0},emptyPage={...page(),total:0,ports:[]},h=await view(draft(),{catalogue:{...catalogue(),panels:[empty,panel(6)]},chosen:hash(1),page:emptyPage});
  assert.ok(h.html.includes('ACR display · 0 controls'));assert.ok(h.html.includes('0 actual interactive controls'));assert.ok(h.html.includes('No ports from this group on this page.'));assert.ok(h.html.replace(/<!--[\s\S]*?-->/g,'').includes('0 declared ports'));assert.ok(!h.html.includes('1–0'));assert.doesNotMatch(h.html,/>Add (?:feedback input|control output)</);h.handlers.capture();assert.equal(h.edits.length,1);assert.deepEqual(h.edits[0].next.document.companionAssignments,[{resourceId:'screen',panelId:'acr-display',publicationHash:hash(1)}]);assert.deepEqual(h.edits[0].next.document.resources[0].ports,[]);assert.deepEqual(h.edits[0].next.document.bindings,[]);assert.deepEqual(h.calls,[]);
});

test('panel selection honors the chosen companion compatibility and cannot capture an incompatible or unknown target',async()=>{
  const source={id:'opaque-instance',name:'Paired display',resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:0,cachedPortCount:0,connections:[],attributes:{componentKind:'paired-companion',companionId:'screen'},diagnostics:[]},reason='This panel exceeds the selected companion display size.',incompatible={...panel(),title:'Oversized panel',compatibility:[{companionId:'screen',compatible:false,reason}]},compatible={...panel(6),title:'Compatible panel',compatibility:[{companionId:'screen',compatible:true}]},metadata={...catalogue(),panels:[incompatible,compatible]};
  let h=await view(draft(),{catalogue:metadata,chosen:hash(6)},{inventory:[source]});assert.ok(h.html.includes('Compatible panel · 1 controls'));assert.ok(!h.html.includes('Oversized panel · 1 controls'));h.handlers.capture();assert.equal(h.edits.length,1);assert.equal(h.edits[0].next.document.companionAssignments[0].publicationHash,hash(6));
  h=await view(draft(),{catalogue:metadata,chosen:hash(1)},{inventory:[source]});h.handlers.capture();assert.deepEqual(h.edits,[]);
  const captured=api.graphCaptureCompanionPanel(draft(),'screen',panel());h=await view(captured,{catalogue:metadata,chosen:hash(1)},{inventory:[source]});assert.ok(h.html.includes(reason));assert.match(h.html,/<button[^>]*disabled[^>]*>Use panel in profile<\/button>/);h.handlers.capture();assert.deepEqual(h.edits,[]);
  h=await view(draft(),{catalogue:metadata,chosen:hash(6),target:'missing-instance'},{inventory:[source]});h.handlers.capture();assert.deepEqual(h.edits,[]);assert.match(h.html,/<button[^>]*disabled[^>]*>Use panel in profile<\/button>/);
});

test('changing a companion with routed ports still requires explicit reconciliation',async()=>{
  let value=api.graphCaptureCompanionPanel(draft(),'screen',panel());value=api.graphSelectCompanionPort(value,'screen',page(),port());value=graph.graphConnect(value,{nodeId:'telemetry',portId:'active'},{nodeId:'screen',portId:value.document.resources[0].ports[0].id});
  const source={id:'unused-paired',name:'Unused paired display',resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:1,cachedPortCount:1,connections:[],attributes:{componentKind:'paired-companion',companionId:'unused'},diagnostics:[]},h=await view(value,{catalogue:catalogue(),chosen:hash(1),target:source.id},{inventory:[source]});
  assert.match(h.html,/<button[^>]*disabled[^>]*>Use panel in profile<\/button>/);h.handlers.capture();assert.deepEqual(h.edits,[]);assert.equal(value.document.bindings.length,1);assert.equal(value.assignments[0].sourceId,'opaque-instance');
});
test('companion choices exclude other blocks by exact source and paired installation aliases while retaining the current legacy assignment',async()=>{
  const base={resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:0,cachedPortCount:0,connections:[],diagnostics:[]};
  const inventory=[
    {...base,id:'opaque-instance',name:'Current paired display',attributes:{componentKind:'paired-companion',companionId:'current'}},
    {...base,id:'used-paired',name:'Already used paired display',attributes:{componentKind:'paired-companion',companionId:'used'}},
    {...base,id:'used-legacy',name:'Current legacy assignment',attributes:{companionId:'used'}},
    {...base,id:'used-exact',name:'Already used exact source',attributes:{componentKind:'paired-companion'}},
    {...base,id:'available',name:'Unused paired display',attributes:{componentKind:'paired-companion',companionId:'available'}},
  ];
  const value=draft();
  for(const [resourceId,sourceId]of[['other','used-legacy'],['exact','used-exact']]){
    value.document.resources.push({id:resourceId,name:resourceId,kind:'companion',required:false,ports:[]});value.assignments.push({resourceId,sourceId,ports:[]});
  }
  const h=await view(value,{},{inventory});
  assert.ok(h.html.includes('Current paired display'));assert.ok(h.html.includes('Unused paired display'));
  assert.ok(!h.html.includes('Already used paired display'));assert.ok(!h.html.includes('Already used exact source'));
  h.handlers.chooseTarget('used-paired');assert.equal(h.handlers.state.target,'opaque-instance');assert.match(h.handlers.state.error,/already used by another profile block/);
  h.handlers.chooseTarget('used-exact');assert.equal(h.handlers.state.target,'opaque-instance');assert.deepEqual(h.edits,[]);
  const duplicated=plain(value);duplicated.assignments[0].sourceId='used-legacy';
  const retained=await view(duplicated,{},{inventory});assert.ok(retained.html.includes('Current legacy assignment'));assert.ok(retained.html.includes('Unused paired display'));
});
test('reconciliation rechecks a stale target against other blocks and cannot bypass alias or exact identity protection',async()=>{
  const base={resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:0,cachedPortCount:0,connections:[],diagnostics:[]};
  for(const alias of[false,true]){
    const value=draft();value.document.resources.push({id:'other',name:'Other screen',kind:'companion',required:false,ports:[]});
    value.assignments.push({resourceId:'other',sourceId:alias?'used-legacy':'used-paired',ports:[]});
    const inventory=[{...base,id:'used-paired',name:'Used screen',attributes:{componentKind:'paired-companion',companionId:'used'}},{...base,id:'used-legacy',name:'Old panel instance',attributes:{companionId:'used'}}];
    const stalePage={...page(),resourceId:'used-paired'};
    const h=await view(value,{catalogue:catalogue(),chosen:hash(1),target:'used-paired',page:stalePage},{inventory});
    assert.match(h.html,/<button[^>]*disabled[^>]*>Assign companion and reconcile panel ports<\/button>/);
    h.handlers.reconcile();assert.equal(h.edits.length,0);assert.match(h.handlers.state.error,/already used by another profile block/);
    assert.equal(value.assignments[0].sourceId,'opaque-instance');assert.deepEqual(h.calls,[]);
  }
});
test('exact prospective feedback choice authors a feedback-only graph and preserves independent real panel controls',async()=>{
  const captured=api.graphCaptureCompanionPanel(draft(),'screen',panel());let h=await view(captured,{catalogue:catalogue(),chosen:hash(1),page:page()});for(const label of ['Prospective retained contracts','no live baseline','Add feedback input','Declared active state'])assert.ok(h.html.includes(label),label);assert.ok(!h.html.includes('Add control output'));assert.equal(h.edits.length,0);
  h.handlers.addPort(port());assert.equal(h.edits.length,1);const next=h.edits[0].next;assert.equal(next.document.resources[0].ports.length,1);assert.equal(next.document.resources[0].ports[0].direction,'input');assert.equal(next.document.resources[0].ports.filter(item=>item.direction==='output').length,0);assert.equal(next.document.bindings.length,0);assert.deepEqual(h.calls,[]);
  const connected=graph.graphConnect(next,{nodeId:'telemetry',portId:'active'},{nodeId:'screen',portId:next.document.resources[0].ports[0].id});h=await view(connected,{catalogue:catalogue(),chosen:hash(1),page:page()});h.handlers.capture();assert.equal(h.edits[0].next.document.bindings.length,1);h.handlers.release();assert.equal(h.edits[1].next.document.bindings.length,1);assert.equal(h.edits[1].next.document.companionAssignments.length,0);
});
test('replacing a publication explicitly clears only selected companion ports/routes and remains undoable draft work',async()=>{
  let current=api.graphCaptureCompanionPanel(draft(),'screen',panel());current=api.graphSelectCompanionPort(current,'screen',page(),port());current=graph.graphConnect(current,{nodeId:'telemetry',portId:'active'},{nodeId:'screen',portId:current.document.resources[0].ports[0].id});
  let h=await view(current,{catalogue:catalogue(),chosen:hash(6)});assert.ok(h.html.includes('1 selected ports and 1 routes'));h.handlers.capture();assert.equal(h.edits.length,0);assert.match(h.handlers.state.error,/explicit removal/);
  h=await view(current,{catalogue:catalogue(),chosen:hash(6),clearPorts:true});h.handlers.capture();assert.equal(h.edits[0].next.document.resources[0].ports.length,0);assert.equal(h.edits[0].next.document.bindings.length,0);assert.equal(h.edits[0].next.document.resources[1].ports.length,1);assert.equal(current.document.bindings.length,1);assert.deepEqual(h.calls,[]);
});
test('unassigned, readonly, inactive and uncaptured selections cannot edit or invent a panel source',async()=>{
  const unassigned=draft();unassigned.assignments=[];let h=await view(unassigned,{catalogue:catalogue(),chosen:hash(1)});assert.ok(h.html.includes('exact paired companion'));h.handlers.capture();assert.equal(h.edits.length,0);
  for(const props of [{readOnly:true},{active:false}]){h=await view(draft(),{catalogue:catalogue(),chosen:hash(1),page:page()},props);h.handlers.capture();h.handlers.release();h.handlers.addPort(port());assert.equal(h.edits.length,0);assert.equal(h.calls.length,0);}
  h=await view(draft(),{catalogue:catalogue(),chosen:hash(1),page:page()});assert.ok(h.html.includes('Use this panel in the profile'));assert.doesNotMatch(h.html,/revision/i);h.handlers.addPort(port());assert.equal(h.edits.length,0);assert.match(h.handlers.state.error,/Capture this exact/);
});
test('failed or late catalogue/port reads cannot change the graph or install a page for a replacement instance',async()=>{
  const pending=deferred(),client={catalogue:()=>pending.promise,inspect:()=>{throw new Error('unavailable native publication');}};
  let h=await view(draft(),{chosen:hash(1)},{client});const loading=h.handlers.refresh();h.destroy();pending.resolve(catalogue());await loading;assert.equal(h.handlers.state.catalogue,null);assert.equal(h.edits.length,0);
  h=await view(draft(),{chosen:hash(1)},{client});await h.handlers.browse('open');assert.match(h.handlers.state.error,/unavailable native publication/);assert.equal(h.edits.length,0);
  const ports=deferred();h=await view(draft(),{chosen:hash(1)},{client:{catalogue:async()=>catalogue(),inspect:()=>ports.promise}});const inspecting=h.handlers.browse('open');const changed=draft();changed.assignments[0].sourceId='replacement-instance';h.handlers.setGraph(changed);ports.resolve(page());await inspecting;assert.equal(h.handlers.state.page,null);assert.equal(h.edits.length,0);
});
test('actual copied-panel reconciliation maps every logical feedback port against desired B while companion still has A',async()=>{
  let value=api.graphCaptureCompanionPanel(draft(),'screen',panel());value=api.graphSelectCompanionPort(value,'screen',page(),port());value=graph.graphConnect(value,{nodeId:'telemetry',portId:'active'},{nodeId:'screen',portId:value.document.resources[0].ports[0].id});value.assignments=[];
  const source={id:'opaque-instance',name:'Explicit paired display',resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:1,cachedPortCount:1,connections:[],attributes:{publicationHash:hash(6)},diagnostics:[]},logical=value.document.resources[0].ports[0].id;
  let h=await view(value,{catalogue:catalogue(),chosen:hash(1),target:'opaque-instance',page:page()},{inventory:[source]});assert.ok(h.html.includes('Explicit instance and port reconciliation'));h.handlers.reconcile();assert.equal(h.edits.length,0);assert.match(h.handlers.state.error,/Map every/);
  h=await view(value,{catalogue:catalogue(),chosen:hash(1),target:'opaque-instance',page:page(),mappings:{[logical]:{observed:port(),page:page()}}},{inventory:[source]});h.handlers.reconcile();assert.equal(h.edits.length,1);assert.equal(h.edits[0].next.assignments[0].sourceId,'opaque-instance');assert.deepEqual(h.edits[0].next.document.bindings,plain(value.document.bindings));assert.equal(h.edits[0].next.document.resources[0].ports.filter(item=>item.direction==='output').length,0);assert.equal(source.attributes.publicationHash,hash(6));assert.deepEqual(h.calls,[]);
});
test('actual capture and exact port handlers participate in graph undo/redo without Save or Apply',async()=>{
  const {ProfileGraphHistory}=modules.get('./profile-graph-history'),history=new ProfileGraphHistory(draft());
  let h=await view(history.current,{catalogue:catalogue(),chosen:hash(1)},{change:(next,label)=>history.commit(next,label)});h.handlers.capture();assert.equal(history.current.document.companionAssignments.length,1);history.undo();assert.equal(history.current.document.companionAssignments.length,0);history.redo();assert.equal(history.current.document.companionAssignments.length,1);
  h=await view(history.current,{catalogue:catalogue(),chosen:hash(1),page:page()},{change:(next,label)=>history.commit(next,label)});h.handlers.addPort(port());assert.equal(history.current.document.resources[0].ports.length,1);assert.equal(history.current.assignments[0].ports.length,1);history.undo();assert.equal(history.current.document.resources[0].ports.length,0);assert.equal(history.current.assignments[0].ports.length,0);history.redo();assert.equal(history.current.assignments[0].ports[0].expectedContractHash,hash(3));assert.equal(history.current.document.bindings.length,0);assert.deepEqual(h.calls,[]);
});
