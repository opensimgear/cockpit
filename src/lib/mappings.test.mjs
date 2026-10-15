import assert from 'node:assert/strict';
import test from 'node:test';
import { oneToOneProfile, operationPreset, builtins, draftDocument, capturedKind, compatibleControl, directProfile, importMapping, savedAssignments } from './mappings.ts';
const native = (kind, min = 0) => ({ capability: { kind, min } });
test('direct mappings retain provenance and never request optional integrations', () => {
  for (const kind of [{ kind: 'button' }, { kind: 'hat' }, { kind: 'axis', range: 'bipolar' }, { kind: 'axis', range: 'unipolar' }]) {
    const source = { origin: 'host-controller', id: 'local-controller-id' };
    const { profile, assignments } = directProfile('  Test  ', kind, source, 'native-control');
    assert.equal(profile.name, 'Test');
    assert.equal(profile.roles[0].origin, source.origin);
    assert.equal(assignments[0].source.id, source.id);
    assert.equal(assignments[0].controls[0].sourceControl, 'native-control');
    assert.deepEqual(profile.requirements, { virtualDevices: false, deviceHiding: false, visibility: [] });
    assert.equal(profile.nodes[0].operation.activation, kind.kind === 'button' ? 'momentary' : 'level');
    assert.equal(profile.targets[0].layout.axes.length, kind.kind === 'axis' ? 1 : 0);
  }
  assert.throws(() => directProfile('', { kind: 'button' }, { origin: 'synthetic', id: 's' }, 'c'));
  assert.throws(() => directProfile('x', { kind: 'button' }, { origin: 'osg-panel-events', id: 's' }, 'c'));
  assert.deepEqual(capturedKind(native('button')), { kind: 'button' });
  assert.deepEqual(capturedKind(native('hat')), { kind: 'hat' });
  assert.deepEqual(capturedKind(native('axis', -100)), { kind: 'axis', range: 'bipolar' });
  assert.deepEqual(capturedKind(native('axis', 0)), { kind: 'axis', range: 'unipolar' });
  assert.equal(capturedKind(native('diagnostic')), null);
  assert.equal(compatibleControl({ kind: 'button' }, native('button')), true);
  assert.equal(compatibleControl({ kind: 'axis', range: 'bipolar' }, native('axis', -1)), true);
  assert.equal(compatibleControl({ kind: 'axis', range: 'bipolar' }, native('axis')), false);
  assert.equal(compatibleControl({ kind: 'hat' }, native('diagnostic')), false);
});
test('full UTF-8 imports use ordered bounded chunks, commit once, and never activate', async () => {
  const text = 'é'.repeat(32768), calls = [];
  const document = { id: 'profile' };
  const result = await importMapping(text, async request => {
    calls.push(request);
    return request.command === 'mapping_import_begin' ? { uploadId: 'upload' } : request.command === 'mapping_import_commit' ? { document } : {};
  });
  assert.equal(result, document);
  assert.equal(calls[0].size, 65536);
  const chunks = calls.filter(c => c.command === 'mapping_import_chunk');
  assert.equal(chunks.length, 8);
  assert.ok(chunks.every((c, i) => c.offset === i * 8192 && c.bytes.length === 8192));
  assert.equal(new TextDecoder().decode(Uint8Array.from(chunks.flatMap(c => c.bytes))), text);
  assert.equal(calls.at(-1).command, 'mapping_import_commit');
  assert.ok(calls.every(c => c.command.startsWith('mapping_import_')));
  await assert.rejects(importMapping(text + 'a', async () => assert.fail('oversized import reached transport')));
  await assert.rejects(importMapping('', async () => assert.fail('empty import reached transport')));
});
test('failed uploads abort their owned transfer and preserve the original error', async () => {
  for (const failAt of ['mapping_import_chunk', 'mapping_import_commit']) {
    const calls = [];
    await assert.rejects(importMapping('{}', async r => {
      calls.push(r.command);
      if (r.command === failAt) throw Error('original failure');
      if (r.command === 'mapping_import_abort') throw Error('abort also failed');
      return { uploadId: 'owned-upload' };
    }), /original failure/);
    assert.equal(calls.at(-1), 'mapping_import_abort');
  }
  await assert.rejects(importMapping('{}', async () => ({})), /did not start/);
  await assert.rejects(importMapping('{}', async r => r.command === 'mapping_import_begin' ? { uploadId: 'u' } : {}), /document is missing/);
});
test('saved local assignments are paged without truncating native control IDs', async () => {
  const controls = Array.from({ length: 65 }, (_, i) => ({ control: 'c' + i, sourceControl: 'native-' + i }));
  const source = { origin: 'host-controller', id: 'controller' };
  const calls = [];
  const result = await savedAssignments('p', async r => {
    calls.push(r);
    return r.role ? { controls: controls.slice(r.offset, r.offset + 32) } : { roles: [{ role: 'input', source, controlCount: 65 }] };
  });
  assert.deepEqual(result, [{ role: 'input', source, controls }]);
  assert.deepEqual(calls.slice(1).map(c => c.offset), [0, 32, 64]);
  await assert.rejects(savedAssignments('p', async () => ({})));
  await assert.rejects(savedAssignments('p', async () => ({ roles: [{ role: 'x', controlCount: 257 }] })));
  await assert.rejects(savedAssignments('p', async r => r.role ? {} : { roles: [{ role: 'x', source, controlCount: 1 }] }));
});

test('built-in operation presets are bounded editable copies',()=>{
 for(const kind of builtins){const first=operationPreset(kind);assert.equal(first.kind,'transform');assert.ok(first.rule.kind);first.rule.changed=true;assert.equal(operationPreset(kind).rule.changed,undefined);}
 assert.equal(operationPreset('direct').activation,'momentary');assert.throws(()=>operationPreset('unknown'));
 assert.equal(operationPreset('button-axis','unipolar').rule.range,'unipolar');
});
test('one-to-one presets preserve every source control without truncating layouts',()=>{
 const control=(id,kind,min=0)=>({capability:{id,kind,min}});
 const source={origin:'host-controller',id:'physical'};
 const controls=[control('a','button'),control('b','axis',-1),control('c','axis'),control('d','hat'),control('e','diagnostic')];
 const {profile,assignments}=oneToOneProfile('all',source,controls);
 assert.equal(profile.nodes.length,4);assert.deepEqual(profile.targets[0].layout,{buttons:1,axes:[{range:'bipolar',fallback:0},{range:'unipolar',fallback:0}],hats:1});assert.deepEqual(assignments[0].controls.map(c=>c.sourceControl),['a','b','c','d']);
 assert.throws(()=>oneToOneProfile('x',source,[]));assert.throws(()=>oneToOneProfile('x',source,Array.from({length:257},(_,i)=>control(String(i),'button'))));
 for(const [kind,count] of [['button',129],['axis',17],['hat',5]]) assert.throws(()=>oneToOneProfile('x',source,Array.from({length:count},(_,i)=>control(String(i),kind))));
 const text=JSON.stringify(profile);assert.deepEqual(draftDocument(text,profile.id),profile);assert.throws(()=>draftDocument(text,'different'));assert.throws(()=>draftDocument('x'.repeat(65537),profile.id));assert.throws(()=>draftDocument('{}','x'));
});
test('draft validation performs no profile write or run activation',async()=>{
 const calls=[];const document={id:'draft'};
 assert.deepEqual(await importMapping('{}',async r=>{calls.push(r.command);return r.command==='mapping_import_begin'?{uploadId:'u'}:r.command==='mapping_validate_commit'?{document}:{};},true),document);
 assert.equal(calls.at(-1),'mapping_validate_commit');assert.ok(!calls.includes('mapping_import_commit'));
});
