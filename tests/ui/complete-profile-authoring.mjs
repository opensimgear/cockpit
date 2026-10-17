// Assembled FRONTEND evidence only. The browser authors every graph edit through
// production controls. All IPC is intercepted before navigation; the supplied
// samples are fixture captures, not device/game acknowledgements or runtime IO.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {loadModules} from '../../src/lib/test-module-loader.mjs';

const require=createRequire(import.meta.url),playwrightModule=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(playwrightModule),{expect}=require(playwrightModule+'/test');
const endpoint=process.env.OSG_PROFILES_UI_URL??'http://127.0.0.1:1420';
const artifacts=resolve('.research/complete-profile-authoring-browser');
const blueprint=JSON.parse(await readFile(new URL('../fixtures/profiles/complete-rig.json',import.meta.url),'utf8'));
const blank=JSON.parse(await readFile(new URL('../fixtures/profiles/blank.json',import.meta.url),'utf8'));
const copy=value=>JSON.parse(JSON.stringify(value));
const hash=value=>'sha256:'+createHash('sha256').update(JSON.stringify(value)).digest('hex');
const defaultId='00000000-0000-4000-8000-000000000001',publication=hash('complete-rig-panel'),programHash=hash('fixture-requested-program');
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph']);
const {profileDocumentError}=modules.get('./profiles'),{graphConnectionError}=modules.get('./profile-graph');

// Compare selected ports and graph roles, independent of generated node/port IDs.
function exactRoutes(document,pins=[]){
 const nodes=[...document.resources,...document.processingNodes,...document.virtualOutputs];
 const endpoint=value=>{
  const node=nodes.find(node=>node.id===value.nodeId);assert.ok(node);
  if(node.layout){const index=node.layout.axes.findIndex(axis=>axis.portId===value.portId);return[node.name,index>=0?'axis-'+index:node.layout.buttons.includes(value.portId)?'button-'+node.layout.buttons.indexOf(value.portId):'hat-'+node.layout.hats.indexOf(value.portId)];}
  if(node.operation){const input=node.inputs.findIndex(port=>port.id===value.portId);return[node.name,node.operation.kind==='gate'&&input>=0?value.portId:(input>=0?'input-'+input:'output-'+node.outputs.findIndex(port=>port.id===value.portId))];}
  const pin=pins.find(pin=>pin.resourceId===node.id)?.ports.find(pin=>pin.profilePortId===value.portId);return[node.name,pin?.observedPortId??value.portId];
 };
 return document.bindings.map(edge=>JSON.stringify([endpoint(edge.from),endpoint(edge.to)])).sort();
}

// Only the advertised port catalogue is reused. No saved or draft graph from
// complete-rig.json is injected into the browser or returned by a read endpoint.
function inventory(){
 return blueprint.resources.map(resource=>{
  const resourceType=resource.kind==='companion'?'companion':resource.kind==='game'?'game-integration':resource.id.startsWith('third-party')?'host-controller':'osg-module';
  const transports=resource.id==='direct-usb'?['usb','can']:resourceType==='host-controller'?['hid']:resourceType==='companion'?['network']:resourceType==='game-integration'?['integration']:['can'];
  return {id:'native-'+resource.id,name:resource.name,resourceType,availability:'online',generation:'1',capabilityRevision:hash(resource.ports),portCount:resource.ports.length,cachedPortCount:resource.ports.length,
   connections:transports.map(transport=>({scope:'fixture-'+transport,key:resource.id,route:{id:transport+'-'+resource.id,transport,providerGeneration:'boot-1',attributes:{physicalUid:resource.id,...(transport==='can'?{gateway:'OSG gateway',canAddress:'7'}:transport==='usb'?{port:'COM-fixture',protocol:'verified OSG console'}:{provider:resourceType})},baselineReady:true,writable:true,priority:transport==='usb'?100:10}})),
   attributes:{physicalIdentity:resource.id,...(resource.kind==='game'?{gameId:'dcs'}:{})},diagnostics:[]};
 });
}

test('author complete rig through exact-port UI, Save/reopen/Undo, and inspect applied revision through stale/lost captures',{timeout:180000},async()=>{
 await mkdir(artifacts,{recursive:true});
 const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
 const page=await browser.newPage({viewport:{width:1600,height:1100}}),requests=[],unexpected=[],pageErrors=[];
 const defaults={...copy(blank),id:defaultId,name:'Default'},documents=new Map([[defaultId,defaults]]),assignments=new Map([[defaultId,[]]]),resources=inventory();
 let appliedDocument=null,captureMode='known',clock=1000,inventoryRevision=1,saveCount=0;
 let profileSession={state:'stopped',paused:false,appliedSnapshot:null,owner:null,degradation:[],fault:null};
 const session=()=>({selectedProfileId:defaultId,selectionMode:'manual',state:profileSession.state,appliedSnapshot:profileSession.appliedSnapshot,fault:null});
 const automatic=()=>({schemaVersion:1,mode:'manual',manualProfileId:defaultId,rules:[],status:'manual',message:'Fixture Manual pin; editor operations do not select a run.',conflicts:[],lastAttempt:null,target:null,readOnly:false});
 const listing=()=>({profiles:[...documents.values()].map(document=>({id:document.id,name:document.name,revision:document.revision,isDefault:document.id===defaultId,semanticHash:hash(document)})),session:session(),profileSession:copy(profileSession),automatic:automatic(),readOnly:false,diagnostics:[]});
 const header=()=>({schemaVersion:1,owner:copy(profileSession.owner),appliedSnapshot:copy(profileSession.appliedSnapshot),programHash,capturedUs:String(++clock)});
 const nodePorts=node=>node.ports??[...node.inputs,...node.outputs];
 function sample(node,port){
  if(captureMode!=='known')return{status:captureMode};
  const contract=port.contract;
  if(contract.delivery==='action'||contract.delivery==='invocation')return{status:'known',value:{kind:'action-pack',value:{arguments:contract.parameters.map(argument=>({id:argument.id,value:argument.schema.type==='boolean'?true:0}))}}};
  const schema=contract.schema;
  const value=schema.type==='boolean'?true:schema.type==='text'?'ALT 3048.0 m':schema.type==='enum'?schema.values[0]:schema.type==='rgb'?'000000':schema.unit==='ft'?10000:schema.unit==='m'?3048:12345;
  return{status:'known',value:{kind:'scalar',value}};
 }
 page.on('pageerror',error=>pageErrors.push(error.message));
 await page.route('**/api/gateway/**',async route=>{unexpected.push('HTTP fallback');await route.abort();});
 await page.exposeFunction('__osgCompleteRigFixture',async(command,args)=>{
  if(command!=='gateway_request'){unexpected.push(command);throw Error('Unexpected IPC '+command);}
  const request=copy(args.request);requests.push(request);const offset=request.offset??0,limit=request.limit??32;
  if(['profile_list','profile_status','profile_automatic_status'].includes(request.command))return{ok:true,state:{profiles:listing()}};
  if(['profile_get','profile_assignments_get'].includes(request.command))return{ok:true,state:{profiles:{...listing(),document:copy(documents.get(request.id)),assignments:copy(assignments.get(request.id))}}};
  if(request.command==='profile_resources_list')return{ok:true,state:{profileResources:{session:'complete-rig-inventory',revision:String(inventoryRevision),offset,total:resources.length,resources:copy(resources.slice(offset,offset+limit)),inventoryDiagnostics:[],readOnly:false}}};
  if(request.command==='profile_resources_inspect'){
   const resource=resources.find(resource=>resource.id===request.resourceId),source=blueprint.resources.find(source=>'native-'+source.id===request.resourceId);
   assert.ok(resource&&source,'Only an advertised native resource can be inspected');
   const ports=source.ports.map(descriptor=>({descriptor,contractHash:hash(descriptor.contract),routeIds:resource.connections.map(connection=>connection.route.id),current:resource.availability==='online'}));
   return{ok:true,state:{profileResources:{session:'complete-rig-inventory',revision:String(inventoryRevision),resource:copy(resource),evidence:[],offset,total:ports.length,ports:copy(ports.slice(offset,offset+limit)),inventoryDiagnostics:[],readOnly:false}}};
  }
  if(request.command==='profile_companion_catalogue')return{ok:true,state:{profileCompanionAuthoring:{schemaVersion:1,revision:hash('retained-library'),panels:[{publicationHash:publication,capabilityFingerprint:hash('real-control-and-widgets'),title:'Complete rig panel',controlCount:1,assetCount:0}],diagnostics:[]}}};
  if(request.command==='profile_companion_inspect'){
   assert.equal(request.resourceId,'native-companion-panel');assert.equal(request.publicationHash,publication);
   const ports=blueprint.resources.find(resource=>resource.kind==='companion').ports.map(descriptor=>({descriptor,contractHash:hash(descriptor.contract),routeIds:[],current:false}));
   return{ok:true,state:{profileCompanionAuthoring:{schemaVersion:1,resourceId:request.resourceId,publicationHash:publication,capabilityFingerprint:hash('real-control-and-widgets'),identityFingerprint:hash('paired-instance'),offset,total:ports.length,ports:copy(ports.slice(offset,offset+limit)),diagnostics:[]}}};
  }
  if(request.command==='profile_editor_save'){
   const previous=documents.get(request.document.id);assert.equal(request.expectedRevision,previous?.revision??null);
   assert.notEqual(request.document.id,defaultId,'This test creates an independent named profile');
   const document={...request.document,revision:previous?previous.revision+1:1};saveCount++;documents.set(document.id,copy(document));assignments.set(document.id,copy(request.assignments));
   return{ok:true,state:{profiles:{...listing(),document:copy(document),assignments:copy(request.assignments)}}};
  }
  if(request.command==='profile_apply'){
   assert.equal(saveCount,1);assert.equal(request.expectedRevision,documents.get(request.id)?.revision);assert.equal(request.gameId,'dcs');
   appliedDocument=copy(documents.get(request.id));const appliedSnapshot={profileId:request.id,revision:appliedDocument.revision,semanticHash:hash(appliedDocument)};
   profileSession={state:'running',paused:false,appliedSnapshot,owner:{session:'44444444-4444-4444-8444-444444444444',generation:1,profileId:request.id,profileRevision:appliedDocument.revision,gameId:'dcs'},degradation:[],fault:null};
   return{ok:true,state:{profiles:listing()}};
  }
  if(request.command==='profile_runtime_topology'){
   assert.deepEqual(request.owner,profileSession.owner);
   const nodes=[...appliedDocument.resources.map(node=>({id:node.id,name:node.name,kind:node.kind,summary:'Declared '+node.kind+' resource'})),...appliedDocument.processingNodes.map(node=>({id:node.id,name:node.name,kind:'processing',summary:node.operation.kind==='unit-convert'?'ft → m · none':node.operation.kind})),...appliedDocument.virtualOutputs.map(node=>({id:node.id,name:node.name,kind:'virtual',summary:'Requested logical virtual controller'}))];
   const items=request.section==='nodes'?nodes:appliedDocument.bindings;
   return{ok:true,state:{profileTopology:{...header(),section:request.section,offset,total:items.length,items:copy(items.slice(offset,offset+limit))}}};
  }
  if(request.command==='profile_runtime_inspect'){
   assert.deepEqual(request.owner,profileSession.owner);
   const nodes=[...appliedDocument.resources,...appliedDocument.processingNodes,...appliedDocument.virtualOutputs];
   const ports=nodes.filter(node=>request.nodeId===undefined||node.id===request.nodeId).flatMap(node=>nodePorts(node).map(port=>({endpoint:{nodeId:node.id,portId:port.id},direction:port.direction,contract:port.contract,sample:sample(node,port),observedUs:port.contract.delivery==='event'&&captureMode==='unavailable'?null:String(clock)})));
   return{ok:true,state:{profileInspection:{...header(),offset,total:ports.length,ports:copy(ports.slice(offset,offset+limit))}}};
  }
  if(request.command==='profile_retired_recovery_status')return{ok:true,state:{retiredRecovery:{required:false,outputs:[],provisioning:[],job:null,diagnostics:[],outputQualified:false,provisioningQualified:false}}};
  if(['ports','companion_status','game_get_state'].includes(request.command))return{ok:true,state:{running:false,needsReconnect:false,modules:[],ports:[],games:{catalogue:[],cleanup:{phase:'idle',error:null}},companions:{host:{instances:[],publications:[],clients:[]}}}};
  unexpected.push(request.command);throw Error('Unexpected complete-rig effect '+request.command);
 });
 await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__osgCompleteRigFixture(command,args)};});

 const inspector=page.getByRole('complementary',{name:'Selected block inspector'}),composer=page.getByRole('region',{name:'Exact connection editor'}),bindings=page.getByRole('region',{name:'Exact bindings table'});
 async function chooseBlock(name){await page.getByLabel('Search profile blocks').fill(name);await page.getByLabel('Search profile blocks').press('Enter');await expect(inspector.getByRole('heading',{name,exact:true})).toBeVisible();}
 async function selectProvider(name,ports){await chooseBlock(name);for(const name of ports){const row=inspector.locator('.port-row').filter({has:page.getByText(name,{exact:true})}).filter({has:page.getByRole('button',{name:'Use this exact port',exact:true})});await row.getByRole('button',{name:'Use this exact port',exact:true}).click();}}
 async function option(select,label){const candidates=await select.locator('option').evaluateAll(options=>options.map(option=>({label:option.textContent,value:option.value})));const choice=candidates.find(option=>option.label===label||option.value===label)??candidates.find(option=>option.label.startsWith(label+' ·'));assert.ok(choice,'Missing select option '+label+' in '+JSON.stringify(candidates));await select.selectOption(choice.value);}
 async function route(from,fromPort,to,toPort,commit=true){if(!await composer.isVisible())await page.getByRole('button',{name:'Connect exact ports',exact:true}).click();await option(composer.getByLabel('Source block'),from);await option(composer.getByLabel('Exact source output'),fromPort);await option(composer.getByLabel('Destination block'),to);await option(composer.getByLabel('Exact destination input / action'),toPort);if(commit){await expect(composer.getByRole('button',{name:'Create exact route',exact:true})).toBeEnabled();await composer.getByRole('button',{name:'Create exact route',exact:true}).click();}}
 async function renameProcessing(name,fields={}){const editor=page.getByRole('region',{name:'Visible transformation editor'});await editor.getByLabel('Block label',{exact:true}).fill(name);for(const [label,value]of Object.entries(fields))await editor.getByLabel(label,{exact:true}).fill(String(value));await expect(editor.getByRole('button',{name:'Update visible node',exact:true})).toBeEnabled();await editor.getByRole('button',{name:'Update visible node',exact:true}).click();}
 async function transform(preset,name,fields={}){await page.getByRole('combobox',{name:/^Visible transformation/}).selectOption(preset);await page.getByRole('button',{name:'Add transformation',exact:true}).click();await renameProcessing(name,fields);}
 async function virtual(name,buttons,axes){await page.getByRole('button',{name:'Add virtual controller',exact:true}).click();const editor=page.getByRole('region',{name:'Virtual controller layout editor'});await editor.getByLabel('Controller label').fill(name);for(const [label,value]of Object.entries({buttons,axes,hats:0})){await editor.getByLabel(label,{exact:true}).fill(String(value));await editor.getByLabel(label,{exact:true}).press('Tab');}for(const select of await editor.getByLabel('Native range').all())await select.selectOption('unipolar');await editor.getByRole('button',{name:'Update controller layout',exact:true}).click();}
 try{
  await page.goto(endpoint);await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Profiles',exact:true}).click();await expect(page.getByRole('button',{name:'Edit Default',exact:true})).toBeVisible();await page.getByRole('button',{name:'Add profile',exact:true}).click();await page.getByLabel('Profile name',{exact:true}).fill('Complete browser rig');
  await expect(page.locator('.resource-block')).toHaveCount(9);await expect(page.locator('path.signal-edge')).toHaveCount(0);
  await chooseBlock('OSG direct USB device');await inspector.getByText('Connection metadata (2)',{exact:true}).click();await expect(inspector.locator('.connection-row').getByText('USB',{exact:true})).toBeVisible();await expect(inspector.locator('.connection-row').getByText('CAN',{exact:true})).toBeVisible();await expect(inspector.getByText('COM-fixture',{exact:true})).toBeVisible();await expect(inspector.getByText('OSG gateway',{exact:true})).toBeVisible();await expect(page.locator('path.signal-edge')).toHaveCount(0);
  for(const resource of blueprint.resources.filter(resource=>resource.kind!=='companion'&&resource.ports.length))await selectProvider(resource.name,resource.ports.map(port=>port.name));
  await chooseBlock('Hidden third party C');await inspector.getByText('Resource assignment and policy',{exact:true}).click();await inspector.getByLabel('Game visibility intent').selectOption('hidden-while-active');
  await chooseBlock('Companion panel');const panel=page.getByRole('region',{name:'Profile companion panel selection'});await panel.getByRole('button',{name:'Browse retained panels',exact:true}).click();await panel.getByLabel('Retained immutable panel').selectOption(publication);await panel.getByRole('button',{name:'Capture panel revision in draft',exact:true}).click();await panel.getByRole('button',{name:'Inspect exact panel ports',exact:true}).click();for(const name of ['Altitude warning widget','Conditional altitude text widget'])await panel.locator('.panel-ports li').filter({has:page.getByText(name,{exact:true})}).getByRole('button',{name:'Add feedback input',exact:true}).click();await panel.getByLabel('Port group').selectOption('output');await panel.getByRole('button',{name:'Add control output',exact:true}).click();
  await virtual('Combined third party controller',1,2);await virtual('Combined OSG controller',0,3);
  for(const [from,port,to,target]of [['Third party A','Steering','Combined third party controller','axis-1'],['Third party B','Throttle','Combined third party controller','axis-2'],['OSG CAN device A','CAN axis A','Combined OSG controller','axis-1'],['OSG CAN device B','CAN axis B','Combined OSG controller','axis-2'],['OSG direct USB device','Direct USB axis','Combined OSG controller','axis-3'],['Companion panel','Panel switch','Combined third party controller','button-1']])await route(from,port,to,target);
  await route('Separate OSG command module','Command button','DCS game data and commands','Master caution command',false);await expect(composer.getByRole('button',{name:'Create exact route',exact:true})).toBeDisabled();await expect(composer.getByRole('status')).toContainText('complete invocation');await composer.getByRole('button',{name:'Add event-triggered argument pack',exact:true}).click();await renameProcessing('Explicit command trigger');await route('Separate OSG command module','Command button','Explicit command trigger','fire');await route('Explicit command trigger','invoke','DCS game data and commands','Master caution command');
  await route('DCS game data and commands','Altitude feet','Companion panel','Conditional altitude text widget',false);await transform('unit-convert','Feet to metres');
  await route('DCS game data and commands','Altitude feet','Feet to metres','in');await route('Feet to metres','out','Companion panel','Altitude warning widget',false);await transform('hysteresis','Altitude threshold',{Press:3000,Release:2900});
  await route('Feet to metres','out','Altitude threshold','in');await route('Altitude threshold','out','OSG CAN device A','Module indicator action',false);await composer.getByRole('button',{name:'Add setter on argument change',exact:true}).click();await renameProcessing('Explicit warning setter');await route('Altitude threshold','out','Explicit warning setter','value');await route('Explicit warning setter','invoke','OSG CAN device A','Module indicator action');await route('Altitude threshold','out','Companion panel','Altitude warning widget');
  await route('Feet to metres','out','Companion panel','Conditional altitude text widget',false);await transform('gate','Only show altitude while warning is active');await route('Feet to metres','out','Only show altitude while warning is active','value');await route('Altitude threshold','out','Only show altitude while warning is active','gate');
  await route('Only show altitude while warning is active','out','Companion panel','Conditional altitude text widget',false);await transform('format','Format altitude in metres',{Decimals:1,Prefix:'ALT ',Suffix:' m'});await route('Only show altitude while warning is active','out','Format altitude in metres','in');await route('Format altitude in metres','out','Companion panel','Conditional altitude text widget');
  await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 17');await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 16');await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 17');
  await page.getByRole('button',{name:'Auto-layout',exact:true}).click();await page.getByRole('button',{name:'Fit all',exact:true}).click();await page.getByLabel('Search profile blocks').fill('');
  await page.getByRole('button',{name:'Save profile',exact:true}).click();await expect(page.getByText('Saved revision 1 · Editing this profile does not activate it.',{exact:true})).toBeVisible();assert.equal(profileSession.owner,null);assert.equal(saveCount,1);
  const authored=[...documents.values()].find(document=>document.id!==defaultId);assert.ok(authored);assert.equal(authored.resources.length,9);assert.equal(authored.processingNodes.length,6);assert.equal(authored.virtualOutputs.length,2);assert.equal(authored.bindings.length,17);assert.equal(authored.visibility.filter(intent=>intent.mode==='hidden-while-active').length,1);assert.equal(authored.companionAssignments[0].publicationHash,publication);assert.deepEqual(documents.get(defaultId),defaults);
  assert.equal(profileDocumentError(authored),'');for(const edge of authored.bindings)assert.equal(graphConnectionError(authored,edge.from,edge.to,edge.id),'',edge.id);
  assert.deepEqual(exactRoutes(authored,assignments.get(authored.id)),exactRoutes(blueprint));for(const node of blueprint.processingNodes)assert.deepEqual(authored.processingNodes.find(other=>other.name===node.name).operation,node.operation);
  assert.equal(authored.editor.positions.length,17);assert.ok(!JSON.stringify(authored).includes('native-'));assert.ok(!JSON.stringify(authored).includes('COM-fixture'));assert.equal(assignments.get(authored.id).length,9);assert.ok(assignments.get(authored.id).some(pin=>pin.sourceId==='native-direct-usb'));
  await writeFile(resolve(artifacts,'authored-profile.json'),JSON.stringify(authored,null,2)+'\n');await writeFile(resolve(artifacts,'authored-local-assignments.json'),JSON.stringify(assignments.get(authored.id),null,2)+'\n');
  await expect(page.locator('.resource-block.dimmed')).toHaveCount(0);await page.getByRole('application',{name:'Profile canvas'}).screenshot({path:resolve(artifacts,'authored-complete-canvas.png'),animations:'disabled'});
  await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();await page.getByRole('button',{name:'Edit Complete browser rig',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 17');await expect(page.locator('.resource-block.processing')).toHaveCount(6);await expect(page.locator('.resource-block.virtual')).toHaveCount(2);assert.equal(saveCount,1);
  await page.getByText('Apply, Pause and applied runtime inspection',{exact:true}).click();const runtime=page.getByRole('region',{name:'Native profile runtime'});await runtime.getByLabel('Game context for Apply').selectOption('dcs');await expect(runtime.getByRole('button',{name:'Apply saved revision 1',exact:true})).toBeEnabled();await runtime.getByRole('button',{name:'Apply saved revision 1',exact:true}).click();await expect(runtime.getByText('Generation 1 · dcs',{exact:true})).toBeVisible();
  // The captured run is revision 1. A later unsaved label edit is only a draft.
  await chooseBlock('Feet to metres');await renameProcessing('Draft-only conversion label');assert.equal(appliedDocument.processingNodes.find(node=>node.operation.kind==='unit-convert').name,'Feet to metres');await expect(runtime.getByRole('button',{name:'Apply saved revision 1',exact:true})).toBeDisabled();
  const activity=page.getByRole('region',{name:'Applied source to destination activity'});await expect(activity.getByRole('combobox',{name:/^Applied connection/})).toBeVisible();await activity.getByLabel('Follow selected applied block',{exact:false}).uncheck();const altitudeId=appliedDocument.resources.find(resource=>resource.name==='DCS game data and commands').id,altitudeEdge=appliedDocument.bindings.find(edge=>edge.from.nodeId===altitudeId&&edge.from.portId===appliedDocument.resources.find(resource=>resource.id===altitudeId).ports.find(port=>port.name==='Altitude feet').id);await activity.getByRole('combobox',{name:/^Applied connection/}).selectOption(altitudeEdge.id);await expect(activity.getByText('Path samples match this exact applied owner.',{exact:false})).toBeVisible();await expect(activity.getByText('Applied revision 1 · run 1.',{exact:false})).toBeVisible();await expect(activity.getByRole('article',{name:'source DCS game data and commands',exact:true})).toContainText('10000');await expect(activity.getByRole('article',{name:'processing Feet to metres',exact:true})).toContainText('3048');await expect(activity.getByRole('article',{name:'processing Format altitude in metres',exact:true})).toContainText('ALT 3048.0 m');await expect(activity.getByRole('article',{name:'destination Companion panel',exact:true})).toContainText('Upstream/requested');await expect(activity.getByRole('article',{name:'processing Draft-only conversion label',exact:true})).toHaveCount(0);
  await page.screenshot({path:resolve(artifacts,'applied-path-with-newer-draft.png'),fullPage:true});
  captureMode='stale';await expect(activity.getByRole('button',{name:'Refresh path',exact:true})).toBeEnabled();await activity.getByRole('button',{name:'Refresh path',exact:true}).click();await expect(activity.getByRole('article',{name:'source DCS game data and commands',exact:true})).toContainText('stale');await expect(activity.getByRole('article',{name:'source DCS game data and commands',exact:true}).locator('.sample-value')).toHaveText('stale');
  captureMode='unavailable';resources.find(resource=>resource.id==='native-direct-usb').availability='offline';inventoryRevision++;await activity.getByRole('button',{name:'Refresh path',exact:true}).click();await expect(activity.getByRole('article',{name:'destination Companion panel',exact:true})).toContainText('unavailable');await expect(activity.getByRole('article',{name:'processing Feet to metres',exact:true}).locator('.sample-value')).toHaveText(['unavailable','unavailable']);
  await activity.screenshot({path:resolve(artifacts,'unavailable-applied-path.png'),animations:'disabled'});
  await page.getByRole('button',{name:'Add from inventory',exact:true}).click();await page.getByRole('button',{name:'Refresh inventory',exact:true}).click();await chooseBlock('OSG direct USB device');await expect(inspector.getByText('Offline · saved block retained',{exact:true})).toBeVisible();await expect(page.locator('.resource-block')).toHaveCount(17);await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 17');
  const appliedOwner=copy(profileSession.owner);await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();await page.getByRole('alertdialog').getByRole('button',{name:'Discard and continue',exact:true}).click();await page.getByRole('button',{name:'Add profile',exact:true}).click();await page.getByLabel('Copy source profile',{exact:true}).selectOption(authored.id);await page.getByRole('button',{name:'Use copy',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 17');await expect(page.locator('.resource-block .block-state').filter({hasText:/^Unassigned$/})).toHaveCount(9);await page.getByRole('button',{name:'Cancel creation',exact:true}).click();await expect(page.getByRole('button',{name:'Edit Complete browser rig',exact:true})).toBeVisible();assert.deepEqual(profileSession.owner,appliedOwner);assert.equal(documents.size,2);
  assert.equal(requests.filter(request=>request.command==='profile_apply').length,1);assert.equal(saveCount,1);assert.equal(profileSession.owner.profileRevision,1);assert.equal(appliedDocument.revision,1);assert.deepEqual(documents.get(defaultId),defaults);assert.deepEqual(unexpected,[]);assert.deepEqual(pageErrors,[]);
  await writeFile(resolve(artifacts,'ipc-commands.json'),JSON.stringify(requests.map(request=>request.command),null,2)+'\n');await page.screenshot({path:resolve(artifacts,'saved-source-after-copy-cancel.png'),fullPage:true});
 }catch(error){await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true});await writeFile(resolve(artifacts,'failure-ipc.json'),JSON.stringify({requests,unexpected,pageErrors},null,2));throw error;}finally{await browser.close();}
});
