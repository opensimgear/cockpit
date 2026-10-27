// Real dev Cockpit, installed DCS-BIOS catalogues, verified fixture process and
// authored export UDP. No mock IPC, catalogue replacement or simulator commands.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,access} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createSocket} from 'node:dgram';
import {randomUUID} from 'node:crypto';
const artifacts=resolve('.research/dcs-aircraft-catalogues-native');
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(read,label,timeout=35000){let last;const end=Date.now()+timeout;while(Date.now()<end){try{last=await read();if(last)return last;}catch(error){last=String(error);}await pause(120);}throw Error(`${label}: ${JSON.stringify(last)}`);}
async function connect(){
 const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`)).json();
 const target=targets.find(t=>t.type==='page'&&/127\.0\.0\.1:1420|tauri\.localhost/.test(t.url));assert.ok(target,'Start the native dev Cockpit.');
 const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let serial=0;
 await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
 socket.addEventListener('message',event=>{const message=JSON.parse(event.data),p=pending.get(message.id);if(!p)return;clearTimeout(p.timer);pending.delete(message.id);message.error?p.reject(Error(message.error.message)):p.resolve(message.result);});
 const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++serial,timer=setTimeout(()=>{pending.delete(id);reject(Error(`CDP ${method} timed out`));},60000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
 return {send,async evaluate(expression){const reply=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;},close(){socket.close();}};
}
async function json(path){return JSON.parse(await readFile(path,'utf8'));}
function control(document,id){return Object.values(document).flatMap(category=>Object.values(category)).find(control=>control.identifier===id);}
function block(address,data){const header=Buffer.alloc(4);header.writeUInt16LE(address);header.writeUInt16LE(data.length,2);return Buffer.concat([header,data]);}
function integerBlock(output,value){const bytes=Buffer.alloc(2);bytes.writeUInt16LE((value<<output.shift_by)&output.mask);return block(output.address,bytes);}
function identity(id,contract){return{id,name:id,operation:{kind:'identity'},inputs:[{id:'in',name:'Input',direction:'input',contract}],outputs:[{id:'out',name:'Output',direction:'output',contract}]};}
async function fixtureExecutable(){
 const executable=resolve(process.env.OSG_DCS_GAME_EXE??resolve(artifacts,'bin/DCS.exe'));
 if(process.env.OSG_DCS_GAME_EXE){await access(executable);return executable;}
 await mkdir(dirname(executable),{recursive:true});
 try{await access(executable);}catch{
  await promisify(execFile)('powershell.exe',['-NoProfile','-NonInteractive','-Command',`$ErrorActionPreference='Stop'; Add-Type -TypeDefinition 'public static class DcsCatalogueFixture { public static void Main() { System.Threading.Thread.Sleep(System.Threading.Timeout.Infinite); } }' -OutputAssembly $env:OSG_DCS_FIXTURE_OUTPUT -OutputType WindowsApplication`],{windowsHide:true,env:{...process.env,OSG_DCS_FIXTURE_OUTPUT:executable}});
 }
 return executable;
}

test('DCS aircraft tabs own catalogues while native aircraft selects and fences the module',{skip:process.env.OSG_DCS_VEHICLES_NATIVE!=='1',timeout:240000},async()=>{
 await mkdir(artifacts,{recursive:true});const cdp=await connect(),checks=[],cleanupErrors=[],name=`DCS catalogue ${randomUUID()}`;
 const evidence={actualNativeCockpit:true,installedDcsBiosCatalogue:true,authoredDcsExports:true,actualSimulatorQualified:false,checks,cleanupErrors};
 const request=async value=>{const reply=await cdp.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'dcs-aircraft-native'})}})`);assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;};
 const game=async()=>(await request({command:'game_get_state'})).games.catalogue.find(game=>game.id==='dcs');
 const status=async()=>(await request({command:'profile_status'})).profiles.profileSession;
 const click=(label,scope='.profiles-workspace')=>until(()=>cdp.evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(scope)}),b=[...(root?.querySelectorAll('button')??[])].find(b=>(b.getAttribute('aria-label')??b.textContent.trim())===${JSON.stringify(label)});if(!b||b.disabled)return false;b.click();return true;})()`),`Missing button ${label}`);
 const select=(label,value)=>until(()=>cdp.evaluate(`(()=>{const s=document.querySelector('select[aria-label='+JSON.stringify(${JSON.stringify(label)})+']');if(!s||s.disabled||![...s.options].some(o=>o.value===${JSON.stringify(value)}))return false;s.value=${JSON.stringify(value)};s.dispatchEvent(new Event('change',{bubbles:true}));return true;})()`),`Missing ${label} option ${value}`);
 const profile=async()=>(await request({command:'profile_assignments_get',id:profileId})).profiles;
 const source=async vehicle=>(await request({command:'profile_dcs_vehicle_resource',vehicleId:vehicle})).profileResources.resource;
 async function ports(id){const result=[];let offset=0,total;do{const page=(await request({command:'profile_resources_inspect',resourceId:id,offset,limit:128})).profileResources;result.push(...page.ports);total=page.total;offset+=page.ports.length;assert.ok(page.ports.length||offset===total);}while(offset<total);return result;}
 let profileId,installationBefore,selectedBefore,modeBefore,child,producer,timer,emitting=true,aircraft='A-10C_2',counter=0,connectionBefore;
 const cleanup=async(label,fn)=>{try{await fn();}catch(error){cleanupErrors.push(`${label}: ${error}`);}};
 try{
  const initial=(await request({command:'profile_status'})).profiles;selectedBefore=initial.session.selectedProfileId;modeBefore=initial.session.selectionMode;
  if(['faulted','recovery-needed'].includes(initial.profileSession.state))await request({command:'profile_recover'});
  await request({command:'profile_stop'});const initialGames=(await request({command:'game_get_state'})).games,dcs=initialGames.catalogue.find(g=>g.id==='dcs');
  assert.equal(dcs.integrationSetup.state,'ready','Set up the actual DCS-BIOS integration before this acceptance.');installationBefore=initialGames.manualInstallations?.dcs??null;connectionBefore=dcs.integrationSetup.connection;
  evidence.providerBefore={revision:dcs.integrationSetup.providerSetupRevision,legacyModule:dcs.integrationSetup.aircraftModule,connection:connectionBefore};
  const catalogue=dcs.integrationSetup.cataloguePath??connectionBefore.cataloguePath;
  const aliases=await json(resolve(catalogue,'AircraftAliases.json'));assert.ok(aliases.A_10C_2??aliases['A-10C_2']);assert.ok(aliases['F-16C_50']);
  const aModule=aliases['A-10C_2'].find(module=>module!=='CommonData'),bModule=aliases['F-16C_50'].find(module=>module!=='CommonData');
  const aDoc=await json(resolve(catalogue,`${aModule}.json`)),bDoc=await json(resolve(catalogue,`${bModule}.json`));
  const aControl=control(aDoc,'AAP_CDUPWR'),bControl=Object.values(bDoc).flatMap(c=>Object.values(c)).find(c=>c.outputs?.some(o=>o.type==='integer'&&o.max_value===1));assert.ok(aControl);assert.ok(bControl);
  const aOutput=aControl.outputs.find(o=>o.type==='integer'),bOutput=bControl.outputs.find(o=>o.type==='integer');
  const commonOutput=control(await json(resolve(catalogue,'CommonData.json')),'ALT_MSL_FT').outputs[0];
  const executable=await fixtureExecutable();
  await request({command:'game_installation',gameId:'dcs',executable});
  child=spawn(executable,[],{cwd:dirname(executable),windowsHide:true,stdio:'ignore'});child.on('error',error=>{evidence.fixtureError=String(error);});
  producer=createSocket('udp4');await new Promise(resolve=>producer.bind(0,resolve));
  if(connectionBefore.multicastGroup){if(connectionBefore.multicastInterface&&connectionBefore.multicastInterface!=='0.0.0.0')producer.setMulticastInterface(connectionBefore.multicastInterface);producer.setMulticastLoopback(true);}
  const target=connectionBefore.multicastGroup||(!connectionBefore.listenAddress||connectionBefore.listenAddress==='0.0.0.0'?'127.0.0.1':connectionBefore.listenAddress);
  timer=setInterval(()=>{if(!emitting)return;const name=Buffer.alloc(24);name.write(aircraft);const count=Buffer.from([counter++%256,0]);const data=Buffer.concat([Buffer.from([85,85,85,85]),block(0,name),integerBlock(commonOutput,1000),integerBlock(aircraft==='A-10C_2'?aOutput:bOutput,1),block(65534,count)]);producer.send(data,connectionBefore.listenPort,target);},40);
  await until(async()=>{const value=await game();return value.currentVehicle?.id===aircraft&&value.integrationSetup.activeAircraftModule===aModule?value:false;},'Native aircraft did not select the A-10C catalogue');
  await cdp.evaluate(`(()=>{const nav=document.querySelector('nav[aria-label="Main navigation"]');[...nav.querySelectorAll('button')].find(b=>b.textContent.trim()==='Profiles').click();})()`);await pause(200);
  for(const label of ['← Back to profile','← Back to Profiles','Discard and continue'])await cdp.evaluate(`(()=>{const b=[...document.querySelectorAll('button')].find(b=>(b.getAttribute('aria-label')??b.textContent.trim())===${JSON.stringify(label)});if(b&&!b.disabled)b.click();})()`);
  await click('Add');await click('Edit profile name');await cdp.evaluate(`(()=>{const i=document.querySelector('[aria-label="Profile name"]');i.value=${JSON.stringify(name)};i.dispatchEvent(new Event('input',{bubbles:true}));i.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));})()`);
  await select('Game context','dcs');
  for(const [vehicle,module] of [['A-10C_2',aModule],['F-16C_50',bModule]]){await click('Add vehicle tab');await select('Tab vehicle',vehicle);await until(()=>cdp.evaluate(`document.querySelector('[aria-label="Profile vehicle tabs"] select[data-active="true"]')?.value===${JSON.stringify(vehicle)}`),'Aircraft tab selection did not complete');await until(()=>cdp.evaluate(`(()=>{const root=document.querySelector('.profiles-workspace');return [...root.querySelectorAll('.vehicle-hint')].some(p=>p.textContent.trim()===${JSON.stringify(`DCS-BIOS catalogue: ${module}`)})&&!root.textContent.includes('Loading this aircraft’s controls and telemetry');})()`),'Aircraft catalogue acquisition did not complete');}
  await click('Save');profileId=await until(async()=>(await request({command:'profile_list'})).profiles.profiles.find(p=>p.name===name)?.id,'UI profile was not saved');
  const initialProfile=await profile(),aSource=await source('A-10C_2'),bSource=await source('F-16C_50');
  const aPorts=await ports(aSource.id),bPorts=await ports(bSource.id);assert.notEqual(aSource.id,bSource.id);
  assert.equal(aSource.attributes.aircraftModule,aModule);assert.equal(bSource.attributes.aircraftModule,bModule);
  assert.ok(aPorts.some(p=>p.descriptor.id==='command:AAP_CDUPWR:set_state'));assert.equal(bPorts.some(p=>p.descriptor.id==='command:AAP_CDUPWR:set_state'),false);
  const aTab=initialProfile.document.vehicleTabs.find(t=>t.vehicleId==='A-10C_2'),bTab=initialProfile.document.vehicleTabs.find(t=>t.vehicleId==='F-16C_50');
  const aBlock=initialProfile.document.resources.find(r=>aTab.nodeIds.includes(r.id)&&r.kind==='game'),bBlock=initialProfile.document.resources.find(r=>bTab.nodeIds.includes(r.id)&&r.kind==='game'),base=initialProfile.document.resources.find(r=>r.kind==='game'&&!initialProfile.document.vehicleTabs.some(t=>t.nodeIds.includes(r.id)));
  await until(async()=>{const current=await profile();return current.assignments.some(a=>a.resourceId===aBlock.id&&a.sourceId===aSource.id)&&current.assignments.some(a=>a.resourceId===bBlock.id&&a.sourceId===bSource.id);},'Saved aircraft blocks did not use their own catalogues');
  checks.push('Real UI aircraft tabs use distinct canonical catalogues; A-10C controls are absent from the F-16 tab.');
  const stable=await game(),ownerBefore=await status();await source('A-10C_2');await source('F-16C_50');await pause(300);const afterBrowse=await game();assert.equal(afterBrowse.integrationContext.contextSession,stable.integrationContext.contextSession);assert.deepEqual((await status()).owner,ownerBefore.owner);assert.equal(afterBrowse.integrationSetup.providerSetupRevision,evidence.providerBefore.revision);assert.deepEqual(afterBrowse.integrationSetup.connection,connectionBefore);checks.push('Browsing both aircraft leaves provider settings and live module authority unchanged.');
  let saved=await profile();const baseAssignment=saved.assignments.find(a=>a.resourceId===base.id);assert.ok(baseAssignment);const basePorts=await ports(baseAssignment.sourceId);
  const choices=[{block:base,sourceId:baseAssignment.sourceId,port:basePorts.find(p=>p.descriptor.id==='channel:common:CommonData:ALT_MSL_FT:0'),node:'base-value',expected:1000},{block:aBlock,sourceId:aSource.id,port:aPorts.find(p=>p.descriptor.id===`channel:${aControl.identifier}:0`),node:'a-value',expected:1},{block:bBlock,sourceId:bSource.id,port:bPorts.find(p=>p.descriptor.id===`channel:${bControl.identifier}:0`),node:'b-value',expected:1}];
  for(const choice of choices){assert.ok(choice.port,`Missing native port for ${choice.node}`);const p=choice.port.descriptor;choice.block=saved.document.resources.find(r=>r.id===choice.block.id);choice.block.ports=[{...p,id:'observed'}];saved.assignments=saved.assignments.filter(a=>a.resourceId!==choice.block.id);saved.assignments.push({resourceId:choice.block.id,sourceId:choice.sourceId,ports:[{profilePortId:'observed',observedPortId:p.id,expectedContractHash:choice.port.contractHash}]});saved.document.processingNodes.push(identity(choice.node,p.contract));saved.document.bindings.push({id:`${choice.node}-route`,from:{nodeId:choice.block.id,portId:'observed'},to:{nodeId:choice.node,portId:'in'}});}
  saved.document.vehicleTabs.find(t=>t.id===aTab.id).nodeIds.push('a-value');saved.document.vehicleTabs.find(t=>t.id===bTab.id).nodeIds.push('b-value');
  const document=(await request({command:'profile_editor_save',document:saved.document,assignments:saved.assignments,expectedRevision:saved.document.revision})).profiles.document;
  evidence.apply=await request({command:'profile_apply',id:profileId,expectedRevision:document.revision,gameId:'dcs'});
  async function applied(expected){return until(async()=>{const current=await status();if(current.owner?.profileId!==profileId)return false;const page=(await request({command:'profile_runtime_topology',owner:current.owner,section:'nodes',offset:0,limit:64})).profileTopology;const ids=page.items.map(n=>n.id).sort();return JSON.stringify(ids)===JSON.stringify([...expected].sort())?current:false;},'Wrong applied aircraft topology');}
  async function sample(owner,id,expected){return until(async()=>{const page=(await request({command:'profile_runtime_inspect',owner,nodeId:id,offset:0,limit:32})).profileInspection;const p=page.ports.find(p=>p.endpoint.portId==='out');return p?.sample.status==='known'&&p.sample.value.kind==='scalar'&&p.sample.value.value===expected?p.sample:false;},`Wrong DCS ${id} sample`);}
  const a=await applied([base.id,aBlock.id,'base-value','a-value']);evidence.a={owner:a.owner,samples:await Promise.all([sample(a.owner,'base-value',1000),sample(a.owner,'a-value',1)])};
  await source('F-16C_50');await pause(6000);assert.deepEqual((await status()).owner,a.owner,'Browsing or refreshing the inactive catalogue restarted the active reader.');
  aircraft='F-16C_50';await until(async()=>(await game()).integrationSetup.activeAircraftModule===bModule,'Loaded F-16 did not select its module');
  const b=await applied([base.id,bBlock.id,'base-value','b-value']);evidence.b={owner:b.owner,samples:await Promise.all([sample(b.owner,'base-value',1000),sample(b.owner,'b-value',1)])};assert.ok(b.owner.generation>a.owner.generation);await pause(6000);assert.deepEqual((await status()).owner,b.owner,'Unchanged F-16 aircraft restarted after catalogue refresh.');checks.push('Native aircraft change selects the F-16 module, replaces A-10C graph nodes and delivers Base plus aircraft telemetry without periodic reactivation.');
  const stale=await request({command:'profile_runtime_inspect',owner:a.owner,nodeId:'a-value',offset:0,limit:32}).then(()=>false,()=>true);assert.equal(stale,true,'Old aircraft owner remained usable.');
  emitting=false;await until(async()=>(await game()).currentVehicle===null,'Stale export retained aircraft authority');await applied([base.id,'base-value']);checks.push('Stale aircraft exports retire the old module and retain Base.');
  emitting=true;aircraft='A-10C_2';const recovered=await applied([base.id,aBlock.id,'base-value','a-value']);await sample(recovered.owner,'a-value',1);checks.push('Fresh exports restore the alias-selected A-10C module without changing saved provider settings.');
  assert.equal((await game()).integrationSetup.providerSetupRevision,evidence.providerBefore.revision);
  await request({command:'profile_stop'});aircraft='F-16C_50';await until(async()=>(await game()).currentVehicle?.id===aircraft,'Passive discovery stopped');assert.equal((await status()).owner,null);
  const expectedInputs=bPorts.filter(p=>p.current&&p.descriptor.direction==='input').length,expectedOutputs=bPorts.filter(p=>p.current&&p.descriptor.direction==='output').length;
  assert.ok(expectedInputs>0&&expectedOutputs>1,'Installed F-16 catalogue must advertise controls and telemetry while inactive.');
  await until(()=>cdp.evaluate(`(()=>{const block=document.querySelector('[data-node-id='+JSON.stringify(${JSON.stringify(bBlock.id)})+']');if(!block||block.querySelector('.block-discovery-error')||block.textContent.includes('Discovering inputs and outputs'))return false;const inputs=block.querySelector('[data-connection-target]')?.getAttribute('aria-label'),outputs=block.querySelector('[data-connection-source]')?.getAttribute('aria-label'),matchingCatalogue=[...document.querySelectorAll('.vehicle-hint')].some(p=>p.textContent.trim()===${JSON.stringify(`DCS-BIOS catalogue: ${bModule}`)});return matchingCatalogue&&inputs?.endsWith('(${expectedInputs})')&&outputs?.endsWith('(${expectedOutputs})');})()`),'F-16 canvas did not rediscover its complete catalogue after Stop');
  checks.push(`The visible aircraft canvas recovers all ${expectedInputs} controls and ${expectedOutputs} telemetry ports after native route changes.`);
  await cdp.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:false}).then(r=>writeFile(resolve(artifacts,'aircraft-tabs.png'),Buffer.from(r.data,'base64')));
 }catch(error){evidence.failure=String(error);try{evidence.failureStatus=await request({command:'profile_status'});evidence.failureGame=await game();}catch{}throw error;}
 finally{
  clearInterval(timer);producer?.close();await cleanup('Stop profile',()=>request({command:'profile_stop'}));if(child&&child.exitCode===null){child.kill();await pause(300);}
  profileId??=(await request({command:'profile_list'})).profiles.profiles.find(p=>p.name===name)?.id;
  if(profileId)await cleanup('Remove temporary profile',async()=>{const value=(await profile()).document;await request({command:'profile_delete',id:profileId,expectedRevision:value.revision});});
  if(installationBefore!==undefined)await cleanup('Restore DCS installation',()=>request({command:'game_installation',gameId:'dcs',executable:installationBefore}));
  if(selectedBefore)await cleanup('Restore selected profile',()=>request({command:'profile_select',id:selectedBefore,selectionSource:'manual'}));
  if(modeBefore)await cleanup('Restore profile mode',()=>request({command:'profile_selection_mode',mode:modeBefore}));
  await cleanup('Leave stopped',()=>request({command:'profile_stop'}));await cleanup('Reset temporary editor',()=>cdp.send('Page.reload'));await writeFile(resolve(artifacts,'evidence.json'),JSON.stringify(evidence,null,2)+'\n');cdp.close();assert.deepEqual(cleanupErrors,[]);
 }
});
