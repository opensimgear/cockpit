import test from 'node:test';
import assert from 'node:assert/strict';
import {compareMacEvidence,validateIndependentProbe} from './verify-macos-core.mjs';

const descriptorHex='05010904a101c0';
const descriptorSha256='3a1b27cab470b1fcb20010d5a22232760591d86ea964b6b05e4f877db0510216';
const element={cookie:17,parentCookie:2,type:1,reportId:1,usagePage:1,usage:48,
  usageMin:null,usageMax:null,reportSizeBits:16,reportCount:1,logicalMin:'0',logicalMax:'65535',physicalMin:null,physicalMax:null,
  unit:0,unitExponent:0,relative:false,wrapping:false,nonLinear:false,preferredState:false,virtual:false,
  array:false,nullState:false,collectionType:null};
function evidence() {
  const probe={origin:'independent-macos-iohid-elements',fixtureEvidence:false,deviceOpened:false,
    reportWrites:false,inputReads:false,featureReads:false,
    descriptorAvailable:true,
    device:{registryEntryId:'123456',vendorId:4660,productId:22136,locationId:1,usagePage:1,usage:4},
    descriptorHex,descriptorSha256,elements:[{...element}]};
  const app={appBuildType:'development',fixtureEvidence:false,deviceOpenedByRunner:true,
    inventory:{interfaceId:'hid:abc'},interfaceId:'hid:abc',identity:{registryEntryId:'123456'},
    descriptorAvailable:true,descriptorHex,descriptorSha256,elements:[{...element}],
    reports:[{sequence:'1',reportId:1,wireHex:'0100',samples:{raw:[1,0]},provenance:{source:'iohid-input-report',wireAvailable:true,reportId:1,timestamp:'1234'}}],
    operatorObservations:[],closed:true};
  return {app,probe};
}

test('independent IOHID inventory is read-only and bounded',()=>{
  const {probe}=evidence();
  assert.equal(validateIndependentProbe(probe),probe);
  for(const edit of [value=>value.deviceOpened=true,value=>value.inputReads=true,
    value=>value.descriptorHex='not hex',value=>value.elements[0].cookie=-1,
    value=>value.elements[0].logicalMax='01']) {
    const {probe}=evidence();edit(probe);assert.throws(()=>validateIndependentProbe(probe));
  }
});

test('live native evidence matches exact registry, descriptor and element identities',()=>{
  const {app,probe}=evidence();
  const result=compareMacEvidence(app,probe);
  assert.equal(result.descriptorSha256,descriptorSha256);
  assert.equal(result.packetSynchronized,false);
  assert.equal(result.releaseQualified,false);
  assert.equal(result.rawReportCallbacks,1);
});

test('fixture and identity or element mismatches cannot pass',()=>{
  for(const edit of [({app})=>app.fixtureEvidence=true,({app})=>app.identity.registryEntryId='99',
    ({app})=>app.descriptorHex='05010905',({app})=>app.elements[0].cookie=18,
    ({app})=>app.closed=false]) {
    const data=evidence();edit(data);assert.throws(()=>compareMacEvidence(data.app,data.probe));
  }
});

test('element callback fallback is explicitly non-raw evidence',()=>{
  const {app,probe}=evidence();
  app.reports=[{sequence:'1',reportId:1,provenance:{source:'iohid-element',wireAvailable:false,reportId:1,timestamp:'1234'},
    samples:{elements:[{cookie:17,usagePage:1,usage:48,integer:'400',valueHex:'9001',relative:false,array:false,logicalMin:'0',logicalMax:'65535',reportSizeBits:16,reportCount:1}]}}];
  const result=compareMacEvidence(app,probe);
  assert.equal(result.rawReportCallbacks,0);
  assert.equal(result.elementCallbacks,1);
});

test('raw versus parsed provenance and callback element identities stay distinct',()=>{
  for(const edit of [report=>delete report.wireHex,report=>report.provenance.wireAvailable=false,
    report=>report.wireHex='0g',report=>report.reportId=7]) {
    const {app,probe}=evidence();edit(app.reports[0]);assert.throws(()=>compareMacEvidence(app,probe));
  }
  for(const edit of [sample=>sample.cookie=99,sample=>sample.usage=49]) {
    const {app,probe}=evidence();
    app.reports=[{sequence:'2',reportId:1,provenance:{source:'iohid-element',wireAvailable:false,reportId:1,timestamp:'1235'},
      samples:{elements:[{cookie:17,usagePage:1,usage:48,integer:'400',valueHex:'9001',relative:false,array:false,logicalMin:'0',logicalMax:'65535',reportSizeBits:16,reportCount:1}]}}];
    edit(app.reports[0].samples.elements[0]);assert.throws(()=>compareMacEvidence(app,probe));
  }
});

test('descriptorless report bytes remain raw and explicitly undecoded',()=>{
  const {app,probe}=evidence();
  app.descriptorAvailable=probe.descriptorAvailable=false;
  app.descriptorHex=probe.descriptorHex=null;
  app.descriptorSha256=probe.descriptorSha256=null;
  app.reports=[{sequence:'1',reportId:1,provenance:{source:'iohid-element',wireAvailable:false,reportId:1,timestamp:'1234'},
    samples:{elements:[{cookie:17,usagePage:1,usage:48,integer:'400',valueHex:'9001',relative:false,array:false,logicalMin:'0',logicalMax:'65535',reportSizeBits:16,reportCount:1}]}}];
  assert.equal(compareMacEvidence(app,probe).descriptorAvailable,false);
  app.reports=[{sequence:'1',reportId:1,wireHex:'0100',provenance:{source:'iohid-input-report',wireAvailable:true,reportId:1,timestamp:'1234'},samples:{unavailable:'IOHID report descriptor unavailable',report_id:1}}];
  assert.equal(compareMacEvidence(app,probe).rawReportCallbacks,1);
  delete app.reports[0].samples.unavailable;
  assert.throws(()=>compareMacEvidence(app,probe));
});
