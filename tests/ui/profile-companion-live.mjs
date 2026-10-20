// Real-browser proof: no IPC mock, fabricated input/feedback frame, delivery token,
// game process or physical device. Native setup authenticates an isolated paired
// browser; only the rendered toggle supplies samples to the profile reducer.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
import {copyFile,mkdir,open,readFile,readdir,rename} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {randomUUID,createHash} from 'node:crypto';
const require=createRequire(import.meta.url), module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module), {expect}=require(module+'/test');
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function atomicJson(path,value){
 const temporary=path+'.'+randomUUID()+'.tmp',file=await open(temporary,'wx');
 try{await file.writeFile(JSON.stringify(value,null,2));await file.sync();}finally{await file.close();}
 for(let attempt=0;;attempt++){try{await rename(temporary,path);break;}catch(error){if(!['EACCES','EPERM','EBUSY'].includes(error.code)||attempt>=19)throw error;await sleep(5);}}
}
async function until(work,description,timeout=15000){
 const end=Date.now()+timeout;let last;
 while(Date.now()<end){try{const value=await work();if(value)return value;}catch(error){last=error;}await sleep(25);}
 throw new Error(description+(last?': '+last.message:''));
}
test('actual browser WSS runs authored altitude presets through native profile and shared renderer',{timeout:170000},async()=>{
 const original=process.env.OSG_COMPANION_LIVE_EXE;
 assert.ok(original,'Set OSG_COMPANION_LIVE_EXE to the freshly built native test executable');
 const directory=resolve(process.env.OSG_COMPANION_LIVE_DIR??join('.research','profile-companion-live-'+randomUUID()));
 await mkdir(directory,{recursive:true});
 assert.deepEqual(await readdir(directory),[],'Choose a fresh empty evidence directory; prior handoff/actions must never be reused');
 const executable=join(directory,'native-tests.exe');await copyFile(resolve(original),executable);
 const log=await open(join(directory,'native-test.log'),'w');
 const child=spawn(executable,['--ignored','--exact','profile_session_host::companion::tests::live::actual_browser_wss_profile_altitude_feedback_proof','--nocapture'],{
   cwd:resolve('.'),windowsHide:true,stdio:['ignore',log.fd,log.fd],env:{...process.env,OSG_COMPANION_LIVE_DIR:directory}
 });
 let nativeExit;const ended=new Promise(resolve=>{child.once('exit',(code,signal)=>{nativeExit={code,signal};resolve(nativeExit);});});
 child.once('error',error=>{nativeExit={error:error.message};});
 let browser,page,sequence=0,finished=false;
 const handoff=async()=>JSON.parse(await readFile(join(directory,'handoff.json'),'utf8'));
 const action=async action=>{
   if(action==='apply')await until(async()=>{
     const state=await handoff(),instance=state.host?.instances?.find(row=>row.instanceId===state.instanceId);
     return instance?.connected&&instance.armed&&instance.values?.['flight-input-1']===false;
   },'Actual neutral browser baseline before explicit Apply');
   const next=++sequence;await atomicJson(join(directory,'actions.json'),{sequence:next,action});
   return until(async()=>{const state=await handoff();if(state.sequence!==next)return;assert.equal(state.lastError,null,'Native action '+action+': '+state.lastError);return state;},'Native action '+action);
 };
 const stages=[],pageErrors=[];
 try{
   const initial=await until(async()=>{if(nativeExit)throw new Error('Native helper exited '+JSON.stringify(nativeExit));const state=await handoff();return state.phase==='polling'?state:null;},'Native isolated WSS listener',20000);
   assert.equal(initial.simulatorTelemetry,false);assert.deepEqual(initial.authoredPresetFeet,{'false':9000,'true':12000});
   browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
   const context=await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1440,height:1050}});
   await context.addInitScript(credential=>{
     localStorage.setItem('osg-browser-credential-v2',JSON.stringify(credential));
     const NativeWebSocket=window.WebSocket;
     const proof=window.__osgProof={sent:[],replies:[],sockets:[],hold:false,disconnect:false};
     window.WebSocket=class extends NativeWebSocket{
       constructor(...args){
         super(...args);proof.sockets.push(this);
         this.addEventListener('open',()=>{if(proof.disconnect)this.close();});
         this.addEventListener('message',event=>{
           if(typeof event.data!=='string')return;let value;try{value=JSON.parse(event.data);}catch{return;}
           if(value.type.startsWith('transfer-'))return;
           proof.replies.push({type:value.type,ok:value.ok,ackStage:value.ackStage,sessionId:value.sessionId,generation:value.generation,feedback:value.feedback});
         });
       }
       send(bytes){
         if(proof.hold)return; // Drop transport traffic, never create a replacement frame.
         const frame=JSON.parse(bytes),{type,sequence,changes,values,sessionId,generation}=frame;
         proof.sent.push({type,sequence,changes,values,sessionId,generation});
         return super.send(bytes);
       }
     };
   },{token:initial.token,companionId:initial.companionId,hostId:initial.hostId});
   page=await context.newPage();page.on('pageerror',error=>pageErrors.push(error.message));
   await page.goto(initial.browserUrl);await page.bringToFront();
   const toggle=page.getByRole('button',{name:'Authored altitude: 9000 / 12000 ft',exact:false});
   const indicator=page.locator('section.widget[aria-label="Above 3000 m / release below 2900 m"]');
   const label=page.locator('section.widget[aria-label="Authored altitude in metres"]');
   const active=()=>expect(page.getByText('Connected · panel active',{exact:true})).toBeVisible({timeout:15000});
   const snapshot=async(name,screenshot=false)=>{
     const state=await action('snapshot');
     const view=await page.evaluate(()=>({status:document.querySelector('.toolbar')?.textContent,widgets:[...document.querySelectorAll('section.widget.output')].map(node=>({label:node.getAttribute('aria-label'),text:node.querySelector('output')?.textContent,footer:node.querySelector('footer')?.textContent,lit:!!node.querySelector('.indicator-value.on')}))}));
     stages.push({name,profileSession:state.profileSession,instance:state.host.instances[0],view});
     if(screenshot)await page.screenshot({path:join(directory,name+'.png'),fullPage:true});
   };
   const high=async()=>{
     await active();await expect(toggle).toHaveAttribute('aria-pressed','false');await toggle.click();
     await expect(indicator.locator('output')).toHaveText('Active');await expect(label.locator('output')).toHaveText('ALT 3657.6 m');
     await expect(indicator.locator('.indicator-value')).toHaveClass(/on/);
     await expect(label.locator('footer')).toContainText('local');
     await expect(label.locator('footer')).toContainText('host');
     await expect(label.locator('footer')).not.toContainText('game-confirmed');
   };
   await active();await snapshot('before-apply');
   const applied=await action('apply');assert.equal(applied.profileSession.state,'running');assert.ok(applied.profileSession.appliedSnapshot);
   await active();await expect(indicator.locator('output')).toHaveText('Inactive');await expect(label.locator('output')).toHaveText('Unknown');
   await snapshot('authored-altitude-low',true);
   await high();await snapshot('authored-altitude-high',true);
   // The same UI toggle selects the below-release preset; native Gate clears text.
   await toggle.click();await expect(indicator.locator('output')).toHaveText('Inactive');await expect(label.locator('output')).toHaveText('Unknown');await snapshot('below-release');
   await high();
   const inputCount=()=>page.evaluate(()=>window.__osgProof.sent.filter(frame=>frame.type==='input').length);
   const beforeReset=await inputCount();
   await page.getByRole('button',{name:'Clear visual state',exact:true}).click();await expect(label.locator('output')).toHaveText('Unknown');
   assert.equal(await inputCount(),beforeReset,'Visual reset must not replay input');
   await page.getByRole('button',{name:'Resync visuals',exact:true}).click();await expect(label.locator('output')).toHaveText('ALT 3657.6 m');
   assert.equal(await inputCount(),beforeReset,'Visual resync must not replay input');await snapshot('visual-resync');
   // A withheld real WSS request lets the renderer age its authentic last sample.
   await page.evaluate(()=>{window.__osgProof.hold=true;});
   await expect(indicator.locator('output')).toContainText('Stale',{timeout:2500});
   await expect(indicator.locator('.indicator-value')).not.toHaveClass(/on/);await snapshot('stale-feedback',true);
   await page.evaluate(()=>{window.__osgProof.hold=false;window.__osgProof.disconnect=true;for(const socket of window.__osgProof.sockets)socket.close();});
   await expect(indicator.locator('output')).toHaveText('Disconnected',{timeout:2000});await expect(label.locator('output')).toHaveText('Disconnected');
   await snapshot('disconnected-feedback',true);
   await page.evaluate(()=>{window.__osgProof.disconnect=false;});await active();
   await expect(toggle).toHaveAttribute('aria-pressed','false');await expect(label.locator('output')).toHaveText('Unknown');await snapshot('reconnected-baseline',true);
   await high();const stoppedBeforeRestart=await action('stop');assert.equal(stoppedBeforeRestart.profileSession.state,'stopped');
   assert.equal(stoppedBeforeRestart.profileSession.owner,null);
   await expect(indicator.locator('.indicator-value')).not.toHaveClass(/on/);await expect(label.locator('output')).not.toHaveText('ALT 3657.6 m');await snapshot('native-stop',true);
   await sleep(400);await expect(label.locator('output')).not.toHaveText('ALT 3657.6 m');await snapshot('stop-requires-apply');
   await action('apply');await active();await expect(toggle).toHaveAttribute('aria-pressed','false');await high();
   const defaulted=await action('default');assert.equal(defaulted.profileSession.state,'running');assert.equal(defaulted.profileSession.appliedSnapshot.profileId,'00000000-0000-4000-8000-000000000001');
   await expect(label.locator('output')).not.toHaveText('ALT 3657.6 m');await expect(indicator.locator('.indicator-value')).not.toHaveClass(/on/);await snapshot('switched-default',true);
   await action('apply');await active();await expect(toggle).toHaveAttribute('aria-pressed','false');await high();
   const stopped=await action('stop');assert.equal(stopped.profileSession.state,'stopped');
   await expect(label.locator('output')).not.toHaveText('ALT 3657.6 m');await expect(indicator.locator('.indicator-value')).not.toHaveClass(/on/);await snapshot('native-stop',true);
   const wire=await page.evaluate(()=>({sent:window.__osgProof.sent,replies:window.__osgProof.replies}));
   assert.ok(wire.sent.some(frame=>frame.type==='input'&&frame.changes?.some(change=>change.value===true)));
   assert.ok(wire.replies.some(frame=>frame.feedback?.values?.some(value=>value.value==='ALT 3657.6 m'&&value.provenance==='local'&&value.sourceKind==='host')));
   assert.equal(pageErrors.length,0,JSON.stringify(pageErrors));
   await action('finish');finished=true;
   assert.deepEqual(await ended,{code:0,signal:null});
   const bytes=await readFile(executable);
   await atomicJson(join(directory,'evidence.json'),{schemaVersion:1,result:'passed',recordedAt:new Date().toISOString(),nativeExecutableSha256:createHash('sha256').update(bytes).digest('hex'),browserVersion:browser.version(),simulatorTelemetry:false,source:'Real rendered browser toggle selects authored 9000/12000 ft presets; native Select → UnitConvert → Hysteresis(3000/2900) → Gate → Format → Hub WSS → shared PanelCanvas.',transportFaults:'Only real outgoing WSS traffic was withheld/closed for stale/disconnected stages; no input or feedback frames were injected.',stages,wire,pageErrors});
   console.log('Native/browser proof evidence: '+directory);
 }catch(error){
   if(page){await page.screenshot({path:join(directory,'failure.png'),fullPage:true}).catch(()=>{});await atomicJson(join(directory,'failure.json'),{error:error.stack,pageErrors,stages,body:await page.locator('body').innerText().catch(()=>''),native:await handoff().then(({token,...safe})=>safe).catch(()=>null),wire:await page.evaluate(()=>({sent:window.__osgProof?.sent,replies:window.__osgProof?.replies})).catch(()=>null)});}
   throw error;
 }finally{
   if(!finished&&nativeExit===undefined){await action('finish').catch(()=>{});await Promise.race([ended,sleep(2000)]);}
   if(nativeExit===undefined)child.kill();
   await browser?.close();await log.close();
 }
});
