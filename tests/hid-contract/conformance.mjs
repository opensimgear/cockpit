// Fixture conformance only: no descriptor parser, OS HID access, or report writes.
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { join, resolve } from 'node:path';

export const limits = Object.freeze({descriptorBytes:4096, payloadBits:4096, fieldBits:128, fields:256, samples:256});
export function integer(value) {
  assert.equal(typeof value, 'string', 'exact integer must be a decimal string');
  assert.match(value, /^(?:0|-?[1-9][0-9]*)$/, 'integer must be canonical decimal');
  assert.ok(value.length <= 40, 'integer exceeds 128-bit fixture bound');
  return BigInt(value);
}
export function bytes(hex, max = limits.descriptorBytes) {
  assert.equal(typeof hex, 'string', 'bytes must be lowercase hex');
  assert.match(hex, /^(?:[0-9a-f]{2})+$/, 'hex must be nonempty lowercase wire-order bytes');
  assert.ok(hex.length / 2 <= max, 'hex exceeds byte bound');
  return Buffer.from(hex, 'hex');
}
function bounded(value, min, max, label) {
  assert.ok(Number.isInteger(value) && value >= min && value <= max, label);
}
export function extract(payload, offset, width, signed) {
  let raw = 0n;
  for (let bit = 0; bit < width; bit++) {
    if (payload[Math.floor((offset + bit) / 8)] & (1 << ((offset + bit) % 8))) raw |= 1n << BigInt(bit);
  }
  return signed && (raw & (1n << BigInt(width - 1))) ? raw - (1n << BigInt(width)) : raw;
}
export function pack(payload, offset, width, value) {
  const raw = BigInt.asUintN(width, value);
  for (let bit = 0; bit < width; bit++) {
    if (raw & (1n << BigInt(bit))) payload[Math.floor((offset + bit) / 8)] |= 1 << ((offset + bit) % 8);
  }
}
export function validateFixture(fixture) {
  assert.match(fixture.id, /^[a-z][a-z0-9-]*$/);
  assert.equal(fixture.provenance, 'authored', 'physical evidence must not be mislabeled');
  assert.ok(typeof fixture.description === 'string' && fixture.description.length > 10);
  const descriptor = bytes(fixture.descriptorHex);
  for (const item of fixture.metadata || []) {
    bounded(item.offsetBytes, 0, descriptor.length - 1, 'metadata byte offset');
    const expected = bytes(item.hex);
    assert.equal(descriptor.subarray(item.offsetBytes,item.offsetBytes+expected.length).toString('hex'),item.hex,'golden metadata bytes at exact descriptor offset');
    assert.ok(typeof item.h1Expectation === 'string' && item.h1Expectation.length > 10, 'explicit H1 preservation expectation');
  }
  assert.ok(Array.isArray(fixture.reports) && fixture.reports.length > 0 && fixture.reports.length <= 32);
  const declarations = new Map();
  for (const report of fixture.reports) {
    assert.ok(['input','output','feature'].includes(report.kind), 'report kind');
    if (report.reportId !== null) bounded(report.reportId, 1, 255, 'report ID (null for unnumbered)');
    bounded(report.payloadBits, 1, limits.payloadBits, 'payload bit count');
    const identity = report.kind + ':' + report.reportId;
    assert.ok(!declarations.has(identity), 'duplicate kind/report ID');
    declarations.set(identity, report);
    assert.ok(Array.isArray(report.fields) && report.fields.length > 0 && report.fields.length <= limits.fields);
    const occupied = new Set(), ids = new Set();
    for (const field of report.fields) {
      assert.match(field.id, /^[a-z][a-z0-9-]*$/);
      assert.ok(!ids.has(field.id), 'duplicate field identity');
      ids.add(field.id);
      bounded(field.offset, 0, report.payloadBits - 1, 'payload-relative bit offset');
      bounded(field.width, 1, limits.fieldBits, 'field bit width');
      bounded(field.count, 1, limits.fields, 'field count');
      assert.equal(typeof field.signed, 'boolean');
      assert.ok(['variable','array','constant'].includes(field.mode), 'field mode');
      assert.ok(typeof field.usage === 'string' && field.usage.length > 0, 'usage declaration');
      const min = integer(field.min), max = integer(field.max);
      const floor = field.signed ? -(1n << BigInt(field.width - 1)) : 0n;
      const ceiling = (1n << BigInt(field.width - (field.signed ? 1 : 0))) - 1n;
      assert.ok(min <= max && (field.mode === 'constant' || (min >= floor && max <= ceiling)), 'logical range exceeds field encoding');
      if (field.relative !== undefined) assert.equal(typeof field.relative, 'boolean', 'relative flag');
      if (field.neutral !== undefined) assert.equal(field.neutral,'unknown','authored asymmetric-axis neutral is explicitly unknown');
      if (field.hatDomain !== undefined) assert.ok(['4way','8way','continuous-angle'].includes(field.hatDomain),'hat domain');
      if (field.unitCode !== undefined) assert.ok(integer(field.unitCode) >= 0n && integer(field.unitCode) <= 4294967295n,'unit code');
      if (field.unitExponent !== undefined) bounded(field.unitExponent,-8,7,'signed four-bit unit exponent');
      if (field.physical !== undefined) assert.ok(integer(field.physical.min) <= integer(field.physical.max),'physical range');
      for (const nullValue of field.nullValues || []) {
        const value = integer(nullValue);
        assert.ok(value >= floor && value <= ceiling && (value < min || value > max), 'null must be encodable outside logical range');
      }
      assert.ok(field.offset + field.width * field.count <= report.payloadBits, 'field exceeds payload');
      for (let bit = field.offset; bit < field.offset + field.width * field.count; bit++) {
        assert.ok(!occupied.has(bit), 'overlapping report fields');
        occupied.add(bit);
      }
    }
    assert.equal(occupied.size, report.payloadBits, 'declare constant padding instead of leaving holes');
  }
  assert.ok(Array.isArray(fixture.samples) && fixture.samples.length > 0 && fixture.samples.length <= limits.samples);
  const exercised = new Set(), sampleNames = new Set();
  for (const sample of fixture.samples) {
    assert.ok(typeof sample.name === 'string' && sample.name.length > 0 && !sampleNames.has(sample.name), 'unique sample name');
    sampleNames.add(sample.name);
    const identity = sample.kind + ':' + sample.reportId, report = declarations.get(identity);
    assert.ok(report, 'sample references an undeclared report');
    exercised.add(identity);
    const wire = bytes(sample.wireHex, Math.ceil(limits.payloadBits / 8) + 1);
    assert.equal(wire.length, Math.ceil(report.payloadBits / 8) + (report.reportId === null ? 0 : 1), 'wire report length');
    if (report.reportId !== null) assert.equal(wire[0], report.reportId, 'report ID framing byte');
    const payload = report.reportId === null ? wire : wire.subarray(1);
    const packed = Buffer.alloc(payload.length), outside = [], nulls = [];
    assert.deepEqual(Object.keys(sample.values).sort(), report.fields.map(field => field.id).sort(), 'exact expected field set');
    for (const field of report.fields) {
      const values = sample.values[field.id];
      assert.ok(Array.isArray(values) && values.length === field.count, 'expected value count');
      let fieldOutside = false, fieldNull = false;
      for (let index = 0; index < field.count; index++) {
        const value = integer(values[index]), offset = field.offset + index * field.width;
        assert.equal(extract(payload, offset, field.width, field.signed).toString(), values[index], `${fixture.id}/${sample.name}/${field.id}[${index}] exact golden value`);
        const isNull = (field.nullValues || []).includes(values[index]);
        fieldNull ||= isNull;
        fieldOutside ||= field.mode !== 'constant' && !isNull && (value < integer(field.min) || value > integer(field.max));
        pack(packed, offset, field.width, value);
      }
      if (fieldOutside) outside.push(field.id);
      if (fieldNull) nulls.push(field.id);
    }
    assert.deepEqual(sample.outsideLogicalRange || [], outside, 'explicit out-of-range expectations');
    assert.deepEqual(sample.nullFields || [], nulls, 'explicit null-state expectations');
    assert.equal(packed.toString('hex'), payload.toString('hex'), 'every authored golden payload bit must match independent expected-value packing');
  }
  assert.equal(exercised.size, declarations.size, 'every input/output/feature declaration needs a golden sample');
  return {id:fixture.id, declarations:declarations.size, samples:fixture.samples.length};
}
export function loadCorpus(directory = fileURLToPath(new URL('./authored/', import.meta.url))) {
  const names = readdirSync(directory).filter(name => name.endsWith('.json')).sort();
  assert.ok(names.length > 0, 'authored corpus is empty');
  const corpus = names.map(name => JSON.parse(readFileSync(join(directory,name),'utf8')));
  assert.equal(new Set(corpus.map(fixture => fixture.id)).size, corpus.length, 'duplicate fixture identity');
  return corpus;
}
export function validateMalformed(fixture) {
  assert.equal(fixture.provenance, 'authored');
  assert.equal(fixture.kind, 'truncated-short-item');
  const descriptor = bytes(fixture.descriptorHex);
  bounded(fixture.offsetBytes, 0, descriptor.length - 1, 'malformed item offset');
  assert.equal(descriptor[fixture.offsetBytes].toString(16).padStart(2,'0'),fixture.prefixHex,'malformed prefix at exact offset');
  assert.notEqual(descriptor[fixture.offsetBytes],0xfe,'short item expected');
  const sizeCode = descriptor[fixture.offsetBytes] & 3;
  assert.equal(fixture.declaredPayloadBytes,sizeCode === 3 ? 4 : sizeCode,'short item framing length');
  assert.equal(fixture.availablePayloadBytes,descriptor.length-fixture.offsetBytes-1,'available truncated bytes');
  assert.ok(fixture.availablePayloadBytes < fixture.declaredPayloadBytes,'fixture must actually be truncated');
  assert.ok(typeof fixture.h1Expectation === 'string' && fixture.h1Expectation.length > 10);
  return fixture.id;
}
export function checkCorpus(directory) {
  const results = loadCorpus(directory).map(validateFixture);
  const malformed = loadCorpus(fileURLToPath(new URL('./malformed/',import.meta.url))).map(validateMalformed);
  console.log(`Authored HID corpus: ${results.length} fixtures, ${results.reduce((sum,row)=>sum+row.declarations,0)} report declarations, ${results.reduce((sum,row)=>sum+row.samples,0)} exact golden reports, ${malformed.length} malformed descriptor case; no hardware writes.`);
  return results;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) checkCorpus(process.argv[2]);
