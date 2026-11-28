import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const MAX_DESCRIPTOR_BYTES=64*1024;
const MAX_ELEMENTS=16384;
const MAX_REPORTS=4096;

export function validateIndependentProbe(probe) {
  assert.equal(probe.origin,'independent-macos-iohid-elements');
  assert.equal(probe.fixtureEvidence,false);
  assert.equal(probe.deviceOpened,false,'identity probe must not open a device');
  assert.equal(probe.reportWrites,false);
  assert.equal(probe.inputReads,false);
  assert.equal(probe.featureReads,false);
  assert.match(probe.device?.registryEntryId??'',/^\d+$/,'select one exact IORegistry entry ID');
  if(probe.descriptorAvailable) {
    assert.match(probe.descriptorHex??'',/^(?:[a-f0-9]{2})+$/i);
    assert.ok(probe.descriptorHex.length/2<=MAX_DESCRIPTOR_BYTES);
    assert.match(probe.descriptorSha256??'',/^[a-f0-9]{64}$/i);
    assert.equal(createHash('sha256').update(Buffer.from(probe.descriptorHex,'hex')).digest('hex'),probe.descriptorSha256.toLowerCase());
  } else {
    assert.equal(probe.descriptorHex,null);
    assert.equal(probe.descriptorSha256,null);
  }
  assert.ok(Array.isArray(probe.elements)&&probe.elements.length>0&&probe.elements.length<=MAX_ELEMENTS);
  const cookies=new Set();
  for(const element of probe.elements) {
    assert.ok(Number.isInteger(element.cookie)&&element.cookie>=0);
    assert.ok(!cookies.has(element.cookie),'IOHID element cookies must be unique within the selected device');
    cookies.add(element.cookie);
    assert.ok(element.parentCookie===null||(Number.isInteger(element.parentCookie)&&element.parentCookie>=0));
    for(const key of ['usagePage','usage','reportId','reportSizeBits','reportCount','unit'])
      assert.ok(Number.isInteger(element[key])&&element[key]>=0,`invalid element ${key}`);
    for(const key of ['logicalMin','logicalMax','physicalMin','physicalMax'])
      assert.ok(element[key]===null||/^-?(?:0|[1-9]\d*)$/.test(element[key]),`invalid element ${key}`);
    for(const key of ['usageMin','usageMax'])
      assert.ok(element[key]===null||(Number.isInteger(element[key])&&element[key]>=0),`invalid element ${key}`);
    assert.ok(Number.isInteger(element.type)&&element.type>=0);
    assert.ok(element.collectionType===null||Number.isInteger(element.collectionType));
    assert.ok(Number.isInteger(element.unitExponent));
    for(const key of ['relative','wrapping','nonLinear','preferredState','virtual','array','nullState'])
      assert.equal(typeof element[key],'boolean',`invalid element ${key}`);
  }
  return probe;
}

function elementKey(element) {
  return String(element.cookie);
}

function sampleRawHex(raw) {
  if(typeof raw==='string')return raw.toLowerCase();
  if(Array.isArray(raw)&&raw.every(byte=>Number.isInteger(byte)&&byte>=0&&byte<=255))return Buffer.from(raw).toString('hex');
  return undefined;
}

export function compareMacEvidence(app,probe) {
  validateIndependentProbe(probe);
  assert.ok(['development','package'].includes(app.appBuildType),'identify the observed app build as development or package');
  assert.equal(app.fixtureEvidence,false);
  assert.equal(app.deviceOpenedByRunner,true);
  assert.equal(app.interfaceId,app.inventory?.interfaceId,'selection must use one current inventory interface ID');
  assert.equal(app.identity?.registryEntryId,probe.device.registryEntryId,'registry entry must match the independent probe');
  assert.equal(app.descriptorAvailable,probe.descriptorAvailable,'native and independent descriptor availability must agree');
  if(probe.descriptorAvailable) {
    assert.equal(app.descriptorHex?.toLowerCase(),probe.descriptorHex.toLowerCase(),'original report descriptor bytes must match');
    assert.equal(createHash('sha256').update(Buffer.from(app.descriptorHex,'hex')).digest('hex'),app.descriptorSha256?.toLowerCase());
    assert.equal(app.descriptorSha256?.toLowerCase(),probe.descriptorSha256.toLowerCase(),'descriptor digest must match');
  } else {
    assert.equal(app.descriptorHex,null);
    assert.equal(app.descriptorSha256,null);
  }
  assert.ok(Array.isArray(app.elements)&&app.elements.length>0);
  const independent=new Map(probe.elements.map(element=>[elementKey(element),element]));
  assert.equal(independent.size,probe.elements.length,'independent IOHID cookies must be unique');
  assert.equal(app.elements.length,probe.elements.length,'compare the complete IOHID element set');
  const observedCookies=new Set();
  for(const element of app.elements) {
    const key=elementKey(element);
    assert.ok(!observedCookies.has(key),'native IOHID cookies must be unique');
    observedCookies.add(key);
    assert.ok(independent.has(key),`native element is absent from independent IOHID evidence: ${key}`);
    const expected=independent.get(key);
    for(const field of ['parentCookie','type','collectionType','reportId','usagePage','usage','usageMin','usageMax','reportSizeBits','reportCount','logicalMin','logicalMax','physicalMin','physicalMax','unit','unitExponent','relative','wrapping','nonLinear','preferredState','virtual','array','nullState'])
      assert.equal(element[field],expected[field],`IOHID element metadata differs for ${key} (${field})`);
  }
  assert.equal(observedCookies.size,independent.size,'every independent IOHID element must appear in the app snapshot');
  assert.ok(Array.isArray(app.reports)&&app.reports.length>0&&app.reports.length<=MAX_REPORTS,'retain actual reports from the selected interface');
  assert.ok(app.closed===true,'close the selected reader and record the acknowledgement');
  for(const report of app.reports) {
    const provenance=report.provenance;
    assert.ok(['iohid-input-report','iohid-element'].includes(provenance?.source));
    assert.equal(report.reportId,provenance.reportId,'native report ID must agree with provenance');
    assert.match(provenance.timestamp??'',/^\d+$/,'retain the native IOHID timestamp');
    if(provenance.source==='iohid-input-report') {
      assert.equal(provenance.wireAvailable,true);
      assert.match(report.wireHex??'',/^(?:[a-f0-9]{2})+$/i,'raw report callback must retain exact bytes');
      if(probe.descriptorAvailable&&report.samples?.raw!==undefined)
        assert.equal(sampleRawHex(report.samples.raw),report.wireHex.toLowerCase(),'decoded raw bytes must equal the retained callback delivery');
      if(!probe.descriptorAvailable)
        assert.ok(typeof report.samples?.unavailable==='string','without a descriptor, raw delivery stays undecoded');
    } else {
      assert.equal(provenance.wireAvailable,false);
      assert.equal(Object.hasOwn(report,'wireHex'),false,'element-only callback cannot invent raw bytes');
      assert.ok(Array.isArray(report.samples?.elements)&&report.samples.elements.length>0,'element callbacks retain only observed elements');
      for(const sample of report.samples.elements) {
        const element=independent.get(String(sample.cookie));
        assert.ok(element,`callback element cookie ${sample.cookie} must exist on the selected device`);
        assert.equal(report.reportId,element.reportId,'callback record report ID must match its element metadata');
        assert.equal(sample.usagePage,element.usagePage);
        assert.equal(sample.usage,element.usage);
        assert.equal(sample.reportSizeBits,element.reportSizeBits);
        assert.equal(sample.reportCount,element.reportCount);
        assert.equal(sample.logicalMin,element.logicalMin);
        assert.equal(sample.logicalMax,element.logicalMax);
        assert.equal(sample.relative,element.relative);
        assert.equal(sample.array,element.array);
        assert.ok(sample.integer===null||/^-?(?:0|[1-9]\d*)$/.test(sample.integer));
        assert.match(sample.valueHex??'',/^(?:[a-f0-9]{2})*$/i);
      }
    }
  }
  const hasRaw=app.reports.some(report=>report.provenance?.source==='iohid-input-report');
  const hasElements=app.reports.some(report=>report.provenance?.source==='iohid-element');
  assert.ok(hasRaw||hasElements,'record whether this run delivered raw reports or parsed element callbacks');
  assert.ok(probe.descriptorAvailable||hasRaw||hasElements,'record live callbacks even when no descriptor is available');
  const observations=app.operatorObservations??[];
  assert.ok(Array.isArray(observations),'operator observations must be a separate optional record');
  for(const observation of observations) {
    assert.ok(typeof observation.control==='string'&&observation.control.length>0);
    assert.ok(['press','release','move','return','disconnect','reconnect'].includes(observation.action));
    assert.ok(typeof observation.observedBy==='string'&&observation.observedBy.length>0);
    assert.ok(typeof observation.appEvidence==='string'&&observation.appEvidence.length>0);
  }
  return {appBuildType:app.appBuildType,registryEntryId:app.identity.registryEntryId,
    descriptorAvailable:app.descriptorAvailable,descriptorSha256:app.descriptorSha256,nativeElements:app.elements.length,
    independentElements:probe.elements.length,nativeReports:app.reports.length,
    rawReportCallbacks:app.reports.filter(report=>report.provenance?.wireAvailable===true).length,
    elementCallbacks:app.reports.filter(report=>report.provenance?.source==='iohid-element').length,
    portableDecoded:probe.descriptorAvailable&&app.reports.some(report=>report.provenance?.source==='iohid-input-report'&&Array.isArray(report.samples?.values)),
    operatorObservations:observations.length,deviceOpenedByRunner:true,readerClosed:true,
    packetSynchronized:false,releaseQualified:false};
}
