import test from 'node:test';
import assert from 'node:assert/strict';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {options} from './verify-linux-receiver.mjs';
import {digest,framingFor,validateProbe,compareEvidence,inventoryEvidence} from './verify-linux-core.mjs';

function evidence() {
  const descriptor={raw:'0501',diagnostics:[],layouts:[{kind:'input',report_id:null}]};
  const sample={raw:'2a',values:[{sample:{Integer:'42'}}]};
  return {probe:{origin:'native-hidraw-read',descriptorSource:'HIDIOCGRDESC',fixtureEvidence:false,
    reportWrites:false,explicitInputReads:false,featureReads:false,completed:'duration',truncated:false,
    descriptorHex:'0501',descriptorSha256:digest('0501'),retainedBytes:1,
    reports:[{sequence:'1',capturedUs:'1234',wireHex:'2a'}]},
    detail:{descriptorHex:'0501',descriptor},records:[{sequence:'1',wireHex:'2a',samples:sample}],
    oracle:{nativeDeviceAccess:false,provenance:'offline-portable-decoder-verification',descriptor,reports:[sample]}};
}
test('CLI requires exact selection, local WebKit, explicit candidate output and hardware opt-in',()=>{
  const base=['--device','/dev/hidraw7','--webkit','http://127.0.0.1:9285','--output',join(tmpdir(),'candidate'),'--allow-hardware-read'];
  assert.equal(options(base).device,'/dev/hidraw7');
  assert.throws(()=>options(base.filter(value=>value!=='--allow-hardware-read')));
  assert.throws(()=>options(base.map(value=>value==='/dev/hidraw7'?'/dev/hidraw*':value)));
  assert.throws(()=>options(base.map(value=>value==='http://127.0.0.1:9285'?'http://remote:9285':value)));
  assert.throws(()=>options([...base,'--duration-ms','0']));
});
test('zero-device native inventory is software evidence with hardware acceptance not run',()=>{
  const args=['--inventory-only','--webkit','http://127.0.0.1:9285','--output',join(tmpdir(),'candidate')];
  assert.equal(options(args).inventoryOnly,true);
  assert.throws(()=>options([...args,'--allow-hardware-read']));
  assert.throws(()=>options([...args,'--device','/dev/hidraw7']));
  const receipt=inventoryEvidence({interfaces:[],failures:[],truncated:false});
  assert.equal(receipt.interfaceCount,0);assert.equal(receipt.hardwareAcceptance,'not-run');
  assert.equal(receipt.deviceOpenedByRunner,false);assert.equal(receipt.releaseQualified,false);
  assert.equal(inventoryEvidence({interfaces:[],failures:[],truncated:true}).inventoryComplete,false);
  assert.equal(inventoryEvidence({interfaces:[],failures:[{kind:'unavailable'}],truncated:false}).inventoryComplete,false);
});
test('fixture-shaped test data exercises assertions without declaring hardware qualification',()=>{
  const {probe,detail,records,oracle}=evidence();
  const result=compareEvidence(probe,detail,records,oracle);
  assert.equal(result.exactWireOverlap,1);assert.equal(result.packetSynchronized,false);
  assert.equal(result.physicalTransitionQualification,false);assert.equal(framingFor(detail.descriptor),'unnumbered');
  detail.descriptor.layouts[0].report_id=7;assert.equal(framingFor(detail.descriptor),'id-prefix');
});
test('missing, truncated, altered or non-hardware evidence cannot pass',()=>{
  for(const modify of [probe=>probe.fixtureEvidence=true,probe=>probe.truncated=true,
    probe=>probe.completed='capacity',probe=>probe.reports=[],probe=>probe.descriptorSha256='bad',
    probe=>probe.reports[0].sequence='2',probe=>probe.retainedBytes=0]) {
    const {probe}=evidence();modify(probe);assert.throws(()=>validateProbe(probe));
  }
  for(const modify of [data=>data.detail.descriptorHex='0502',data=>data.records[0].wireHex='2b',
    data=>data.records[0].samples={raw:'2a',values:[]},data=>data.oracle.nativeDeviceAccess=true]) {
    const data=evidence();modify(data);assert.throws(()=>compareEvidence(data.probe,data.detail,data.records,data.oracle));
  }
});
