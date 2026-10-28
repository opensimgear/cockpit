import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profiles','./profile-graph','./profile-discovery','./profile-processing','./profile-connection-draft']);
const graph=modules.get('./profile-graph');
const {connectionDraftPlan,commitConnectionDraft,discardCommittedConnection,createConnectionTransform,createRouteTransform}=modules.get('./profile-connection-draft');
const {processingTemplate}=modules.get('./profile-processing');
const bool={type:'boolean'},number={type:'number',min:0,max:100,unit:null},state=(schema=bool)=>({delivery:'state',schema,freshnessMs:1000,fallback:null});
const port=(id,direction,contract=state())=>({id,name:id,direction,contract});
const endpoint=(nodeId,p)=>({nodeId,port:{...p,key:p.id,profilePortId:p.id,sourceId:null,observed:null,current:true}});
const sourcePort={descriptor:{id:'native:button',name:'Button',direction:'output',contract:state()},contractHash:`sha256:${'1'.repeat(64)}`,routeIds:['usb'],current:true};
const fresh=()=>({document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Draft',revision:1,eligibility:{gameIds:[]},resources:[{id:'source',name:'Source',kind:'device',required:false,ports:[]},{id:'target',name:'Target',kind:'device',required:false,ports:[port('input','input')]}],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[{resourceId:'source',sourceId:'native',ports:[]}]});
const from=()=>({nodeId:'source',port:{...sourcePort.descriptor,id:'@observed:button',key:'@observed:button',profilePortId:null,sourceId:'native',observed:sourcePort,current:true}});
const to=()=>endpoint('target',port('input','input'));
const identity=(id='stage')=>({id,name:'Stage',operation:{kind:'identity'},inputs:[port('in','input')],outputs:[port('out','output')]});
const plain=value=>JSON.parse(JSON.stringify(value));

function replaceFixture(shared=false){
 let value=fresh();value.document.resources.push({id:'other',name:'Other',kind:'device',required:false,ports:[port('input','input')]},{id:'new-source',name:'New source',kind:'device',required:false,ports:[port('out','output')]});
 value=graph.graphSelectProviderPort(value,'source','native',sourcePort);
 const sourceId=value.document.resources.find(resource=>resource.id==='source').ports[0].id;
 value=graph.graphSetProcessing(value,identity('prior'));
 value=graph.graphConnect(value,{nodeId:'source',portId:sourceId},{nodeId:'prior',portId:'in'});
 value=graph.graphConnect(value,{nodeId:'prior',portId:'out'},{nodeId:'target',portId:'input'});
 if(shared)value=graph.graphConnect(value,{nodeId:'source',portId:sourceId},{nodeId:'other',portId:'input'});
 return{value,sourceId,writerId:value.document.bindings.find(binding=>binding.to.nodeId==='target').id};
}

test('planning is discardable and does not materialize provider pins or graph changes',()=>{
 const original=fresh(),before=plain(original),plan=connectionDraftPlan(original,from(),to(),[identity()]);
 assert.equal(plan.error,'');assert.equal(plan.nodes.length,1);assert.equal(original.document.resources[0].ports.length,0);assert.equal(original.document.bindings.length,0);assert.deepEqual(plain(original),before);
 assert.equal(plan.draft.assignments[0].ports.length,1);assert.equal(plan.draft.document.bindings.length,2);
});

test('configured parameters survive the complete candidate and one returned graph edit',()=>{
 const original=fresh(),stage={...identity(),operation:{kind:'fallback',unknown:true,stale:false,unavailable:true}},stage2={...identity('stage-2'),operation:{kind:'fallback',unknown:false,stale:true,unavailable:false}};
 const plan=connectionDraftPlan(original,from(),to(),[stage,stage2]);assert.equal(plan.error,'');assert.deepEqual(plain(plan.nodes.map(node=>node.operation)),[stage.operation,stage2.operation]);
 const committed=commitConnectionDraft(original,from(),to(),[stage,stage2]);assert.equal(committed.document.processingNodes.length,2);assert.deepEqual(plain(committed.document.processingNodes.map(node=>node.operation)),[stage.operation,stage2.operation]);assert.equal(committed.document.bindings.length,3);assert.equal(original.document.bindings.length,0);
});

test('an invalid later stage returns no partial candidate and leaves input untouched',()=>{
 const original=fresh(),before=plain(original),first=identity('first'),rgb=state({type:'rgb'}),bad={...identity('bad'),inputs:[port('in','input',rgb)],outputs:[port('out','output',rgb)]};
 const plan=connectionDraftPlan(original,from(),to(),[first,bad]);assert.match(plan.error,/compatible|Types|transformation/i);assert.deepEqual(plain(plan.nodes),[]);assert.deepEqual(plain(original),before);assert.equal(plan.draft.document.processingNodes.length,0);
 assert.throws(()=>commitConnectionDraft(original,from(),to(),[first,bad]));assert.deepEqual(plain(original),before);
});

test('switching provider selections to an incompatible final endpoint leaves pins and assignments unchanged',()=>{
 const original=fresh();original.document.resources.find(resource=>resource.id==='target').ports=[];original.assignments.push({resourceId:'target',sourceId:'native-target',ports:[]});
 const before=plain({ports:original.document.resources.map(resource=>resource.ports),assignments:original.assignments});
 const incompatible={descriptor:{id:'native:rgb',name:'Colour input',direction:'input',contract:state({type:'rgb'})},contractHash:`sha256:${'2'.repeat(64)}`,routeIds:['integration'],current:true};
 const changedTarget={nodeId:'target',port:{...incompatible.descriptor,id:'@observed:rgb',key:'@observed:rgb',profilePortId:null,sourceId:'native-target',observed:incompatible,current:true}};
 const failedPlan=connectionDraftPlan(original,from(),changedTarget,[]);
 assert.match(failedPlan.error,/Types|range|contract|conversion/i);assert.deepEqual(plain({ports:failedPlan.draft.document.resources.map(resource=>resource.ports),assignments:failedPlan.draft.assignments}),before);
 assert.throws(()=>commitConnectionDraft(original,from(),changedTarget,[]),/Types|range|contract|conversion/i);
 assert.deepEqual(plain({ports:original.document.resources.map(resource=>resource.ports),assignments:original.assignments}),before);
});

test('reconnecting replaces the requested route in place and confirms an occupied writer',()=>{
 let original=fresh();original=graph.graphSetProcessing(original,identity('prior'));original=graph.graphConnect(original,{nodeId:'prior',portId:'out'},{nodeId:'target',portId:'input'});
 const id=original.document.bindings[0].id,discoverFrom=endpoint('prior',port('out','output'));
 const plan=connectionDraftPlan(original,discoverFrom,to(),[],id);assert.equal(plan.error,'');
 const committed=commitConnectionDraft(original,discoverFrom,to(),[],id);assert.equal(committed.document.bindings.length,1);assert.equal(committed.document.bindings[0].id,id);
 const occupied=connectionDraftPlan(original,discoverFrom,to(),[]);assert.equal(occupied.error,'');assert.equal(occupied.writer.id,id);assert.equal(occupied.draft.document.bindings.length,1);
 assert.throws(()=>commitConnectionDraft(original,discoverFrom,to(),[]),/already connected/);
 const replaced=commitConnectionDraft(original,discoverFrom,to(),[],undefined,true);assert.equal(replaced.document.bindings.length,1);assert.equal(replaced.document.bindings[0].id,id);
});

test('confirmed replacement removes only the orphaned old transform chain and its provider pin',()=>{
 const{value,sourceId,writerId}=replaceFixture();
 const before=plain(value),committed=commitConnectionDraft(value,endpoint('new-source',port('out','output')),to(),[identity('replacement')],writerId,true);
 assert.deepEqual(plain(value),before);assert.equal(committed.document.bindings.length,2);assert.equal(committed.document.bindings.find(binding=>binding.id===writerId).to.nodeId,'target');
 assert.deepEqual(plain(committed.document.processingNodes.map(node=>node.id)),['replacement']);assert.equal(committed.document.resources.find(resource=>resource.id==='source').ports.length,0);assert.deepEqual(plain(committed.assignments[0].ports),[]);
 assert.ok(committed.document.bindings.every(binding=>binding.from.nodeId!=='prior'&&binding.to.nodeId!=='prior'));
 for(const binding of committed.document.bindings)assert.equal(graph.graphConnectionError(committed.document,binding.from,binding.to,binding.id),'');
});

test('replacement retains a shared upstream source port while removing its unused former branch',()=>{
 const{value,sourceId,writerId}=replaceFixture(true),before=plain(value),committed=commitConnectionDraft(value,endpoint('new-source',port('out','output')),to(),[identity('replacement')],writerId,true);
 assert.deepEqual(plain(value),before);assert.ok(committed.document.processingNodes.some(node=>node.id==='prior')===false);
 assert.ok(committed.document.resources.find(resource=>resource.id==='source').ports.some(port=>port.id===sourceId));assert.ok(committed.assignments[0].ports.some(pin=>pin.profilePortId===sourceId));
 assert.ok(committed.document.bindings.some(binding=>binding.from.nodeId==='source'&&binding.from.portId===sourceId&&binding.to.nodeId==='other'));
 for(const binding of committed.document.bindings)assert.equal(graph.graphConnectionError(committed.document,binding.from,binding.to,binding.id),'');
});

test('unconfirmed or invalid replacements do not prune any former source nodes or ports',()=>{
 const{value,sourceId,writerId}=replaceFixture(),before=plain(value),newSource=endpoint('new-source',port('out','output'));
 assert.throws(()=>commitConnectionDraft(value,newSource,to(),[],undefined,false),/already connected/);assert.deepEqual(plain(value),before);
 const incompatible={...identity('invalid'),inputs:[port('in','input',state({type:'rgb'}))],outputs:[port('out','output',state({type:'rgb'}))]};
 assert.throws(()=>commitConnectionDraft(value,newSource,to(),[incompatible],writerId,true),/compatible|Types|range|contract|conversion/i);assert.deepEqual(plain(value),before);
 assert.ok(value.document.processingNodes.some(node=>node.id==='prior'));assert.ok(value.document.resources[0].ports.some(port=>port.id===sourceId));
});

function compareFixture(shared=false){
 let value=fresh();
 value.document.resources.find(resource=>resource.id==='source').ports=[port('axis','output',state(number))];
 value.assignments[0].ports=[{profilePortId:'axis',observedPortId:'native:axis',expectedContractHash:`sha256:${'3'.repeat(64)}`}];
 if(shared)value.document.resources.push({id:'other-target',name:'Other target',kind:'device',required:false,ports:[port('input','input')]});
 value=graph.graphSetProcessing(value,processingTemplate('compare','compare',state(number)));
 value=graph.graphConnect(value,{nodeId:'source',portId:'axis'},{nodeId:'compare',portId:'in'});
 value=graph.graphConnect(value,{nodeId:'compare',portId:'out'},{nodeId:'target',portId:'input'});
 if(shared)value=graph.graphConnect(value,{nodeId:'compare',portId:'out'},{nodeId:'other-target',portId:'input'});
 return value;
}

test('discarding a sole committed Compare row removes its stages and unused provider pin atomically',()=>{
 const value=compareFixture(),before=plain(value),terminal=value.document.bindings.find(binding=>binding.to.nodeId==='target').id;
 const discarded=discardCommittedConnection(value,terminal);
 assert.deepEqual(plain(value),before);
 assert.equal(discarded.document.bindings.length,0);
 assert.equal(discarded.document.processingNodes.length,0);
 assert.deepEqual(plain(discarded.document.resources.find(resource=>resource.id==='source').ports),[]);
 assert.deepEqual(plain(discarded.assignments[0].ports),[]);
 assert.equal(discarded.document.resources.find(resource=>resource.id==='target').ports.length,1);
 assert.throws(()=>discardCommittedConnection(discarded,terminal),/existing terminal route/);
});

test('discarding a route removes an unused assigned target port and local pin',()=>{
 const value=compareFixture();value.assignments.push({resourceId:'target',sourceId:'native-target',ports:[{profilePortId:'input',observedPortId:'native:input',expectedContractHash:`sha256:${'4'.repeat(64)}`}]});
 const before=plain(value),terminal=value.document.bindings.find(binding=>binding.to.nodeId==='target').id,discarded=discardCommittedConnection(value,terminal);
 assert.deepEqual(plain(value),before);
 assert.deepEqual(plain(discarded.document.resources.find(resource=>resource.id==='target').ports),[]);
 assert.deepEqual(plain(discarded.assignments.find(assignment=>assignment.resourceId==='target').ports),[]);
 assert.equal(discarded.document.processingNodes.length,0);
});

test('discarding one of two rows sharing Compare retains the other route, stage and source pin',()=>{
 const value=compareFixture(true),terminal=value.document.bindings.find(binding=>binding.to.nodeId==='target').id,before=plain(value);
 const discarded=discardCommittedConnection(value,terminal);
 assert.deepEqual(plain(value),before);
 assert.equal(discarded.document.bindings.length,2);
 assert.ok(discarded.document.bindings.some(binding=>binding.from.nodeId==='source'&&binding.from.portId==='axis'&&binding.to.nodeId==='compare'));
 assert.ok(discarded.document.bindings.some(binding=>binding.from.nodeId==='compare'&&binding.to.nodeId==='other-target'));
 assert.ok(discarded.document.processingNodes.some(node=>node.id==='compare'));
 assert.deepEqual(plain(discarded.document.resources.find(resource=>resource.id==='source').ports.map(port=>port.id)),['axis']);
 assert.deepEqual(plain(discarded.assignments[0].ports.map(pin=>pin.profilePortId)),['axis']);
});

test('discarding a nonterminal transform route is rejected without changing the graph',()=>{
 const value=compareFixture(),before=plain(value),internal=value.document.bindings.find(binding=>binding.to.nodeId==='compare').id;
 assert.throws(()=>discardCommittedConnection(value,internal),/terminal route into a resource or output/);
 assert.deepEqual(plain(value),before);
});

test('new transforms use unused graph identities and favor a compatible value input',()=>{
 const value=fresh(),existing=[{...identity('transform-1'),inputs:[port('value','input')]}];
 const node=createConnectionTransform(value,state(),state(),'gate',existing);assert.equal(node.id,'transform-2');assert.equal(node.inputs.some(input=>input.id==='value'),true);
 assert.match(connectionDraftPlan(value,from(),to(),Array.from({length:33},(_,i)=>identity(`s${i}`))).error,/32 transforms/);
});

test('a requested Boolean OR remains selected for a numeric source and its invalid route stays isolated',()=>{
 const value=fresh(),before=plain(value),node=createRouteTransform(value,state(number),state(number),'any');
 assert.equal(node.operation.kind,'any');assert.equal(node.draft,undefined);
 assert.equal(node.inputs.length,2);assert.equal(node.inputs[0].contract.schema.type,'boolean');
 const descriptor={...sourcePort.descriptor,id:'native:axis',name:'Axis',contract:state(number)};
 const numericFrom={nodeId:'source',port:{...descriptor,id:'@observed:axis',key:'@observed:axis',profilePortId:null,sourceId:'native',observed:{...sourcePort,descriptor},current:true}};
 const plan=connectionDraftPlan(value,numericFrom,to(),[node]);
 assert.match(plan.error,/compatible|Types|contract/i);assert.deepEqual(plain(plan.nodes),[]);
 assert.deepEqual(plain(value),before);assert.equal(plan.draft.document.resources[0].ports.length,0);assert.deepEqual(plain(plan.draft.assignments[0].ports),[]);
});

test('a multi-input OR route names its missing second input and does not save its provider pin',()=>{
 const value=fresh(),before=plain(value),node=createRouteTransform(value,state(),state(),'any');
 assert.equal(node.operation.kind,'any');
 const plan=connectionDraftPlan(value,from(),to(),[node]);
 assert.match(plan.error,/"second" input/);assert.deepEqual(plain(plan.nodes),[]);
 assert.deepEqual(plain(value),before);assert.equal(plan.draft.document.resources[0].ports.length,0);assert.deepEqual(plain(plan.draft.assignments[0].ports),[]);
});

test('a two-button OR route previews and commits both provider pins and input wires atomically',()=>{
 const value=fresh(),before=plain(value),node=createRouteTransform(value,state(),state(),'any');
 const button=(id,contract=state())=>{
  const descriptor={id,name:id,direction:'output',contract};
  const observed={descriptor,contractHash:`sha256:${id==='native:left'?'5':'6'.repeat(1)}${'a'.repeat(63)}`,routeIds:['usb'],current:true};
  return{nodeId:'source',port:{...descriptor,id:`@observed:${id}`,key:`@observed:${id}`,profilePortId:null,sourceId:'native',observed,current:true}};
 };
 const left=button('native:left'),right=button('native:right'),extraInputs={[`${node.id}/second`]:right};
 const plan=connectionDraftPlan(value,left,to(),[node],undefined,'',extraInputs);
 assert.equal(plan.error,'');assert.equal(plan.nodes.length,1);assert.equal(plan.draft.assignments[0].ports.length,2);assert.equal(plan.draft.document.bindings.length,3);
 const committed=commitConnectionDraft(value,left,to(),[node],undefined,false,'',extraInputs);
 assert.equal(committed.assignments[0].ports.length,2);assert.equal(committed.document.bindings.length,3);
 assert.deepEqual(plain(committed.document.bindings.map(binding=>binding.to.portId)),['first','second','input']);
 for(const binding of committed.document.bindings)assert.equal(graph.graphConnectionError(committed.document,binding.from,binding.to,binding.id),'');
 assert.deepEqual(plain(value),before);
});

test('a direct route into Gate.value does not require wiring Gate.gate in the same path',()=>{
 let value=fresh();
 const gate=processingTemplate('gate','gate',state());value=graph.graphSetProcessing(value,gate);
 const valueInput=endpoint('gate',gate.inputs.find(input=>input.id==='value'));
 const plan=connectionDraftPlan(value,from(),valueInput,[]);
 assert.equal(plan.error,'');assert.equal(plan.draft.document.bindings.length,1);
 assert.equal(plan.draft.document.bindings[0].to.portId,'value');
 assert.deepEqual(plain(value.document.bindings),[]);
});

test('a missing or type-invalid extra input leaves every provider pin unmaterialized',()=>{
 const value=fresh(),before=plain(value),node=createRouteTransform(value,state(),state(),'any');
 const descriptor={...sourcePort.descriptor,id:'native:axis',name:'Axis',contract:state(number)};
 const observed={...sourcePort,descriptor,contractHash:`sha256:${'7'.repeat(64)}`};
 const numeric={nodeId:'source',port:{...descriptor,id:'@observed:axis',key:'@observed:axis',profilePortId:null,sourceId:'native',observed,current:true}};
 const missing=connectionDraftPlan(value,from(),to(),[node]);
 const wrong=connectionDraftPlan(value,from(),to(),[node],undefined,'',{[`${node.id}/second`]:numeric});
 assert.match(missing.error,/"second" input/);assert.match(wrong.error,/Types|range|contract|conversion/i);
 for(const plan of [missing,wrong]){
  assert.deepEqual(plain(plan.nodes),[]);assert.equal(plan.draft.document.resources[0].ports.length,0);assert.deepEqual(plain(plan.draft.assignments[0].ports),[]);
 }
 assert.deepEqual(plain(value),before);
});

test('an output contract mismatch rejects the route without changing provider pins or edges',()=>{
 const value=fresh(),node=createRouteTransform(value,state(),state(number),'identity');
 value.document.resources.find(resource=>resource.id==='target').ports[0].contract=state(number);
 const before=plain(value),numericTarget=endpoint('target',value.document.resources.find(resource=>resource.id==='target').ports[0]);
 const plan=connectionDraftPlan(value,from(),numericTarget,[node]);
 assert.match(plan.error,/Types|range|contract|conversion/i);assert.deepEqual(plain(plan.nodes),[]);
 assert.deepEqual(plain(value),before);assert.equal(plan.draft.document.resources[0].ports.length,0);assert.deepEqual(plain(plan.draft.assignments[0].ports),[]);
});

test('vehicle route drafts preserve another aircraft writer and scope every added stage',()=>{
 let value=fresh();value.document.eligibility.gameIds=['dcs'];
 value.document.resources.push({id:'viper',name:'Viper',kind:'game',required:false,ports:[port('out','output')]});
 value.document.vehicleTabs=[{id:'hornet',gameId:'dcs',vehicleId:'hornet',vehicleName:'Hornet',nodeIds:['source']},{id:'viper-tab',gameId:'dcs',vehicleId:'viper',vehicleName:'Viper',nodeIds:['viper']}];
 value=commitConnectionDraft(value,from(),to(),[identity('hornet-stage')],undefined,false,'hornet');
 const prior=plain(value.document.bindings),viper=endpoint('viper',port('out','output'));
 const plan=connectionDraftPlan(value,viper,to(),[identity('viper-stage')],undefined,'viper-tab');
 assert.equal(plan.error,'');assert.equal(plan.writer,undefined);
 value=commitConnectionDraft(value,viper,to(),[identity('viper-stage')],undefined,false,'viper-tab');
 assert.equal(value.document.bindings.length,4);
 for(const binding of prior)assert.deepEqual(plain(value.document.bindings.find(row=>row.id===binding.id)),binding);
 assert.deepEqual(plain(value.document.vehicleTabs.find(tab=>tab.id==='viper-tab').nodeIds),['viper','viper-stage']);
 const existing=value.document.bindings.find(binding=>binding.from.nodeId==='viper-stage').id;
 const changed=commitConnectionDraft(value,viper,to(),[identity('new-viper-stage')],existing,false,'viper-tab');
 assert.equal(changed.document.processingNodes.some(node=>node.id==='viper-stage'),false);
 assert.equal(changed.document.processingNodes.some(node=>node.id==='hornet-stage'),true);
 for(const binding of prior)assert.deepEqual(plain(changed.document.bindings.find(row=>row.id===binding.id)),binding);
});

test('an auxiliary integration input scopes the entire shared endpoint chain to its vehicle',()=>{
 const value=fresh();value.document.eligibility.gameIds=['dcs'];
 value.document.resources.push({id:'hornet',name:'Hornet',kind:'game',required:false,ports:[port('out','output')]});
 value.document.vehicleTabs=[{id:'hornet-tab',gameId:'dcs',vehicleId:'hornet',vehicleName:'Hornet',nodeIds:['hornet']}];
 const first=identity('first'),merge=createRouteTransform(value,state(),state(),'any',[first]),last=identity('last');
 const extraInputs={[`${merge.id}/second`]:endpoint('hornet',port('out','output'))};
 const plan=connectionDraftPlan(value,from(),to(),[first,merge,last],undefined,'',extraInputs);
 assert.equal(plan.error,'');
 assert.deepEqual(plain(plan.draft.document.vehicleTabs[0].nodeIds),['hornet','first',merge.id,'last']);
 assert.equal(value.document.processingNodes.length,0);
 const committed=commitConnectionDraft(value,from(),to(),[first,merge,last],undefined,false,'',extraInputs);
 assert.deepEqual(plain(committed.document.vehicleTabs[0].nodeIds),['hornet','first',merge.id,'last']);
});

test('mutually exclusive auxiliary inputs or explicit scopes fail as an isolated draft error',()=>{
 const value=fresh();value.document.eligibility.gameIds=['dcs'];
 value.document.resources.push(...['hornet','viper'].map(id=>({id,name:id,kind:'game',required:false,ports:[port('out','output')]})));
 value.document.vehicleTabs=['hornet','viper'].map(id=>({id:`${id}-tab`,gameId:'dcs',vehicleId:id,vehicleName:id,nodeIds:[id]}));
 const merge=createRouteTransform(value,state(),state(),'any');
 const extras={[`${merge.id}/second`]:endpoint('viper',port('out','output'))},before=plain(value);
 const plan=connectionDraftPlan(value,endpoint('hornet',port('out','output')),to(),[merge],undefined,'',extras);
 assert.match(plan.error,/Different vehicles/);assert.deepEqual(plain(plan.draft),before);assert.deepEqual(plain(plan.nodes),[]);
 const explicit=connectionDraftPlan(value,from(),to(),[merge],undefined,'hornet-tab',extras);
 assert.match(explicit.error,/Different vehicles/);assert.deepEqual(plain(value),before);
});
