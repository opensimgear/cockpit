import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

export function digest(hex) {
  assert.match(hex,/^(?:[0-9a-f]{2})+$/,'original bytes are nonempty lowercase hex');
  return createHash('sha256').update(Buffer.from(hex,'hex')).digest('hex');
}
export function inventoryEvidence(inventory) {
  assert.ok(Array.isArray(inventory.interfaces));
  assert.ok(Array.isArray(inventory.failures));
  assert.equal(typeof inventory.truncated,'boolean');
  return {origin:'native-linux-webkit-inventory-only',fixtureEvidence:false,
    hardwareAcceptance:'not-run',deviceOpenedByRunner:false,releaseQualified:false,
    inventoryComplete:!inventory.truncated&&inventory.failures.length===0,interfaceCount:inventory.interfaces.length,inventory};
}
export function framingFor(descriptor) {
  assert.deepEqual(descriptor.diagnostics,[],'qualification requires a structurally valid descriptor');
  const layouts=descriptor.layouts.filter(layout=>layout.kind==='input');
  assert.ok(layouts.length,'selected interface must declare an input layout for stream acceptance');
  assert.ok(layouts.every(layout=>layout.report_id===null)||layouts.every(layout=>Number.isInteger(layout.report_id)));
  return layouts[0].report_id===null?'unnumbered':'id-prefix';
}
export function validateProbe(probe) {
  assert.equal(probe.origin,'native-hidraw-read');
  assert.equal(probe.descriptorSource,'HIDIOCGRDESC');
  assert.equal(probe.fixtureEvidence,false);
  assert.equal(probe.reportWrites,false);
  assert.equal(probe.explicitInputReads,false);
  assert.equal(probe.featureReads,false);
  assert.equal(probe.completed,'duration','a bounded retained prefix is not a completed acceptance run');
  assert.equal(probe.truncated,false);
  assert.equal(digest(probe.descriptorHex),probe.descriptorSha256);
  assert.ok(probe.reports.length>0 && probe.reports.length<=4096,'no reports means input reception is unqualified');
  let bytes=0,previous=0n;
  for(const [index,report] of probe.reports.entries()) {
    assert.equal(report.sequence,String(index+1));
    assert.match(report.capturedUs,/^(?:0|[1-9][0-9]*)$/);
    const at=BigInt(report.capturedUs);assert.ok(at>=previous);previous=at;
    digest(report.wireHex);assert.ok(report.wireHex.length/2<=65536);bytes+=report.wireHex.length/2;
  }
  assert.equal(bytes,probe.retainedBytes);assert.ok(bytes<=8*1024*1024);
}
export function compareEvidence(probe,detail,records,oracle) {
  validateProbe(probe);
  assert.equal(detail.descriptorHex,probe.descriptorHex,'Cockpit descriptor equals independent HIDIOCGRDESC bytes');
  assert.equal(oracle.nativeDeviceAccess,false);
  assert.equal(oracle.provenance,'offline-portable-decoder-verification');
  assert.deepEqual(detail.descriptor,oracle.descriptor,'same original descriptor parses identically through both entry points');
  assert.equal(oracle.descriptor.raw,probe.descriptorHex);
  assert.ok(records.length>0,'Cockpit did not receive a report');
  const independent=new Set(probe.reports.map(report=>report.wireHex));
  const matches=records.filter(record=>independent.has(record.wireHex)).length;
  assert.ok(matches>0,'no exact wire-byte overlap with the independently opened hidraw reader');
  assert.equal(oracle.reports.length,records.length);
  let previous=0n;
  for(const [index,record] of records.entries()) {
    assert.match(record.sequence,/^[1-9][0-9]*$/);assert.ok(BigInt(record.sequence)>previous);previous=BigInt(record.sequence);
    assert.equal(record.samples.raw,record.wireHex,'projection retains the original app delivery');
    assert.deepEqual(record.samples,oracle.reports[index],'exact values/quality/framing/usages equal the portable CLI');
  }
  return {independentReports:probe.reports.length,cockpitComparedReports:records.length,exactWireOverlap:matches,
    independentDistinctReports:independent.size,cockpitDistinctReports:new Set(records.map(record=>record.wireHex)).size,
    packetSynchronized:false,physicalTransitionQualification:false,fullTravelQualification:false,
    featureReadQualification:false,outputQualification:false};
}
