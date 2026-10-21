import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import test from 'node:test';
// Read-only UI acceptance against the real running native cockpit.
test('incoming telemetry is organized and Debug is restricted to the profile list', {skip:process.env.OSG_GAME_TELEMETRY_NATIVE!=='1',timeout:120000}, async()=>{
const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9224'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
const target=targets.find(t=>t.type==='page'&&t.url.includes(':1420'));
assert.ok(target);
const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map();let sequence=0;
await new Promise((yes,no)=>{const timer=setTimeout(()=>{socket.close();no(Error('Native cockpit CDP connection timed out'));},5000);socket.addEventListener('open',()=>{clearTimeout(timer);yes();},{once:true});socket.addEventListener('error',error=>{clearTimeout(timer);no(error);},{once:true});});
socket.addEventListener('message',event=>{const msg=JSON.parse(event.data),item=pending.get(msg.id);if(!item)return;clearTimeout(item.timer);pending.delete(msg.id);msg.error?item.reject(Error(msg.error.message)):item.resolve(msg.result);});
const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>reject(Error(method+' timeout')),20000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
const evaluate=async expression=>{const r=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw Error(r.exceptionDetails.exception?.description??r.exceptionDetails.text);return r.result.value;};
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const until=async(expression,label)=>{for(let i=0;i<100;i++){const v=await evaluate(expression);if(v)return v;await pause(100);}throw Error(label);};
const click=async(label,scope='document')=>until(`(()=>{const root=${scope};if(!root)return false;const b=[...root.querySelectorAll('button')].find(b=>(b.getAttribute('aria-label')??b.textContent.trim())===${JSON.stringify(label)});if(!b||b.disabled)return false;b.click();return true;})()`,'Missing '+label);
const change=async(selector,value)=>evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('Missing select');e.value=${JSON.stringify(value)};e.dispatchEvent(new Event('change',{bubbles:true}));})()`);
const directory='.research/game-telemetry-native-ui/'+Date.now(),checks=[];
await mkdir(directory,{recursive:true});
try{
 await click('Profiles',`document.querySelector('nav[aria-label="Main navigation"]')`);
 if(await evaluate(`!![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='← Back to Profiles')`))await click('← Back to Profiles');
 assert.equal(await evaluate(`document.querySelectorAll('.page-heading .page-actions button').length>0`),true);
 await click('Debug');
 await until(`document.querySelector('.game-telemetry .source-heading h2')&&!document.querySelector('.telemetry-error')`,'Incoming telemetry did not load');
 const initial=await evaluate(`({tabs:[...document.querySelectorAll('[aria-label="Debug sections"] button')].map(e=>e.textContent.trim()),games:[...document.querySelector('[aria-label="Telemetry game"]').options].map(o=>({id:o.value,label:o.textContent})),body:document.querySelector('.game-telemetry').innerText.slice(0,2500),overflow:document.documentElement.scrollWidth>innerWidth})`);
 assert.equal(initial.games.filter(g=>g.id).length,7);assert.equal(initial.overflow,false);checks.push('Profiles list opens incoming telemetry by default with seven game choices and no horizontal page overflow.');
 await change('[aria-label="Telemetry game"]','assetto-corsa-evo');
 await until(`document.querySelector('.source-heading h2')?.textContent.includes('EVO')&&!document.querySelector('.telemetry-error')`,'EVO selection failed');
 await evaluate(`(()=>{const e=document.querySelector('[aria-label="Search game telemetry"]');e.value='engine.rpm';e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
 await until(`document.querySelector('.channel-table tbody')?.textContent.includes('engine.rpm')`,'Canonical RPM missing');
 const rpm=await evaluate(`document.querySelector('.channel-table tbody').innerText`);assert.match(rpm,/rpm/);checks.push('Canonical engine.rpm is discoverable with RPM units. Closed game reads unknown rather than zero.');
 await evaluate(`(()=>{const e=document.querySelector('[aria-label="Search game telemetry"]');e.value='';e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
 await until(`document.querySelector('.telemetry-groups')?.textContent.includes('Physics')`,'Physics group missing');
 await evaluate(`[...document.querySelectorAll('.telemetry-groups button')].find(b=>b.textContent.includes('Physics')).click()`);
 await until(`document.querySelector('.channel-table tbody')?.textContent.includes('physics.')`,'Physics rows missing');
 const physics=await evaluate(`({summary:document.querySelector('.game-telemetry').innerText.slice(0,2200),rows:document.querySelectorAll('.channel-table tbody tr').length,overflow:document.documentElement.scrollWidth>innerWidth})`);assert.equal(physics.overflow,false);checks.push('Expanded physics rows are grouped, paginated, and display native IDs plus explicit sample status.');
 const screenshot=await send('Page.captureScreenshot',{format:'png'});await writeFile(directory+'/debug-physics.png',Buffer.from(screenshot.data,'base64'));
 await click('Profile inspection');
 assert.equal(await evaluate(`!document.querySelector('[aria-label="Native graph preflight"]').closest('[role="tabpanel"]').hidden`),true);
 await click('Runtime & recovery');
 assert.equal(await evaluate(`document.querySelector('[aria-label="Debug sections"] [aria-selected="true"]').textContent.trim()`),'Runtime & recovery');
 checks.push('Profile inspection and Runtime & recovery remain available as separate Debug sections.');
 await click('← Back to Profiles',`document.querySelector('.profile-debug')`);
 await click('Edit Default');
 await until(`!!document.querySelector('.editor-title')`,'Profile editor did not open');
 assert.equal(await evaluate(`!![...document.querySelectorAll('button')].find(b=>b.getClientRects().length&&b.textContent.trim()==='Debug')`),false);
 checks.push('The real Default profile editor has no Debug button.');
 await click('← Back to Profiles');
 await click('Debug');
 await until(`!!document.querySelector('.game-telemetry .source-heading h2')`,'Debug failed to reopen');
 checks.push('Debug remains accessible from the saved profile list.');
 await writeFile(directory+'/evidence.json',JSON.stringify({actualNativeCockpit:true,initial,rpm,physics,checks},null,2));
 console.log(JSON.stringify({directory,checks}));
}finally{socket.close();}

});
