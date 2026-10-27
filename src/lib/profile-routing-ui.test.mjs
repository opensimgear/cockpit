import assert from 'node:assert/strict';import test from 'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import * as server from'svelte/internal/server';import * as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./controllers','./controller-inputs','./ui-settings-guard','./profile-runtime','./profile-desktop-runtime','./profile-desktop-preview','./profiles','./runtime-config','./panel-input-metadata','./games','./game-telemetry','./profile-inventory','./profile-graph','./profile-companion-authoring','./profile-graph-history','./profile-processing','./profile-transform-drafts','./profile-canvas-construction','./profile-connection-draft','./profile-chain-edit','./profile-overview-routing','./profile-learning','./profile-learning-controller','./profile-compilation','./profile-editor','./profile-canvas','./profile-palette','./profile-port-browser','./profile-discovery','./profile-wire-layout','./profile-input-availability','./canvas-styling','./canvas-navigation','./profile-canvas-render','./profile-overview-layout','./profile-route-endpoints','./profile-signal-overview','./profile-inspection-controller','./profile-activity-controller','./profile-retired-recovery']);
const graph=modules.get('./profile-graph'),processing=modules.get('./profile-processing'),plain=value=>JSON.parse(JSON.stringify(value));
const {PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT}=modules.get('./profile-editor');
const noRequest=()=>{throw new Error('SSR/authoring must not issue an effect or native request.');};
const defaultElkLayout=async(nodes,bindings)=>({positions:modules.get('./profile-editor').profileAutoLayout(nodes.map(node=>node.id),bindings),wireRoutes:[]});
const bool={type:'boolean'},integer=(min=0,max=32767)=>({type:'integer',min,max,unit:null}),state=(schema=bool)=>({delivery:'state',schema,freshnessMs:1000,fallback:null}),event=()=>({delivery:'event',schema:bool});
const port=(id,direction='output',contract=state())=>({id,name:id,direction,contract}),action=(semantics='setter',parameters=[{id:'value',name:'Value',schema:bool}])=>({delivery:'action',parameters,semantics,endpoint:'fixture-endpoint',readable:true,display:null,ownership:null});
const resource=(id,ports=[],kind='device')=>({id,name:id,kind,required:false,ports});
const draft=()=>({document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Authoring',revision:1,eligibility:{gameIds:[]},resources:[resource('third-party-a',[port('button')]),resource('third-party-b',[port('button')]),resource('can-one',[port('button'),port('event','output',event())]),resource('can-two',[port('button')]),resource('physical-usb',[port('led','input',action())]),resource('dcs',[port('command','input',action('one-shot',[])),port('altitude','output',state({type:'number',min:0,max:100000,unit:'ft'}))],'game'),resource('companion',[port('indicator','input',action())],'companion')],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[]});
async function components(hooks={}){
  const cache=new Map();async function component(name){if(cache.has(name))return cache.get(name);const filename=new URL(name,import.meta.url);let source=await readFile(filename,'utf8');if(name==='./ProfileCanvas.svelte'&&!hooks.overview)source=source.replace('const overview=true','const overview=false');if(name==='./ProfileCanvas.svelte'&&hooks.overviewPair)source=source.replace("let overviewPair=$state<{from:string;to:string}|null>(null);", `let overviewPair=$state<{from:string;to:string}|null>(null);overviewPair=seedOverviewPair();overviewBundleId=\`overview:\${JSON.stringify([overviewPair.from,overviewPair.to])}\`;activeTransform=${JSON.stringify(hooks.activeTransform??'')};showTransforms=!!activeTransform;`);
    if(name==='./ProfileCanvas.svelte'&&hooks.captureCanvas)source=source.replace('</script>',`captureCanvas({connectionKey,layout,begin,beginGroup,move,finish,lostPointer,groupKeyboard,addGroup,renameGroup,deleteGroup,focusGroup,togglePorts,overPalette,leavePalette,dropPalette,clearPalettePreview,keyboard,setStage(value){stage=value;},setView(value){view=value;},setDocument(value){document=value;},setVehicleTab(value){activeVehicleTabId=value;},get state(){return{layoutError,layoutBusy,palettePreview,view,selectedId,groupPreview,groupBlocked};}});</script>`);
    if(name==='./ProfileProcessingEditor.svelte'&&hooks.captureProcessing)source=source.replace('</script>',`captureProcessing({field,save,get draft(){return draft;}});</script>`);
    // Seed visible state for SSR; browser checks cover opening, typing and selecting.
    if(name==='./ProfilePortCombobox.svelte'&&hooks.portComboboxOpen){const original=source;source=source.replace(/open\s*=\s*\$state\(false\)/,'open=$state(true)');assert.notEqual(source,original,'The combobox open-state seed must match the production component.');}
    if(name==='./ProfilePortCombobox.svelte'&&hooks.portComboboxScroll)source=source.replace('scrollTop=$state(0)',`scrollTop=$state(label==='Find an output'?${hooks.portComboboxScroll}:0)`);
    if(name==='./ProfileRoutingEditor.svelte'&&hooks.capture){source=source.replace('  const nodes=$derived',`  const seeded=seedRouting();incompleteRows=seeded.incompleteRows??incompleteRows;activeIncompleteRowId=seeded.activeIncompleteRowId??activeIncompleteRowId;queuedInputs=seeded.queuedInputs??queuedInputs;fromNode=seeded.fromNode??fromNode;fromPort=seeded.fromPort??fromPort;toNode=seeded.toNode??toNode;toPort=seeded.toPort??toPort;preset=seeded.preset??preset;assignmentSource=seeded.assignmentSource??assignmentSource;assignmentPage=seeded.assignmentPage??assignmentPage;selectedMappings=seeded.selectedMappings??selectedMappings;connectionRoute=seeded.connectionRoute??connectionRoute;pendingConnection=seeded.pendingConnection??pendingConnection;routeOpen=seeded.routeOpen??routeOpen;draftRowOpen=seeded.draftRowOpen??draftRowOpen;discoveries=seeded.discoveries??discoveries;sourceSearch=seeded.sourceSearch??sourceSearch;targetSearch=seeded.targetSearch??targetSearch;editingRoute=seeded.editingRoute??editingRoute;highlighted=seeded.highlighted??highlighted;message=seeded.message??message;selectedEdgeId=seeded.selectedEdgeId??selectedEdgeId;autoCommitArmed=seeded.autoCommitArmed??autoCommitArmed;queuedTransforms=seeded.queuedTransforms??queuedTransforms;\n  const nodes=$derived`);source=source.replace('</script>',`captureRouting({removeTransformInput,chooseLaneTransformInput,activateIncomplete,chooseTransformInput,addTransformInput,removeOverviewTransform,refreshDiscoveries,source,destination,connectBlocks,connect,commitReadyConnection,choosePort,autoConnect,reconnectConnection,selectConnection,disconnectConnection,addTransformation,dropOnConnection,configureDraft,editRoute,closeRoute,selectPort,selectOverviewTransform,changeOverviewTransform,addVirtual,addDesktop,openConnection,openPendingConnection,insertTransformation,processingInsertion,addConnectionTransform,changeConnectionTransform,removeConnectionTransform,startConnectionDraft,discardConnectionDraft,routeKeyboard,setPreset(value){preset=value;},actionPack,updateProcessing,quickFactor,updateVirtual,publicationPolicy,resourcePolicy,removeNode,chooseMapping,assignResource,browseAssignment,get state(){return{queuedInputs,inputOptions,queuedInputSpecs,incompleteRows,activeIncompleteRowId,discoveries,message,routeOpen,draftRowOpen,fromNode,fromPort,toNode,toPort,sourceSearch,targetSearch,editingRoute,highlighted,preset,selectedId,selectedTransformLane,assignmentPage,assignmentError,connectionRoute,pendingConnection,connectionChoices,discoveredChoices,sourcePorts,targetPorts,connectionError,selectedEdgeId,autoCommitArmed,pickerAnchor,sourceOptions,targetOptions,connectionPlan,connectionWarning,transformOptions,queuedTransforms};}});</script>`);}
    if(name==='./ProfileDebugView.svelte'&&hooks.captureDebug){source=source.replace('  const draft=$derived',`  const seeded=seedRouting();section=seeded.section??section;preview=seeded.preview??preview;signature=seeded.signature??signature;environment=seeded.environment??environment;diagnostics=seeded.diagnostics??diagnostics;\n  const draft=$derived`);source=source.replace('</script>',`captureDebug({compile,page,read,get state(){return{preview,current,diagnostics,offset,busy,error};}});</script>`);}
    const{js}=compile(source,{filename:filename.pathname,generate:'server'}),dependencies=new Map();for(const match of js.code.matchAll(/from\s+['"]([^'"]+\.svelte)['"]/g))dependencies.set(match[1],{default:await component(match[1])});const exports={};vm.runInNewContext(ts.transpileModule(js.code,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,TextEncoder,seedRouting:()=>hooks.seed??{},captureRouting:value=>hooks.capture?.(value),captureDebug:value=>hooks.captureDebug?.(value),captureCanvas:value=>hooks.captureCanvas?.(value),seedOverviewPair:()=>hooks.overviewPair??null,captureProcessing:value=>hooks.captureProcessing?.(value),require(name){if(name==='svelte/internal/server')return server;if(name==='svelte')return hooks.destroyers?{...svelte,onDestroy(callback){hooks.destroyers.push(callback);}}:svelte;if(dependencies.has(name))return dependencies.get(name);const key=name.replace(/\.ts$/,'');if(modules.has(key))return modules.get(key);if(name==='./profile-elk-runtime')return{layoutProfileWithElk:hooks.elkLayout??defaultElkLayout};if(name==='./gateway')return{gatewayRequest:noRequest};throw new Error(`Unexpected UI dependency ${name}`);}},{filename:filename.pathname});cache.set(name,exports.default);return exports.default;}
  return component;
}
const component=await components();const Routing=await component('./ProfileRoutingEditor.svelte'),Processing=await component('./ProfileProcessingEditor.svelte'),Virtual=await component('./ProfileVirtualEditor.svelte');
const renderRouting=(value,props={})=>render(Routing,{props:{...value,inventory:[],inventoryClient:{inspect:noRequest},change:noRequest,...props}}).body;

test('mixed-epoch catalogue discovery retries on the next native inventory revision and stays quiet for unchanged snapshots',async()=>{
 const value=draft(),source={id:'dcs:F-16C',name:'F-16C',resourceType:'game-integration',availability:'online',generation:'1',capabilityRevision:'capabilities',portCount:65,cachedPortCount:65,connections:[],attributes:{gameId:'dcs',aircraftModule:'F-16C'},diagnostics:[]},sources=[{sourceId:source.id,resource:source,companion:false}],calls=[],ports=Array.from({length:65},(_,i)=>({descriptor:port(`aircraft:${i}`,i%2?'input':'output'),contractHash:`sha256:${'a'.repeat(64)}`,current:true,routeIds:[]}));
 let epoch=1,rejectLate;const client={async inspect(id,offset,limit){calls.push([epoch,offset]);if(epoch===3&&offset===0)return new Promise((_resolve,reject)=>rejectLate=reject);return {session:'inventory',revision:String(epoch===1&&offset?2:epoch),resource:source,evidence:[],ports:ports.slice(offset,offset+limit),offset,total:ports.length};}};
 const destroyers=[],h=await harness(value,{}, {inventory:[source],inventoryClient:client},{destroyers}),settle=()=>new Promise(resolve=>setImmediate(resolve));
 h.handlers.refreshDiscoveries(sources,1);await settle();assert.match(h.handlers.state.discoveries[source.id].error,/changed while discovering/);assert.deepEqual(calls,[[1,0],[1,32]]);
 h.handlers.refreshDiscoveries(sources,1);await settle();assert.equal(calls.length,2,'Rendering the same snapshot must not retry a failed discovery.');
 epoch=2;h.handlers.refreshDiscoveries(sources,2);await settle();assert.equal(h.handlers.state.discoveries[source.id].error,'');assert.equal(h.handlers.state.discoveries[source.id].details.ports.length,65);assert.deepEqual(calls.slice(2),[[2,0],[2,32],[2,64]]);
 const stableDiscoveries=h.handlers.state.discoveries;h.handlers.refreshDiscoveries(sources,2);await settle();assert.equal(calls.length,5);assert.equal(h.handlers.state.discoveries,stableDiscoveries,'An unchanged snapshot must preserve catalogue state identity.');
 epoch=3;h.handlers.refreshDiscoveries(sources,3);epoch=4;h.handlers.refreshDiscoveries(sources,4);await settle();assert.equal(h.handlers.state.discoveries[source.id].details.revision,'4');rejectLate(Error('Previous route epoch retired'));await settle();assert.equal(h.handlers.state.discoveries[source.id].error,'');assert.equal(h.handlers.state.discoveries[source.id].details.ports.length,65);
 assert.deepEqual(h.edits,[]);destroyers.forEach(destroy=>destroy());
});

test('vehicle canvas and exact route menus expose only the selected tab while Base retains its own graph',async()=>{
 const value=draft();value.document.eligibility.gameIds=['dcs'];value.document.vehicleTabs=[{id:'a',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:['third-party-a','can-one']},{id:'b',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:['third-party-b','can-two']}];
 for(const[tab,from,to]of[['a','third-party-a','can-one'],['b','third-party-b','can-two']]){value.document.resources.find(node=>node.id===to).ports.push(port('receive','input'));value.document.bindings.push({id:`route-${tab}`,from:{nodeId:from,portId:'button'},to:{nodeId:to,portId:'receive'}});}
 const base=renderRouting(value);assert.ok(!base.includes('data-node-id="third-party-a"'));assert.ok(!base.includes('data-node-id="third-party-b"'));assert.ok(base.includes('data-node-id="dcs"'));assert.ok(!base.includes('data-binding-id='));
 const h=await harness(value,{routeOpen:true,fromNode:'third-party-a',toNode:'can-one',draftRowOpen:true},{activeVehicleTabId:'a'},{overview:true,overviewPair:{from:'third-party-a',to:'can-one'}});assert.ok(h.body.includes('data-node-id="third-party-a"'));assert.ok(!h.body.includes('data-node-id="third-party-b"'));assert.ok(!h.body.includes('data-node-id="dcs"'));assert.ok(h.body.includes('pending-connection'));assert.ok(!h.body.includes('data-from-node="third-party-b"'));assert.ok(!h.body.includes('route-b'));
 h.handlers.editRoute('route-b');assert.equal(h.handlers.state.fromNode,'third-party-a');h.handlers.addDesktop({x:20,y:40});assert.equal(h.edits.length,1);assert.ok(h.edits[0].next.document.vehicleTabs[0].nodeIds.includes(h.edits[0].next.document.desktopOutputs[0].id));assert.deepEqual(plain(h.edits[0].next.document.resources),value.document.resources);
});

test('auto layout of one vehicle preserves every hidden node position and never moves it into Base groups',async()=>{
 const value=draft();value.document.eligibility.gameIds=['dcs'];value.document.vehicleTabs=[{id:'a',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:['third-party-a']}];
 value.document.editor.positions=value.document.resources.map((node,index)=>({nodeId:node.id,x:600+index*320,y:50}));value.document.editor.groups=[{id:'base-group',name:'Base controls',members:['can-one'],bounds:{x:1200,y:0,width:440,height:360}}];
 const before=plain(value.document),h=await canvasHarness(value.document,{activeVehicleTabId:'a',selectedId:''});stageFor(h);await h.handlers.layout();assert.equal(h.edits.length,1);
 const next=h.edits[0].next;for(const position of before.editor.positions.filter(position=>position.nodeId!=='third-party-a'))assert.deepEqual(next.editor.positions.find(row=>row.nodeId===position.nodeId),position);assert.deepEqual(next.editor.groups,before.editor.groups);assert.deepEqual(next.vehicleTabs,before.vehicleTabs);assert.deepEqual(next.editor.viewport,before.editor.viewport);assert.equal(h.handlers.addGroup('Hidden group'),undefined);
});
// These invoke the real Svelte handlers inside an SSR closure. They do not simulate DOM pointer capture.
async function harness(value,seed={},props={},hooks={}){let handlers;const component=await components({seed,...hooks,capture:value=>handlers=value}),Component=await component('./ProfileRoutingEditor.svelte'),edits=[];const body=render(Component,{props:{...plain(value),inventory:[],inventoryClient:{inspect:noRequest},change:(next,label)=>edits.push({next,label}),...props}}).body;return{body,handlers,edits};}
const connect=async(value,fromNode,fromPort,toNode,toPort)=>{const h=await harness(value,{fromNode,toNode});h.handlers.connectBlocks(fromNode,toNode,undefined,fromPort,toPort);assert.equal(h.handlers.state.message,'');assert.equal(h.edits.length,1);return plain(h.edits[0].next);};
async function debugHarness(value,seed={},props={}){let handlers;const component=await components({seed,captureDebug:value=>handlers=value}),Component=await component('./ProfileDebugView.svelte');const body=render(Component,{props:{editor:{...plain(value),baseDocument:plain(value.document),baseAssignments:plain(value.assignments)},profiles:[{id:value.document.id,name:value.document.name,revision:1,isDefault:false}],inventory:[],inventoryRevision:0,compiler:{compile:noRequest,inspect:noRequest},runtimeState:{status:null,stale:false},inspection:{state:{page:null}},activity:{state:{owner:null,topology:null,path:null,captures:[]}},loadProfile:noRequest,close:noRequest,locate:noRequest,...props}}).body;return{body,handlers};}

test('actual publication policy control saves exact intent, warns about reconnect/rebinding and never treats metadata as Ready',async()=>{
 const value=draft(),proof={schema:1,epoch:1,revision:1,module:{id:'0123456789abcdef0123456789abcdef',boot:1,configurationGeneration:1,node:1,sourceNode:1,profile:{revision:1,buttons:2,axes:0,hats:0,neutral:[],axisUsage:[]},publicationSlot:0,configuredEnabled:true,publicationReportId:1,configuredDescriptorIndex:0}},source={id:'native:can',name:'CAN module',resourceType:'osg-module',availability:'online',generation:'1',capabilityRevision:`sha256:${'1'.repeat(64)}`,portCount:2,cachedPortCount:2,connections:[{scope:'gateway',key:'module',route:{id:'can:gateway:module',transport:'can',providerGeneration:'1',attributes:{},baselineReady:true,writable:true,priority:1}}],attributes:{controllerPublicationOverride:JSON.stringify(proof)},diagnostics:[]};
 value.assignments=[{resourceId:'can-one',sourceId:source.id,ports:value.document.resources.find(row=>row.id==='can-one').ports.map(port=>({profilePortId:port.id,observedPortId:`native:${port.id}`,expectedContractHash:source.capabilityRevision}))}];const props={selectedId:'can-one',inventory:[source]},node=()=>modules.get('./profile-canvas').canvasNodes(value.document,value.assignments,[source]).find(node=>node.id==='can-one');
 let h=await harness(value,{},props);for(const label of ['Controller publication policy','Keep configured','Enable while active','Disable while active','Gateway CAN capability advertised','not a Ready result','USB controller collections','USB reconnect','game rebinding','does not save device flash','Configured controller publication: enabled'])assert.ok(h.body.includes(label),label);h.handlers.publicationPolicy(node(),'disabled');assert.equal(h.edits.length,1);assert.deepEqual(plain(h.edits[0].next.document.overrides),[{resourceId:'can-one',enabled:false}]);assert.equal(h.edits[0].label,'Edit controller publication policy');assert.deepEqual(value.document.overrides,[]);
 value.document=plain(h.edits[0].next.document);h=await harness(value,{},props);h.handlers.publicationPolicy(node(),'enabled');assert.deepEqual(plain(h.edits[0].next.document.overrides),[{resourceId:'can-one',enabled:true}]);h.handlers.publicationPolicy(node(),'configured');assert.deepEqual(plain(h.edits[1].next.document.overrides),[]);h.handlers.publicationPolicy(node(),'other');assert.equal(h.edits.length,2);assert.match(h.handlers.state.message,/explicit/);
 const unsupported={...source,attributes:{}};h=await harness(value,{}, {...props,inventory:[unsupported]});assert.match(h.body,/<option value="enabled" disabled/);assert.match(h.body,/<option value="disabled" disabled/);h.handlers.publicationPolicy({...node(),observation:unsupported},'enabled');assert.equal(h.edits.length,0);assert.match(h.handlers.state.message,/does not advertise/);h.handlers.publicationPolicy({...node(),observation:unsupported},'configured');assert.equal(h.edits.length,1);
 const offline={...source,availability:'offline',attributes:{}};h=await harness(value,{}, {...props,inventory:[offline]});assert.ok(h.body.includes('saved policy is retained'));assert.deepEqual(value.document.overrides,[{resourceId:'can-one',enabled:false}]);h.handlers.publicationPolicy({...node(),observation:offline},'enabled');assert.equal(h.edits.length,1);h=await harness(value,{}, {...props,readOnly:true});h.handlers.publicationPolicy(node(),'configured');assert.equal(h.edits.length,0);assert.ok(!renderRouting(value,{selectedId:'dcs'}).includes('Controller publication policy'));
});

async function canvasHarness(document,props={},hooks={}){
 let handlers;const component=await components({...hooks,captureCanvas:value=>handlers=value}),Canvas=await component('./ProfileCanvas.svelte'),edits=[];
 const body=render(Canvas,{props:{document:plain(document),assignments:[],inventory:[],inventoryClient:{inspect:noRequest},selectedId:'can-one',change:(next,label)=>edits.push({next:plain(next),label}),...props}}).body;
 return{handlers,edits,body};
}
const pointer=(x=0,y=0,extra={})=>({button:0,pointerId:7,clientX:x,clientY:y,target:{closest:()=>null},preventDefault(){},stopPropagation(){},...extra});
function stageFor(h){const captured=new Set();h.handlers.setStage({clientWidth:1000,clientHeight:650,setPointerCapture:id=>captured.add(id),hasPointerCapture:id=>captured.has(id),releasePointerCapture:id=>captured.delete(id)});return captured;}

test('canvas collapses shared physical endpoints while a selected signal retains its own wire and editing affordances',async()=>{
 const document=draft().document;document.resources=document.resources.filter(node=>['can-one','can-two'].includes(node.id));
 document.resources.find(node=>node.id==='can-two').ports.push(port('receive-a','input'),port('receive-b','input'));
 document.bindings=['a','b'].map(id=>({id:`route-${id}`,from:{nodeId:'can-one',portId:'button'},to:{nodeId:'can-two',portId:`receive-${id}`}}));
 document.editor.positions=[{nodeId:'can-one',x:0,y:0},{nodeId:'can-two',x:700,y:100}];
 const before=plain(document);
 const paths=body=>(body.match(/<path[^>]*class="signal-edge(?:\s|")/g)??[]).length;
 const collapsed=await canvasHarness(document,{selectedId:'',selectConnection(){}});assert.equal(paths(collapsed.body),1);assert.match(collapsed.body,/data-bundle-count="2"/);assert.match(collapsed.body,/Choose from 2 connections/);assert.match(collapsed.body,/connection-bundle-badge/);assert.deepEqual(collapsed.edits,[]);
 const selected=await canvasHarness(document,{selectedId:'',selectedEdgeId:'route-b',selectConnection(){},reconnectConnection(){}});assert.equal(paths(selected.body),2);assert.equal((selected.body.match(/data-bundle-count="1"/g)??[]).length,2);assert.match(selected.body,/data-reconnect-binding="route-b"/);assert.doesNotMatch(selected.body,/data-reconnect-binding="route-a"/);
 assert.deepEqual(document,before);
});

test('canvas exposes empty, named groups without the former membership form and deletion keeps every component',async()=>{
 let document=draft().document,h=await canvasHarness(document);assert.ok(!h.body.includes('Editor groups ·'));assert.ok(!h.body.includes('Group members'));
 assert.equal(h.handlers.addGroup('CAN controls'),'group-1');assert.equal(h.edits.length,1);document=h.edits[0].next;assert.deepEqual(document.editor.groups[0].members,[]);assert.equal(h.edits[0].label,'Create group');assert.equal(h.handlers.state.selectedId,'can-one');
 h=await canvasHarness(document);assert.ok(h.body.includes('Focus group CAN controls'));assert.ok(h.body.includes('Drop components here'));assert.match(h.body,/data-group-id="group-1"/);h.handlers.renameGroup('group-1','Primary CAN');document=h.edits[0].next;assert.equal(document.editor.groups[0].name,'Primary CAN');assert.deepEqual(document.editor.groups[0].bounds,{x:32,y:32,width:440,height:360});
 h=await canvasHarness(document);stageFor(h);h.handlers.focusGroup('group-1');assert.deepEqual(h.edits,[]);assert.ok(h.handlers.state.view.zoom>0);h.handlers.togglePorts('outputs');assert.equal(h.edits.length,1);document=h.edits[0].next;assert.deepEqual(document.editor.collapsedPortGroups,[{nodeId:'can-one',group:'outputs'}]);
 h=await canvasHarness(document);assert.match(h.body,/aria-label="Source outputs for can-one" aria-expanded="false"/);assert.match(h.body,/aria-label="Destination inputs for can-one" aria-expanded="true"/);h.handlers.togglePorts('outputs');assert.equal(h.edits[0].next.editor.collapsedPortGroups,undefined);h.handlers.deleteGroup('group-1');assert.equal(h.edits[1].next.editor.groups,undefined);assert.deepEqual(h.edits[1].next.resources,document.resources);assert.deepEqual(h.edits[1].next.editor.positions,document.editor.positions);
 h=await canvasHarness(document,{readOnly:true});h.handlers.togglePorts('actions');h.handlers.deleteGroup('group-1');h.handlers.renameGroup('group-1','No');h.handlers.addGroup('No');assert.equal(h.edits.length,0);assert.ok(!h.body.includes('Resize group Primary CAN'));
 h=await canvasHarness(document);h.handlers.renameGroup('group-1','');assert.equal(h.edits.length,0);assert.match(h.handlers.state.layoutError,/name/);
});

test('canvas renders a selectable common output trunk toward different blocks without changing logical routes',async()=>{
 const document=draft().document;document.resources=document.resources.filter(node=>['can-one','can-two','physical-usb'].includes(node.id));
 document.resources.find(node=>node.id==='can-two').ports.push(port('receive','input'));
 document.bindings=[{id:'branch-a',from:{nodeId:'can-one',portId:'button'},to:{nodeId:'can-two',portId:'receive'}},{id:'branch-b',from:{nodeId:'can-one',portId:'button'},to:{nodeId:'physical-usb',portId:'led'}}];
 document.editor.positions=[{nodeId:'can-one',x:0,y:200},{nodeId:'can-two',x:700,y:0},{nodeId:'physical-usb',x:700,y:450}];
 const before=plain(document),h=await canvasHarness(document,{selectedId:'',selectConnection(){}});
 assert.match(h.body,/class="signal-trunk[\s"]/);assert.match(h.body,/data-trunk-direction="output"/);assert.match(h.body,/class="signal-trunk-hit/);assert.match(h.body,/class="connection-trunk-badge/);assert.match(h.body,/Choose from 2 connections sharing this output/);
 assert.equal((h.body.match(/<path[^>]*class="signal-edge(?:\s|")/g)??[]).length,2);assert.deepEqual(h.edits,[]);assert.deepEqual(document,before);
 const selected=await canvasHarness(document,{selectedId:'',selectedEdgeId:'branch-a',selectConnection(){},reconnectConnection(){}});assert.match(selected.body,/data-reconnect-binding="branch-a"/);assert.doesNotMatch(selected.body,/class="signal-trunk[\s"]/);assert.deepEqual(selected.edits,[]);
});

test('canvas pointer drops join and leave fixed groups in one history edit using world coordinates',async()=>{
 const document=draft().document;document.editor.positions=[{nodeId:'can-one',x:600,y:80}];document.editor.groups=[{id:'controls',name:'Controls',members:[],bounds:{x:0,y:0,width:440,height:360}}];
 let h=await canvasHarness(document),captured=stageFor(h);h.handlers.setView({x:0,y:0,zoom:.5});h.handlers.begin(pointer(500,400),'can-one');h.handlers.move(pointer(250,400));assert.equal(h.edits.length,0);h.handlers.finish(pointer(250,400));assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Move component');assert.deepEqual(h.edits[0].next.editor.groups[0].members,['can-one']);assert.deepEqual(h.edits[0].next.editor.groups[0].bounds,document.editor.groups[0].bounds);assert.deepEqual(h.edits[0].next.editor.positions.find(p=>p.nodeId==='can-one'),{nodeId:'can-one',x:100,y:80});assert.equal(h.handlers.state.selectedId,'can-one');assert.equal(captured.size,0);
 h=await canvasHarness(h.edits[0].next);stageFor(h);h.handlers.begin(pointer(100,80),'can-one');h.handlers.move(pointer(800,80));h.handlers.finish(pointer(800,80));assert.equal(h.edits.length,1);assert.deepEqual(h.edits[0].next.editor.groups[0].members,[]);assert.deepEqual(h.edits[0].next.bindings,document.bindings);
 h=await canvasHarness(document);stageFor(h);h.handlers.begin(pointer(500,400),'can-one');h.handlers.move(pointer(250,400));h.handlers.finish(pointer(250,400),true);assert.equal(h.edits.length,0);assert.equal(h.handlers.state.groupPreview,null);
});

test('group gestures preview frame and member movement, reject overlapping releases and support cancellation',async()=>{
 const document=draft().document;document.editor.positions=[{nodeId:'can-one',x:50,y:60}];document.editor.groups=[{id:'controls',name:'Controls',members:['can-one'],bounds:{x:0,y:0,width:440,height:360}},{id:'other',name:'Other',members:[],bounds:{x:700,y:0,width:440,height:360}}];
 const frame=modules.get('./profile-canvas').canvasGroups(document,modules.get('./profile-canvas').canvasNodes(document,[],[]))[0];
 let h=await canvasHarness(document);stageFor(h);h.handlers.beginGroup(pointer(10,10),frame,'move');h.handlers.move(pointer(110,110));assert.equal(h.edits.length,0);assert.deepEqual(plain(h.handlers.state.groupPreview.editor.groups[0].bounds),{x:100,y:100,width:440,height:360});h.handlers.finish(pointer(110,110));assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Move group');assert.deepEqual(h.edits[0].next.editor.positions.find(p=>p.nodeId==='can-one'),{nodeId:'can-one',x:150,y:160});
 h=await canvasHarness(document);stageFor(h);h.handlers.beginGroup(pointer(10,10),frame,'move');h.handlers.move(pointer(510,10));assert.equal(h.handlers.state.groupBlocked,true);assert.match(h.handlers.state.layoutError,/overlap/);h.handlers.finish(pointer(510,10));assert.equal(h.edits.length,0);
 h=await canvasHarness(document);stageFor(h);h.handlers.beginGroup(pointer(10,10),frame,'resize');h.handlers.move(pointer(90,70));h.handlers.finish(pointer(90,70));assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Resize group');assert.deepEqual(h.edits[0].next.editor.groups[0].bounds,{x:0,y:0,width:520,height:420});assert.deepEqual(plain(modules.get('./profile-canvas').canvasNodes(h.edits[0].next,[],[]).map(node=>[node.id,node.position.x,node.position.y])),plain(modules.get('./profile-canvas').canvasNodes(document,[],[]).map(node=>[node.id,node.position.x,node.position.y])));
 h=await canvasHarness(document);const captured=stageFor(h);h.handlers.beginGroup(pointer(10,10),frame,'move');h.handlers.move(pointer(110,110));h.handlers.keyboard({key:'Escape',preventDefault(){}});assert.equal(h.handlers.state.groupPreview,null);assert.equal(captured.size,0);h.handlers.finish(pointer(110,110));assert.equal(h.edits.length,0);
});

test('auto layout keeps fixed frames and reconciles every moved component membership in one edit',async()=>{
 const document=draft().document;document.editor.positions=[{nodeId:'can-one',x:50,y:60}];document.editor.groups=[{id:'controls',name:'Controls',members:['can-one'],bounds:{x:0,y:0,width:440,height:360}}];
 const h=await canvasHarness(document);stageFor(h);await h.handlers.layout();assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Auto-layout');const next=h.edits[0].next;
 assert.deepEqual(next.editor.groups[0].bounds,document.editor.groups[0].bounds);const {canvasGroupAt,canvasNodes}=modules.get('./profile-canvas');for(const node of canvasNodes(next,[],[])){const expected=canvasGroupAt(next,{x:node.position.x+PROFILE_BLOCK_WIDTH/2,y:node.position.y+PROFILE_BLOCK_HEIGHT/2});assert.equal(next.editor.groups.find(g=>g.members.includes(node.id))?.id??null,expected);}
 assert.deepEqual(next.bindings,document.bindings);assert.deepEqual(document.editor.groups[0].members,['can-one']);
});

test('ELK positions and automatic corridors replace the visible layout in one atomic edit',async()=>{
 const document=draft().document;document.resources.find(node=>node.id==='can-two').ports.push(port('receive','input'));
 document.bindings=[{id:'route-1',from:{nodeId:'can-one',portId:'button'},to:{nodeId:'can-two',portId:'receive'}}];
 document.editor.wireRoutes=[{bindingId:'route-1',points:[{x:0,y:0},{x:100,y:0}]}];
 const before=plain(document),calls=[],positions=document.resources.map((node,index)=>({nodeId:node.id,x:48+index*420,y:64}));
 const wireRoutes=[{bindingId:'route-1',points:[{x:1176,y:232},{x:1308,y:232}],automatic:true}];
 const h=await canvasHarness(document,{}, {elkLayout:async(...args)=>{calls.push(plain(args));return{positions,wireRoutes};}});stageFor(h);
 const pending=h.handlers.layout();assert.equal(h.handlers.state.layoutBusy,true);assert.deepEqual(h.edits,[]);
 await pending;assert.equal(calls.length,1);assert.equal(calls[0][0].length,document.resources.length);assert.deepEqual(calls[0][1],document.bindings);
 assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Auto-layout');assert.deepEqual(h.edits[0].next.editor.positions,positions);assert.deepEqual(h.edits[0].next.editor.wireRoutes,wireRoutes);
 assert.deepEqual(h.edits[0].next.bindings,before.bindings);assert.deepEqual(h.edits[0].next.resources,before.resources);assert.deepEqual(h.edits[0].next.editor.viewport,before.editor.viewport);
 assert.equal(h.handlers.state.layoutBusy,false);assert.equal(h.handlers.state.layoutError,'');assert.deepEqual(document,before);
});

test('ELK layout coalesces pending requests and read-only canvases never start the engine',async()=>{
 const document=draft().document;let resolveLayout,calls=0;
 const result=await defaultElkLayout(document.resources.map(node=>({id:node.id})),[]),pendingResult=new Promise(resolve=>resolveLayout=resolve);
 const elkLayout=async()=>{calls++;return pendingResult;},h=await canvasHarness(document,{}, {elkLayout});stageFor(h);
 const pending=h.handlers.layout();assert.equal(h.handlers.state.layoutBusy,true);
 await new Promise(resolve=>setImmediate(resolve));assert.equal(calls,1);
 await h.handlers.layout();assert.equal(calls,1);assert.deepEqual(h.edits,[]);
 resolveLayout(result);await pending;assert.equal(h.edits.length,1);assert.equal(h.handlers.state.layoutBusy,false);
 const locked=await canvasHarness(document,{readOnly:true},{elkLayout});await locked.handlers.layout();assert.equal(calls,1);assert.deepEqual(locked.edits,[]);assert.equal(locked.handlers.state.layoutBusy,false);
});

test('ELK worker failure leaves the graph untouched and permits a clean retry',async()=>{
 const document=draft().document,before=plain(document);let calls=0;
 const h=await canvasHarness(document,{}, {elkLayout:async(...args)=>{if(++calls===1)throw Error('ELK worker stopped');return defaultElkLayout(...args);}});stageFor(h);
 await h.handlers.layout();assert.deepEqual(h.edits,[]);assert.deepEqual(document,before);assert.equal(h.handlers.state.layoutBusy,false);assert.match(h.handlers.state.layoutError,/ELK worker stopped/);
 await h.handlers.layout();assert.equal(calls,2);assert.equal(h.edits.length,1);assert.equal(h.handlers.state.layoutError,'');assert.equal(h.handlers.state.layoutBusy,false);
});

test('pending ELK results are discarded after document edits, vehicle tab changes or unmount',async()=>{
 for(const reason of ['document','tab','unmount']){
   const document=draft().document;document.vehicleTabs=[{id:'aircraft',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:[]}];
   const destroyers=[];let resolveLayout;
   const result=await defaultElkLayout(document.resources.map(node=>({id:node.id})),[]),pendingResult=new Promise(resolve=>resolveLayout=resolve);
   const h=await canvasHarness(document,{}, {destroyers,elkLayout:async()=>pendingResult});stageFor(h);
   const pending=h.handlers.layout();await new Promise(resolve=>setImmediate(resolve));assert.equal(h.handlers.state.layoutBusy,true);
   if(reason==='document')h.handlers.setDocument({...plain(document),name:'Changed during layout'});
   else if(reason==='tab')h.handlers.setVehicleTab('aircraft');
   else destroyers.forEach(destroy=>destroy());
   resolveLayout(result);await pending;assert.deepEqual(h.edits,[],reason);
   if(reason==='document')assert.match(h.handlers.state.layoutError,/graph changed/i);
   else assert.equal(h.handlers.state.layoutError,'',reason);
 }
});

test('selected companion uses its profile-owned immutable panel picker rather than the currently installed generic catalogue',()=>{
  const value=draft(),hash=`sha256:${'1'.repeat(64)}`,source={id:'native-paired-instance',name:'Explicit companion',resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash,portCount:3,cachedPortCount:3,connections:[],attributes:{publicationHash:hash},diagnostics:[]};
  value.assignments=[{resourceId:'companion',sourceId:source.id,ports:[{profilePortId:'indicator',observedPortId:'current-indicator',expectedContractHash:hash}]}];
  const body=renderRouting(value,{selectedId:'companion',inventory:[source],companionClient:{catalogue:noRequest,inspect:noRequest}});assert.ok(body.includes('Profile companion panel selection'));assert.ok(body.includes('Paired companion'));assert.ok(body.includes('Refresh panels'));assert.ok(!body.includes('Provider catalogue ·'));assert.ok(body.includes('Connection metadata'));
});

test('companion assignment uses its panel inspector and cannot bypass installation guards through generic resource assignment',async()=>{
  const value=draft();value.document.resources.find(resource=>resource.id==='companion').ports=[];
  const source={id:'legacy-panel',name:'Legacy panel title',resourceType:'companion',availability:'online',generation:'1',capabilityRevision:`sha256:${'1'.repeat(64)}`,portCount:0,cachedPortCount:0,connections:[],attributes:{companionId:'tablet'},diagnostics:[]};
  const h=await harness(value,{assignmentSource:source.id},{selectedId:'companion',inventory:[source]});
  assert.ok(h.body.includes('Profile companion panel selection'));assert.ok(!h.body.includes('Inventory resource'));assert.ok(!h.body.includes('Assign resource and exact ports'));
  h.handlers.assignResource();await h.handlers.browseAssignment('open');assert.deepEqual(h.edits,[]);assert.equal(h.handlers.state.assignmentPage,null);
  assert.ok(renderRouting(value,{selectedId:'third-party-a'}).includes('Inventory resource'));
});

test('routing uses canvas connection gestures and leaves history controls to the profile header',async()=>{
  const body=renderRouting(draft());for(const label of ['Exact signal bindings','Choose a row','Advanced connections'])assert.ok(!body.includes(label),label);
  assert.ok(!body.includes('<tbody>'),'The advanced binding table is removed.');
  for(const label of ['Edit history','Undo','Redo'])assert.ok(!body.includes(`aria-label="${label}"`),label);
  for(const removed of ['Connect exact ports','Visible transformation','Add transformation','Add virtual controller','Add keyboard and mouse','Keyboard and mouse preset','Desktop output presets'])assert.ok(!body.includes(removed),removed);assert.ok(!body.includes('profile_apply'));assert.ok(!body.includes('Start integration'));assert.ok(!body.includes('Native preflight'));
});
test('advanced bindings and full graph controls stay absent even with saved routes',()=>{
  const value=draft();value.document.resources[1].ports[0].direction='input';
  const routed=graph.graphConnect(value,{nodeId:'third-party-a',portId:'button'},{nodeId:'third-party-b',portId:'button'});
  const body=renderRouting(routed);
  for(const removed of ['<tbody>','Advanced connections','Exact bindings table','Detailed graph','Toggle detailed graph','Ports &amp; branches'])assert.ok(!body.includes(removed),removed);
});

test('routing forwards profile history callbacks to canvas keyboard shortcuts without local history controls',async()=>{
  const calls=[],props={undo:()=>calls.push('undo'),redo:()=>calls.push('redo')};let canvas,prevented=0;
  const hooks={captureCanvas:value=>canvas=value},h=await harness(draft(),{},props,hooks);
  const press=options=>canvas.keyboard({key:'z',ctrlKey:true,target:{closest:()=>null},preventDefault(){prevented++;},...options});
  press({});press({shiftKey:true});press({ctrlKey:false,metaKey:true});press({key:'y'});
  assert.deepEqual(calls,['undo','redo','undo','redo']);assert.equal(prevented,4);assert.deepEqual(h.edits,[]);
  press({target:{closest:selector=>selector.startsWith('input,')?{}:null}});
  await harness(draft(),{},{...props,readOnly:true},hooks);press({});press({shiftKey:true});
  assert.deepEqual(calls,['undo','redo','undo','redo']);assert.equal(prevented,4);
});

test('incomplete or invalid connection selections create no ports, pins, nodes or routes',async()=>{
  const original=draft(),before=plain(original),h=await harness(original);h.handlers.connectBlocks('can-one','dcs');assert.equal(h.handlers.state.routeOpen,true);assert.equal(h.edits.length,0);assert.deepEqual(plain(original),before);
  const assertCancelled=handlers=>{assert.equal(handlers.state.routeOpen,false);for(const key of ['fromNode','fromPort','toNode','toPort','sourceSearch','targetSearch','editingRoute','selectedId','message'])assert.equal(handlers.state[key],'',key);assert.deepEqual(plain(handlers.state.highlighted),[]);assert.equal(handlers.state.pendingConnection,false);assert.equal(handlers.state.connectionRoute,'');assert.equal(handlers.state.preset,'identity');};
  h.handlers.closeRoute();assertCancelled(h.handlers);assert.deepEqual(h.edits,[]);
  const value=draft(),valueBefore=plain(value),incompatible=await harness(value,{routeOpen:true,fromNode:'can-one',fromPort:'button',toNode:'dcs',toPort:'command',sourceSearch:'button',targetSearch:'command',pendingConnection:true,preset:'hysteresis'},{selectedId:'can-one'});incompatible.handlers.autoConnect();assert.equal(incompatible.edits.length,0);assert.match(incompatible.handlers.state.connectionError,/complete invocation/);
  incompatible.handlers.closeRoute();assertCancelled(incompatible.handlers);incompatible.handlers.closeRoute();assertCancelled(incompatible.handlers);assert.deepEqual(incompatible.edits,[]);assert.deepEqual(value,valueBefore);
  const initial=draft();initial.document.resources[1].ports[0].direction='input';const routed=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'button'},{nodeId:'third-party-b',portId:'button'}),saved=plain(routed),binding=routed.document.bindings[0],editing=await harness(routed,{}, {selectedId:'third-party-a'});
  editing.handlers.editRoute(binding.id);assert.equal(editing.handlers.state.editingRoute,binding.id);assert.deepEqual(plain(editing.handlers.state.highlighted),[binding.id]);editing.handlers.closeRoute();assertCancelled(editing.handlers);assert.deepEqual(editing.edits,[]);assert.deepEqual(plain(routed),saved,'Cancelling an existing route edit keeps the route and assignments unchanged.');
});

function discoveredFixture(sourceContract=state(),targetContract=state(),sourceCount=1){
  const value=draft();value.document.resources=[resource('source'),resource('target')];value.assignments=['source','target'].map(resourceId=>({resourceId,sourceId:`native:${resourceId}`,ports:[]}));
  const observed=(id,direction,contract)=>({descriptor:{...port(id,direction,contract),name:`Discovered ${id}`},contractHash:`sha256:${'1'.repeat(64)}`,routeIds:['native:route'],current:true});
  const details=(id,ports)=>({session:'inventory',revision:'1',resource:{id,name:id,resourceType:'osg-module',availability:'online',generation:'1',capabilityRevision:'capabilities',portCount:ports.length,cachedPortCount:ports.length,connections:[],attributes:{},diagnostics:[]},evidence:[],ports,offset:0,total:ports.length});
  const source=details('native:source',Array.from({length:sourceCount},(_,index)=>observed(`output:${index}`,'output',sourceContract))),target=details('native:target',[observed('input:0','input',targetContract)]);
  const discoveries=Object.fromEntries([source,target].map(details=>[details.resource.id,{signature:'captured',details,busy:false,error:''}]));
  const discovered=modules.get('./profile-discovery'),sourcePort=discovered.profileDiscoveredPorts(value,'source',source).at(-1),targetPort=discovered.profileDiscoveredPorts(value,'target',target)[0];
  return{value,sourcePort,targetPort,seed:{fromNode:'source',fromPort:sourcePort.key,toNode:'target',toPort:targetPort.key,routeOpen:true,discoveries}};
}

test('a complete exact discovered pair auto-commits once and preserves provider identities',async()=>{
 const{value,seed}=discoveredFixture(state(),state(),2),target=seed.discoveries['native:target'].details;
 target.ports.push({...plain(target.ports[0]),descriptor:{...plain(target.ports[0].descriptor),id:'input:1',name:'Second input'}});target.total=2;target.resource.portCount=2;target.resource.cachedPortCount=2;
 const found=modules.get('./profile-discovery'),outputs=found.profileDiscoveredPorts(value,'source',seed.discoveries['native:source'].details),inputs=found.profileDiscoveredPorts(value,'target',target),h=await harness(value,{...seed,fromPort:'',toPort:'',draftRowOpen:true});
 h.handlers.choosePort('from',outputs[1].key);assert.deepEqual(h.edits,[]);assert.equal(h.handlers.state.routeOpen,true);
 h.handlers.choosePort('to',inputs[1].key);assert.equal(h.edits.length,1);assert.equal(h.handlers.state.draftRowOpen,false);assert.equal(h.handlers.state.queuedTransforms.length,0);
 const next=plain(h.edits[0].next);assert.equal(next.document.bindings.length,1);assert.deepEqual(next.assignments.map(assignment=>assignment.ports[0].observedPortId),['output:1','input:1']);assert.deepEqual(next.document.resources.map(resource=>resource.ports.length),[1,1]);assert.equal(next.document.processingNodes.length,0);
 h.handlers.autoConnect();h.handlers.autoConnect();assert.equal(h.edits.length,1,'Repeated effect or discovery checks must not commit the same connection again.');
});

test('invalid selections stay uncommitted and occupied inputs auto-replace exactly once',async()=>{
 const{value,seed}=discoveredFixture(state(),state(),2);seed.discoveries['native:source'].details.ports[1].descriptor.contract=state(integer());
 const sourceChoices=modules.get('./profile-discovery').profileDiscoveredPorts(value,'source',seed.discoveries['native:source'].details),automatic=await harness(value,{...seed,fromPort:sourceChoices[1].key,autoCommitArmed:true,draftRowOpen:true});automatic.handlers.autoConnect();assert.equal(automatic.edits.length,0);assert.equal(automatic.handlers.state.routeOpen,true);assert.equal(automatic.handlers.state.fromPort,sourceChoices[1].key);
 automatic.handlers.choosePort('from',sourceChoices[0].key);assert.equal(automatic.edits.length,1);assert.equal(automatic.handlers.state.draftRowOpen,false);assert.equal(automatic.edits[0].next.assignments[0].ports[0].observedPortId,'output:0');automatic.handlers.autoConnect();assert.equal(automatic.edits.length,1);
 const initial=draft();initial.document.resources.find(node=>node.id==='can-two').ports.push(port('receive','input'));const routed=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'button'},{nodeId:'can-two',portId:'receive'}),binding=routed.document.bindings[0],h=await harness(routed,{fromNode:'third-party-b',fromPort:'button',toNode:'can-two',toPort:'receive',routeOpen:true,draftRowOpen:true});
 assert.equal(h.handlers.state.connectionPlan.writer.id,binding.id);h.handlers.choosePort('to','receive');assert.equal(h.edits.length,1);assert.equal(h.handlers.state.draftRowOpen,false);assert.equal(h.edits[0].label,'Replace connection');assert.equal(h.edits[0].next.document.bindings.length,1);assert.deepEqual(plain(h.edits[0].next.document.bindings[0]),{id:binding.id,from:{nodeId:'third-party-b',portId:'button'},to:{nodeId:'can-two',portId:'receive'}});
 const cancel=await harness(routed,{fromNode:'third-party-b',fromPort:'button',toNode:'can-two',toPort:'receive',routeOpen:true});cancel.handlers.closeRoute();assert.deepEqual(cancel.edits,[]);assert.deepEqual(plain(routed.document.bindings),plain([binding]));
});

test('reconnecting a wire endpoint preserves its exact route identity in one canvas edit',async()=>{
 const initial=draft();initial.document.resources.find(node=>node.id==='can-two').ports.push(port('receive','input'));const routed=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'button'},{nodeId:'can-two',portId:'receive'}),binding=routed.document.bindings[0],h=await harness(routed,{fromNode:'can-one',fromPort:'',toNode:'can-two',toPort:'receive'});
 h.handlers.reconnectConnection(binding.id,'from','can-one',{x:650,y:340});assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Reconnect ports');assert.deepEqual(plain(h.edits[0].next.document.bindings),[{id:binding.id,from:{nodeId:'can-one',portId:'button'},to:plain(binding.to)}]);assert.equal(h.handlers.state.draftRowOpen,false);
});

test('complete port gestures auto-commit the exact Gate port in either click order',async()=>{
 const value=draft(),gate=processing.processingTemplate('gate','gate',state());value.document.processingNodes.push(gate);
 for(const mode of ['drag','output-first','input-first']){
  const h=await harness(value,{fromNode:'can-one',toNode:'gate'});
  if(mode==='drag')h.handlers.connectBlocks('can-one','gate',{x:600,y:300},'button','value');
  else if(mode==='output-first'){h.handlers.source('can-one',undefined,'button');h.handlers.destination('gate',undefined,'value');h.handlers.startConnectionDraft('can-one','gate');}
  else{h.handlers.destination('gate',undefined,'value');h.handlers.source('can-one',undefined,'button');h.handlers.startConnectionDraft('can-one','gate');}
  assert.equal(h.edits.length,1,mode);assert.deepEqual(plain(h.edits[0].next.document.bindings[0]),{id:'route-1',from:{nodeId:'can-one',portId:'button'},to:{nodeId:'gate',portId:'value'}},mode);
  assert.equal(h.handlers.state.draftRowOpen,false);assert.equal(h.handlers.state.message,'');
 }
 const control=await harness(value,{fromNode:'can-one',toNode:'gate'});control.handlers.connectBlocks('can-one','gate',undefined,'button','gate');assert.equal(control.edits.length,1);assert.equal(control.edits[0].next.document.bindings[0].to.portId,'gate','The boolean gate control must not be replaced by the equally compatible value input.');
});

test('named transform outputs and reconnect targets preserve multiport identities',async()=>{
 const value=draft(),script=processing.processingTemplate('script','script',state());script.outputs.push({...plain(script.outputs[0]),id:'alternate',name:'Alternate'});value.document.processingNodes.push(script);value.document.resources.find(node=>node.id==='can-two').ports.push(port('receive','input'));
 const h=await harness(value,{fromNode:'script',toNode:'can-two'});h.handlers.connectBlocks('script','can-two',undefined,'alternate','receive');assert.equal(h.edits.length,1);const connected=plain(h.edits[0].next),binding=connected.document.bindings[0];assert.equal(binding.from.portId,'alternate');
 const gate=processing.processingTemplate('gate','gate',state());connected.document.processingNodes.push(gate);const retarget=await harness(connected,{fromNode:'script',fromPort:'alternate',toNode:'gate'});retarget.handlers.reconnectConnection(binding.id,'to','gate',{x:500,y:200},'value');assert.equal(retarget.edits.length,1);assert.deepEqual(plain(retarget.edits[0].next.document.bindings[0]),{id:binding.id,from:{nodeId:'script',portId:'alternate'},to:{nodeId:'gate',portId:'value'}});
 const reoutput=await harness(connected,{fromNode:'script',fromPort:'alternate',toNode:'can-two',toPort:'receive'});reoutput.handlers.reconnectConnection(binding.id,'from','script',undefined,'out');assert.equal(reoutput.edits.length,1);assert.equal(reoutput.edits[0].next.document.bindings[0].from.portId,'out');assert.equal(reoutput.edits[0].next.document.bindings[0].id,binding.id);
});

test('named draft ports reject the wrong Gate input without substituting a compatible choice',async()=>{
 const value=modules.get('./profile-transform-drafts').graphDropProcessing(draft(),'gate');
 const blocked=await harness(value,{fromNode:'can-one',toNode:'transform-1'});blocked.handlers.connectBlocks('can-one','transform-1',undefined,'button','gate');assert.deepEqual(blocked.edits,[]);assert.equal(blocked.handlers.state.routeOpen,true);assert.equal(blocked.handlers.state.toPort,'gate');assert.match(blocked.handlers.state.connectionError,/value input first/);
 const accepted=await harness(value,{fromNode:'can-one',toNode:'transform-1'});accepted.handlers.connectBlocks('can-one','transform-1',undefined,'button','value');assert.equal(accepted.edits.length,1);assert.equal(accepted.edits[0].next.document.processingNodes[0].draft,undefined);assert.equal(accepted.edits[0].next.document.bindings[0].to.portId,'value');
});

test('discovered ports render in the same overview row and selecting the complete pair auto-commits',async()=>{
  const{value,sourcePort,seed}=discoveredFixture(state(),state(),260),before=plain(value),h=await harness(value,{...seed,fromPort:'',toPort:'',routeOpen:true,draftRowOpen:true},{},{overview:true,overviewPair:{from:'source',to:'target'},portComboboxOpen:true});
  for(const label of ['Source','Destination','Find an output','Find an input','Add transform'])assert.ok(h.body.includes(label),label);
  assert.ok(h.body.includes('pending-connection'),h.body.slice(-1600));assert.ok(!h.body.includes('Choose connection ports'));assert.ok(!h.body.includes('class="canvas-connection-panel'));assert.doesNotMatch(h.body,/>Connect<\/button>|>Cancel<\/button>/);
  assert.equal(h.handlers.state.sourcePorts.length,260);assert.equal(h.handlers.state.targetPorts.length,1);assert.match(h.handlers.state.connectionError,/Choose the ports/);assert.equal(h.edits.length,0);
  h.handlers.choosePort('from',sourcePort.key);assert.equal(h.edits.length,1);assert.equal(h.edits[0].next.document.bindings.length,1);assert.deepEqual(plain(h.edits[0].next.document.resources.map(node=>node.ports.length)),[1,1]);assert.deepEqual(plain(value),before);
});

test('choosing ports, adding and configuring stages, cancel and Escape leave provider declarations untouched',async()=>{
  const{value,sourcePort,targetPort,seed}=discoveredFixture(state(integer(-100,100)),state()),before=plain(value),choosing=await harness(value,{...seed,fromPort:'',toPort:'',routeOpen:false,draftRowOpen:false});
  choosing.handlers.startConnectionDraft('source','target');assert.equal(choosing.handlers.state.draftRowOpen,true);choosing.handlers.choosePort('from',sourcePort.key);choosing.handlers.choosePort('to',targetPort.key);assert.equal(choosing.edits.length,0);choosing.handlers.discardConnectionDraft();assert.equal(choosing.handlers.state.draftRowOpen,false);assert.deepEqual(choosing.edits,[]);assert.deepEqual(plain(value),before);
  const h=await harness(value,{...seed,routeOpen:true,draftRowOpen:true});h.handlers.addConnectionTransform('hysteresis');assert.equal(h.handlers.state.queuedTransforms.length,1,h.handlers.state.message);const stage=h.handlers.state.queuedTransforms[0];assert.equal(stage.operation.kind,'hysteresis');const configured=plain(stage);configured.operation.press=30;configured.operation.release=-30;assert.equal(h.handlers.updateProcessing(configured),true);assert.equal(h.handlers.state.queuedTransforms[0].operation.press,30);assert.equal(h.edits.length,0);h.handlers.discardConnectionDraft();assert.equal(h.handlers.state.draftRowOpen,false);assert.equal(h.handlers.state.queuedTransforms.length,0);assert.deepEqual(h.edits,[]);assert.deepEqual(plain(value),before);
  const readOnly=await harness(value,{...seed,fromPort:'',toPort:'',routeOpen:true,draftRowOpen:true,autoCommitArmed:true},{readOnly:true});readOnly.handlers.choosePort('from',sourcePort.key);readOnly.handlers.choosePort('to',targetPort.key);readOnly.handlers.addConnectionTransform('compare');assert.deepEqual(readOnly.edits,[]);assert.equal(readOnly.handlers.state.fromPort,'');assert.equal(readOnly.handlers.state.toPort,'');assert.deepEqual(plain(value),before);
  const escape=await harness(value,{...seed,routeOpen:true,draftRowOpen:true});escape.handlers.addConnectionTransform('hysteresis');let prevented=0,stopped=0;escape.handlers.routeKeyboard({key:'Escape',preventDefault(){prevented++;},stopPropagation(){stopped++;}});assert.equal(prevented,1);assert.equal(stopped,1);assert.equal(escape.handlers.state.draftRowOpen,false);assert.deepEqual(plain(escape.handlers.state.queuedTransforms),[]);assert.deepEqual(escape.edits,[]);assert.deepEqual(plain(value),before);
});

test('a configured compatible transform chain auto-commits exact provider pins and every edge once',async()=>{
  const{value,sourcePort,targetPort,seed}=discoveredFixture(state(integer(-100,100)),state()),before=plain(value),h=await harness(value,{...seed,routeOpen:true,draftRowOpen:true});
  assert.match(h.handlers.state.connectionError,/Types/);h.handlers.addConnectionTransform('compare');assert.equal(h.handlers.state.queuedTransforms.length,1,h.handlers.state.message);h.handlers.addConnectionTransform('not');assert.equal(h.handlers.state.queuedTransforms.length,2,h.handlers.state.message);
  const compare=plain(h.handlers.state.queuedTransforms[0]);compare.operation.threshold=25;h.handlers.updateProcessing(compare);
  assert.equal(h.edits.length,0);h.handlers.choosePort('from',sourcePort.key);assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Connect through transformations');h.handlers.autoConnect();h.handlers.autoConnect();assert.equal(h.edits.length,1);
  const next=plain(h.edits[0].next);assert.equal(next.document.processingNodes.length,2);assert.equal(next.document.bindings.length,3);assert.deepEqual(next.document.processingNodes.map(node=>node.operation.kind),['compare','not']);assert.equal(next.document.processingNodes[0].operation.threshold,25);
  assert.deepEqual(next.document.resources.map(resource=>resource.ports.length),[1,1]);assert.deepEqual(next.assignments.map(assignment=>assignment.ports.map(pin=>pin.observedPortId)),[[sourcePort.observed.descriptor.id],[targetPort.observed.descriptor.id]]);
  for(const binding of next.document.bindings)assert.equal(graph.graphConnectionError(next.document,binding.from,binding.to,binding.id),'');assert.deepEqual(plain(value),before);
});

test('queued stage type changes stay local until an armed complete chain becomes valid',async()=>{
  const{value,seed}=discoveredFixture(state(integer(-100,100)),state()),h=await harness(value,{...seed,routeOpen:true,draftRowOpen:true});
  assert.match(h.handlers.state.connectionError,/Types/);h.handlers.addConnectionTransform('identity');assert.equal(h.handlers.state.queuedTransforms.length,1,h.handlers.state.message);
  h.handlers.changeConnectionTransform(0,'compare');assert.equal(h.handlers.state.queuedTransforms[0].operation.kind,'compare');assert.equal(h.edits.length,0);h.handlers.choosePort('from',seed.fromPort);assert.equal(h.edits.length,1);
  const readOnly=await harness(value,{...seed,routeOpen:true,draftRowOpen:true},{readOnly:true});readOnly.handlers.addConnectionTransform('compare');assert.equal(readOnly.handlers.state.queuedTransforms.length,0);assert.deepEqual(readOnly.edits,[]);
});
test('palette entry points add only the requested virtual or keyboard endpoint, preserve selection and remain read-only without effects',async()=>{
  const value=draft();let h=await harness(value,{}, {selectedId:'can-one'});h.handlers.addDesktop();assert.equal(h.edits.length,1);const next=plain(h.edits[0].next);assert.deepEqual(next.document.processingNodes,[]);assert.deepEqual(next.document.bindings,[]);assert.deepEqual(next.document.virtualOutputs,[]);assert.equal(next.document.desktopOutputs.length,1);assert.equal(next.document.desktopOutputs[0].id,'desktop-1');assert.equal(next.document.desktopOutputs[0].ports.length,1);assert.deepEqual(next.document.desktopOutputs[0].actions,[{portId:'action-1',action:{kind:'key-hold',key:44}}]);assert.deepEqual(next.document.desktopOutputs[0].scope,{kind:'selected-game'});assert.equal(h.handlers.state.selectedId,'can-one');assert.deepEqual(value.document.processingNodes,[]);
  h=await harness(next,{}, {selectedId:'third-party-a'});h.handlers.addDesktop();assert.equal(h.edits.length,0);assert.equal(h.handlers.state.selectedId,'third-party-a');h.handlers.addVirtual();assert.equal(h.edits.length,1);assert.equal(h.edits[0].next.document.virtualOutputs.length,1);assert.equal(h.edits[0].next.document.processingNodes.length,0);assert.equal(h.edits[0].next.document.bindings.length,0);assert.equal(h.handlers.state.selectedId,'third-party-a');
  h=await harness(value,{}, {readOnly:true});h.handlers.addVirtual();h.handlers.addDesktop();assert.deepEqual(h.edits,[]);assert.equal(h.handlers.state.selectedId,'');
});

test('dropping an output positions it in one undoable edit without moving implicit blocks, the viewport or selection',async()=>{
  const {canvasNodes}=modules.get('./profile-canvas'),{ProfileGraphHistory}=modules.get('./profile-graph-history');
  const value=draft();value.document.editor.viewport={x:-80,y:170,zoom:.65};value.document.editor.positions=[{nodeId:'third-party-a',x:-400,y:75}];
  const previous=plain(canvasNodes(value.document,value.assignments,[]));
  for(const [method,id,position] of [['addVirtual','virtual-1',{x:420.25,y:-260.5}],['addDesktop','desktop-1',{x:-730,y:860}]]){
    const h=await harness(value,{}, {selectedId:'can-one'});h.handlers[method](position);assert.equal(h.edits.length,1);assert.equal(h.handlers.state.selectedId,'can-one');
    const next=plain(h.edits[0].next),nodes=plain(canvasNodes(next.document,next.assignments,[]));assert.deepEqual(nodes.find(node=>node.id===id).position,{nodeId:id,...position});
    for(const original of previous){const actual=nodes.find(node=>node.id===original.id).position;assert.equal(actual.x,original.position.x);assert.equal(actual.y,original.position.y);}
    assert.deepEqual(next.document.editor.viewport,value.document.editor.viewport);assert.equal(next.document.bindings.length,0);
    const history=new ProfileGraphHistory(value);history.commit(next,h.edits[0].label);assert.deepEqual(plain(history.undo()),value);assert.equal(history.canUndo,false);assert.deepEqual(plain(history.redo()),next);
  }
});

test('canvas palette preview uses the live pan and zoom and commits that exact centered position without selecting or changing the graph',async()=>{
  const value=draft(),drops=[],edits=[];let ended=0,handlers;
  const drag={component:{kind:'virtual'},name:'Virtual controller',category:'Virtual controller'};
  const component=await components({captureCanvas:value=>handlers=value}),Canvas=await component('./ProfileCanvas.svelte');
  render(Canvas,{props:{...value,inventory:[],inventoryClient:{inspect:noRequest},selectedId:'can-one',paletteDrag:drag,canDropComponent:()=>true,dropComponent:(component,position)=>drops.push({component,position}),endPaletteDrag:()=>ended++,change:next=>edits.push(next)}}).body;
  const rect={left:120,top:180,right:1120,bottom:880};handlers.setStage({getBoundingClientRect:()=>rect,contains:node=>node?.inside===true});handlers.setView({x:-150,y:90,zoom:.5});
  const makeEvent=(clientX=720,clientY=560)=>({clientX,clientY,target:{closest:()=>null},dataTransfer:{dropEffect:'none'},prevented:0,stopped:0,preventDefault(){this.prevented++;},stopPropagation(){this.stopped++;}});
  const expectedPosition={x:1500-PROFILE_BLOCK_WIDTH/2,y:580-PROFILE_BLOCK_HEIGHT/2};
  const over=makeEvent();handlers.overPalette(over);assert.equal(over.prevented,1);assert.equal(over.dataTransfer.dropEffect,'copy');assert.deepEqual(plain(handlers.state.palettePreview),expectedPosition);assert.equal(handlers.state.selectedId,'can-one');assert.deepEqual(drops,[]);assert.deepEqual(edits,[]);
  handlers.leavePalette({relatedTarget:{inside:true}});assert.deepEqual(plain(handlers.state.palettePreview),expectedPosition);
  const drop=makeEvent();handlers.dropPalette(drop);assert.deepEqual(plain(drops),[{component:drag.component,position:expectedPosition}]);assert.equal(drop.prevented,1);assert.equal(drop.stopped,1);assert.equal(ended,1);assert.equal(handlers.state.palettePreview,null);assert.equal(handlers.state.selectedId,'can-one');assert.deepEqual(plain(handlers.state.view),{x:-150,y:90,zoom:.5});assert.deepEqual(edits,[]);
  handlers.overPalette(makeEvent());handlers.leavePalette({relatedTarget:null});assert.equal(handlers.state.palettePreview,null);assert.equal(drops.length,1);
  handlers.overPalette(makeEvent());handlers.clearPalettePreview();assert.equal(handlers.state.palettePreview,null);assert.equal(drops.length,1);
  handlers.setView({x:-1e6,y:1e6,zoom:.1});handlers.overPalette(makeEvent());assert.deepEqual(plain(handlers.state.palettePreview),{x:1e6,y:-1e6});
});

test('canvas ignores external, unavailable, read-only, inspector and outside palette drops without canceling the native event',async()=>{
  for(const props of [{paletteDrag:null},{canDropComponent:()=>false},{readOnly:true},{inspector:true},{outside:true}]){
    const edits=[],drops=[];let handlers,ended=0;const component=await components({captureCanvas:value=>handlers=value}),Canvas=await component('./ProfileCanvas.svelte');
    render(Canvas,{props:{...draft(),inventory:[],inventoryClient:{inspect:noRequest},paletteDrag:{component:{kind:'resource',sourceId:'native:device'},name:'Device',category:'Controller'},dropComponent:(...args)=>drops.push(args),endPaletteDrag:()=>ended++,change:next=>edits.push(next),...props}}).body;
    handlers.setStage({getBoundingClientRect:()=>({left:100,top:200,right:800,bottom:900}),contains:()=>false});const event={clientX:props.outside?99:400,clientY:400,target:{closest:()=>props.inspector?{}:null},dataTransfer:{types:['Files'],dropEffect:'none'},preventDefault:noRequest,stopPropagation:noRequest};
    handlers.overPalette(event);handlers.dropPalette(event);assert.equal(handlers.state.palettePreview,null);assert.deepEqual(edits,[]);assert.deepEqual(drops,[]);assert.equal(ended,0);assert.equal(event.dataTransfer.dropEffect,'none');
  }
});
test('plus inserts into the existing overview chain without opening a new connection row',async()=>{
  const initial=draft();initial.document.resources[1].ports[0].direction='input';const value=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'button'},{nodeId:'third-party-b',portId:'button'}),binding=value.document.bindings[0],before=plain(value),bundle=modules.get('./profile-signal-overview').signalOverview(value.document)[0],lane=bundle.lanes[0],h=await harness(value);
  h.handlers.openConnection(binding.id,lane.id);assert.equal(h.edits.length,1);assert.equal(h.handlers.state.draftRowOpen,false);assert.equal(h.handlers.state.editingRoute,'');assert.equal(h.handlers.state.routeOpen,false);
  const next=plain(h.edits[0].next),node=next.document.processingNodes[0],nextLane=modules.get('./profile-signal-overview').signalOverview(next.document).flatMap(bundle=>bundle.lanes).find(row=>row.transforms.includes(node.id));assert.equal(node.operation.kind,'identity');assert.equal(next.document.bindings.length,2);assert.equal(h.handlers.state.selectedTransformLane,nextLane.id);assert.ok(next.document.bindings.some(route=>route.id===binding.id&&route.from.nodeId===node.id&&route.to.nodeId===binding.to.nodeId));assert.ok(next.document.bindings.some(route=>route.from.nodeId===binding.from.nodeId&&route.to.nodeId===node.id));for(const route of next.document.bindings)assert.equal(graph.graphConnectionError(next.document,route.from,route.to,route.id),'');assert.deepEqual(plain(value),before);
  const{ProfileGraphHistory}=modules.get('./profile-graph-history'),history=new ProfileGraphHistory(value);history.commit(next,'Insert transformation');assert.deepEqual(plain(history.undo()),before);assert.deepEqual(plain(history.redo()),next);
  const readOnly=await harness(value,{}, {readOnly:true});readOnly.handlers.openConnection(binding.id,lane.id);assert.deepEqual(readOnly.edits,[]);
});
test('editing shared transforms in different overview rows creates fresh independent configurations',async()=>{
  const numeric=state(integer(-100,100)),value=draft();value.document.resources.find(row=>row.id==='third-party-a').ports=[port('number','output',numeric)];for(const id of ['third-party-b','can-two'])value.document.resources.find(row=>row.id===id).ports=[port('pressed','input',state())];
  const shared=processing.processingTemplate('hysteresis','shared-transform',numeric);value.document.processingNodes.push(shared);let graphValue=graph.graphConnect(value,{nodeId:'third-party-a',portId:'number'},{nodeId:shared.id,portId:'in'});graphValue=graph.graphConnect(graphValue,{nodeId:shared.id,portId:'out'},{nodeId:'third-party-b',portId:'pressed'});graphValue=graph.graphConnect(graphValue,{nodeId:shared.id,portId:'out'},{nodeId:'can-two',portId:'pressed'});
  const rows=modules.get('./profile-signal-overview').signalOverview(graphValue.document).flatMap(bundle=>bundle.lanes).filter(lane=>lane.transforms.includes(shared.id));assert.equal(rows.length,2);
  let first=await harness(graphValue);first.handlers.selectOverviewTransform(rows[0].id,shared.id);assert.equal(first.handlers.updateProcessing({...plain(shared),operation:{...plain(shared.operation),press:35,release:15}}),true);assert.equal(first.edits.length,1);
  const afterFirst=plain(first.edits[0].next),firstRows=modules.get('./profile-signal-overview').signalOverview(afterFirst.document).flatMap(bundle=>bundle.lanes),firstCopy=afterFirst.document.processingNodes.find(node=>node.id!==shared.id&&node.operation.kind==='hysteresis');assert.ok(firstCopy);assert.notEqual(firstCopy.id,shared.id);assert.equal(firstCopy.operation.press,35);assert.equal(afterFirst.document.processingNodes.find(node=>node.id===shared.id).operation.press,shared.operation.press);
  const remaining=firstRows.find(lane=>lane.transforms.includes(shared.id));assert.ok(remaining);let second=await harness(afterFirst);second.handlers.selectOverviewTransform(remaining.id,shared.id);const sharedAfter=afterFirst.document.processingNodes.find(node=>node.id===shared.id);assert.equal(second.handlers.updateProcessing({...plain(sharedAfter),operation:{...plain(sharedAfter.operation),press:85,release:65}}),true);assert.equal(second.edits.length,1);
  const final=plain(second.edits[0].next),lanes=modules.get('./profile-signal-overview').signalOverview(final.document).flatMap(bundle=>bundle.lanes),configured=lanes.map(lane=>final.document.processingNodes.find(node=>node.id===lane.transforms[0])).filter(Boolean);assert.equal(configured.length,2);assert.equal(new Set(configured.map(node=>node.id)).size,2);assert.ok(configured.some(node=>node.id!==shared.id));assert.deepEqual(plain(configured.map(node=>node.operation.press).sort((a,b)=>a-b)),[35,85]);assert.deepEqual(plain(configured.map(node=>node.operation.release).sort((a,b)=>a-b)),[15,65]);
});
test('multi-input Gate waits for its required control and action invocations have no scalar transform choices',async()=>{
  const initial=draft(),contract=state(integer(-100,100));initial.document.resources[0].ports=[port('number','output',contract)];initial.document.resources[1].ports=[port('number','input',contract)];const value=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'number'},{nodeId:'third-party-b',portId:'number'}),binding=value.document.bindings[0],h=await harness(value,{routeOpen:true,draftRowOpen:true,autoCommitArmed:true,editingRoute:binding.id,fromNode:binding.from.nodeId,fromPort:binding.from.portId,toNode:binding.to.nodeId,toPort:binding.to.portId});h.handlers.addConnectionTransform('gate');assert.equal(h.edits.length,0);assert.equal(h.handlers.state.queuedTransforms.length,1);assert.match(h.handlers.state.connectionError,/"gate" input/i);assert.equal(h.handlers.state.draftRowOpen,true);assert.deepEqual(plain(value.document.bindings),plain([binding]));
  const pack=processing.processingActionPack('pack',action('one-shot',[]),'action-pack'),invocations=graph.graphConnect(graph.graphSetProcessing(draft(),pack),{nodeId:pack.id,portId:pack.outputs[0].id},{nodeId:'dcs',portId:'command'});const blocked=await harness(invocations);blocked.handlers.editRoute(invocations.document.bindings[0].id);blocked.handlers.addConnectionTransform('identity');assert.deepEqual(plain(blocked.handlers.state.queuedTransforms.map(node=>node.operation.kind)),['identity']);assert.ok(blocked.handlers.state.connectionError);assert.deepEqual(blocked.edits,[]);
});
test('incompatible exact ports remain untouched until a compatible transform completes the chain',async()=>{
  const value=draft();value.document.resources[0].ports=[port('number','output',state(integer(-100,100)))];value.document.resources[1].ports=[port('pressed','input',state())];const before=plain(value),h=await harness(value,{fromNode:'third-party-a',fromPort:'number',toNode:'third-party-b',toPort:'pressed',routeOpen:true,draftRowOpen:true});
  assert.match(h.handlers.state.connectionError,/Types/);h.handlers.addConnectionTransform('hysteresis');assert.equal(h.edits.length,0);h.handlers.choosePort('from',h.handlers.state.sourcePorts[0].key);assert.equal(h.edits.length,1);assert.equal(h.edits[0].label,'Connect through transformations');const next=plain(h.edits[0].next),node=next.document.processingNodes[0];assert.equal(node.operation.kind,'hysteresis');assert.equal(next.document.bindings.length,2);for(const binding of next.document.bindings)assert.equal(graph.graphConnectionError(next.document,binding.from,binding.to,binding.id),'');assert.deepEqual(plain(value),before);
  const units=draft();units.document.resources[0].ports=[port('feet','output',state({type:'number',min:0,max:100000,unit:'ft'}))];units.document.resources[1].ports=[port('metres','input',state({type:'number',min:0,max:30480,unit:'m'}))];const conversion=await harness(units,{fromNode:'third-party-a',fromPort:'feet',toNode:'third-party-b',toPort:'metres',routeOpen:true,draftRowOpen:true,autoCommitArmed:true});conversion.handlers.addConnectionTransform('unit-convert');assert.equal(conversion.edits.length,1);assert.deepEqual(plain(conversion.edits[0].next.document.processingNodes[0].operation),{kind:'unit-convert',from:'ft',to:'m',rounding:'none'});assert.equal(conversion.edits[0].next.document.bindings.length,2);
});
test('ratio to percentage transform connects in one edit and preserves the existing speed route',async()=>{
  const value=draft(),speed={type:'number',min:0,max:2000,unit:'km/h'},ratio={type:'number',min:0,max:100000,unit:'ratio'},percentage={type:'number',min:0,max:100,unit:'%'};value.document.resources[0].ports=[port('speed','output',state(speed)),port('rev-ratio','output',state(ratio))];value.document.resources[1].ports=[port('speed','input',state(speed)),port('gauge','input',state(percentage))];
  const saved=graph.graphConnect(value,{nodeId:'third-party-a',portId:'speed'},{nodeId:'third-party-b',portId:'speed'}),speedBinding=plain(saved.document.bindings[0]),before=plain(saved),h=await harness(saved,{fromNode:'third-party-a',fromPort:'rev-ratio',toNode:'third-party-b',toPort:'gauge',routeOpen:true,draftRowOpen:true,autoCommitArmed:true});
  h.handlers.addConnectionTransform('ratio-to-percent');assert.equal(h.edits.length,1);const next=plain(h.edits[0].next),node=next.document.processingNodes[0];assert.deepEqual(node.operation,{kind:'unit-convert',from:'ratio',to:'%',rounding:'none',clamp:true});assert.deepEqual(node.outputs[0].contract.schema,percentage);assert.equal(next.document.bindings.length,3);assert.deepEqual(next.document.bindings.find(binding=>binding.id===speedBinding.id),speedBinding);for(const binding of next.document.bindings)assert.equal(graph.graphConnectionError(next.document,binding.from,binding.to,binding.id),'');assert.deepEqual(plain(saved),before);
});

test('canvas connections expose keyboard activation without entering pan or drag gestures',async()=>{
  const initial=draft();initial.document.resources[1].ports[0].direction='input';const value=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'button'},{nodeId:'third-party-b',portId:'button'}),binding=value.document.bindings[0],calls=[];let handlers;
  const component=await components({captureCanvas:value=>handlers=value}),Canvas=await component('./ProfileCanvas.svelte');const body=render(Canvas,{props:{...value,inventory:[],inventoryClient:{inspect:noRequest},selectConnection:id=>calls.push(id),change:noRequest}}).body;assert.match(body,/class="signal-edge-hit[^\"]*"[^>]*role="button"[^>]*tabindex="0"[^>]*aria-label="Select connection/);assert.ok(body.includes('a wire to manage its connection'));
  let prevented=0,stopped=0;for(const key of ['Enter',' '])handlers.connectionKey({key,preventDefault(){prevented++;},stopPropagation(){stopped++;}},binding.id);handlers.connectionKey({key:'Escape',preventDefault:noRequest,stopPropagation:noRequest},binding.id);assert.deepEqual(calls,[binding.id,binding.id]);assert.equal(prevented,2);assert.equal(stopped,2);
  handlers.begin({button:0,target:{closest:selector=>selector.startsWith('.signal-edge-hit')?{}:null},preventDefault:noRequest,pointerId:1});assert.deepEqual(calls,[binding.id,binding.id]);
});
test('the actual composer authors third-party A+B and CAN groups into separate exact virtual inputs',async()=>{
  let value=draft();let h=await harness(value);h.handlers.addVirtual();assert.equal(h.edits.length,1);value=plain(h.edits[0].next);const first=value.document.virtualOutputs[0].id;
  value=await connect(value,'third-party-a','button',first,'button-1');value=await connect(value,'third-party-b','button',first,'button-2');h=await harness(value);h.handlers.addVirtual();value=plain(h.edits[0].next);const second=value.document.virtualOutputs[1].id;
  value=await connect(value,'can-one','button',second,'button-1');value=await connect(value,'can-two','button',second,'button-2');assert.equal(value.document.bindings.length,4);assert.notEqual(first,second);assert.equal(value.document.bindings.filter(binding=>binding.to.nodeId===first).length,2);
  const body=renderRouting(value);for(const binding of value.document.bindings)assert.ok(body.includes(binding.id));assert.equal((body.match(/class="signal-edge(?: |")/g)||[]).length,4);assert.ok(!body.includes('Exact signal bindings'));
});
test('actual action composer keeps DCS command, telemetry conversion/hysteresis, companion setter and physical feedback as distinct explicit edges',async()=>{
  let value=draft();value.document.resources.find(resource=>resource.id==='dcs').ports[1].contract=state(integer(0,65535));
  assert.throws(()=>processing.processingTemplate('unit-convert','invalid',state(integer(0,65535))),/declared source unit/);let h;
  h=await harness(value,{fromNode:'can-one',fromPort:'event',toNode:'dcs',toPort:'command',routeOpen:true,draftRowOpen:true,autoCommitArmed:true});h.handlers.actionPack('action-pack');assert.equal(h.edits.length,1);value=plain(h.edits[0].next);assert.equal(value.document.bindings.length,2);const command=value.document.processingNodes.at(-1);
  value=graph.graphSetProcessing(value,processing.processingTemplate('declare-quantity',graph.graphFreshId(value.document,'transform'),value.document.resources.find(resource=>resource.id==='dcs').ports[1].contract));let declaration=plain(value.document.processingNodes.at(-1));declaration.operation.quantity='Fixture-defined altitude MSL';declaration.operation.unit='ft';declaration.outputs[0].contract.schema.unit='ft';h=await harness(value);h.handlers.updateProcessing(declaration);value=plain(h.edits[0].next);
  value=graph.graphSetProcessing(value,processing.processingTemplate('unit-convert',graph.graphFreshId(value.document,'transform'),declaration.outputs[0].contract));const units=value.document.processingNodes.at(-1);assert.equal(units.operation.from,'ft');assert.equal(units.operation.to,'m');
  value=graph.graphSetProcessing(value,processing.processingTemplate('hysteresis',graph.graphFreshId(value.document,'transform'),units.outputs[0].contract));const condition=value.document.processingNodes.at(-1);
  h=await harness(value,{fromNode:condition.id,fromPort:'out',toNode:'companion',toPort:'indicator',routeOpen:true,draftRowOpen:true,autoCommitArmed:true});h.handlers.actionPack('set-on-change');assert.equal(h.edits.length,1);value=plain(h.edits[0].next);const setter=value.document.processingNodes.at(-1);
  value=await connect(value,'dcs','altitude',declaration.id,'in');value=await connect(value,declaration.id,'out',units.id,'in');value=await connect(value,units.id,'out',condition.id,'in');value=await connect(value,setter.id,setter.outputs[0].id,'physical-usb','led');
  assert.equal(value.document.processingNodes.length,5);assert.equal(value.document.bindings.length,8);assert.equal(value.document.resources.length,7);const body=renderRouting(value,{selectedId:condition.id});assert.ok(body.includes('Condition with hysteresis'));assert.ok(body.includes('Press'));assert.ok(body.includes('Release'));assert.ok(!body.includes('aria-label="Visible transformation"'));assert.ok(body.includes('Fixture-defined altitude MSL'));for(const binding of value.document.bindings)assert.equal(graph.graphConnectionError(value.document,binding.from,binding.to,binding.id),'');
});
test('exact provider selection adds one logical declaration with an exact local hash and never serializes a whole catalogue',async()=>{
  const{value,seed}=discoveredFixture(state(),state()),nativeId=`native:${'x'.repeat(100)}`,details=seed.discoveries['native:source'].details,observed=details.ports[0];observed.descriptor.id=nativeId;
  const choice=modules.get('./profile-discovery').profileDiscoveredPorts(value,'source',details)[0],target=seed.discoveries['native:target'].details.ports[0],targetChoice=modules.get('./profile-discovery').profileDiscoveredPorts(value,'target',seed.discoveries['native:target'].details)[0],before=plain(value),h=await harness(value,{...seed,fromNode:'',toNode:'target',fromPort:'',toPort:targetChoice.key,routeOpen:true,draftRowOpen:true});
  h.handlers.selectPort('source',observed);assert.equal(h.edits.length,1);assert.deepEqual(plain(value),before);assert.deepEqual(value.document.resources.map(node=>node.ports),[[],[]]);assert.deepEqual(value.assignments.map(assignment=>assignment.ports),[[],[]]);
  const next=plain(h.edits[0].next);assert.deepEqual(next.document.resources.map(node=>node.ports.length),[1,1]);assert.equal(next.assignments[0].ports[0].observedPortId,nativeId);assert.equal(next.assignments[1].ports[0].observedPortId,target.descriptor.id);assert.ok(!JSON.stringify(next.document).includes(nativeId));assert.deepEqual(plain(value),before);
});

test('physical controller settings contain only label, required, visibility, removal and simple live inputs',async()=>{
  const value=draft(),hash=`sha256:${'1'.repeat(64)}`,physical='22222222-2222-4222-8222-222222222222';value.assignments=[{resourceId:'third-party-a',sourceId:'native:controller',ports:[]}];value.document.resources[0].ports=[];
  const observed={id:'native:controller',name:'Controller',resourceType:'host-controller',availability:'online',generation:'1',capabilityRevision:hash,portCount:2,cachedPortCount:2,connections:[{scope:'host-controllers',key:physical,route:{id:'hid:'+physical,transport:'hid',providerGeneration:'1',attributes:{},baselineReady:true,writable:false,priority:1}}],attributes:{},diagnostics:[]},props={selectedId:'third-party-a',inventory:[observed],learningClient:{start:noRequest,status:noRequest,cancel:noRequest},runtimeIdle:true,editorContext:'exact-editor/1'};
  const h=await harness(value,{},props),inspector=h.body.match(/<aside[^>]*aria-label="Selected block inspector"[^]*?<\/aside>/)[0];
  for(const label of ['Label','Required','Visibility','Remove','aria-label="Live inputs"'])assert.ok(inspector.includes(label),label);
  for(const label of ['Selected graph ports','Provider catalogue','Connection metadata','Resource assignment and policy','Controller publication policy','Inventory resource','Learn physical input','Remove block and its routes','Optional resource'])assert.ok(!inspector.includes(label),label);
  assert.equal((inspector.match(/<input\b/g)||[]).length,2);assert.equal((inspector.match(/<select\b/g)||[]).length,1);assert.equal((inspector.match(/<button\b/g)||[]).length,1,'Remove is the only action.');assert.ok(!inspector.includes('Close component settings'));
  const node=modules.get('./profile-canvas').canvasNodes(value.document,value.assignments,[observed])[0];
  h.handlers.resourcePolicy(node,'name','Stick');assert.equal(h.edits[0].next.document.resources[0].name,'Stick');h.handlers.resourcePolicy(node,'required',true);assert.equal(h.edits[1].next.document.resources[0].required,true);
  h.handlers.removeNode(node.id);assert.ok(!h.edits[2].next.document.resources.some(resource=>resource.id===node.id));assert.ok(!h.edits[2].next.assignments.length);
  const peripheral=renderRouting(value,{...props,inventory:[{...observed,resourceType:'osg-module'}]});assert.ok(peripheral.includes('Provider catalogue'));assert.ok(peripheral.includes('Controller publication policy'));assert.ok(!peripheral.includes('aria-label="Live inputs"'));
  const offline=renderRouting(value,{...props,inventory:[{...observed,availability:'offline'}]});assert.ok(offline.includes('Controller is offline.'));assert.ok(!offline.includes('Connection metadata'));
  const missing=renderRouting(value,{...props,inventory:[]});assert.ok(missing.includes('Inventory resource'));assert.ok(!missing.includes('Live inputs'));assert.deepEqual(value.document.resources[0].ports,[]);assert.deepEqual(value.assignments[0].ports,[]);assert.deepEqual(plain(h.edits[0].next.assignments),value.assignments);
});
test('typed processing/script and virtual layout forms expose real graph contracts instead of opaque action JSON',()=>{
  const script=processing.processingTemplate('script','typed-script',state(integer()));script.operation.parameters={enabled:true,amount:3,label:'Typed'};const scriptHtml=render(Processing,{props:{node:script,change:noRequest}}).body;for(const label of ['Typed Rhai source','Typed script parameters','Declared ports','Logical port ID','Freshness (ms)','Fallback','Update visible node','ctx.inputs'])assert.ok(scriptHtml.includes(label),label);assert.ok(!scriptHtml.includes('Declared source inputs JSON'));
  const packed=processing.processingActionPack('tuple',action('one-shot',[{id:'trigger',name:'Trigger parameter',schema:bool},{id:'brightness',name:'Brightness',schema:{type:'number',min:0,max:100,unit:null}}]),'action-pack');const tupleHtml=render(Processing,{props:{node:packed,change:noRequest}}).body;assert.ok(tupleHtml.includes('separate event input'));assert.ok(tupleHtml.includes('Trigger parameter'));assert.ok(tupleHtml.includes('Brightness'));assert.ok(tupleHtml.includes('On explicit event'));
  const output=graph.graphSetVirtual(draft(),{id:'virtual',name:'Layout',required:false,layout:{buttons:['stable-fire'],axes:[{portId:'stable-steer',range:'bipolar',fallback:0}],hats:['stable-view']}}).document.virtualOutputs[0];const virtualHtml=render(Virtual,{props:{output,change:noRequest}}).body;for(const label of ['stable-steer','Stable logical control IDs','−32768','32767','Neutral fallback','stable-fire','stable-view','Update controller layout'])assert.ok(virtualHtml.includes(label),label);
});
test('the quantity editor updates the declared output unit atomically and persists the visible author interpretation without changing the raw source',async()=>{
  const profiles=modules.get('./profiles'),node=processing.processingTemplate('declare-quantity','raw-altitude',state(integer(0,65535))),original=plain(node),edits=[];let handlers;
  const component=await components({captureProcessing:value=>handlers=value}),Editor=await component('./ProfileProcessingEditor.svelte');
  const body=render(Editor,{props:{node,change:next=>edits.push(next)}}).body;
  for(const label of ['Quantity definition','Declared unit','altitude reference','128 UTF-8 bytes','Source quality and provenance','value unchanged'])assert.ok(body.includes(label),label);
  assert.ok(!body.includes('Add input port'));assert.ok(!body.includes('Remove output'));assert.ok(!body.includes('Unit (empty means none)'));
  handlers.field('quantity','Provider-reported altitude MSL');handlers.field('unit','ft');
  assert.equal(handlers.draft.operation.unit,'ft');assert.equal(handlers.draft.outputs[0].contract.schema.unit,'ft');assert.equal(handlers.draft.inputs[0].contract.schema.unit,null);assert.deepEqual(plain(node),original);
  handlers.save();assert.equal(edits.length,1);assert.equal(processing.processingNodeError(edits[0]),'');assert.deepEqual(plain(edits[0].inputs),original.inputs);assert.equal(edits[0].outputs[0].contract.schema.max,65535);
  const value=graph.graphSetProcessing(draft(),edits[0]),reopened=profiles.parseProfileDocument(profiles.exportProfileDocument(value.document));assert.equal(reopened.processingNodes[0].operation.quantity,'Provider-reported altitude MSL');assert.equal(reopened.processingNodes[0].outputs[0].contract.schema.unit,'ft');
  const html=renderRouting({...value,document:reopened},{selectedId:node.id});assert.ok(html.includes('Provider-reported altitude MSL'));assert.ok(html.includes('declared ft'));assert.equal(value.document.bindings.length,0);
  handlers.field('unit',' m');assert.equal(handlers.draft.operation.unit,' m');assert.equal(handlers.draft.outputs[0].contract.schema.unit,' m');handlers.save();assert.equal(edits.length,1);
  handlers.field('unit','m');handlers.save();assert.equal(edits.length,2);assert.equal(edits[1].outputs[0].contract.schema.unit,'m');assert.equal(edits[0].outputs[0].contract.schema.unit,'ft');
});

test('editing built-in timing updates the visible canvas and inspector summary without creating a route or effect',async()=>{
  const node=processing.processingTemplate('pulse','visible-pulse');let value=graph.graphSetProcessing(draft(),node);
  const initial=renderRouting(value,{selectedId:node.id});assert.ok(initial.includes('duration 100 ms'));assert.ok(initial.includes('no repeat'));
  const h=await harness(value,{}, {selectedId:node.id}),changed=plain(node);changed.operation.rule.durationUs=125000;changed.operation.rule.repeatUs=500000;h.handlers.updateProcessing(changed);
  assert.equal(h.edits.length,1);value=plain(h.edits[0].next);assert.equal(value.document.bindings.length,0);assert.equal(value.document.revision,1);
  const reopened=modules.get('./profiles').parseProfileDocument(modules.get('./profiles').exportProfileDocument(value.document));
  const body=renderRouting({...value,document:reopened},{selectedId:node.id});assert.ok(body.includes('duration 125 ms'));assert.ok(body.includes('repeat 500 ms'));assert.ok(!body.includes('duration 100 ms'));assert.ok(!body.includes('Declared ports'));
});

test('quick multiply factor restores rejected values and applies compatible output bounds in one visible graph edit',async()=>{
  let value=draft();const node=processing.processingTemplate('scale','quick-scale',state({type:'number',min:0,max:100,unit:'rpm'}));node.operation.factor=10;
  const typed=modules.get('./profile-transform-drafts').processingDerivedNode(node);value=graph.graphSetProcessing(value,typed);
  value.document.resources.find(row=>row.id==='can-two').ports.push(port('rpm-input','input',state({type:'number',min:0,max:1000,unit:'rpm'})));
  value=graph.graphConnect(value,{nodeId:typed.id,portId:'out'},{nodeId:'can-two',portId:'rpm-input'});const original=plain(value),h=await harness(value,{}, {selectedId:typed.id});
  assert.ok(h.body.includes('aria-label="Quick multiply factor"'));
  const rejected={value:'20'};h.handlers.quickFactor(typed,rejected);assert.equal(rejected.value,'10');assert.deepEqual(h.edits,[]);assert.match(h.handlers.state.message,/ranges differ/);assert.deepEqual(plain(value),original);
  const accepted={value:'5'};h.handlers.quickFactor(typed,accepted);assert.equal(accepted.value,'5');assert.equal(h.edits.length,1);assert.equal(h.edits[0].next.document.processingNodes[0].operation.factor,5);assert.equal(h.edits[0].next.document.processingNodes[0].outputs[0].contract.schema.max,500);assert.equal(h.edits[0].next.document.bindings[0].id,value.document.bindings[0].id);
  const readOnly=await harness(value,{}, {readOnly:true}),blocked={value:'5'};readOnly.handlers.quickFactor(typed,blocked);assert.equal(blocked.value,'10');assert.deepEqual(readOnly.edits,[]);
});
test('Debug defaults to telemetry and retains working profile inspection and runtime panels behind their own tabs',async()=>{
  const {ProfileRuntimeController}=(await loadModules(['./profile-runtime-controller'])).get('./profile-runtime-controller');
  for(const section of ['telemetry','profile','runtime']){
    const runtime=new ProfileRuntimeController({status:noRequest}),h=await debugHarness(draft(),section==='telemetry'?{}:{section},{runtime});
    const panels=[...h.body.matchAll(/<div\b[^>]*role="tabpanel"[^>]*>/g)].map(([tag])=>tag);
    assert.equal(panels.length,3);assert.equal(panels.filter(tag=>! /\shidden(?:[\s=>])/.test(tag)).length,1);
    for(const id of ['telemetry','profile','runtime'])assert.equal(/\shidden(?:[\s=>])/.test(panels.find(tag=>tag.includes(`-${id}-panel`))),id!==section);
    for(const label of ['Incoming telemetry','Profile inspection','Runtime &amp; recovery','Native graph preflight','Applied port samples','Runtime profile status','Native runtime and desktop output details','Companion loading and image cache details'])assert.ok(h.body.includes(label),label);
    assert.ok(h.body.includes('aria-label="Incoming game telemetry"'));runtime.dispose();
  }
});

test('standalone Debug retains all diagnostic sections without a profile-owned Back button',async()=>{
  const h=await debugHarness(draft(),{},{editor:null,close:undefined,locate:undefined});
  assert.match(h.body.replace(/<!--[\s\S]*?-->/g,''),/<h1\b[^>]*>Debug<\/h1>/);assert.ok(h.body.includes('aria-label="Debug sections"'));
  for(const label of ['Incoming telemetry','Profile inspection','Runtime &amp; recovery'])assert.ok(h.body.includes(label),label);
  assert.ok(!h.body.includes('Back to Profiles'));assert.ok(!h.body.includes('Current editor draft'));
});

test('Profile debug preflight captures the unsaved graph and pages diagnostics by actual returned counts without saving or effects',async()=>{
  const value=draft(),calls=[],report={schemaVersion:1,compilationId:'22222222-2222-4222-8222-222222222222',profileId:value.document.id,profileRevision:1,programHash:null,documentSemanticHash:null,captureFingerprint:`sha256:${'1'.repeat(64)}`,readiness:'blocked',diagnostics:[],sectionTotals:{diagnostics:0,nodes:0,order:0,sources:0,sinks:0,branches:0,controllers:0,visibility:0},budgets:null};
  const diagnostics=Array.from({length:7},(_,index)=>({code:`problem-${index}`,severity:'warning',message:`Diagnostic ${index}`,location:{resourceId:'third-party-a',nodeId:null,portId:'button',edgeIds:[]}}));report.diagnostics=diagnostics.slice(0,2);report.sectionTotals.diagnostics=7;
  const pages=[],compiler={async compile(document,assignments){calls.push({document,assignments});return report;},async inspect(captured,section,offset,limit){assert.equal(captured.compilationId,report.compilationId);assert.equal(section,'diagnostics');assert.equal(limit,32);pages.push(offset);return{offset,items:diagnostics.slice(offset,offset+(offset===2?3:2))};}};
  const h=await debugHarness(value,{}, {compiler});assert.ok(h.body.includes('Profile debug'));assert.ok(h.body.includes('without saving or starting it'));assert.deepEqual(calls,[]);await h.handlers.compile();assert.equal(calls.length,1);assert.deepEqual(plain(calls[0]),value);assert.equal(h.handlers.state.preview.readiness,'blocked');
  await h.handlers.page('next');assert.equal(h.handlers.state.offset,2);assert.equal(h.handlers.state.diagnostics.length,3);await h.handlers.page('next');assert.equal(h.handlers.state.offset,5);assert.equal(h.handlers.state.diagnostics.length,2);await h.handlers.page('previous');assert.equal(h.handlers.state.offset,2);await h.handlers.page('previous');assert.equal(h.handlers.state.offset,0);assert.deepEqual(pages,[2,5,2,0]);assert.equal(calls.length,1);
});
test('Profile debug labels stale draft or inventory captures and disables current-block navigation until refreshed',async()=>{
  const initial=draft();initial.document.resources[1].ports[0].direction='input';const value=graph.graphConnect(initial,{nodeId:'third-party-a',portId:'button'},{nodeId:'third-party-b',portId:'button'}),hash=`sha256:${'1'.repeat(64)}`;
  const inventory=[{id:'native:source',name:'Controller',resourceType:'host-controller',availability:'online',generation:'1',capabilityRevision:hash,portCount:1,cachedPortCount:1,connections:[],attributes:{},diagnostics:[]}],capturedEnvironment=`0/${graph.graphInventorySignature(inventory)}`;
  const diagnostic={code:'waitingForBaseline',severity:'warning',message:'Fixture provider awaits a fresh baseline.',location:{resourceId:'third-party-a',nodeId:null,portId:'button',edgeIds:[value.document.bindings[0].id]}},report={schemaVersion:1,compilationId:'22222222-2222-4222-8222-222222222222',profileId:value.document.id,profileRevision:1,programHash:hash,documentSemanticHash:hash,captureFingerprint:hash,readiness:'ready',diagnostics:[diagnostic],sectionTotals:{diagnostics:1,nodes:0,order:0,sources:0,sinks:0,branches:0,controllers:0,visibility:0},budgets:null};
  const seed={preview:report,signature:JSON.stringify(value),environment:capturedEnvironment,diagnostics:[diagnostic]},props={inventory,selectedId:'third-party-a'};
  const current=await debugHarness(value,seed,props);assert.equal(current.handlers.state.current,true);assert.ok(current.body.includes(diagnostic.code));assert.ok(current.body.includes(diagnostic.message));assert.ok(current.body.includes('Show block in editor'));assert.ok(!current.body.includes('Profile or inventory changed'));
  const offline=plain(inventory);offline[0].availability='offline';offline[0].generation='2';
  for(const staleProps of [{...props,inventory:offline},{...props,inventory:[...inventory,{...inventory[0],id:'unrelated'}]},{...props,inventoryRevision:1}]){const stale=await debugHarness(value,seed,staleProps);assert.equal(stale.handlers.state.current,false);assert.ok(stale.body.includes('Profile or inventory changed; refresh preflight'));assert.ok(!stale.body.includes('Show block in editor'));assert.ok(stale.body.includes(diagnostic.message),'captured diagnostics remain visibly stale');}
  const edited=plain(value);edited.document.name='Unsaved new name';const staleDraft=await debugHarness(edited,seed,props);assert.equal(staleDraft.handlers.state.current,false);assert.ok(staleDraft.body.includes('Profile or inventory changed; refresh preflight'));assert.ok(!staleDraft.body.includes('Show block in editor'));
});

test('deleting the last conversion removes its saved row and leaves a clean editable draft',async()=>{
 const {value,seed}=discoveredFixture(state(integer(0,4095)),state()),sourcePort=modules.get('./profile-discovery').profileDiscoveredPorts(value,'source',seed.discoveries['native:source'].details)[0],targetPort=modules.get('./profile-discovery').profileDiscoveredPorts(value,'target',seed.discoveries['native:target'].details)[0];
 const creating=await harness(value,seed);creating.handlers.connectBlocks('source','target',undefined,sourcePort.key,targetPort.key);creating.handlers.addConnectionTransform('compare');assert.equal(creating.edits.length,1);
 const complete=plain(creating.edits[0].next),lane=modules.get('./profile-signal-overview').signalOverview(complete.document).flatMap(bundle=>bundle.lanes)[0],h=await harness(complete,seed);
 h.handlers.removeOverviewTransform(complete.document.processingNodes[0].id,lane.id);assert.equal(h.edits.length,1);
 const next=plain(h.edits[0].next);assert.equal(next.document.processingNodes.length,0);assert.equal(next.document.bindings.length,0);assert.equal(next.document.resources[0].ports.length,0);assert.equal(next.assignments[0].ports.length,0);
 assert.equal(h.handlers.state.draftRowOpen,true);assert.equal(h.handlers.state.queuedTransforms.length,0);assert.equal(h.handlers.state.editingRoute,'');assert.equal(complete.document.processingNodes.length,1);
});

test('overview transform type changes retain incomplete rows and can be repaired in place',async()=>{
 const {value,seed}=discoveredFixture(state(),state(),2),discovered=modules.get('./profile-discovery'),sourcePorts=discovered.profileDiscoveredPorts(value,'source',seed.discoveries['native:source'].details),sourcePort=sourcePorts.find(port=>port.observed?.descriptor.id==='output:0'),targetPort=discovered.profileDiscoveredPorts(value,'target',seed.discoveries['native:target'].details)[0],identity=processing.processingTemplate('identity','overview-identity',sourcePort.contract),unrelated=processing.processingTemplate('identity','unrelated-dirty-node',state()),endpoint=(nodeId,port)=>({nodeId,port});unrelated.name='Unrelated dirty edit';const withUnrelated=graph.graphSetProcessing(value,unrelated);
 const connected=modules.get('./profile-connection-draft').commitConnectionDraft(withUnrelated,endpoint('source',sourcePort),endpoint('target',targetPort),[identity]);
 const lane=modules.get('./profile-signal-overview').signalOverview(connected.document)[0].lanes[0],h=await harness(connected,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'},activeTransform:identity.id});
 assert.match(h.body,/Boolean OR/);assert.match(h.body,/Boolean AND/);
 assert.equal(h.handlers.changeOverviewTransform(identity.id,'any'),true);assert.equal(h.edits.length,1);
 let next=plain(h.edits[0].next);assert.ok(!next.document.bindings.some(binding=>lane.bindingIds.includes(binding.id)),'The incomplete route must not retain authored bindings.');assert.ok(!next.document.processingNodes.some(node=>node.id===identity.id),'The selected OR stage stays in the local pending row until its second input is connected.');assert.ok(next.document.processingNodes.some(node=>node.id===unrelated.id&&node.name==='Unrelated dirty edit'));
 assert.equal(h.handlers.state.incompleteRows.length,1);assert.equal(h.handlers.state.incompleteRows[0].lane.id,lane.id);assert.equal(h.handlers.state.incompleteRows[0].transforms[0].operation.kind,'any');assert.match(h.handlers.state.incompleteRows[0].error,/second/);
 const repair=await harness(next,{incompleteRows:plain(h.handlers.state.incompleteRows),...seed});repair.handlers.activateIncomplete(lane.id);repair.handlers.changeConnectionTransform(0,'identity');assert.equal(repair.edits.length,1);
 next=plain(repair.edits[0].next);assert.ok(next.document.processingNodes.some(node=>node.id===identity.id&&node.operation.kind==='identity'));assert.ok(next.document.processingNodes.some(node=>node.id===unrelated.id&&node.name==='Unrelated dirty edit'));
 assert.equal(next.document.bindings.length,2);assert.equal(repair.handlers.state.incompleteRows.length,0);assert.deepEqual(plain(next.document.bindings.map(binding=>binding.id)),plain(connected.document.bindings.map(binding=>binding.id)));assert.deepEqual(plain(next.assignments),plain(connected.assignments));
});

test('numeric overview type changes expose every preset and keep incompatible results as unsaved rows',async()=>{
 const numeric=state(integer(-100,100)),{value,sourcePort,targetPort,seed}=discoveredFixture(numeric,state()),compare=processing.processingTemplate('compare','overview-compare',numeric,state()),endpoint=(nodeId,port)=>({nodeId,port});
 const connected=modules.get('./profile-connection-draft').commitConnectionDraft(value,endpoint('source',sourcePort),endpoint('target',targetPort),[compare]);
 const h=await harness(connected,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'},activeTransform:compare.id}),before=plain(connected);
 for(const [preset,label] of [['identity','Identity'],['scale','Scale and offset'],['not','Boolean NOT'],['any','Boolean OR'],['all','Boolean AND']])assert.ok(h.body.includes(`<option value="${preset}">${label}</option>`),`Expected ${label} in the saved transform type selector.`);
 assert.equal(h.handlers.changeOverviewTransform(compare.id,'scale'),true);assert.equal(h.edits.length,1);
 const next=plain(h.edits[0].next);assert.ok(!next.document.processingNodes.some(node=>node.id===compare.id));assert.ok(!next.document.bindings.some(binding=>modules.get('./profile-signal-overview').signalOverview(before.document)[0].lanes[0].bindingIds.includes(binding.id)));assert.equal(next.assignments.reduce((count,assignment)=>count+assignment.ports.length,0),0);
 assert.equal(h.handlers.state.incompleteRows.length,1);assert.equal(h.handlers.state.incompleteRows[0].transforms[0].operation.kind,'scale');assert.match(h.handlers.state.incompleteRows[0].error,/Types, units or declared ranges differ/);assert.equal(before.document.processingNodes[0].operation.kind,'compare');
});
test('a pending Boolean OR row can select its second source and then commit a valid graph',async()=>{
 const {value,seed}=discoveredFixture(state(),state(),2),discovered=modules.get('./profile-discovery'),sourcePorts=discovered.profileDiscoveredPorts(value,'source',seed.discoveries['native:source'].details),first=sourcePorts.find(port=>port.observed?.descriptor.id==='output:0'),second=sourcePorts.find(port=>port.observed?.descriptor.id==='output:1'),target=discovered.profileDiscoveredPorts(value,'target',seed.discoveries['native:target'].details)[0],identity=processing.processingTemplate('identity','or-row',first.contract),endpoint=(nodeId,port)=>({nodeId,port});
 const connected=modules.get('./profile-connection-draft').commitConnectionDraft(value,endpoint('source',first),endpoint('target',target),[identity]),lane=modules.get('./profile-signal-overview').signalOverview(connected.document)[0].lanes[0],h=await harness(connected,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'},activeTransform:identity.id});
 h.handlers.changeOverviewTransform(identity.id,'any');const row=plain(h.handlers.state.incompleteRows[0]),orNode=row.transforms[0],pending=plain(h.edits[0].next),editor=await harness(pending,{...seed,incompleteRows:[row],routeOpen:true,draftRowOpen:true,autoCommitArmed:true});editor.handlers.activateIncomplete(row.lane.id);const slot=editor.handlers.state.queuedInputSpecs.find(input=>input.nodeId===orNode.id);
 assert.equal(row.lane.id,modules.get('./profile-signal-overview').signalOverview(connected.document)[0].lanes[0].id);assert.equal(slot.port.id,orNode.inputs[1].id);
 const secondChoice=JSON.stringify(['source',second.key]);assert.ok(editor.handlers.state.inputOptions.some(option=>option.value===secondChoice));editor.handlers.chooseTransformInput(orNode.id,slot.port.id,secondChoice);editor.handlers.autoConnect();
 assert.equal(editor.edits.length,1,JSON.stringify({message:editor.handlers.state.message,error:editor.handlers.state.connectionError,queued:editor.handlers.state.queuedInputs}));const saved=plain(editor.edits[0].next);assert.ok(saved.document.processingNodes.some(node=>node.id===identity.id&&node.operation.kind==='any'));assert.equal(editor.handlers.state.incompleteRows.length,0);
 assert.deepEqual(saved.assignments.map(assignment=>assignment.ports.map(pin=>pin.observedPortId)),[['output:0','output:1'],['input:0']]);for(const binding of saved.document.bindings)assert.equal(graph.graphConnectionError(saved.document,binding.from,binding.to,binding.id),'');assert.equal(editor.edits[0].next.assignments.reduce((count,assignment)=>count+assignment.ports.length,0),3);
 const andEditor=await harness(saved,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'},activeTransform:identity.id});andEditor.handlers.changeOverviewTransform(identity.id,'all');assert.equal(andEditor.edits.length,1);const all=plain(andEditor.edits[0].next);assert.equal(all.document.processingNodes.find(node=>node.id===identity.id).operation.kind,'all');assert.deepEqual(plain(all.document.bindings.map(binding=>binding.id)),plain(saved.document.bindings.map(binding=>binding.id)));assert.deepEqual(plain(all.assignments),plain(saved.assignments));for(const binding of all.document.bindings)assert.equal(graph.graphConnectionError(all.document,binding.from,binding.to,binding.id),'');
 const orLane=modules.get('./profile-signal-overview').signalOverview(saved.document)[0].lanes[0],add=await harness(saved,{...seed,routeOpen:true,draftRowOpen:true,autoCommitArmed:true},{},{overview:true,overviewPair:{from:'source',to:'target'}});add.handlers.addTransformInput(orLane.id,identity.id);assert.equal(add.edits.length,1);const extendedRow=plain(add.handlers.state.incompleteRows[0]),extendedNode=extendedRow.transforms[0];assert.equal(extendedRow.lane.id,orLane.id);assert.equal(extendedNode.inputs.length,3);assert.equal(extendedNode.inputs[2].id,'input-3');assert.equal(extendedRow.inputs[`${identity.id}/${extendedNode.inputs[1].id}`].port.observed.descriptor.id,'output:1');assert.equal(extendedRow.inputs[`${identity.id}/input-3`],undefined);
 const extended=plain(add.edits[0].next),editor3=await harness(extended,{...seed,incompleteRows:[extendedRow],routeOpen:true,draftRowOpen:true,autoCommitArmed:true});editor3.handlers.activateIncomplete(orLane.id);const slot3=editor3.handlers.state.queuedInputSpecs.find(input=>input.nodeId===identity.id&&input.port.id==='input-3');assert.ok(slot3);const thirdChoice=JSON.stringify(['source',first.key]);assert.ok(editor3.handlers.state.inputOptions.some(option=>option.value===thirdChoice));editor3.handlers.chooseTransformInput(identity.id,'input-3',thirdChoice);editor3.handlers.autoConnect();assert.equal(editor3.edits.length,1,JSON.stringify({message:editor3.handlers.state.message,error:editor3.handlers.state.connectionError}));const three=plain(editor3.edits[0].next);assert.equal(three.document.processingNodes.find(node=>node.id===identity.id).inputs.length,3);assert.deepEqual(three.assignments.map(assignment=>assignment.ports.map(pin=>pin.observedPortId)),[['output:0','output:1'],['input:0']]);assert.equal(three.document.bindings.filter(binding=>binding.to.nodeId===identity.id).length,3);for(const binding of three.document.bindings)assert.equal(graph.graphConnectionError(three.document,binding.from,binding.to,binding.id),'');
});
async function committedThreeInputOr(inputCount=3){
 const {value,seed}=discoveredFixture(state(),state(),3),discovered=modules.get('./profile-discovery'),outputs=discovered.profileDiscoveredPorts(value,'source',seed.discoveries['native:source'].details),target=discovered.profileDiscoveredPorts(value,'target',seed.discoveries['native:target'].details)[0],identity=processing.processingTemplate('identity','or-removal',outputs[0].contract),endpoint=(nodeId,port)=>({nodeId,port});let or=processing.processingTemplate('any',identity.id,outputs[0].contract);
 while(or.inputs.length<inputCount)or=modules.get('./profile-chain-edit').addChainInput(or);const extraInputs=Object.fromEntries(or.inputs.slice(1).map((input,index)=>[`${or.id}/${input.id}`,endpoint('source',outputs[index+1])]));
 const connected=modules.get('./profile-connection-draft').commitConnectionDraft(value,endpoint('source',outputs[0]),endpoint('target',target),[or],undefined,false,'',extraInputs);
 const lane=modules.get('./profile-signal-overview').signalOverview(connected.document)[0].lanes[0];
 return{connected,seed,or,lane};
}

test('removing a committed third Boolean OR input preserves the route and releases only its unused provider pin',async()=>{
 const {connected,seed,or,lane}=await committedThreeInputOr(),h=await harness(connected,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'}});
 assert.ok(h.body.includes(`data-input-key="${or.id}/${or.inputs[2].id}"`));assert.ok(h.body.includes(`aria-label="Remove ${or.inputs[2].name} input"`));assert.equal(h.handlers.removeTransformInput(lane.id,or.id,or.inputs[2].id),true);assert.equal(h.edits.length,1);
  const saved=plain(h.edits[0].next),savedOr=saved.document.processingNodes.find(node=>node.id===or.id);assert.equal(savedOr.inputs.length,2);assert.deepEqual(savedOr.inputs.map(input=>input.id),plain(or.inputs.slice(0,2)).map(input=>input.id));assert.equal(saved.document.bindings.filter(binding=>binding.to.nodeId===or.id).length,2);assert.equal(saved.document.bindings.length,3);assert.deepEqual(saved.assignments.map(assignment=>assignment.ports.map(pin=>pin.observedPortId)),[['output:0','output:1'],['input:0']]);assert.ok(!saved.document.bindings.some(binding=>binding.from.nodeId==='source'&&binding.from.portId==='output:2'));
 for(const binding of saved.document.bindings)assert.equal(graph.graphConnectionError(saved.document,binding.from,binding.to,binding.id),'');
});

test('a Boolean OR cannot remove an input when only its minimum two remain',async()=>{
 const {connected:two,seed,or}=await committedThreeInputOr(2),twoLane=modules.get('./profile-signal-overview').signalOverview(two.document)[0].lanes[0],h=await harness(two,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'}}),before=plain(two);
 assert.equal(h.handlers.removeTransformInput(twoLane.id,or.id,or.inputs[1].id),false);assert.deepEqual(h.edits,[]);assert.deepEqual(plain(two),before);
});

test('clearing a committed third OR input preserves its slot until the incomplete row removes it',async()=>{
 const {connected,seed,or,lane}=await committedThreeInputOr(),h=await harness(connected,seed,{}, {overview:true,overviewPair:{from:'source',to:'target'}}),third=or.inputs[2];
 assert.equal(h.handlers.chooseLaneTransformInput(lane.id,or.id,third.id,''),true);assert.equal(h.edits.length,1);
  const pending=plain(h.edits[0].next),row=plain(h.handlers.state.incompleteRows[0]),pendingOr=row.transforms.find(node=>node.id===or.id)??assert.fail(JSON.stringify(row));assert.equal(pending.document.processingNodes.find(node=>node.id===or.id),undefined);assert.ok(!pending.document.bindings.some(binding=>binding.to.nodeId===or.id));assert.equal(row.from.port.observed.descriptor.id,'output:0');assert.equal(row.inputs[`${or.id}/${or.inputs[1].id}`].port.observed.descriptor.id,'output:1');assert.equal(row.inputs[`${or.id}/${third.id}`],undefined);assert.equal(pendingOr.inputs.length,3);
 const repair=await harness(pending,{...seed,incompleteRows:[row],routeOpen:true,draftRowOpen:true,autoCommitArmed:true});repair.handlers.activateIncomplete(lane.id);assert.equal(repair.handlers.removeTransformInput(lane.id,or.id,third.id),true);assert.equal(repair.edits.length,1,JSON.stringify({message:repair.handlers.state.message,error:repair.handlers.state.connectionError}));
  const saved=plain(repair.edits[0].next),savedOr=saved.document.processingNodes.find(node=>node.id===or.id);assert.equal(savedOr.inputs.length,2);assert.deepEqual(savedOr.inputs.map(input=>input.id),plain(or.inputs.slice(0,2)).map(input=>input.id));assert.deepEqual(saved.assignments.map(assignment=>assignment.ports.map(pin=>pin.observedPortId)),[['output:0','output:1'],['input:0']]);assert.equal(repair.handlers.state.incompleteRows.length,0);
});
