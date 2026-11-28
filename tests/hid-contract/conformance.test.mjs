import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { bytes, integer, extract, pack, loadCorpus, validateFixture, validateMalformed, checkCorpus, limits } from './conformance.mjs';

const corpus = loadCorpus(), clone = id => structuredClone(corpus.find(fixture => fixture.id === id));
const malformed = loadCorpus(fileURLToPath(new URL('./malformed/',import.meta.url)))[0];
const rejects = (id, mutate, pattern) => {
  const fixture = clone(id);
  mutate(fixture);
  assert.throws(()=>validateFixture(fixture),pattern);
};

test('all authored golden reports and malformed byte framing conform without device access', () => {
  const results = checkCorpus();
  assert.equal(results.length,7);
  assert.equal(results.reduce((sum,row)=>sum+row.declarations,0),11);
  assert.equal(results.reduce((sum,row)=>sum+row.samples,0),18);
  assert.equal(validateMalformed(malformed),'truncated-usage');
  assert.deepEqual(new Set(corpus.flatMap(fixture=>fixture.reports.map(report=>report.kind))),new Set(['input','output','feature']));
});
test('decimal transport preserves exact u64/i64 extremes and the first unsafe JavaScript integer', () => {
  for (const value of ['18446744073709551615','-9223372036854775808','9007199254740993','9223372036854775809']) {
    assert.equal(integer(value).toString(),value);
    assert.equal(JSON.parse(JSON.stringify({value})).value,value);
  }
  assert.notEqual(BigInt(Number('9007199254740993')),integer('9007199254740993'));
  rejects('vendor-wide',fixture=>{fixture.samples[0].values.sequence[0]='9007199254740992';},/exact golden value/);
  rejects('vendor-wide',fixture=>{fixture.samples[0].values.counter[0]=18446744073709551615;},/decimal string/);
});
test('integer/byte encodings reject noncanonical, imprecise or oversized input', () => {
  for (const invalid of ['-0','00','+1','1.0','1e3',' 1','١','1'.repeat(41),1,null]) assert.throws(()=>integer(invalid));
  for (const invalid of ['','a','AB','0g','00 01',null]) assert.throws(()=>bytes(invalid));
  assert.throws(()=>bytes('00'.repeat(limits.descriptorBytes+1)),/byte bound/);
});
test('unaligned signed 12-bit goldens match actual payload bits and sign extension', () => {
  const payload = bytes('85ff3fa8');
  assert.equal(extract(payload,0,12,true),-123n);
  assert.equal(extract(payload,12,12,true),1023n);
  assert.equal(extract(payload,24,4,false),8n);
  const encoded = Buffer.alloc(4);
  pack(encoded,0,12,-123n); pack(encoded,12,12,1023n); pack(encoded,24,4,8n); pack(encoded,28,4,10n);
  assert.equal(encoded.toString('hex'),'85ff3fa8');
});
test('golden bit or expected-value changes fail rather than merely validating report length', () => {
  rejects('packed-joystick',fixture=>{fixture.samples[0].wireHex='0184ff3fa8';},/exact golden value/);
  rejects('boot-keyboard',fixture=>{fixture.samples[0].values.keys[1]='41';},/exact golden value/);
  rejects('relative-mouse',fixture=>{fixture.samples[0].wireHex='0d817fff';},/exact golden value/);
  const nonzero = clone('relative-mouse').samples.find(sample=>sample.name==='nonzero-constant-bits-preserved');
  assert.equal(nonzero.values.padding[0],'31');
  assert.equal(nonzero.wireHex,'fd817fff');
});
test('report IDs are framing only and unnumbered reports use null without a leading zero', () => {
  rejects('packed-joystick',fixture=>{fixture.samples[0].wireHex='0285ff3fa8';},/report ID framing/);
  rejects('packed-joystick',fixture=>{fixture.samples[0].wireHex='85ff3fa8';},/wire report length/);
  rejects('relative-mouse',fixture=>{fixture.samples[0].wireHex='0005817fff';},/wire report length/);
  rejects('relative-mouse',fixture=>{fixture.reports[0].reportId=0;},/report ID/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[0].offset=8;},/overlapping|exceeds|holes/);
});
test('same report ID remains separated by input, output and feature kind', () => {
  const fixture = clone('vendor-multikind');
  assert.deepEqual(fixture.reports.map(report=>report.reportId),[5,5,5]);
  assert.deepEqual(fixture.samples.map(sample=>sample.wireHex.length/2),[5,2,5]);
  rejects('vendor-multikind',fixture=>{fixture.samples[1].kind='input';},/wire report length/);
  rejects('vendor-multikind',fixture=>{fixture.reports[1].kind='input';},/duplicate kind/);
});
test('null hats and out-of-logical-range wide scalars remain distinct explicit classifications', () => {
  rejects('packed-joystick',fixture=>{fixture.samples[0].nullFields=[];},/null-state/);
  rejects('vendor-wide',fixture=>{fixture.samples[0].outsideLogicalRange=[];},/out-of-range/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[1].nullValues=['7'];},/null must be encodable outside/);
});
test('four-way, eight-way and continuous hats retain native domains and asymmetric neutral stays unknown', () => {
  const fixture = clone('hat-domains'), fields = fixture.reports[0].fields;
  assert.equal(fields[0].hatDomain,'4way');
  assert.equal(clone('packed-joystick').reports[0].fields[1].hatDomain,'8way');
  assert.equal(fields[2].hatDomain,'continuous-angle');
  assert.equal(fields[2].unitCode,'20');
  assert.equal(fields[2].unitExponent,-2);
  assert.equal(integer(fixture.samples[1].values['continuous-angle'][0])/100n,90n);
  assert.equal(fields[3].neutral,'unknown');
  assert.deepEqual(fixture.samples.map(sample=>sample.values['asymmetric-axis'][0]),['65000','100','12345']);
  rejects('hat-domains',fixture=>{fixture.reports[0].fields[2].unitExponent=8;},/unit exponent/);
  rejects('hat-domains',fixture=>{fixture.reports[0].fields[3].neutral='32550';},/neutral is explicitly unknown/);
});
test('identical relative reports remain two ordered authored events with separate sample identities', () => {
  const fixture = clone('relative-mouse');
  assert.equal(fixture.samples[0].wireHex,fixture.samples[1].wireHex);
  assert.notEqual(fixture.samples[0].name,fixture.samples[1].name);
  assert.equal(fixture.reports[0].fields.filter(field=>field.relative).length,3);
  assert.equal(validateFixture(fixture).samples,4);
  rejects('relative-mouse',fixture=>{fixture.samples[1].name=fixture.samples[0].name;},/unique sample name/);
});
test('metadata expectations point to actual descriptor bytes and preserve opaque items', () => {
  const fixture = clone('metadata-preservation');
  for (const item of fixture.metadata) assert.equal(bytes(fixture.descriptorHex).subarray(item.offsetBytes,item.offsetBytes+item.hex.length/2).toString('hex'),item.hex);
  rejects('metadata-preservation',fixture=>{fixture.metadata[0].offsetBytes++;},/golden metadata bytes/);
  rejects('metadata-preservation',fixture=>{fixture.metadata[0].h1Expectation='';},/H1 preservation/);
  assert.throws(()=>validateMalformed({...malformed,availablePayloadBytes:1}),/available truncated bytes/);
  assert.throws(()=>validateMalformed({...malformed,declaredPayloadBytes:2}),/short item framing/);
});
test('fixture bounds, missing fields, overlaps, holes and unevidenced declarations fail', () => {
  rejects('packed-joystick',fixture=>{fixture.reports[0].payloadBits=limits.payloadBits+1;},/payload bit count/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[0].width=limits.fieldBits+1;},/field bit width/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[0].count=limits.fields+1;},/field count/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[0].max='999999';},/logical range/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[2].offset=29;},/field exceeds/);
  rejects('packed-joystick',fixture=>{fixture.reports[0].fields[2].count=3;},/padding instead of leaving holes/);
  rejects('packed-joystick',fixture=>{delete fixture.samples[0].values.hat;},/exact expected field set/);
  rejects('packed-joystick',fixture=>{fixture.samples[0].values.axes=[];},/expected value count/);
  rejects('vendor-multikind',fixture=>{fixture.samples.pop();},/every input\/output\/feature/);
  rejects('packed-joystick',fixture=>{fixture.samples[0].kind='unknown';},/undeclared report/);
});
test('corpus discovery rejects empty directories and duplicate authored identities', () => {
  const directory=mkdtempSync(join(tmpdir(),'hid-contract-'));
  try {
    assert.throws(()=>loadCorpus(directory),/corpus is empty/);
    for (const name of ['first.json','second.json']) writeFileSync(join(directory,name),JSON.stringify(clone('relative-mouse')));
    assert.throws(()=>loadCorpus(directory),/duplicate fixture identity/);
  } finally { rmSync(directory,{recursive:true,force:true}); }
});
