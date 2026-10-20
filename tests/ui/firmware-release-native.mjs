// Read-only acceptance against the already-running development Cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require=createRequire(import.meta.url);
const {ws:RawSocket}=createRequire(require.resolve(process.env.OSG_PLAYWRIGHT_MODULE??'playwright'))('playwright-core/lib/utilsBundle');

test('running dev Cockpit reports local firmware without downloading, flashing or publishing', {timeout:30000}, async()=>{
  const targets=await(await fetch(`${process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222'}/json/list`,{signal:AbortSignal.timeout(5000)})).json();
  const endpoint=targets.find(target=>target.type==='page'&&/127\.0\.0\.1:1420|localhost:1420/.test(target.url))?.webSocketDebuggerUrl;
  assert.ok(endpoint,'The actual native development Cockpit must be running');
  const socket=new RawSocket(endpoint,{handshakeTimeout:5000});
  await new Promise((resolve,reject)=>{socket.once('open',resolve);socket.once('error',reject);});
  let sequence=0;const pending=new Map();
  socket.on('message',bytes=>{const response=JSON.parse(bytes.toString()),task=pending.get(response.id);if(!task)return;clearTimeout(task.timer);pending.delete(response.id);if(response.error)task.reject(new Error(response.error.message));else task.resolve(response.result);});
  socket.on('error',()=>{});
  const send=(method,params)=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out`));},12000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>{const result=await send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});if(result.exceptionDetails)throw new Error(result.exceptionDetails.exception?.description??result.exceptionDetails.text);return result.result.value;};
  try {
    assert.equal(await evaluate("typeof window.__TAURI_INTERNALS__?.invoke"),'function');
    const call=async command=>{
      const result=await evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command,clientId:'firmware-release-native'})}})`);
      assert.equal(result.ok,true,result.error);return result.state.companions;
    };
    const before=await call('companion_status');
    const checked=await call('companion_firmware_check');
    assert.equal(checked.firmwareSource,'development');
    assert.equal(checked.firmwareChecked,true);
    assert.ok(typeof checked.firmwareVersion==='string'&&checked.firmwareVersion.length>0);
    assert.deepEqual(checked.firmwareProfiles.map(profile=>profile.id).sort(),['sunton-3248s035r','waveshare-lcd7-s3']);
    assert.ok(checked.firmwareProfiles.every(profile=>profile.source==='development'));
    const after=await call('companion_status');
    assert.equal(after.firmwareVersion,checked.firmwareVersion);
    assert.equal(after.usb?.flashing,before.usb?.flashing);
    const directory=resolve('.research/firmware-release-native');await mkdir(directory,{recursive:true});
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({actualNativeCockpit:true,readOnly:true,checked,checks:['Native dev source is local development firmware','Both board profiles are exposed','Firmware check does not flash or publish','Ordinary status remains readable']},null,2)+'\n');
  } finally {for(const task of pending.values())clearTimeout(task.timer);socket.close();}
});
