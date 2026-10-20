import assert from 'node:assert/strict';import test from 'node:test';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph','./profile-companion-authoring']),api=modules.get('./profile-companion-authoring'),graph=modules.get('./profile-graph');
const plain=value=>JSON.parse(JSON.stringify(value)),hash=n=>`sha256:${String(n).repeat(64)}`,panel=(n=1)=>({publicationHash:hash(n),capabilityFingerprint:hash(2),title:'Actual immutable panel',controlCount:1,assetCount:0}),state={delivery:'state',schema:{type:'boolean'},freshnessMs:1500,fallback:null};
const observed=(id='feedback:indicator:active',direction='input')=>({descriptor:{id,name:'Declared indicator',direction,contract:plain(state)},contractHash:hash(3),routeIds:[],current:false});
const page=(offset=0,total=1,ports=[observed()])=>({schemaVersion:1,resourceId:'opaque-local-instance',publicationHash:hash(1),capabilityFingerprint:hash(2),identityFingerprint:hash(4),offset,total,ports,diagnostics:[]});
const catalogue=()=>({schemaVersion:1,revision:hash(5),panels:[panel()],diagnostics:[]});
const draft=()=>({document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Feedback profile',revision:1,eligibility:{gameIds:[]},resources:[{id:'screen',name:'Companion',kind:'companion',required:false,ports:[]},{id:'telemetry',name:'Condition',kind:'game',required:false,ports:[{id:'condition',name:'Authored condition',direction:'output',contract:plain(state)}]}],bindings:[],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[{resourceId:'screen',sourceId:'opaque-local-instance',ports:[]}]});
test('read-only client captures bounded real panel metadata and prospective ports without activation commands',async()=>{
  const calls=[],result=catalogue(),nativePage=page(),client=new api.ProfileCompanionAuthoringClient(async request=>{calls.push(plain(request));return request.command==='profile_companion_catalogue'?result:nativePage;});
  const retained=await client.catalogue(),ports=await client.inspect('opaque-local-instance',hash(1));result.panels[0].title='Later native object edit';nativePage.ports[0].descriptor.name='Later edit';
  assert.equal(retained.panels[0].title,'Actual immutable panel');assert.equal(ports.ports[0].descriptor.name,'Declared indicator');assert.ok(Object.isFrozen(ports.ports[0].descriptor.contract));
  assert.deepEqual(calls,[{command:'profile_companion_catalogue'},{command:'profile_companion_inspect',resourceId:'opaque-local-instance',publicationHash:hash(1),offset:0,limit:32}]);
});
test('invalid requests, fake zero-control panels and malformed replies fail without silent coercion',async()=>{
  let calls=0;const client=new api.ProfileCompanionAuthoringClient(async()=>{calls++;return catalogue();});
  for(const args of [['',hash(1)],['x','bad'],['x',hash(1),-1],['x',hash(1),0,65],['x',hash(1),NaN],['x',hash(1),0,0]])await assert.rejects(client.inspect(...args));assert.equal(calls,0);
  for(const alter of [value=>value.schemaVersion=2,value=>value.panels[0].controlCount=0,value=>value.panels[0].controlCount=129,value=>value.panels.push(panel()),value=>value.token='forged',value=>value.revision=['manual'],value=>value.panels[0].title='bad\nname']){
    const value=catalogue();alter(value);await assert.rejects(new api.ProfileCompanionAuthoringClient(async()=>value).catalogue());
  }
  for(const alter of [value=>value.resourceId='another-instance',value=>value.publicationHash=hash(2),value=>value.offset=1,value=>value.total=0,value=>value.ports=[],value=>value.ports[0].current=true,value=>value.ports[0].routeIds=['fake-live-route'],value=>value.ports[0].descriptor.contract.freshnessMs=NaN,value=>value.ports[0].contractHash='bad',value=>value.ports.push(observed())]){
    const value=page();alter(value);await assert.rejects(new api.ProfileCompanionAuthoringClient(async()=>value).inspect('opaque-local-instance',hash(1)));
  }
});
test('paged prospective browser follows actual lengths, preserves page on changed pairing and fences late replies',async()=>{
  const calls=[],browser=new api.ProfileCompanionPortBrowser({inspect:async(resource,publication,offset)=>{calls.push(offset);return page(offset,3,[observed(`port-${offset}`)]);}});
  await browser.open('opaque-local-instance',hash(1));assert.equal(browser.hasNext,true);assert.equal(browser.hasPrevious,false);await browser.next();await browser.next();assert.equal(browser.hasNext,false);await browser.next();await browser.previous();assert.equal(browser.page.offset,1);assert.deepEqual(calls,[0,1,2,1]);
  const changing=new api.ProfileCompanionPortBrowser({inspect:async(_r,_h,offset)=>({...page(offset,3,[observed(`p-${offset}`)]),identityFingerprint:hash(offset?6:4)})});await changing.open('opaque-local-instance',hash(1));await assert.rejects(changing.next(),/paired companion/);assert.equal(changing.page.offset,0);
  let release;const pending=new api.ProfileCompanionPortBrowser({inspect:()=>new Promise(resolve=>release=resolve)});const opening=pending.open('opaque-local-instance',hash(1));pending.clear();release(page());assert.equal(await opening,null);assert.equal(pending.page,null);assert.equal(await pending.next(),null);assert.equal(await pending.previous(),null);
});
test('explicit real panel capture adds no controls or routes; feedback-only graph remains possible',()=>{
  const original=draft();let next=api.graphCaptureCompanionPanel(original,'screen',panel());assert.equal(next.document.resources[0].ports.length,0);assert.equal(next.document.bindings.length,0);assert.equal(next.document.virtualOutputs.length,0);assert.equal(original.document.companionAssignments.length,0);
  next=api.graphSelectCompanionPort(next,'screen',page(),observed());assert.equal(next.document.resources[0].ports.length,1);assert.equal(next.document.resources[0].ports[0].direction,'input');assert.equal(next.document.resources[0].ports.filter(port=>port.direction==='output').length,0);
  next=graph.graphConnect(next,{nodeId:'telemetry',portId:'condition'},{nodeId:'screen',portId:next.document.resources[0].ports[0].id});
  assert.equal(next.document.bindings.length,1);assert.equal(next.assignments[0].ports[0].observedPortId,'feedback:indicator:active');assert.equal(JSON.stringify(next.document).includes('opaque-local-instance'),false);
  const retained=api.graphCaptureCompanionPanel(next,'screen',panel());assert.deepEqual(plain(retained.document.bindings),plain(next.document.bindings));
  const released=api.graphReleaseCompanionPanel(next,'screen');assert.equal(released.document.companionAssignments.length,0);assert.deepEqual(plain(released.document.bindings),plain(next.document.bindings));
});

test('a saved panel keeps its stable identity and exact graph ports when its latest contents change',()=>{
  const first={...panel(),panelId:'123-panel'},latest={...panel(6),panelId:'123-panel',publicationHashes:[hash(1),hash(6)]};
  let value=api.graphCaptureCompanionPanel(draft(),'screen',first);value=api.graphSelectCompanionPort(value,'screen',page(),observed());
  const next=api.graphCaptureCompanionPanel(value,'screen',latest);
  assert.deepEqual(plain(next.document.resources[0].ports),plain(value.document.resources[0].ports));
  assert.deepEqual(plain(next.assignments),plain(value.assignments));
  assert.deepEqual(plain(next.document.companionAssignments),[{resourceId:'screen',panelId:'123-panel',publicationHash:hash(6)}]);
  assert.equal(api.selectedCompanionPanel([latest],{publicationHash:hash(1)}).panelId,'123-panel');
  assert.equal(api.selectedCompanionPanel([latest],{panelId:'123-panel',publicationHash:hash(1)}).publicationHash,hash(6));
  assert.equal(api.selectedCompanionPanel([latest],undefined),undefined);
  assert.deepEqual(plain(api.latestCompanionPanels([first,latest]).map(panel=>panel.publicationHash)),[hash(6)]);
});
test('changing a panel requires explicit port/route removal and never deletes unrelated graph work',()=>{
  let value=api.graphCaptureCompanionPanel(draft(),'screen',panel());value=api.graphSelectCompanionPort(value,'screen',page(),observed());value=graph.graphConnect(value,{nodeId:'telemetry',portId:'condition'},{nodeId:'screen',portId:value.document.resources[0].ports[0].id});
  assert.throws(()=>api.graphCaptureCompanionPanel(value,'screen',panel(6)),/explicit removal/);assert.equal(value.document.bindings.length,1);
  const next=api.graphCaptureCompanionPanel(value,'screen',panel(6),true);assert.equal(next.document.resources[0].ports.length,0);assert.equal(next.assignments[0].ports.length,0);assert.equal(next.document.bindings.length,0);assert.equal(next.document.resources[1].ports.length,1);assert.equal(value.document.resources[0].ports.length,1);
});
test('panel/port edits require exact local source and explicit capture, with no inference from display labels',()=>{
  assert.throws(()=>api.graphSelectCompanionPort(draft(),'screen',page(),observed()),/Capture this exact/);
  const value=api.graphCaptureCompanionPanel(draft(),'screen',panel());
  for(const changed of [{...page(),resourceId:'same-name-other-instance'},{...page(),publicationHash:hash(2)},{...page(),ports:[]}])assert.throws(()=>api.graphSelectCompanionPort(value,'screen',changed,observed()),/Capture this exact/);
  const unassigned=draft();unassigned.assignments=[];assert.throws(()=>api.graphCaptureCompanionPanel(unassigned,'screen',panel()),/Assign this companion/);
  assert.throws(()=>api.graphCaptureCompanionPanel(draft(),'telemetry',panel()),/Assign this companion/);assert.throws(()=>api.graphReleaseCompanionPanel(value,'telemetry'),/existing companion/);
  const duplicate=draft();duplicate.document.resources.push({id:'screen-two',name:'Same label',kind:'companion',required:false,ports:[]});duplicate.assignments.push({resourceId:'screen-two',sourceId:'opaque-local-instance',ports:[]});duplicate.document.companionAssignments.push({resourceId:'screen-two',publicationHash:hash(1)});assert.throws(()=>api.graphCaptureCompanionPanel(duplicate,'screen',panel()),/another captured/);
  const full=draft();for(let index=0;index<8;index++){const id=`other-${index}`;full.document.resources.push({id,name:id,kind:'companion',required:false,ports:[]});full.assignments.push({resourceId:id,sourceId:`different-native-${index}`,ports:[]});full.document.companionAssignments.push({resourceId:id,publicationHash:hash(1)});}assert.throws(()=>api.graphCaptureCompanionPanel(full,'screen',panel()),/at most eight/);assert.equal(full.document.companionAssignments.length,8);
});
test('copied feedback graph reconciles desired B to explicit companion currently on A without installing or inferring ports',()=>{
  let value=api.graphCaptureCompanionPanel(draft(),'screen',panel());value=api.graphSelectCompanionPort(value,'screen',page(),observed());value=graph.graphConnect(value,{nodeId:'telemetry',portId:'condition'},{nodeId:'screen',portId:value.document.resources[0].ports[0].id});value.assignments=[];
  const source={id:'opaque-local-instance',name:'Same panel name',resourceType:'companion',availability:'offline',generation:'1',capabilityRevision:hash(7),portCount:1,cachedPortCount:1,connections:[],attributes:{publicationHash:hash(6)},diagnostics:[]};
  const id=value.document.resources[0].ports[0].id,selections=[{profilePortId:id,observed:observed(),page:page()}];
  const reconciled=api.graphReconcileCompanionPanel(value,'screen',source,panel(),page(),selections);assert.equal(reconciled.assignments[0].sourceId,source.id);assert.deepEqual(plain(reconciled.document.bindings),plain(value.document.bindings));assert.equal(reconciled.document.resources[0].ports.filter(port=>port.direction==='output').length,0);assert.equal(value.assignments.length,0);assert.equal(source.attributes.publicationHash,hash(6));
  assert.throws(()=>api.graphReconcileCompanionPanel(value,'screen',source,panel(),page(),[]),/Map every/);assert.throws(()=>api.graphReconcileCompanionPanel(value,'screen',source,panel(),{...page(),resourceId:'other'},selections),/exact retained/);assert.throws(()=>api.graphReconcileCompanionPanel(value,'screen',{...source,resourceType:'host-controller'},panel(),page(),selections),/exact retained/);
});
test('multi-page reconciliation requires exact observed membership and unchanged native catalogue evidence for every mapping',()=>{
  const first=observed('feedback:first:active'),second=observed('feedback:second:active'),one=page(0,2,[first]),two=page(1,2,[second]);
  let value=api.graphCaptureCompanionPanel(draft(),'screen',panel());value=api.graphSelectCompanionPort(value,'screen',one,first);value=api.graphSelectCompanionPort(value,'screen',two,second);value.assignments=[];
  const source={id:'opaque-local-instance',name:'Explicit companion',resourceType:'companion',availability:'online',generation:'1',capabilityRevision:hash(7),portCount:2,cachedPortCount:2,connections:[],attributes:{publicationHash:hash(6)},diagnostics:[]};
  const selections=value.document.resources[0].ports.map((logical,index)=>({profilePortId:logical.id,observed:index?second:first,page:index?two:one}));
  assert.equal(api.graphReconcileCompanionPanel(value,'screen',source,panel(),two,selections).assignments[0].ports.length,2);
  for(const alter of [selection=>delete selection.page,selection=>selection.observed.descriptor.id='same-shaped-fabricated',selection=>selection.page.publicationHash=hash(6),selection=>selection.page.identityFingerprint=hash(6),selection=>selection.page.capabilityFingerprint=hash(6),selection=>selection.page.resourceId='other-instance',selection=>selection.page.total=3]){
    const changed=plain(selections);alter(changed[0]);assert.throws(()=>api.graphReconcileCompanionPanel(value,'screen',source,panel(),two,changed),/same captured native/);assert.equal(value.assignments.length,0);
  }
});
