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
const artifacts=resolve('.research/complete-profile-authoring-browser-raw-quantity');
const blueprint=JSON.parse(await readFile(new URL('../fixtures/profiles/complete-rig.json',import.meta.url),'utf8'));
const rawAltitudeId='channel:common:CommonData:ALT_MSL_FT:0';
const rawAltitude=blueprint.resources.find(resource=>resource.id==='dcs').ports.find(port=>port.id==='altitude');
const altitudeName=rawAltitude.name;
const quantityName='Interpret MSL altitude',quantity='Reported MSL altitude';
const observedDescriptor=(resource,port)=>resource.id==='dcs'&&port.id==='altitude'?{...port,id:rawAltitudeId}:port;
const blank=JSON.parse(await readFile(new URL('../fixtures/profiles/blank.json',import.meta.url),'utf8'));
const copy=value=>JSON.parse(JSON.stringify(value));
const hash=value=>'sha256:'+createHash('sha256').update(JSON.stringify(value)).digest('hex');
const defaultId='00000000-0000-4000-8000-000000000001',publication=hash('complete-rig-panel'),programHash=hash('fixture-requested-program');
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph','./profile-processing']);
const {profileDocumentError}=modules.get('./profiles'),{graphConnectionError}=modules.get('./profile-graph');
const {processingSummary}=modules.get('./profile-processing');

// Compare selected ports and graph roles, independent of generated node/port IDs.
function exactRoutes(document,pins=[]){
 const nodes=[...document.resources,...document.processingNodes,...document.virtualOutputs];
 const endpoint=value=>{
  const node=nodes.find(node=>node.id===value.nodeId);assert.ok(node);
  if(node.layout){const index=node.layout.axes.findIndex(axis=>axis.portId===value.portId);return[node.name,index>=0?'axis-'+index:node.layout.buttons.includes(value.portId)?'button-'+node.layout.buttons.indexOf(value.portId):'hat-'+node.layout.hats.indexOf(value.portId)];}
  if(node.operation){const input=node.inputs.findIndex(port=>port.id===value.portId);return[node.name,node.operation.kind==='gate'&&input>=0?value.portId:(input>=0?'input-'+input:'output-'+node.outputs.findIndex(port=>port.id===value.portId))];}
  const pin=pins.find(pin=>pin.resourceId===node.id)?.ports.find(pin=>pin.profilePortId===value.portId);return[node.name,pin?.observedPortId??observedDescriptor(node,node.ports.find(port=>port.id===value.portId)).id];
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
   attributes:{physicalIdentity:resource.id,...(resource.id==='can-a'?{controllerPublicationOverride:JSON.stringify({schema:1,epoch:1,revision:1,module:{id:'0123456789abcdef0123456789abcdef',boot:1,configurationGeneration:1,node:1,sourceNode:1,profile:{revision:1,buttons:2,axes:1,hats:0,neutral:[0],axisUsage:[0x30]},publicationSlot:0,configuredEnabled:true,publicationReportId:1,configuredDescriptorIndex:0}})}:{}),...(resource.kind==='game'?{gameId:'dcs',aircraftModule:'CommonData'}:{}),...(resource.kind==='companion'?{componentKind:'paired-companion',companionId:'complete-fixture-companion'}:{})},diagnostics:[]};
 });
}

test('author complete rig through exact-port UI, Save/reopen/Undo, activate and inspect its graph in Debug',{timeout:180000},async()=>{
 assert.deepEqual(rawAltitude.contract,{delivery:'state',schema:{type:'integer',min:0,max:65535,unit:null},freshnessMs:1500,fallback:null});
 await mkdir(artifacts,{recursive:true});
 const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
 const page=await browser.newPage({viewport:{width:1600,height:1100}}),requests=[],unexpected=[],pageErrors=[];
 const defaults={...copy(blank),id:defaultId,name:'Default'},documents=new Map([[defaultId,defaults]]),assignments=new Map([[defaultId,[]]]),resources=inventory();
 let appliedDocument=null,captureMode='known',clock=1000,inventoryRevision=1,saveCount=0;
 let profileSession={state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null};
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
  const value=schema.type==='boolean'?true:schema.type==='text'?'ALT 3048.0 m':schema.type==='enum'?schema.values[0]:schema.type==='rgb'?'000000':node.operation?.kind==='declare-quantity'||node.kind==='game'&&port.name===altitudeName?10000:schema.unit==='ft'?10000:schema.unit==='m'?3048:12345;
  return{status:'known',value:{kind:'scalar',value}};
 }
 page.on('pageerror',error=>pageErrors.push(error.message));
 await page.route('**/api/gateway/**',async route=>{unexpected.push('HTTP fallback');await route.abort();});
 await page.exposeFunction('__osgCompleteRigFixture',async(command,args)=>{
  if(command!=='gateway_request'){unexpected.push(command);throw Error('Unexpected IPC '+command);}
  const request=copy(args.request);requests.push(request);const offset=request.offset??0,limit=request.limit??32;
  if(request.command==='controller_list')return{ok:true,state:{controllers:{session:'complete-rig-controllers',backendStatus:'ready',offset,total:0,limit,sources:[]}}};
  if(request.command==='module_usb_list')return{ok:true,state:{moduleUsb:{offset,total:0,modules:[]}}};
  if(request.command==='system_inventory')return{ok:true,state:{systemInventory:{offset,total:0,nextOffset:null,virtualDevices:[],hiddenDevices:[],controllerAssociations:[],diagnostics:[],visibilityInspected:true}}};
  if(request.command==='profile_desktop_status')return{ok:true,state:{desktop:{enabled:false,qualified:false,state:'disabled',reason:'Fixture observation only.',scope:{kind:'application'}}}};
  if(['profile_list','profile_status','profile_automatic_status'].includes(request.command))return{ok:true,state:{profiles:listing()}};
  if(['profile_get','profile_assignments_get'].includes(request.command))return{ok:true,state:{profiles:{...listing(),document:copy(documents.get(request.id)),assignments:copy(assignments.get(request.id))}}};
  if(request.command==='profile_resources_list')return{ok:true,state:{profileResources:{session:'complete-rig-inventory',revision:String(inventoryRevision),offset,total:resources.length,resources:copy(resources.slice(offset,offset+limit)),inventoryDiagnostics:[],readOnly:false}}};
  if(request.command==='profile_resources_inspect'){
   const resource=resources.find(resource=>resource.id===request.resourceId),source=blueprint.resources.find(source=>'native-'+source.id===request.resourceId);
   assert.ok(resource&&source,'Only an advertised native resource can be inspected');
   const ports=source.ports.map(port=>{const descriptor=observedDescriptor(source,port);return{descriptor,contractHash:hash(descriptor.contract),routeIds:resource.connections.map(connection=>connection.route.id),current:resource.availability==='online'};});
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
   return{ok:true,state:{profiles:{...listing(),document:copy(document),assignments:copy(request.assignments),saveApplication:{state:'inactive',error:null}}}};
  }
  if(request.command==='profile_apply'){
   assert.equal(saveCount,1);assert.equal(request.expectedRevision,documents.get(request.id)?.revision);assert.equal(request.gameId,'dcs');assert.equal(request.expectedActiveOwner,null);
   appliedDocument=copy(documents.get(request.id));const appliedSnapshot={profileId:request.id,revision:appliedDocument.revision,semanticHash:hash(appliedDocument)};
   profileSession={state:'running',appliedSnapshot,owner:{session:'44444444-4444-4444-8444-444444444444',generation:1,profileId:request.id,profileRevision:appliedDocument.revision,gameId:'dcs'},degradation:[],fault:null};
   return{ok:true,state:{profiles:listing()}};
  }
  if(request.command==='profile_runtime_topology'){
   assert.deepEqual(request.owner,profileSession.owner);
   const nodes=[...appliedDocument.resources.map(node=>({id:node.id,name:node.name,kind:node.kind,summary:'Declared '+node.kind+' resource'})),...appliedDocument.processingNodes.map(node=>({id:node.id,name:node.name,kind:'processing',summary:processingSummary(node)})),...appliedDocument.virtualOutputs.map(node=>({id:node.id,name:node.name,kind:'virtual',summary:'Requested logical virtual controller'}))];
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

 const inspector=page.getByRole('complementary',{name:'Selected block inspector'}),composer=page.getByRole('dialog',{name:'Choose connection ports'}),bindings=page.locator('details[aria-label="Exact bindings table"]');
 async function chooseBlock(name){await page.getByLabel('Search profile blocks').fill(name);await page.getByLabel('Search profile blocks').press('Enter');await inspector.hover();await expect(inspector.getByRole('heading',{name,exact:true})).toBeVisible();}
 async function selectProvider(name,ports){await chooseBlock(name);for(const name of ports){const row=inspector.locator('.port-row').filter({has:page.getByText(name,{exact:true})}).filter({has:page.getByRole('button',{name:'Use this exact port',exact:true})});await row.getByRole('button',{name:'Use this exact port',exact:true}).click();}}
 async function option(select,label){const candidates=await select.locator('option').evaluateAll(options=>options.map(option=>({label:option.textContent,value:option.value})));const choice=candidates.find(option=>option.label===label||option.value===label)??candidates.find(option=>option.label.startsWith(label+' ·'));assert.ok(choice,'Missing select option '+label+' in '+JSON.stringify(candidates));await select.selectOption(choice.value);}
 async function portOption(label,value){
  const input=composer.getByRole('combobox',{name:label,exact:true});if(await input.getAttribute('data-value')===value||await input.inputValue()===value)return;await input.click();await input.fill(value);
  await expect(input).toHaveAttribute('aria-expanded','true');
  const listbox=page.locator('[role="listbox"]:visible');
  const candidates=await listbox.getByRole('option').evaluateAll(options=>options.map(option=>({label:(option.querySelector('.port-name')?.textContent??option.textContent).replace(/\s+/g,' ').trim(),value:option.getAttribute('data-value')})));
  const choice=candidates.find(option=>option.value===value||option.label===value)??candidates.find(option=>option.label.startsWith(value+' ·')||option.label.startsWith(value+' '));
  assert.ok(choice,'Missing port result '+value+' in '+JSON.stringify(candidates));
  await listbox.getByRole('option').nth(candidates.indexOf(choice)).click();
 }
 async function route(from,fromPort,to,toPort,commit=true){
  if(await composer.isVisible())await composer.getByRole('button',{name:'Close connection editor',exact:true}).click();
  await page.getByLabel('Search profile blocks').fill('');if(await inspector.isVisible())await inspector.getByRole('button',{name:'Close component settings',exact:true}).click();await page.getByRole('button',{name:'Fit all',exact:true}).click();
  await page.getByRole('application',{name:'Profile canvas'}).scrollIntoViewIfNeeded();
  const block=name=>page.locator('.resource-block').filter({has:page.locator('.block-heading').getByText(name,{exact:true})}),output=block(from).locator('[data-connection-source]'),input=block(to).locator('[data-connection-target]'),before=await page.locator('.signal-edge').count();
  const sourceId=await block(from).getAttribute('data-node-id'),targetId=await block(to).getAttribute('data-node-id');await expect(output).toBeEnabled();await expect(input).toBeEnabled();
  const source=await output.boundingBox(),target=await input.boundingBox();assert.ok(source&&target,'Both canvas port groups must be visible');
  await page.mouse.move(source.x+source.width/2,source.y+source.height/2);await page.mouse.down();await page.mouse.move(target.x+target.width/2,target.y+target.height/2,{steps:8});await page.mouse.up();
  await expect.poll(async()=>await composer.isVisible()||await page.locator('.signal-edge').count()===before+1).toBe(true);
  if(await composer.isVisible()){
   await expect(composer).toHaveAttribute('data-from-node',sourceId);await expect(composer).toHaveAttribute('data-to-node',targetId);await expect(composer.getByLabel('Source block',{exact:true})).toHaveCount(0);await expect(composer.getByLabel('Destination block',{exact:true})).toHaveCount(0);
   await portOption('Find an output',fromPort);if(await composer.isVisible())await portOption('Find an input',toPort);
  }
  if(commit){await expect(composer).not.toBeVisible();await expect(page.locator('.signal-edge')).toHaveCount(before+1);}else{await expect(composer).toBeVisible();await expect(page.locator('.signal-edge')).toHaveCount(before);}
 }
 async function renameProcessing(name,fields={}){await inspector.hover();const settings=page.getByRole('region',{name:'Transformation settings'}),advanced=settings.locator('details').filter({has:page.locator('summary').getByText('Advanced transformation settings',{exact:true})});if(await advanced.getAttribute('open')===null)await advanced.locator('summary').first().click();const editor=page.getByRole('region',{name:'Visible transformation editor'});await editor.getByLabel('Block label',{exact:true}).fill(name);for(const [label,value]of Object.entries(fields))await editor.getByLabel(label,{exact:true}).fill(String(value));await expect(editor.getByRole('button',{name:'Update visible node',exact:true})).toBeEnabled();await editor.getByRole('button',{name:'Update visible node',exact:true}).click();}
 async function transform(preset,name,fields={}){
  if(preset==='format'&&await composer.isVisible()){await composer.getByRole('button',{name:'Connect with transformation',exact:true}).click();const dialog=page.locator('.connection-dialog[open]');await dialog.getByRole('combobox',{name:'Connection transformation',exact:true}).selectOption(preset);await dialog.getByRole('button',{name:'Insert transformation',exact:true}).click();}
  else{if(await composer.isVisible())await composer.getByRole('button',{name:'Close connection editor',exact:true}).click();const group=page.locator('.component-palette [aria-label="Transformation components"]');if(await group.getAttribute('open')===null)await group.locator('summary').click();await group.locator(`[data-processing-preset="${preset}"]`).click();}
  await renameProcessing(name,fields);
 }
 async function addArgumentPack(label,name){await composer.getByText('Action arguments',{exact:true}).click();await composer.getByRole('button',{name:label,exact:true}).click();await composer.getByRole('button',{name:'Close connection editor',exact:true}).click();await renameProcessing(name);}
 async function virtual(name,buttons,axes){const group=page.locator('.component-palette [aria-label="Output components"]');if(await group.getAttribute('open')===null)await group.locator('summary').click();await group.getByRole('button',{name:'Add virtual controller',exact:true}).click();const id=await page.locator('.resource-block.virtual').last().getAttribute('data-node-id');await page.getByLabel('Search profile blocks').fill(id);await page.getByLabel('Search profile blocks').press('Enter');const editor=page.getByRole('region',{name:'Virtual controller layout editor'});await editor.getByLabel('Controller label').fill(name);for(const [label,value]of Object.entries({buttons,axes,hats:0})){await editor.getByLabel(label,{exact:true}).fill(String(value));await editor.getByLabel(label,{exact:true}).press('Tab');}for(const select of await editor.getByLabel('Native range').all())await select.selectOption('unipolar');await editor.getByRole('button',{name:'Update controller layout',exact:true}).click();}
 try{
  await page.goto(endpoint);await page.getByRole('navigation',{name:'Main navigation'}).getByRole('button',{name:'Profiles',exact:true}).click();await expect(page.getByRole('button',{name:'Edit Default',exact:true})).toBeVisible();await page.getByRole('button',{name:'Add',exact:true}).click();await page.getByRole('button',{name:'Edit profile name',exact:true}).click();await page.getByLabel('Profile name',{exact:true}).fill('Complete browser rig');await page.getByLabel('Profile name',{exact:true}).press('Enter');
  await expect(page.locator('.resource-block')).toHaveCount(0);await expect(page.locator('path.signal-edge')).toHaveCount(0);
  const palette=page.getByRole('complementary',{name:'Component palette',exact:true});await palette.getByRole('button',{name:'Refresh components',exact:true}).click();
  for(const category of ['Controllers','OSG devices','Companions']){const group=palette.getByLabel(category,{exact:true});if(await group.count()&&await group.getAttribute('open')===null)await group.locator('summary').click();}
  for(const resource of resources.filter(resource=>resource.resourceType!=='game-integration'))await palette.getByRole('button',{name:`Add ${resource.name}`,exact:true}).click();
  await page.getByLabel('Game context',{exact:true}).selectOption('dcs');await chooseBlock('DCS World');const policy=inspector.getByText('Resource assignment and policy',{exact:true});if(!await inspector.getByLabel('Block label',{exact:true}).isVisible())await policy.click();await inspector.getByLabel('Block label',{exact:true}).fill('DCS game data and commands');await inspector.getByLabel('Block label',{exact:true}).press('Tab');await inspector.getByRole('button',{name:'Close component settings',exact:true}).click();
  await expect(page.locator('.resource-block')).toHaveCount(9);await expect(page.locator('path.signal-edge')).toHaveCount(0);
  await page.setViewportSize({width:2400,height:1100});
  const scrollArea=page.locator('main');
  const wideLayout=await scrollArea.evaluate(node=>{
   const outer=node.getBoundingClientRect(),inner=node.querySelector('.main-content').getBoundingClientRect();
   return{left:outer.left,right:outer.right,top:outer.top,width:outer.width,contentLeft:inner.left,contentRight:inner.right,contentWidth:inner.width};
  });
  assert.equal(wideLayout.contentWidth,1680,'Wide windows retain the centered content width limit.');
  assert.ok(wideLayout.contentLeft>wideLayout.left&&wideLayout.contentRight<wideLayout.right);
  for(const x of [(wideLayout.left+wideLayout.contentLeft)/2,(wideLayout.contentRight+wideLayout.right)/2]){
   await scrollArea.evaluate(node=>node.scrollTop=0);await page.mouse.move(x,wideLayout.top+120);await page.mouse.wheel(0,200);
   await expect.poll(()=>scrollArea.evaluate(node=>node.scrollTop)).toBeGreaterThan(0);
  }
  await scrollArea.evaluate(node=>node.scrollTop=0);await page.setViewportSize({width:1600,height:1100});
  await expect(inspector).toHaveCount(0);
  await page.getByRole('button',{name:'Fit all',exact:true}).click();
  await page.getByRole('application',{name:'Profile canvas'}).scrollIntoViewIfNeeded();
  const firstBlock=page.locator('.resource-block').first();
  for(const surface of ['.block-body','.block-heading']){
   const before=await firstBlock.getAttribute('style'),box=await firstBlock.locator(surface).boundingBox();
   assert.ok(box);
   await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();
   await expect(inspector).toHaveCount(0);
   await page.mouse.move(box.x+box.width/2+48,box.y+box.height/2+24,{steps:5});
   await expect(page.getByRole('application',{name:'Profile canvas'})).toHaveCSS('cursor','grabbing');
   await expect(firstBlock.locator(surface)).toHaveCSS('cursor','grabbing');
   await expect(inspector).toHaveCount(0);await page.mouse.up();
   await expect(firstBlock.locator(surface)).toHaveCSS('cursor','grab');
   await expect(inspector).toHaveCount(0);assert.notEqual(await firstBlock.getAttribute('style'),before,'Dragging the body or heading moves the node without selecting it.');
  }
  const clickBox=await firstBlock.locator('.block-body').boundingBox();
  await page.mouse.move(clickBox.x+clickBox.width/2,clickBox.y+clickBox.height/2);await page.mouse.down();
  await expect(inspector).toHaveCount(0);await page.mouse.up();await expect(inspector).toBeVisible();
  await inspector.getByRole('button',{name:'Close component settings',exact:true}).click();await expect(inspector).toHaveCount(0);
  await firstBlock.locator('.block-heading').focus();await page.keyboard.press('Enter');await expect(inspector).toBeVisible();
  await inspector.getByRole('button',{name:'Close component settings',exact:true}).click();await expect(inspector).toHaveCount(0);
  const canvas=page.getByRole('application',{name:'Profile canvas'});
  const fullCanvasWidth=await canvas.evaluate(node=>node.clientWidth);
  await chooseBlock('OSG direct USB device');
  await expect.poll(async()=>inspector.evaluate(node=>Math.round(node.getBoundingClientRect().width))).toBe(Math.round(fullCanvasWidth*.75));
  assert.equal(await canvas.evaluate(node=>node.clientWidth),fullCanvasWidth,'Opening details overlays the canvas without resizing it.');
  await page.locator('.canvas-layout').screenshot({path:resolve(artifacts,'node-details-overlay.png'),animations:'disabled'});
  await page.setViewportSize({width:700,height:900});
  await expect.poll(async()=>inspector.evaluate(node=>Math.round(node.getBoundingClientRect().width))).toBe(Math.round((await canvas.evaluate(node=>node.clientWidth))*.75));
  await page.setViewportSize({width:1600,height:1100});
  await canvas.scrollIntoViewIfNeeded();
  const emptyPoint=await canvas.evaluate(node=>{
   const box=node.getBoundingClientRect();
   for(let y=12;y<Math.min(box.height,window.innerHeight-box.top)-12;y+=24)for(let x=12;x<box.width*.24;x+=24){
    const target=document.elementFromPoint(box.left+x,box.top+y);
    if(target&&node.contains(target)&&!target.closest('.resource-block,button'))return{x:box.left+x,y:box.top+y};
   }
   throw Error('No exposed empty canvas point found.');
  });
  await page.mouse.click(emptyPoint.x,emptyPoint.y);
  await expect(inspector).toHaveCount(0);await expect(page.locator('.resource-block.selected')).toHaveCount(0);
  await chooseBlock('OSG direct USB device');
  await expect(inspector).toHaveCSS('transform','none');
  const exitMotion=await inspector.evaluate(node=>new Promise(resolve=>{
   node.addEventListener('outrostart',()=>requestAnimationFrame(()=>requestAnimationFrame(()=>{
    resolve({attached:node.isConnected,animated:node.getAnimations().some(animation=>animation.playState==='running')});
   })),{once:true});
   node.querySelector('[aria-label="Close component settings"]').click();
  }));
  assert.deepEqual(exitMotion,{attached:true,animated:true},'Details remain mounted during their slide-out transition.');
  await expect(inspector).toHaveCount(0);
  await chooseBlock('OSG direct USB device');await inspector.getByText('Connection metadata (2)',{exact:true}).click();await expect(inspector.locator('.connection-row').getByText('USB',{exact:true})).toBeVisible();await expect(inspector.locator('.connection-row').getByText('CAN',{exact:true})).toBeVisible();await expect(inspector.getByText('COM-fixture',{exact:true})).toBeVisible();await expect(inspector.getByText('OSG gateway',{exact:true})).toBeVisible();await expect(page.locator('path.signal-edge')).toHaveCount(0);
  for(const resource of blueprint.resources.filter(resource=>resource.kind!=='companion'&&resource.ports.length))await selectProvider(resource.name,resource.ports.map(port=>port.name));
  await chooseBlock('Hidden third party C');await inspector.getByText('Resource assignment and policy',{exact:true}).click();await inspector.getByLabel('Game visibility intent').selectOption('hidden-while-active');
  await chooseBlock('Companion panel');const panel=page.getByRole('region',{name:'Profile companion panel selection'});await expect(panel.getByLabel('Panel',{exact:true})).toBeVisible();await panel.getByLabel('Panel',{exact:true}).selectOption(publication);await panel.getByRole('button',{name:'Use panel in profile',exact:true}).click();await panel.getByRole('button',{name:'Inspect exact panel ports',exact:true}).click();for(const name of ['Altitude warning widget','Conditional altitude text widget'])await panel.locator('.panel-ports li').filter({has:page.getByText(name,{exact:true})}).getByRole('button',{name:'Add feedback input',exact:true}).click();await panel.getByLabel('Port group').selectOption('output');await panel.getByRole('button',{name:'Add control output',exact:true}).click();
  await virtual('Combined third party controller',1,2);await virtual('Combined OSG controller',0,3);
  for(const [from,port,to,target]of [['Third party A','Steering','Combined third party controller','axis-1'],['Third party B','Throttle','Combined third party controller','axis-2'],['OSG CAN device A','CAN axis A','Combined OSG controller','axis-1'],['OSG CAN device B','CAN axis B','Combined OSG controller','axis-2'],['OSG direct USB device','Direct USB axis','Combined OSG controller','axis-3'],['Companion panel','Panel switch','Combined third party controller','button-1']])await route(from,port,to,target);
  await route('Separate OSG command module','Command button','DCS game data and commands','Master caution command',false);await expect(composer.getByRole('button',{name:'Create exact route',exact:true})).toHaveCount(0);await expect(composer.getByRole('status')).toContainText('complete invocation');await addArgumentPack('Add event-triggered argument pack','Explicit command trigger');await route('Separate OSG command module','Command button','Explicit command trigger','fire');await route('Explicit command trigger','invoke','DCS game data and commands','Master caution command');
  await route('DCS game data and commands',altitudeName,'Companion panel','Conditional altitude text widget',false);await composer.getByRole('button',{name:'Connect with transformation',exact:true}).click();const conversions=page.locator('.connection-dialog[open]');await expect(conversions.getByRole('combobox',{name:'Connection transformation',exact:true}).locator('option[value=unit-convert]')).toHaveCount(0);await conversions.getByRole('button',{name:'Close connection transformations',exact:true}).click();
  await transform('declare-quantity',quantityName,{'Quantity definition':quantity,'Declared unit':'ft'});
  const quantityEditor=page.getByRole('region',{name:'Visible transformation editor'});await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(quantityEditor.getByLabel('Quantity definition',{exact:true})).toHaveValue('Raw count');await expect(quantityEditor.getByLabel('Declared unit',{exact:true})).toHaveValue('count');await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(quantityEditor.getByLabel('Quantity definition',{exact:true})).toHaveValue(quantity);await expect(quantityEditor.getByLabel('Declared unit',{exact:true})).toHaveValue('ft');
  await route('DCS game data and commands',altitudeName,quantityName,'in');await route(quantityName,'out','Companion panel','Conditional altitude text widget',false);await transform('unit-convert','Feet to metres');
  await route(quantityName,'out','Feet to metres','in');await route('Feet to metres','out','Companion panel','Altitude warning widget',false);await transform('hysteresis','Altitude threshold',{Press:3000,Release:2900});
  await route('Feet to metres','out','Altitude threshold','in');await route('Altitude threshold','out','OSG CAN device A','Module indicator action',false);await addArgumentPack('Add setter on argument change','Explicit warning setter');await route('Altitude threshold','out','Explicit warning setter','value');await route('Explicit warning setter','invoke','OSG CAN device A','Module indicator action');await route('Altitude threshold','out','Companion panel','Altitude warning widget');
  await route('Feet to metres','out','Companion panel','Conditional altitude text widget',false);await transform('gate','Only show altitude while warning is active');await route('Feet to metres','out','Only show altitude while warning is active','value');await route('Altitude threshold','out','Only show altitude while warning is active','gate');
  await route('Only show altitude while warning is active','out','Companion panel','Conditional altitude text widget',false);await transform('format','Format altitude in metres',{Decimals:1,Prefix:'ALT ',Suffix:' m'});
  await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 18');await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 17');await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 18');
  await chooseBlock('OSG CAN device A');const publicationPolicy=inspector.getByLabel('Controller publication policy');if(!await publicationPolicy.isVisible())await inspector.getByText('Resource assignment and policy',{exact:true}).click();await expect(inspector.getByText('Gateway CAN capability advertised.',{exact:false})).toBeVisible();await publicationPolicy.selectOption('disabled');await expect(publicationPolicy).toHaveValue('disabled');await expect(inspector.getByText('Enabling or disabling publication can change USB controller collections',{exact:false})).toBeVisible();await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(publicationPolicy).toHaveValue('configured');await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(publicationPolicy).toHaveValue('disabled');
  await page.getByRole('button',{name:'Auto-layout',exact:true}).click();await page.getByRole('button',{name:'Fit all',exact:true}).click();await page.getByLabel('Search profile blocks').fill('');
  const routed=await page.locator('.canvas-world').evaluate(world=>{
    const boxes=[...world.querySelectorAll('[data-node-id]')].map(node=>({id:node.dataset.nodeId,x:parseFloat(node.style.left),y:parseFloat(node.style.top),w:parseFloat(node.style.width),h:parseFloat(node.style.height)}));
    const paths=[...world.querySelectorAll('.signal-edge')];const collisions=[];
    for(const path of paths){const length=path.getTotalLength();for(let distance=0;distance<=length;distance+=4){const p=path.getPointAtLength(distance);for(const box of boxes)if(p.x>box.x+.5&&p.x<box.x+box.w-.5&&p.y>box.y+.5&&p.y<box.y+box.h-.5){collisions.push({edge:path.dataset.bindingId,block:box.id});break;}}}
    const endpoints=paths.map(path=>({start:path.getPointAtLength(0),end:path.getPointAtLength(path.getTotalLength())}));
    const anchors=boxes.map(box=>{const at=(point,x)=>Math.abs(point.x-x)<.1&&point.y>=box.y&&point.y<=box.y+box.h;return{id:box.id,outputs:new Set(endpoints.filter(edge=>at(edge.start,box.x+box.w)).map(edge=>edge.start.y)).size,inputs:new Set(endpoints.filter(edge=>at(edge.end,box.x)).map(edge=>edge.end.y)).size};});
    return{edges:paths.length,collisions,anchors};
  });
  assert.equal(routed.edges,18);assert.deepEqual(routed.collisions,[],'Rendered routes must clear every block, including rounded corners');
  assert.ok(routed.anchors.every(anchor=>anchor.outputs<=1&&anchor.inputs<=1),'Every block shares one output anchor and a separate input anchor.');
  await page.getByRole('button',{name:'Close component settings',exact:true}).click();
  await expect(page.getByLabel('Selected block inspector')).toHaveCount(0);
  const firstRoute=page.locator('.signal-edge').first();
  const hoverPoint=await firstRoute.evaluate(path=>{const p=path.getPointAtLength(Math.min(20,path.getTotalLength()/2));const screen=new DOMPoint(p.x,p.y).matrixTransform(path.getScreenCTM());return{x:screen.x,y:screen.y};});
  await page.mouse.move(hoverPoint.x,hoverPoint.y);await expect(firstRoute).toHaveCSS('stroke-width','3px');
  await page.locator('.canvas-layout').screenshot({path:resolve(artifacts,'auto-layout-clear-routes.png'),animations:'disabled'});

  // Author portable editor metadata through the production controls as well.
  const groupPalette=page.locator('.component-palette [aria-label="Profile groups"]');
  await groupPalette.getByRole('button',{name:'Add group',exact:true}).click();await expect(page.locator('.editor-group-frame')).toHaveCount(1);
  await groupPalette.getByRole('button',{name:'Rename Group',exact:true}).click();await groupPalette.getByLabel('Group name',{exact:true}).fill('CAN controls');await groupPalette.getByLabel('Group name',{exact:true}).press('Enter');
  async function dragIntoGroup(name,ratio){
    await page.getByRole('button',{name:'Fit all',exact:true}).click();await page.getByRole('application',{name:'Profile canvas',exact:true}).scrollIntoViewIfNeeded();
    const heading=page.locator('.block-heading').filter({hasText:name}),frame=page.locator('.editor-group-frame');
    const source=await heading.boundingBox(),card=await heading.locator('..').boundingBox(),target=await frame.boundingBox();assert.ok(source&&card&&target);
    const x=source.x+source.width/2,y=source.y+source.height/2,zoom=card.width/288;
    const center={x:target.x+target.width*ratio,y:target.y+36*zoom+(target.height-36*zoom)/2};
    await page.mouse.move(x,y);await page.mouse.down();await page.mouse.move(center.x-(card.x+card.width/2-x),center.y-(card.y+card.height/2-y),{steps:8});await page.mouse.up();
  }
  await dragIntoGroup('OSG CAN device A',.35);await dragIntoGroup('OSG CAN device B',.65);
  await expect(page.locator('.editor-group-heading')).toContainText('2');
  await groupPalette.getByRole('button',{name:'Rename CAN controls',exact:true}).click();await groupPalette.getByLabel('Group name',{exact:true}).fill('CAN feedback');await groupPalette.getByLabel('Group name',{exact:true}).press('Enter');
  await expect(page.getByRole('button',{name:'Focus group CAN feedback',exact:true})).toBeAttached();await groupPalette.getByRole('button',{name:'Remove CAN feedback group',exact:true}).click();await expect(page.locator('.editor-group-frame')).toHaveCount(0);await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(page.locator('.editor-group-frame')).toHaveCount(1);await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(page.locator('.editor-group-frame')).toHaveCount(0);await page.getByRole('button',{name:'Undo',exact:true}).click();
  await groupPalette.getByRole('button',{name:'Focus CAN feedback',exact:true}).click();await chooseBlock('OSG CAN device A');const outputs=inspector.getByRole('button',{name:'Source outputs for OSG CAN device A',exact:true});await outputs.click();await expect(outputs).toHaveAttribute('aria-expanded','false');await page.getByRole('button',{name:'Undo',exact:true}).click();await expect(outputs).toHaveAttribute('aria-expanded','true');await page.getByRole('button',{name:'Redo',exact:true}).click();await expect(outputs).toHaveAttribute('aria-expanded','false');await inspector.getByRole('button',{name:'Destination actions for OSG CAN device A',exact:true}).click();await page.getByLabel('Search profile blocks').fill('');await page.getByRole('button',{name:'Fit all',exact:true}).click();
  await page.getByRole('button',{name:'Save profile',exact:true}).click();await expect(page.getByRole('region',{name:'Notifications',exact:true}).getByText('Profile saved.',{exact:true}).last()).toBeVisible();assert.equal(profileSession.owner,null);assert.equal(saveCount,1);
  const authored=[...documents.values()].find(document=>document.id!==defaultId);assert.ok(authored);assert.equal(authored.resources.length,9);assert.equal(authored.processingNodes.length,7);assert.equal(authored.virtualOutputs.length,2);assert.equal(authored.bindings.length,18);assert.equal(authored.visibility.filter(intent=>intent.mode==='hidden-while-active').length,1);assert.equal(authored.companionAssignments[0].publicationHash,publication);assert.deepEqual(documents.get(defaultId),defaults);
  assert.equal(profileDocumentError(authored),'');for(const edge of authored.bindings)assert.equal(graphConnectionError(authored,edge.from,edge.to,edge.id),'',edge.id);
  assert.deepEqual(exactRoutes(authored,assignments.get(authored.id)),exactRoutes(blueprint));for(const node of blueprint.processingNodes)assert.deepEqual(authored.processingNodes.find(other=>other.name===node.name).operation,node.operation);
  const dcsResource=authored.resources.find(resource=>resource.kind==='game'),altitudePin=assignments.get(authored.id).find(pin=>pin.resourceId===dcsResource.id).ports.find(pin=>pin.observedPortId===rawAltitudeId),declaration=authored.processingNodes.find(node=>node.operation.kind==='declare-quantity');assert.ok(altitudePin);assert.equal(altitudePin.expectedContractHash,hash(rawAltitude.contract));assert.deepEqual(dcsResource.ports.find(port=>port.id===altitudePin.profilePortId).contract,rawAltitude.contract);assert.deepEqual(declaration.operation,{kind:'declare-quantity',quantity,unit:'ft'});assert.deepEqual(declaration.inputs[0].contract.schema,rawAltitude.contract.schema);assert.deepEqual(declaration.outputs[0].contract.schema,{...rawAltitude.contract.schema,unit:'ft'});assert.ok(!JSON.stringify(authored).includes(rawAltitudeId));
  assert.equal(authored.editor.positions.length,18);assert.ok(!JSON.stringify(authored).includes('native-'));assert.ok(!JSON.stringify(authored).includes('COM-fixture'));assert.equal(assignments.get(authored.id).length,9);assert.ok(assignments.get(authored.id).some(pin=>pin.sourceId==='native-direct-usb'));
  assert.equal(authored.editor.groups[0].name,'CAN feedback');assert.deepEqual(authored.editor.groups[0].members.map(id=>authored.resources.find(node=>node.id===id).name).sort(),['OSG CAN device A','OSG CAN device B']);assert.deepEqual(authored.editor.collapsedPortGroups.map(pair=>pair.group).sort(),['actions','outputs']);
  assert.deepEqual(authored.overrides,[{resourceId:authored.resources.find(node=>node.name==='OSG CAN device A').id,enabled:false}]);
  await writeFile(resolve(artifacts,'authored-profile.json'),JSON.stringify(authored,null,2)+'\n');await writeFile(resolve(artifacts,'authored-local-assignments.json'),JSON.stringify(assignments.get(authored.id),null,2)+'\n');
  await expect(page.locator('.resource-block.dimmed')).toHaveCount(0);await page.getByRole('application',{name:'Profile canvas'}).screenshot({path:resolve(artifacts,'authored-complete-canvas.png'),animations:'disabled'});
  await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();await page.getByRole('button',{name:'Edit Complete browser rig',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 18');await expect(page.locator('.resource-block.processing')).toHaveCount(7);await expect(page.locator('.resource-block.virtual')).toHaveCount(2);assert.equal(saveCount,1);
  await expect(page.getByRole('button',{name:'Focus group CAN feedback',exact:true})).toBeAttached();await page.locator('.block-heading').filter({hasText:'OSG CAN device A'}).click();await expect(inspector.getByRole('button',{name:'Source outputs for OSG CAN device A',exact:true})).toHaveAttribute('aria-expanded','false');await expect(inspector.getByRole('button',{name:'Destination actions for OSG CAN device A',exact:true})).toHaveAttribute('aria-expanded','false');
  const reopenedPolicy=inspector.getByLabel('Controller publication policy');if(!await reopenedPolicy.isVisible())await inspector.getByText('Resource assignment and policy',{exact:true}).click();await expect(reopenedPolicy).toHaveValue('disabled');
  const runtime=page.getByRole('region',{name:'Runtime profile status'});await runtime.getByLabel('Game context',{exact:true}).selectOption('dcs');await expect(runtime.getByRole('button',{name:'Activate profile',exact:true})).toBeEnabled();await runtime.getByRole('button',{name:'Activate profile',exact:true}).click();await expect(runtime).toContainText('Active profile · Complete browser rig');assert.equal(profileSession.owner.gameId,'dcs');
  // Unsaved edits remain local; saving this active profile would update its run.
  await chooseBlock('Feet to metres');await renameProcessing('Draft-only conversion label');assert.equal(appliedDocument.processingNodes.find(node=>node.operation.kind==='unit-convert').name,'Feet to metres');await expect(page.getByRole('button',{name:'Save profile',exact:true})).toBeEnabled();await expect(runtime).toContainText('Saving this profile applies its changes while it is active.');
  await page.getByRole('button',{name:'Debug',exact:true}).click();const debug=page.getByRole('region',{name:'Profile debug'});await expect(debug.getByRole('heading',{name:'Profile debug',exact:true})).toBeVisible();
  const activity=debug.getByRole('region',{name:'Applied source to destination activity'});await expect(activity.getByRole('combobox',{name:/^Applied connection/})).toBeVisible();await activity.getByLabel('Follow selected applied block',{exact:false}).uncheck();const altitudeId=appliedDocument.resources.find(resource=>resource.name==='DCS game data and commands').id,altitudeEdge=appliedDocument.bindings.find(edge=>edge.from.nodeId===altitudeId&&edge.from.portId===appliedDocument.resources.find(resource=>resource.id===altitudeId).ports.find(port=>port.name===altitudeName).id);await activity.getByRole('combobox',{name:/^Applied connection/}).selectOption(altitudeEdge.id);await expect(activity.getByText('Path samples match this exact applied owner.',{exact:false})).toBeVisible();await expect(activity.getByText('Current run 1.',{exact:false})).toBeVisible();await expect(activity.getByRole('article',{name:'source DCS game data and commands',exact:true})).toContainText('10000');await expect(activity.getByRole('article',{name:'processing Feet to metres',exact:true})).toContainText('3048');await expect(activity.getByRole('article',{name:'processing Format altitude in metres',exact:true})).toContainText('ALT 3048.0 m');await expect(activity.getByRole('article',{name:'destination Companion panel',exact:true})).toContainText('Upstream/requested');await expect(activity.getByRole('article',{name:'processing Draft-only conversion label',exact:true})).toHaveCount(0);
  await expect(activity.getByRole('article',{name:`processing ${quantityName}`,exact:true})).toContainText('10000');await expect(activity.getByRole('article',{name:`processing ${quantityName}`,exact:true})).toContainText('declared ft');
  await page.screenshot({path:resolve(artifacts,'applied-path-with-newer-draft.png'),fullPage:true});
  captureMode='stale';await expect(activity.getByRole('button',{name:'Refresh path',exact:true})).toBeEnabled();await activity.getByRole('button',{name:'Refresh path',exact:true}).click();await expect(activity.getByRole('article',{name:'source DCS game data and commands',exact:true})).toContainText('stale');await expect(activity.getByRole('article',{name:'source DCS game data and commands',exact:true}).locator('.sample-value')).toHaveText('stale');
  captureMode='unavailable';resources.find(resource=>resource.id==='native-direct-usb').availability='offline';inventoryRevision++;await activity.getByRole('button',{name:'Refresh path',exact:true}).click();await expect(activity.getByRole('article',{name:'destination Companion panel',exact:true})).toContainText('unavailable');await expect(activity.getByRole('article',{name:'processing Feet to metres',exact:true}).locator('.sample-value')).toHaveText(['unavailable','unavailable']);
  await expect(activity.getByRole('article',{name:`processing ${quantityName}`,exact:true}).locator('.sample-value')).toHaveText(['unavailable','unavailable']);
  await activity.screenshot({path:resolve(artifacts,'unavailable-applied-path.png'),animations:'disabled'});
  await debug.getByRole('button',{name:'← Back to profile',exact:true}).click();await expect(debug).not.toBeVisible();
  await page.getByRole('button',{name:'Add from inventory',exact:true}).click();await page.getByRole('button',{name:'Refresh inventory',exact:true}).click();await chooseBlock('OSG direct USB device');await expect(inspector.getByText('Offline · saved block retained',{exact:true})).toBeVisible();await expect(page.locator('.resource-block')).toHaveCount(18);await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 18');
  const appliedOwner=copy(profileSession.owner);await page.getByRole('button',{name:'Back to Profiles',exact:false}).click();await page.getByRole('alertdialog').getByRole('button',{name:'Discard and continue',exact:true}).click();await page.getByRole('button',{name:'Add',exact:true}).click();await page.getByLabel('Copy source profile',{exact:true}).selectOption(authored.id);await page.getByRole('button',{name:'Use copy',exact:true}).click();await expect(bindings.getByRole('heading')).toHaveText('Exact signal bindings · 18');await expect(page.locator('.resource-block .block-state').filter({hasText:/^Unassigned$/})).toHaveCount(9);await page.getByRole('button',{name:'Cancel creation',exact:true}).click();await expect(page.getByRole('button',{name:'Edit Complete browser rig',exact:true})).toBeVisible();assert.deepEqual(profileSession.owner,appliedOwner);assert.equal(documents.size,2);
  assert.equal(requests.filter(request=>request.command==='profile_apply').length,1);assert.equal(saveCount,1);assert.equal(profileSession.owner.profileRevision,1);assert.equal(appliedDocument.revision,1);assert.deepEqual(documents.get(defaultId),defaults);assert.deepEqual(unexpected,[]);assert.deepEqual(pageErrors,[]);
  await writeFile(resolve(artifacts,'ipc-commands.json'),JSON.stringify(requests.map(request=>request.command),null,2)+'\n');await page.screenshot({path:resolve(artifacts,'saved-source-after-copy-cancel.png'),fullPage:true});
 }catch(error){await page.screenshot({path:resolve(artifacts,'failure.png'),fullPage:true});await writeFile(resolve(artifacts,'failure-ipc.json'),JSON.stringify({requests,unexpected,pageErrors},null,2));throw error;}finally{await browser.close();}
});
