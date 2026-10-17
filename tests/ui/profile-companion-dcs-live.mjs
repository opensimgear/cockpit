// Actual loopback DCS-BIOS UDP -> native profile -> Hub HTTPS/WSS -> PanelCanvas.
// The native helper injects process identity and emits fixed protocol fixtures.
// No real game/device, IPC mock, replacement WebSocket or supplied signal value.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
import {copyFile,mkdir,open,readFile,readdir,rename,writeFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {randomUUID,createHash} from 'node:crypto';

const require=createRequire(import.meta.url),playwrightModule=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(playwrightModule),{expect}=require(playwrightModule+'/test');
const nativeTest='profile_session_host::companion::tests::live::dcs_live::actual_browser_wss_dcs_commondata_feedback_proof';
const controls=new Set(['apply','default','stop','pause','resume','snapshot','finish','fixture-low','fixture-high','fixture-hysteresis','fixture-below-release','fixture-stale','fixture-frozen','fixture-wrong-aircraft','fixture-none','fixture-context-loss','fixture-context-restore']);
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const sha256=bytes=>createHash('sha256').update(bytes).digest('hex');
async function atomicJson(path,value){
  const temporary=path+'.'+randomUUID()+'.tmp',file=await open(temporary,'wx');
  try{await file.writeFile(JSON.stringify(value,null,2));await file.sync();}finally{await file.close();}
  for(let attempt=0;;attempt++){try{await rename(temporary,path);break;}catch(error){if(!['EACCES','EPERM','EBUSY'].includes(error.code)||attempt>=19)throw error;await sleep(5);}}
}
async function until(work,description,timeout=15000){
  const end=Date.now()+timeout;let last;
  while(Date.now()<end){try{const value=await work();if(value)return value;}catch(error){if(error.fatal)throw error;last=error;}await sleep(25);}
  throw new Error(description+(last?': '+last.message:''));
}
function frame(payload){
  if(typeof payload!=='string')return null;
  let value;try{value=JSON.parse(payload);}catch{return null;}
  if(typeof value.type!=='string'||value.type.startsWith('transfer-'))return null;
  // Never retain pairing credentials, UI transfer bytes or another authority token.
  const {type,sequence,changes,values,sessionId,generation,ok,ackStage,feedback}=value;
  return{type,sequence,changes,values,sessionId,generation,ok,ackStage,feedback};
}

test('actual DCS-BIOS CommonData protocol fixtures reach the real companion through native profile and WSS',{timeout:220000},async()=>{
  const original=process.env.OSG_COMPANION_DCS_LIVE_EXE;
  assert.ok(original,'Set OSG_COMPANION_DCS_LIVE_EXE to the freshly built native test executable');
  const archive=resolve(process.env.OSG_DCS_SETUP_ARCHIVE??'.research/p5-altitude-audit/DCS-BIOS_v0.11.7.zip');
  const archiveHash=sha256(await readFile(archive));
  const directory=resolve(process.env.OSG_COMPANION_DCS_LIVE_DIR??join('.research','profile-companion-dcs-live-'+randomUUID()));
  await mkdir(directory,{recursive:true});
  assert.deepEqual(await readdir(directory),[],'Use a fresh empty proof directory; old native actions must never be replayed');
  const driverBytes=await readFile(new URL(import.meta.url));await writeFile(join(directory,'driver-source.mjs'),driverBytes);
  const executable=join(directory,'native-tests.exe'),nativeBytes=await readFile(resolve(original));
  await copyFile(resolve(original),executable);
  assert.equal(sha256(await readFile(executable)),sha256(nativeBytes));
  const log=await open(join(directory,'native-test.log'),'w');
  const child=spawn(executable,['--ignored','--exact',nativeTest,'--nocapture'],{
    cwd:resolve('.'),windowsHide:true,stdio:['ignore',log.fd,log.fd],
    env:{...process.env,OSG_DCS_SETUP_ARCHIVE:archive,OSG_COMPANION_DCS_LIVE_DIR:directory,LLVM_PROFILE_FILE:join(directory,'native-%m-%p.profraw')},
  });
  let nativeExit;
  const exited=()=>Object.assign(new Error('Native helper exited '+JSON.stringify(nativeExit)),{fatal:true});
  const ended=new Promise(resolve=>{child.once('exit',(code,signal)=>{nativeExit={code,signal};resolve(nativeExit);});child.once('error',error=>{nativeExit={error:error.message};resolve(nativeExit);});});
  let browser,page,sequence=0,finished=false;
  const stages=[],wire={sent:[],received:[],urls:[]},pageErrors=[];
  const handoff=async()=>JSON.parse(await readFile(join(directory,'handoff.json'),'utf8'));
  const action=async action=>{
    assert.ok(controls.has(action),'Only fixed native fixture/lifecycle actions are accepted');
    if(nativeExit)throw exited();
    if(action==='apply')await until(async()=>{const state=await handoff(),instance=state.host?.instances?.find(row=>row.instanceId===state.instanceId);return instance?.connected&&instance.armed;},'An actual connected, armed browser baseline before Apply');
    const next=++sequence;
    // Sequence is an acknowledgement nonce. The action is an enum; no telemetry
    // number, frame, ownership receipt or source sample can be sent here.
    await atomicJson(join(directory,'actions.json'),{sequence:next,action});
    const state=await until(async()=>{const state=await handoff();if(state.sequence===next)return state;if(nativeExit)throw exited();},'Native action '+action);
    assert.equal(state.lastError,null,'Native action '+action+': '+state.lastError);return state;
  };
  try{
    const initial=await until(async()=>{if(nativeExit)throw exited();const state=await handoff();return state.phase==='polling'?state:null;},'Native isolated DCS UDP and HTTPS/WSS listeners',25000);
    assert.equal(initial.protocolFixture,true);assert.equal(initial.actualGame,false);
    assert.equal(initial.rawPort,'channel:common:CommonData:ALT_MSL_FT:0');assert.equal(initial.measurementValidityAdvertised,false);
    assert.equal(initial.games.catalogue.find(game=>game.id==='dcs').integrationSetup.connection.freshnessMs,1500,'Use the actual production-default provider freshness policy');
    const url=new URL(initial.browserUrl);assert.equal(url.protocol,'https:');assert.ok(['127.0.0.1','localhost'].includes(url.hostname));
    browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
    const context=await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1440,height:1050}});
    await context.addInitScript(credential=>localStorage.setItem('osg-browser-credential-v2',JSON.stringify(credential)),{token:initial.token,companionId:initial.companionId,hostId:initial.hostId});
    page=await context.newPage();page.on('pageerror',error=>pageErrors.push(error.message));
    // Playwright observes the real transport. It does not replace, drop, resend
    // or synthesize any browser request or native reply.
    page.on('websocket',socket=>{
      wire.urls.push(socket.url());
      socket.on('framesent',event=>{const value=frame(event.payload);if(value)wire.sent.push(value);});
      socket.on('framereceived',event=>{const value=frame(event.payload);if(value)wire.received.push(value);});
    });
    await page.goto(initial.browserUrl);await page.bringToFront();
    const indicator=page.locator('section.widget[aria-label="Above 3000 m / release below 2900 m"]');
    const label=page.locator('section.widget[aria-label="Reported MSL altitude in metres"]');
    const active=()=>expect(page.getByText('Connected · panel active',{exact:true})).toBeVisible({timeout:15000});
    const safe=async()=>{await expect(indicator.locator('.indicator-value')).not.toHaveClass(/on/);await expect(indicator.locator('output')).not.toHaveText('Active');await expect(label.locator('output')).not.toHaveText(/^ALT (?:3048\.0|2956\.6) m$/);};
    const low=async()=>{await active();await expect(indicator.locator('output')).toHaveText('Inactive');await expect(label.locator('output')).toHaveText('Unknown');await safe();};
    const high=async(text='ALT 3048.0 m')=>{await active();await expect(indicator.locator('output')).toHaveText('Active');await expect(indicator.locator('.indicator-value')).toHaveClass(/on/);await expect(label.locator('output')).toHaveText(text);for(const output of [indicator,label]){await expect(output.locator('footer')).toContainText('local');await expect(output.locator('footer')).toContainText('host');await expect(output.locator('footer')).not.toContainText('game-confirmed');}};
    const snapshot=async(name)=>{
      const state=await action('snapshot'),{token,...native}=state;
      const view=await page.evaluate(()=>({status:document.querySelector('.toolbar')?.textContent,widgets:[...document.querySelectorAll('section.widget.output')].map(node=>({label:node.getAttribute('aria-label'),text:node.querySelector('output')?.textContent,footer:node.querySelector('footer')?.textContent,lit:!!node.querySelector('.indicator-value.on')}))}));
      stages.push({name,native,view});await page.screenshot({path:join(directory,name+'.png'),fullPage:true});return state;
    };
    const observed=async(nodeId,portId,status,value)=>until(async()=>{
      const state=await handoff(),port=state.inspection?.ports?.find(port=>port.endpoint.nodeId===nodeId&&port.endpoint.portId===portId);
      if(port?.sample.status!==status)return;
      if(status==='known'&&port.sample.value?.kind!=='scalar')return;
      if(status==='known'){const actual=port.sample.value.value;if(typeof value==='number'&&!Number.isInteger(value)?typeof actual!=='number'||!Number.isFinite(actual)||Math.abs(actual-value)>1e-9:actual!==value)return;}
      return state;
    },`Actual native cache ${nodeId}/${portId}: ${status}${value===undefined?'':' '+value}`,6000);
    await active();await expect(indicator.locator('output')).toHaveText('Unknown');await expect(label.locator('output')).toHaveText('Unknown');const beforeApply=await snapshot('before-apply-unknown');assert.equal(beforeApply.profileSession.owner,null);assert.equal(beforeApply.inspection,null);
    const applied=await action('apply');assert.equal(applied.profileSession.state,'running');assert.equal(applied.profileSession.owner.gameId,'dcs');
    const unknown=await observed('dcs','altitude','unknown');await observed('altitude-quantity','feet','unknown');
    const captured=(nodeId,portId)=>unknown.inspection.ports.find(port=>port.endpoint.nodeId===nodeId&&port.endpoint.portId===portId);
    assert.deepEqual(captured('dcs','altitude').contract.schema,{type:'integer',min:0,max:65535,unit:null});assert.deepEqual(captured('altitude-quantity','feet').contract.schema,{type:'integer',min:0,max:65535,unit:'ft'});
    await expect(indicator.locator('output')).toHaveText('Unknown');await expect(label.locator('output')).toHaveText('Unknown');await safe();await snapshot('raw-unknown');
    await action('fixture-low');await observed('dcs','altitude','known',9000);await observed('altitude-quantity','feet','known',9000);await observed('feet-to-metres','metres','known',2743.2);await low();await snapshot('reported-low');
    await action('fixture-high');await observed('dcs','altitude','known',10000);await observed('altitude-quantity','feet','known',10000);await observed('feet-to-metres','metres','known',3048);await high();await snapshot('reported-high');
    await action('fixture-hysteresis');await observed('dcs','altitude','known',9700);await observed('altitude-threshold','warning','known',true);await high('ALT 2956.6 m');await snapshot('hysteresis-retains-warning');
    await action('fixture-below-release');await observed('dcs','altitude','known',9500);await observed('altitude-threshold','warning','known',false);await low();await snapshot('below-release');
    await action('fixture-high');await high();await action('fixture-stale');await observed('dcs','altitude','stale');await observed('altitude-quantity','feet','stale');await expect(indicator.locator('output')).toContainText('Stale',{timeout:5000});await safe();await snapshot('telemetry-stale');
    await action('fixture-low');await low();await snapshot('fresh-low-baseline');
    await action('fixture-high');await high();const frozenStart=await action('fixture-frozen');assert.ok(Number.isInteger(frozenStart.fixture.completedExportCounter)&&frozenStart.fixture.completedExportCounter>=0&&frozenStart.fixture.completedExportCounter<=255);await observed('dcs','altitude','stale');await observed('altitude-quantity','feet','stale');await expect(indicator.locator('output')).toContainText('Stale',{timeout:5000});await safe();const frozenEnd=await snapshot('frozen-export-counter');assert.equal(frozenEnd.fixture.completedExportCounter,frozenStart.fixture.completedExportCounter);assert.ok(frozenEnd.fixture.packetsSent>frozenStart.fixture.packetsSent,'Real UDP packets continue while the completed export counter is frozen');
    await action('fixture-low');await low();await action('fixture-high');await high();
    await action('fixture-wrong-aircraft');await observed('dcs','altitude','stale');await safe();await snapshot('wrong-aircraft');
    await action('fixture-low');await low();await action('fixture-high');await high();
    await action('fixture-none');await observed('dcs','altitude','stale');await safe();await snapshot('none-aircraft');
    await action('fixture-low');await low();await action('fixture-high');await high();
    await action('fixture-context-loss');await observed('dcs','altitude','unavailable');await safe();await snapshot('process-context-lost');
    await action('stop');await action('fixture-context-restore');await action('fixture-low');
    const reapplied=await action('apply');assert.equal(reapplied.profileSession.state,'running');assert.notDeepEqual(reapplied.profileSession.owner,applied.profileSession.owner);await low();await snapshot('explicit-new-context-baseline');
    await action('fixture-high');await high();const paused=await action('pause');assert.equal(paused.profileSession.state,'paused');await safe();await snapshot('native-pause');
    const resumed=await action('resume');assert.equal(resumed.profileSession.state,'stopped');await sleep(400);await safe();await snapshot('resume-does-not-apply');
    await action('fixture-low');await action('apply');await low();await action('fixture-high');await high();
    const defaulted=await action('default');assert.equal(defaulted.profileSession.state,'running');assert.equal(defaulted.profileSession.appliedSnapshot.profileId,'00000000-0000-4000-8000-000000000001');await safe();await snapshot('default-withdraws-feedback');
    await action('fixture-low');await action('apply');await low();await action('fixture-high');await high();
    const stopped=await action('stop');assert.equal(stopped.profileSession.state,'stopped');await safe();await snapshot('stop-withdraws-feedback');
    assert.ok(wire.received.some(frame=>frame.feedback?.values?.some(value=>value.value==='ALT 3048.0 m'&&value.provenance==='local'&&value.sourceKind==='host')));
    assert.ok(wire.received.some(frame=>frame.feedback?.values?.some(value=>value.status==='stale')));
    assert.ok(!wire.received.some(frame=>frame.feedback?.values?.some(value=>value.provenance==='game-confirmed')));
    assert.ok(!wire.sent.some(frame=>frame.type==='input'&&frame.changes?.some(change=>change.value===true)),'The browser did not author a high input or telemetry sample');
    for(const address of wire.urls){const socket=new URL(address);assert.equal(socket.protocol,'wss:');assert.equal(socket.host,url.host);}
    assert.deepEqual(pageErrors,[]);
    await action('finish');finished=true;assert.deepEqual(await ended,{code:0,signal:null});
    assert.equal(sha256(await readFile(new URL(import.meta.url))),sha256(driverBytes),'Driver source stayed unchanged during the proof');
    await atomicJson(join(directory,'evidence.json'),{schemaVersion:1,result:'passed',recordedAt:new Date().toISOString(),nativeTest,nativeExecutableSha256:sha256(nativeBytes),driverSha256:sha256(driverBytes),dcsArchiveSha256:archiveHash,browserVersion:browser.version(),browserChannel:process.env.OSG_BROWSER_CHANNEL??'chromium',userAgent:await page.evaluate(()=>navigator.userAgent),nodeVersion:process.version,protocolFixture:true,actualGame:false,scope:'Actual loopback DCS-BIOS UDP reader, native compiler/reducer/Host/Coordinator, real HTTPS/WSS and shared PanelCanvas. Native process Platform is injected; no running DCS, physical device or game-confirmed result.',control:'Fixed enum actions only; sequence is acknowledgement metadata. No numeric telemetry payload, IPC interception, input/feedback injection or transport modification.',stages,wire,pageErrors});
    console.log('Native DCS protocol/browser proof evidence: '+directory);
  }catch(error){
    if(page){await page.screenshot({path:join(directory,'failure.png'),fullPage:true}).catch(()=>{});await atomicJson(join(directory,'failure.json'),{error:error.stack,pageErrors,stages,body:await page.locator('body').innerText().catch(()=>''),native:await handoff().then(({token,...safe})=>safe).catch(()=>null),wire});}
    throw error;
  }finally{
    if(!finished&&nativeExit===undefined){await action('finish').catch(()=>{});await Promise.race([ended,sleep(2000)]);}
    if(nativeExit===undefined)child.kill();
    await browser?.close();await log.close();
  }
});
