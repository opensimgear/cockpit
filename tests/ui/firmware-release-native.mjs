// Read-only acceptance against the already-running development Cockpit.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

test('running dev Cockpit reports local firmware without downloading, flashing or publishing', {timeout:30000}, async()=>{
  const page=await nativePage();
  try {
    await until(()=>page.evaluate("typeof window.__TAURI_INTERNALS__?.invoke === 'function'"));
    const call=async command=>{
      const result=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({command,clientId:'firmware-release-native'})}})`);
      assert.equal(result.ok,true,result.error);return result.state.companions;
    };
    const before=await call('companion_status');
    const checked=await call('companion_firmware_check');
    assert.equal(checked.firmwareSource,'development');
    assert.equal(checked.firmwareChecked,true);
    assert.ok(typeof checked.firmwareVersion==='string'&&checked.firmwareVersion.length>0);
    if(process.env.OSG_EXPECTED_LOCAL_FIRMWARE)assert.equal(checked.firmwareVersion,process.env.OSG_EXPECTED_LOCAL_FIRMWARE);
    assert.deepEqual(checked.firmwareProfiles.map(profile=>profile.id).sort(),['sunton-3248s035r','waveshare-lcd7-s3']);
    for(const profile of checked.firmwareProfiles){
      assert.equal(profile.source,'development');
      assert.equal(profile.available,true,profile.error??profile.id);
      assert.equal(profile.version,checked.firmwareVersion);
    }
    const after=await call('companion_status');
    assert.equal(after.firmwareVersion,checked.firmwareVersion);
    assert.equal(after.usb?.flashing,before.usb?.flashing);
    const directory=resolve('.research/firmware-release-native');await mkdir(directory,{recursive:true});
    await writeFile(resolve(directory,'evidence.json'),JSON.stringify({actualNativeCockpit:true,readOnly:true,checked,checks:['Native dev source is local development firmware','Both board profiles are exposed','Firmware check does not flash or publish','Ordinary status remains readable']},null,2)+'\n');
  } finally {page.close();}
});
