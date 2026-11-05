// Native audit checkpoints in the running application; never authors input.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {nativePage} from '../../scripts/reactive/cdp.mjs';

test('native audit markers checkpoint a complete ordered prefix without input',{
  skip:process.platform!=='win32'||process.env.OSG_DESKTOP_AUDIT_NATIVE!=='1',timeout:30000,
},async()=>{
  const page=await nativePage();
  const request=async value=>{
    const reply=await page.evaluate(`window.__TAURI_INTERNALS__.invoke('gateway_request',{request:${JSON.stringify({...value,clientId:'audit-capacity-native'})}})`);
    assert.equal(reply.ok,true,`${value.command}: ${reply.error}`);return reply.state;
  };
  const check=report=>{
    assert.ok(report.configured&&report.enabled&&report.valid&&report.complete&&!report.finished,JSON.stringify(report));
    assert.equal(report.counters.lastWrittenSequence,report.throughSequence);
    for(const name of ['queueDrops','capDrops','aliasFailures','qpcFailures','invalidFields',
      'serializationErrors','ioErrors','flushTimeouts','evidenceFaults','afterFinish'])assert.equal(report.counters[name],'0',name);
  };
  try{
    const profile=(await request({command:'profile_status'})).profiles.profileSession;
    assert.equal(profile.owner,null);
    const before=(await request({command:'profile_desktop_audit_checkpoint'})).desktop.audit;check(before);
    const markers=[];
    for(let index=0;index<64;index++){
      const marker=(await request({command:'profile_desktop_audit_marker',phase:index%9})).desktop.auditMarker;
      assert.equal(marker.phase,index%9);
      if(markers.length)assert.ok(BigInt(marker.id)>BigInt(markers.at(-1).id));
      markers.push(marker);
    }
    const after=(await request({command:'profile_desktop_audit_checkpoint'})).desktop.audit;check(after);
    assert.ok(BigInt(after.throughSequence)-BigInt(before.throughSequence)>=64n);
    assert.equal((await request({command:'profile_status'})).profiles.profileSession.owner,null);
    const directory=`.research/reactive/audit-capacity-native/${Date.now()}`;
    await mkdir(directory,{recursive:true});
    await writeFile(directory+'/evidence.json',JSON.stringify({actualNativeCockpit:true,
      authoredInput:false,capacitySaturationQualifiedBy:'isolated blocked-writer unit tests',before,after,markers},null,2));
  }finally{page.close();}
});
