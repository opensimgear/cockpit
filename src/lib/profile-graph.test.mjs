import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph']);
const graph=modules.get('./profile-graph');
const {profileDocumentError}=modules.get('./profiles');
const plain=value=>JSON.parse(JSON.stringify(value));
const bool={type:'boolean'},number=(min=0,max=100,unit=null)=>({type:'number',min,max,unit});
const state=(schema=bool)=>({delivery:'state',schema,freshnessMs:1000,fallback:null});
const port=(id,direction='output',contract=state())=>({id,name:id,direction,contract});
const resource=(id,ports=[])=>({id,name:id,kind:'device',required:false,ports});
const endpoint=(nodeId,portId)=>({nodeId,portId});
const hash=`sha256:${'1'.repeat(64)}`;
const document=()=>({schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Graph',revision:1,eligibility:{gameIds:[]},resources:[resource('source',[port('button'),port('event','output',{delivery:'event',schema:bool})]),resource('target',[port('value','input')])],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}});
const draft=()=>({document:document(),assignments:[]});
const processing=(id)=>({id,name:id,operation:{kind:'identity'},inputs:[port('in','input')],outputs:[port('out')]});
const observed=(id='native:button',contract=state(),direction='output')=>({descriptor:{id,name:'Advertised control',direction,contract},contractHash:hash,routeIds:['usb'],current:true});
const inventory=(id='native:resource')=>({id,name:'Inventory resource',resourceType:'host-controller',availability:'online',generation:'1',capabilityRevision:hash,portCount:20000,cachedPortCount:32,connections:[],attributes:{},diagnostics:[]});
const publicationProof=()=>({schema:1,epoch:1,revision:1,module:{id:'0123456789abcdef0123456789abcdef',boot:1,configurationGeneration:1,node:1,sourceNode:1,profile:{revision:1,buttons:2,axes:1,hats:0,neutral:[0],axisUsage:[0x30]},publicationSlot:0,configuredEnabled:true,publicationReportId:1,configuredDescriptorIndex:0}});
const publicationResource=(proof=publicationProof())=>({...inventory(),resourceType:'osg-module',connections:[{scope:'gateway',key:'module',route:{id:'can:gateway:module',transport:'can',providerGeneration:'1',attributes:{},baselineReady:true,writable:true,priority:1}}],attributes:{controllerPublicationOverride:JSON.stringify(proof)}});

test('vehicle graph admission checks Base with each vehicle and ignores cycles assembled only across exclusive tabs',()=>{
 const value=draft();value.document.eligibility.gameIds=['dcs'];value.document.processingNodes=['base-x','base-y','aircraft-a','aircraft-b'].map(processing);
 value.document.vehicleTabs=[{id:'a',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:['aircraft-a']},{id:'b',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:['aircraft-b']}];
 let next=value;
 for(const[from,to]of[['base-x','aircraft-a'],['aircraft-a','base-y'],['base-y','aircraft-b'],['aircraft-b','base-x']])next=graph.graphConnect(next,endpoint(from,'out'),endpoint(to,'in'));
 assert.equal(next.document.bindings.length,4);assert.equal(profileDocumentError(next.document),'');
 assert.match(graph.graphConnectionError(next.document,endpoint('aircraft-a','out'),endpoint('aircraft-b','in')),/Different vehicle tabs/);
 assert.match(graph.graphConnectionError(next.document,endpoint('base-x','out'),endpoint('base-y','in')),/already has a writer/);
 assert.deepEqual(value.document.bindings,[]);
});

test('vehicle output and transformation additions retain their scope through edits and removal',()=>{
 const value=draft();value.document.eligibility.gameIds=['dcs'];value.document.vehicleTabs=[{id:'a',gameId:'dcs',vehicleId:'F-16C_50',vehicleName:'F-16C',nodeIds:[]},{id:'b',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'F/A-18C',nodeIds:[]}];
 const desktop=id=>({id,name:id,required:false,scope:{kind:'selected-game'},ports:[],actions:[{portId:'key',action:{kind:'key-hold',key:44}}]});
 let next=graph.graphSetDesktop(value,desktop('keys-a'),'a');next=graph.graphSetDesktop(next,desktop('keys-b'),'b');assert.equal(next.document.desktopOutputs.length,2);
 next=graph.graphSetDesktop(next,{...desktop('keys-a'),name:'Updated keys'});assert.equal(next.document.desktopOutputs.length,2);assert.equal(next.document.desktopOutputs.find(node=>node.id==='keys-b').name,'keys-b');
 assert.throws(()=>graph.graphSetDesktop(next,desktop('keys-base')),/one active/);
 next=graph.graphSetProcessing(next,processing('aircraft-transform'),'a');assert.deepEqual(plain(next.document.vehicleTabs[0].nodeIds),['keys-a','aircraft-transform']);
 next=graph.graphRemoveNode(next,'keys-a');assert.deepEqual(plain(next.document.vehicleTabs[0].nodeIds),['aircraft-transform']);assert.equal(profileDocumentError(next.document),'');
 assert.deepEqual(value.document.vehicleTabs[0].nodeIds,[]);
});

test('publication capability is advisory only and rejects absent, offline, wrong-transport or malformed native metadata',()=>{
 const capability=graph.graphPublicationCapability;assert.equal(capability().status,'unavailable');assert.equal(capability(inventory()).status,'unsupported');const source=publicationResource();assert.equal(capability(source).status,'advertised');assert.equal(capability(source).configuredEnabled,true);assert.match(capability(source).message,/not a Ready result/);assert.equal(capability({...source,availability:'offline'}).status,'unavailable');assert.equal(capability({...source,availability:'unavailable'}).status,'unavailable');assert.equal(capability({...source,connections:[]}).status,'unsupported');assert.equal(capability({...source,connections:[{route:{transport:'usb'}}]}).status,'unsupported');
 for(const text of ['', '{', 'x'.repeat(4097)])assert.equal(capability({...source,attributes:{controllerPublicationOverride:text}}).status,'unsupported');
 const invalid=[null,[],{}, {...publicationProof(),schema:2},{...publicationProof(),extra:true},{...publicationProof(),epoch:0},{...publicationProof(),revision:0},{...publicationProof(),epoch:0x100000000}];for(const proof of invalid)assert.equal(capability(publicationResource(proof)).status,'unsupported');
 for(const patch of [{id:'0'.repeat(32)},{id:'f'.repeat(32)},{id:'BAD'},{boot:0},{configurationGeneration:0},{node:0},{node:33},{sourceNode:-1},{publicationSlot:32},{configuredEnabled:'true'},{publicationReportId:0},{publicationReportId:33},{configuredDescriptorIndex:-1},{extra:true}])assert.equal(capability(publicationResource({...publicationProof(),module:{...publicationProof().module,...patch}})).status,'unsupported',JSON.stringify(patch));
 for(const profile of [{},{...publicationProof().module.profile,revision:0},{...publicationProof().module.profile,buttons:129},{...publicationProof().module.profile,axes:9},{...publicationProof().module.profile,hats:5},{...publicationProof().module.profile,neutral:[]},{...publicationProof().module.profile,neutral:[65536]},{...publicationProof().module.profile,axisUsage:[0x39]},{revision:1,buttons:0,axes:2,hats:0,neutral:[0,0],axisUsage:[0x30,0x30]}])assert.equal(capability(publicationResource({...publicationProof(),module:{...publicationProof().module,profile}})).status,'unsupported');
 const configuredOff=publicationProof();configuredOff.module.configuredEnabled=false;configuredOff.module.configuredDescriptorIndex=-1;assert.equal(capability(publicationResource(configuredOff)).configuredEnabled,false);
});

test('controller publication intent is an explicit per-device Boolean policy without a local effect or permanent write',()=>{
 const value=draft(),enabled=graph.graphSetRuntimeOverride(value,'source',true);assert.deepEqual(plain(enabled.document.overrides),[{resourceId:'source',enabled:true}]);assert.deepEqual(value.document.overrides,[]);assert.deepEqual(plain(enabled.document.resources),value.document.resources);assert.deepEqual(plain(enabled.assignments),[]);
 const other=graph.graphSetRuntimeOverride(enabled,'target',false),disabled=graph.graphSetRuntimeOverride(other,'source',false);assert.equal(disabled.document.overrides.length,2);assert.equal(disabled.document.overrides.find(row=>row.resourceId==='source').enabled,false);const kept=graph.graphSetRuntimeOverride(disabled,'source',null);assert.deepEqual(plain(kept.document.overrides),[{resourceId:'target',enabled:false}]);assert.equal(graph.graphRemoveNode(other,'source').document.overrides.length,1);
 for(const input of [undefined,'false',0,{}])assert.throws(()=>graph.graphSetRuntimeOverride(value,'source',input),/explicitly/);assert.throws(()=>graph.graphSetRuntimeOverride(value,'missing',true),/physical device/);const foreign=plain(value);foreign.document.resources[0].kind='game';assert.throws(()=>graph.graphSetRuntimeOverride(foreign,'source',true),/physical device/);foreign.document.resources[0].kind='companion';assert.throws(()=>graph.graphSetRuntimeOverride(foreign,'source',false),/physical device/);
});

test('wire contracts preserve exact state/event, numeric type, unit, range, text encoding and enum constraints',()=>{
  const {graphScalarFits:fits,graphContractError:error}=graph;
  assert.equal(fits(number(0,10,'ft'),number(0,100,'ft')),true);assert.equal(fits(number(0,10,'ft'),number(0,100,'m')),false);assert.equal(fits({type:'integer',min:0,max:10,unit:null},number()),false);assert.equal(fits(number(-1,10),number(0,10)),false);
  assert.equal(fits({type:'text',maxBytes:10,encoding:'ascii'},{type:'text',maxBytes:20,encoding:'utf8'}),true);assert.equal(fits({type:'text',maxBytes:10,encoding:'utf8'},{type:'text',maxBytes:20,encoding:'ascii'}),false);assert.equal(fits({type:'enum',values:['on']},{type:'enum',values:['on','off']}),true);assert.equal(fits({type:'enum',values:['missing']},{type:'enum',values:['on']}),false);assert.equal(fits(bool,{type:'rgb'}),false);
  assert.equal(error(state(number(0,10,'ft')),state(number(0,100,'ft'))),'');assert.match(error(state(number(0,10,'ft')),state(number(0,100,'m'))),/conversion/);assert.match(error({delivery:'event',schema:bool},state()),/delivery/);
});
test('action wires require a complete invocation with ordered named arguments and explicit semantics',()=>{
  const parameters=[{id:'brightness',name:'Brightness',schema:number(0,100)},{id:'active',name:'Active',schema:bool}];
  const action={delivery:'action',parameters,semantics:'setter',endpoint:'lamp',readable:true,display:null,ownership:null};
  const invocation={delivery:'invocation',parameters:plain(parameters),semantics:'setter'};
  assert.equal(graph.graphContractError(invocation,action),'');invocation.parameters[0].name='A cosmetic label';assert.equal(graph.graphContractError(invocation,action),'');
  assert.match(graph.graphContractError({...invocation,parameters:[...invocation.parameters].reverse()},action),/Ordered/);assert.match(graph.graphContractError({...invocation,semantics:'one-shot'},action),/semantics/);assert.match(graph.graphContractError(state(),action),/complete invocation/);
});
test('connect, replace and remove retain exact endpoints, stable route IDs and immutable independent documents',()=>{
  const original=draft(),from=endpoint('source','button'),to=endpoint('target','value');
  const connected=graph.graphConnect(original,from,to);assert.equal(connected.document.bindings[0].id,'route-1');assert.deepEqual(plain(connected.document.bindings[0]),{id:'route-1',from,to});assert.equal(original.document.bindings.length,0);from.portId='mutated';assert.equal(connected.document.bindings[0].from.portId,'button');
  const replaced=graph.graphConnect(connected,endpoint('source','button'),to,'route-1');assert.equal(replaced.document.bindings.length,1);assert.equal(replaced.document.bindings[0].id,'route-1');
  assert.equal(graph.graphDisconnect(replaced,'route-1').document.bindings.length,0);assert.throws(()=>graph.graphDisconnect(original,'missing'),/existing route/);assert.throws(()=>graph.graphConnect(original,endpoint('source','button'),to,'missing'),/existing route/);
});
test('missing/wrong-direction/incompatible ports and duplicate writers reject before editing the graph',()=>{
  const original=draft();assert.match(graph.graphConnectionError(original.document,endpoint('missing','button'),endpoint('target','value')),/existing concrete/);assert.match(graph.graphConnectionError(original.document,endpoint('target','value'),endpoint('source','button')),/source output/);
  assert.throws(()=>graph.graphConnect(original,endpoint('source','event'),endpoint('target','value')),/delivery/);
  const connected=graph.graphConnect(original,endpoint('source','button'),endpoint('target','value'));assert.throws(()=>graph.graphConnect(connected,endpoint('source','button'),endpoint('target','value')),/already has a writer/);assert.equal(connected.document.bindings.length,1);
});
test('processing cycles reject while independent external command and telemetry directions remain authorable',()=>{
  let next=draft();next=graph.graphSetProcessing(next,processing('one'));next=graph.graphSetProcessing(next,processing('two'));next=graph.graphConnect(next,endpoint('one','out'),endpoint('two','in'));
  assert.throws(()=>graph.graphConnect(next,endpoint('two','out'),endpoint('one','in')),/cycle/);assert.throws(()=>graph.graphConnect(next,endpoint('one','out'),endpoint('one','in')),/cycle/);
  const external=draft();external.document.resources[0].ports.push(port('feedback','input'));external.document.resources[1].ports.push(port('telemetry'));
  let pair=graph.graphConnect(external,endpoint('source','button'),endpoint('target','value'));pair=graph.graphConnect(pair,endpoint('target','telemetry'),endpoint('source','feedback'));assert.equal(pair.document.bindings.length,2);
});
test('native source and route caps produce actionable errors instead of truncation',()=>{
  const next=draft();next.document.resources=Array.from({length:17},(_,i)=>resource(`source-${i}`,[port('out'),port('in','input')]));
  for(let i=0;i<16;i++)next.document.bindings.push({id:`edge-${i}`,from:endpoint(`source-${i}`,'out'),to:endpoint(`source-${i}`,'in')});
  assert.match(graph.graphConnectionError(next.document,endpoint('source-16','out'),endpoint('source-16','in')),/16 used source/);
  const full=draft();full.document.bindings=Array.from({length:512},(_,i)=>({id:`edge-${i}`,from:endpoint('source','button'),to:endpoint('source',`in-${i}`)}));assert.match(graph.graphConnectionError(full.document,endpoint('source','button'),endpoint('target','value')),/512 explicit/);
});
test('provider selection persists only selected typed declarations and exact local IDs/hashes, including cached capabilities',()=>{
  const next=draft();next.document.resources=[resource('device')];next.assignments=[{resourceId:'device',sourceId:'native:resource',ports:[]}];const longId=`native:${'a'.repeat(150)}`;
  const selected=graph.graphSelectProviderPort(next,'device','native:resource',{...observed(longId),current:false});assert.equal(selected.document.resources[0].ports.length,1);assert.equal(selected.document.resources[0].ports[0].id,'port-1');assert.equal(selected.assignments[0].ports[0].observedPortId,longId);assert.equal(selected.assignments[0].ports[0].expectedContractHash,hash);assert.ok(!JSON.stringify(selected.document).includes(longId));assert.equal(next.document.resources[0].ports.length,0);
  assert.deepEqual(plain(graph.graphSelectProviderPort(selected,'device','native:resource',observed(longId))),plain(selected));assert.throws(()=>graph.graphSelectProviderPort(selected,'device','native:resource',{...observed(longId),contractHash:`sha256:${'2'.repeat(64)}`}),/contract changed/);
  assert.throws(()=>graph.graphSelectProviderPort(selected,'device','wrong',observed()),/Explicitly assign/);assert.throws(()=>graph.graphSelectProviderPort(selected,'device','native:resource',{...observed(),contractHash:'invalid'}),/exact native/);assert.throws(()=>graph.graphSelectProviderPort(selected,'device','native:resource',observed(),'port-1'),/unused logical/);
});
test('copy assignment requires explicit complete matching contracts and preserves cosmetic labels/object key order',()=>{
  const next=draft();next.document.resources=[resource('device',[port('logical')])];
  assert.throws(()=>graph.graphAssignResource(next,'device',inventory(),[]),/every saved logical/);assert.throws(()=>graph.graphAssignResource(next,'missing',inventory(),[]),/existing resource/);assert.throws(()=>graph.graphAssignResource(next,'device',{...inventory(),resourceType:'companion'},[]),/same category/);
  assert.throws(()=>graph.graphAssignResource(next,'device',inventory(),[{profilePortId:'wrong',observed:observed()}]),/every saved logical/);
  assert.throws(()=>graph.graphAssignResource(next,'device',inventory(),[{profilePortId:'logical',observed:observed('axis',state(number()))}]),/contract differs/);
  const compatible=observed();compatible.descriptor.contract={fallback:null,freshnessMs:1000,schema:bool,delivery:'state'};const assigned=graph.graphAssignResource(next,'device',inventory(),[{profilePortId:'logical',observed:compatible}]);assert.equal(assigned.assignments[0].ports[0].profilePortId,'logical');assert.equal(assigned.assignments[0].ports[0].observedPortId,'native:button');assert.equal(assigned.document.resources[0].ports[0].id,'logical');assert.equal(next.assignments.length,0);
});
test('removing selected resource ports or whole blocks removes only their explicit edges, pins, layout and policies',()=>{
  const next=draft();next.assignments=[{resourceId:'source',sourceId:'native:source',ports:[{profilePortId:'button',observedPortId:'button',expectedContractHash:hash},{profilePortId:'event',observedPortId:'event',expectedContractHash:hash}]}];next.document.editor.positions=[{nodeId:'source',x:12,y:20}];
  const connected=graph.graphConnect(next,endpoint('source','button'),endpoint('target','value'));const hidden=graph.graphSetVisibility(connected,'source','hidden-while-active');hidden.document.overrides=[{resourceId:'source',enabled:true}];
  const removed=graph.graphRemovePort(hidden,'source','button');assert.equal(removed.document.bindings.length,0);assert.equal(removed.assignments[0].ports.length,1);assert.equal(removed.document.resources.length,2);assert.throws(()=>graph.graphRemovePort(next,'source','missing'),/existing resource port/);
  const all=graph.graphRemoveNode(hidden,'source');assert.equal(all.document.resources.length,1);assert.equal(all.document.bindings.length,0);assert.equal(all.assignments.length,0);assert.equal(all.document.editor.positions.length,0);assert.equal(all.document.visibility.length,0);assert.equal(all.document.overrides.length,0);assert.throws(()=>graph.graphRemoveNode(next,'missing'),/existing block/);assert.throws(()=>graph.graphSetVisibility(next,'missing','visible'),/physical device/);
});
test('virtual controllers declare exact stable button/axis/hat contracts without numeric normalization',()=>{
  const next=draft(),layout={buttons:['fire'],axes:[{portId:'steer',range:'bipolar',fallback:0},{portId:'throttle',range:'unipolar',fallback:0}],hats:['view']};
  const created=graph.graphSetVirtual(next,{id:'virtual-1',name:'Rig',required:false,layout});const ports=created.document.virtualOutputs[0].ports;
  assert.deepEqual(plain(ports.map(port=>port.id)),['fire','steer','throttle','view']);assert.deepEqual(plain(ports[1].contract.schema),{type:'integer',min:-32768,max:32767,unit:null});assert.equal(ports[3].contract.fallback,-1);assert.equal(next.document.virtualOutputs.length,0);
  const connected=graph.graphConnect(created,endpoint('source','button'),endpoint('virtual-1','fire'));assert.throws(()=>graph.graphSetVirtual(connected,{id:'virtual-1',name:'Rig',required:false,layout:{buttons:[],axes:layout.axes,hats:[]}}),/existing concrete/);
  assert.throws(()=>graph.graphSetVirtual(next,{id:'source',name:'Collision',required:false,layout}),/unique virtual/);assert.throws(()=>graph.graphVirtualPorts({buttons:[],axes:[],hats:[]}),/1–128/);assert.throws(()=>graph.graphVirtualPorts({buttons:[],axes:Array(17).fill(layout.axes[0]),hats:[]}),/16 axes/);
  let many=next;for(let i=0;i<8;i++)many=graph.graphSetVirtual(many,{id:`virtual-${i}`,name:'Output',required:false,layout:{buttons:['fire'],axes:[],hats:[]}});assert.throws(()=>graph.graphSetVirtual(many,{id:'virtual-nine',name:'Output',required:false,layout}),/eight virtual/);
});
test('processing replacement preserves node identity and rejects dangling wires, duplicate ports and category collisions',()=>{
  const original=draft();let next=graph.graphSetProcessing(original,processing('processor'));next=graph.graphConnect(next,endpoint('source','button'),endpoint('processor','in'));const changed=processing('processor');changed.name='Visible inversion';changed.operation={kind:'not'};
  const updated=graph.graphSetProcessing(next,changed);assert.equal(updated.document.processingNodes.length,1);assert.equal(updated.document.processingNodes[0].operation.kind,'not');assert.equal(updated.document.bindings.length,1);changed.name='Mutated';assert.equal(updated.document.processingNodes[0].name,'Visible inversion');
  assert.throws(()=>graph.graphSetProcessing(next,{...changed,inputs:[port('different','input')]}),/existing concrete/);assert.throws(()=>graph.graphSetProcessing(next,{...changed,id:'source'}),/unique processing/);assert.throws(()=>graph.graphSetProcessing(next,{...changed,outputs:[]}),/1–16/);assert.throws(()=>graph.graphSetProcessing(next,{...changed,outputs:[port('in')]}),/unique directed/);
});
test('graph edits validate bounds before JSON cloning and require complete local pin sets',()=>{
  const next=draft();next.document.name='';assert.equal(graph.graphDraft(next).document.name,'');next.document.editor.viewport.x=Infinity;assert.throws(()=>graph.graphDraft(next),/bounded editor/);
  const invalid=draft();invalid.assignments=[{resourceId:'source',sourceId:'native',ports:[]}];assert.throws(()=>graph.graphDraft(invalid),/every selected logical port/);assert.throws(()=>graph.graphSelectProviderPort(draft(),'source','native',observed()),/Explicitly assign/);
});
test('the complete native mixed fixture is inspectable and every saved binding is an exact compatible graph edge',async()=>{
  const mixed=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/mixed.json',import.meta.url),'utf8'));assert.equal(profileDocumentError(mixed),'');
  for(const binding of mixed.bindings)assert.equal(graph.graphConnectionError(mixed,binding.from,binding.to,binding.id),'',binding.id);
  const selected=graph.graphDraft({document:mixed,assignments:[]});assert.equal(selected.document.processingNodes.length,mixed.processingNodes.length);assert.equal(selected.document.bindings.length,mixed.bindings.length);
});
test('route insertion, bypass and compatible transformation reorder are atomic visible edge edits',()=>{
  const initial=graph.graphConnect(draft(),endpoint('source','button'),endpoint('target','value'));
  const inserted=graph.graphInsertProcessing(initial,'route-1',processing('one'),'in','out');assert.equal(inserted.document.processingNodes.length,1);assert.equal(inserted.document.bindings.length,2);assert.equal(initial.document.processingNodes.length,0);
  const edge=inserted.document.bindings.find(binding=>binding.to.nodeId==='target');const chain=graph.graphInsertProcessing(inserted,edge.id,processing('two'),'in','out');const ids=chain.document.bindings.map(binding=>binding.id).sort();
  const reordered=graph.graphReorderProcessing(chain,['two','one']);assert.equal(reordered.document.bindings.find(binding=>binding.from.nodeId==='source').to.nodeId,'two');assert.equal(reordered.document.bindings.find(binding=>binding.from.nodeId==='one').to.nodeId,'target');assert.deepEqual(plain(reordered.document.bindings.map(binding=>binding.id).sort()),plain(ids));assert.equal(chain.document.bindings.find(binding=>binding.from.nodeId==='source').to.nodeId,'one');
  const bypassed=graph.graphBypassProcessing(reordered,'two');assert.equal(bypassed.document.processingNodes.length,1);assert.equal(bypassed.document.bindings.length,2);assert.equal(bypassed.document.bindings.find(binding=>binding.from.nodeId==='source').to.nodeId,'one');
  assert.throws(()=>graph.graphInsertProcessing(initial,'missing',processing('other'),'in','out'),/existing route/);assert.throws(()=>graph.graphInsertProcessing(chain,chain.document.bindings[0].id,processing('one'),'in','out'),/new transformation/);assert.throws(()=>graph.graphBypassProcessing(initial,'missing'),/one connected input/);assert.throws(()=>graph.graphReorderProcessing(chain,['one','one']),/unique/);
  const branch=graph.graphSetProcessing(chain,processing('branch'));branch.document.bindings.push({id:'fanout',from:endpoint('one','out'),to:endpoint('branch','in')});assert.throws(()=>graph.graphReorderProcessing(branch,['one','two']),/linear route/);
  const incompatible=processing('incompatible');incompatible.inputs[0].contract=state(number());assert.throws(()=>graph.graphInsertProcessing(initial,'route-1',incompatible,'in','out'),/Types/);assert.equal(initial.document.bindings.length,1);
});
test('captured preflight inventory metadata invalidates on disconnect, contract/setup/route changes and unrelated refresh conservatively',async()=>{
  const original=inventory(),snapshot=graph.graphInventorySignature([original]);let complete;const pending=new Promise(resolve=>complete=resolve),captured=(async()=>({report:await pending,inventory:snapshot}))();
  const offline=plain(original);offline.availability='offline';offline.generation='2';complete({readiness:'ready'});const preview=await captured;assert.equal(preview.report.readiness,'ready');assert.notEqual(graph.graphInventorySignature([offline]),preview.inventory);
  for(const change of [resource=>resource.capabilityRevision=`sha256:${'2'.repeat(64)}`,resource=>resource.attributes.providerSetupRevision='changed',resource=>resource.connections.push({scope:'scope',key:'key',route:{id:'route',transport:'usb',providerGeneration:'new-boot',attributes:{},baselineReady:false,writable:false,priority:1}})]){const changed=plain(original);change(changed);assert.notEqual(graph.graphInventorySignature([changed]),snapshot);}
  const unrelated=inventory('another-native-resource');assert.notEqual(graph.graphInventorySignature([original,unrelated]),snapshot);assert.equal(graph.graphInventorySignature([original,unrelated]),graph.graphInventorySignature([unrelated,original]));const relabeled=plain(original);relabeled.name='Cosmetic name';assert.equal(graph.graphInventorySignature([relabeled]),snapshot);
});
