// Offline integrity/structure verification. Does not contact or drive any device.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const sha=value=>createHash('sha256').update(value).digest('hex');
const candidate=process.argv[2]==='--candidate';
if(process.argv.length>3||process.argv.length===3&&!candidate)throw Error('Use no arguments or --candidate');
for(const [name,pid,count,rawCount,knownHash] of [
  ['231d-011f-idle',0x011f,10,63,'ca37f5c15ecf546bbd41c111effe599fbca9a1db13443167e64df52d392d5268'],
  ['231d-0200-idle',0x0200,137,62,'68c7c8f9d5ac5d7c60856114402a8180767b2c6dd13ef440369f6acf3c8220bb']]){
  const text=await readFile(new URL(`${candidate?'.generated/candidates/':''}${name}.json`,import.meta.url),'utf8'),fixture=JSON.parse(text);
  assert.ok(text.split('\n').length<500);
  assert.equal(fixture.fixtureId,name);assert.equal(fixture.device.vendorId,0x231d);assert.equal(fixture.device.productId,pid);
  assert.equal(fixture.device.virtual,false);assert.equal(fixture.device.usagePage,1);assert.equal(fixture.device.usage,4);
  assert.equal(fixture.descriptor.rawReportDescriptorAvailable,false);
  assert.equal(fixture.provenance.outputWrites,false);assert.equal(fixture.provenance.featureReads,false);
  assert.equal(fixture.provenance.keyboardMouseCaptured,false);assert.equal(fixture.provenance.deviceFileOpenAttempted,false);
  assert.ok(fixture.rawInput.reportCount>0&&fixture.rawInput.reportCount<=512);
  assert.equal(fixture.rawInput.samples.length,fixture.rawInput.reportCount);
  assert.ok(fixture.cockpitParsed.snapshotCount>0&&fixture.cockpitParsed.snapshotCount<=128);
  assert.equal(fixture.cockpitParsed.samples.length,fixture.cockpitParsed.snapshotCount);
  if(!candidate){
    assert.equal(fixture.rawInput.reportCount,rawCount);assert.deepEqual(fixture.rawInput.reportBytes,[64]);
    assert.equal(fixture.cockpitParsed.snapshotCount,76);assert.equal(fixture.cockpitParsed.controlCount,count);
    assert.equal(fixture.rawInput.states[0].sha256,knownHash);
  }
  const controls=fixture.cockpitParsed.controlCount;
  assert.ok(controls<=256);assert.equal(fixture.descriptor.osParsedCapabilities.length,controls);
  for(const row of fixture.rawInput.states){assert.match(row.reportHex,/^[0-9a-f]{128}$/);assert.equal(sha(Buffer.from(row.reportHex,'hex')),row.sha256);}
  for(const row of fixture.cockpitParsed.states){assert.equal(row.values.length,controls);assert.equal(sha(JSON.stringify(row.values)),row.sha256);}
  for(const section of [fixture.rawInput,fixture.cockpitParsed]){
    const changes=section.samples.slice(1).filter((row,index)=>row.stateId!==section.samples[index].stateId).length;
    assert.equal(section.observedTransitions,changes);
    if(!candidate){assert.equal(section.observedTransitions,0);assert.equal(section.states.length,1);}
    section.samples.forEach((row,index)=>{assert.ok(section.states.some(s=>s.id===row.stateId));
      if(index)assert.ok(row.elapsedMs>=section.samples[index-1].elapsedMs);});
  }
  assert.equal(fixture.qualification.physicalTransitions,false);
  assert.ok(!/HID#|USB#|\\\\\?\\|[A-Z]:\\Users\\|source-[a-f0-9]{16}/i.test(text),'Machine identity must be redacted');
  console.log(`${name}: ${fixture.rawInput.reportCount} raw reports, ${fixture.cockpitParsed.snapshotCount} OS-parsed snapshots, ${controls} controls; ${candidate?'candidate bounds':'archived hashes'} verified.`);
}
