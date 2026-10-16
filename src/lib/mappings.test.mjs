import assert from 'node:assert/strict';
import test from 'node:test';
import { migrateMappingSchema, oneToOneProfile, operationPreset, builtins, draftDocument, capturedKind, compatibleControl, directProfile, importMapping, savedAssignments } from './mappings.ts';
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
  assert.throws(() => directProfile('x', { kind: 'hat' }, { origin: 'osg-panel-events', id: 's' }, 'c'));
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

test('panel mappings retain explicit provenance and exclude hats',()=>{const source={origin:'osg-panel-events',id:'panel-events:gateway/module'};for(const kind of [{kind:'button'},{kind:'axis',range:'unipolar'}]){const result=directProfile('Panel',kind,source,'button:0');assert.equal(result.profile.roles[0].origin,source.origin);assert.deepEqual(result.assignments[0].source,source);}});


test('companion mappings use a dedicated origin and explicit versioned migration', () => {
  const source = { origin: 'companion', id: 'retained-instance' };
  const { profile, assignments } = directProfile('Remote panel', { kind: 'button' }, source, 'stable-button');
  assert.equal(profile.schema, 2);
  assert.equal(profile.roles[0].origin, 'companion');
  assert.deepEqual(assignments[0].source, source);
  assert.equal(draftDocument(JSON.stringify(profile), profile.id).schema, 2);
  const invalidLegacy = { ...profile, schema: 1 };
  assert.throws(() => draftDocument(JSON.stringify(invalidLegacy), profile.id), /schema 2/);
  const legacy = directProfile('Legacy', { kind: 'button' }, { origin: 'host-controller', id: 'native' }, 'c').profile;
  const before = structuredClone(legacy), migrated = migrateMappingSchema(legacy);
  assert.equal(legacy.schema, 1); assert.deepEqual(legacy, before);
  assert.equal(migrated.schema, 2); assert.equal(migrated.roles[0].origin, 'host-controller');
  assert.notEqual(migrated.roles, legacy.roles);
  assert.throws(() => migrateMappingSchema({ ...legacy, schema: 9 }));
});

const companionHelpers = await import('./companion-mappings.ts');
const slotFor=(id=2)=>({id,status:'free',ownerPid:null,buttons:2,axes:[{usage:48,min:0,max:32767},{usage:49,min:0,max:32767}],continuousHats:1,discreteHats:0,forceFeedback:false});
const targetFor=(id='output')=>({id,name:id,layout:{buttons:2,axes:[{range:'unipolar',fallback:8192},{range:'bipolar',fallback:-4000}],hats:1}});
test('companion target matching preserves each range/fallback and refuses shape truncation or axis aliasing',()=>{
  const {equalTargetLayout,virtualBindingIssues,targetLayoutLabel}=companionHelpers,target=targetFor(),slot=slotFor();
  const binding={target:target.id,slot:2,axes:[48,49],layout:structuredClone(target.layout)};
  assert.equal(equalTargetLayout(target.layout,binding.layout),true);assert.deepEqual(virtualBindingIssues(target,binding,slot),[]);assert.match(targetLayoutLabel(target.layout),/fallback 8192/);assert.match(targetLayoutLabel(target.layout),/fallback -4000/);assert.match(targetLayoutLabel({buttons:1,axes:[],hats:0}),/1 buttons/);
  for(const changed of [{...binding,target:'other'},{...binding,slot:0},{...binding,axes:[48,48]},{...binding,axes:[48]},{...binding,axes:[48,99]},{...binding,layout:{...binding.layout,buttons:1}},{...binding,layout:{...binding.layout,axes:[{range:'unipolar',fallback:0},binding.layout.axes[1]]}}])assert.ok(virtualBindingIssues(target,changed,slot).length);
  for(const changed of [{...slot,id:3},{...slot,buttons:3},{...slot,continuousHats:0},{...slot,discreteHats:1},{...slot,forceFeedback:true},{...slot,axes:[{usage:48,min:0,max:0},slot.axes[1]]}])assert.ok(virtualBindingIssues(target,binding,changed).length);
  assert.match(virtualBindingIssues(target,binding).join(' '),/Inspect/);
  for(const invalid of [{...target.layout,buttons:129},{...target.layout,axes:[{range:'unipolar',fallback:-1}]},{buttons:0,axes:[],hats:0}])assert.equal(equalTargetLayout(invalid,invalid),false);
});
test('multiple authored targets require separate complete native slots and keep nonzero fallbacks',()=>{
  const {exactTargetBindings}=companionHelpers,profile={targets:[targetFor('first'),targetFor('second')]};
  const result=exactTargetBindings(profile,{first:2,second:3},[slotFor(2),slotFor(3)]);assert.deepEqual(result.map(binding=>binding.slot),[2,3]);assert.equal(result[0].layout.axes[0].fallback,8192);result[0].layout.axes[0].fallback=0;assert.equal(profile.targets[0].layout.axes[0].fallback,8192);
  assert.throws(()=>exactTargetBindings(profile,{first:2,second:2},[slotFor(2)]),/distinct/);assert.throws(()=>exactTargetBindings(profile,{first:2,second:3},[slotFor(2)]),/Inspect/);
});
test('same-UI companion roles keep instance identity and descriptor/baseline readiness separate',()=>{
  const {companionInstanceLabel,companionRoleReadiness}=companionHelpers;
  const source=id=>({id,name:'Identical flight panel',origin:'companion',online:true,baselineReady:true,publicationHash:'sha256:current',controlCount:4,generation:'1',sequence:'0',fault:null});
  const first=source('instance-a'),second=source('instance-b');assert.notEqual(companionInstanceLabel(first),companionInstanceLabel(second));
  const profile={roles:[{id:'a',name:'A',origin:'companion'},{id:'b',name:'B',origin:'companion'},{id:'physical',origin:'host-controller'}]},assignments=[{role:'a',source:{origin:'companion',id:first.id}},{role:'b',source:{origin:'companion',id:second.id}}];
  const ready=companionRoleReadiness(profile,assignments,[first,second]);assert.deepEqual(ready.map(role=>role.state),['ready','ready']);assert.notEqual(ready[0].sourceId,ready[1].sourceId);
  assert.deepEqual(companionRoleReadiness(profile,assignments,[{...first,online:false},second]).map(role=>role.state),['offline','ready']);
  assert.equal(companionRoleReadiness(profile,assignments,[{...first,baselineReady:false},second])[0].state,'waiting-baseline');assert.equal(companionRoleReadiness(profile,assignments,[{...first,fault:'lease expired'},second])[0].state,'fault');
  assert.equal(companionRoleReadiness(profile,assignments,[first,second],{'instance-a':'sha256:old'})[0].state,'descriptor-changed');assert.equal(companionRoleReadiness(profile,assignments,[second])[0].state,'offline');assert.equal(companionRoleReadiness(profile,[],[first,second])[0].state,'unassigned');
});

test('companion template assignments preserve all axes and reject unsupported backend shapes',()=>{
  const {companionTargetBindings}=companionHelpers,profile={targets:[targetFor()]};
  assert.match(companionTargetBindings(profile,{output:2},null).issues[0],/Inspect/);
  assert.deepEqual(companionTargetBindings(profile,{output:2},{backend:'vjoy',slots:[slotFor()]}).issues,[]);
  for(const backend of ['uinput','macos-hid']){const output={backend,limits:{axes:8,buttons:32,hats:4,targets:8}};const valid=companionTargetBindings(profile,{output:2},output);assert.deepEqual(valid.issues,[]);assert.deepEqual(valid.bindings[0].axes,[48,49]);assert.equal(valid.bindings[0].layout.axes[0].fallback,8192);assert.ok(companionTargetBindings(profile,{output:0},output).issues.length);assert.ok(companionTargetBindings(profile,{output:2},output,{output:[48,48]}).issues.length);assert.ok(companionTargetBindings({targets:[{...targetFor(),layout:{...targetFor().layout,buttons:33}}]},{output:2},output).issues.length);assert.ok(companionTargetBindings({targets:[targetFor('a'),targetFor('b')]},{a:2,b:2},output).issues.length);assert.ok(companionTargetBindings(profile,{output:2},{backend,limits:{axes:8,buttons:32,hats:4,targets:0}}).issues.length);}
});
test('per-instance operation guards reject late Start/inspect results after Stop without cancelling other native instances',()=>{
  const guard=companionHelpers.createCompanionOperationGuard();guard.select('a');const inspection=guard.capture(),start=guard.begin('a');assert.equal(guard.current(inspection),false);assert.equal(guard.current(start),true);const stop=guard.begin('a');assert.equal(guard.current(start),false);assert.equal(guard.current(stop),true);guard.select('b');const second=guard.begin('b');assert.equal(guard.current(stop),false);assert.equal(guard.current(second),true);guard.begin('a');assert.equal(guard.current(second),true);guard.dispose();assert.equal(guard.current(second),false);
});

test('Start requires the saved revision, native readiness, fresh source and completed cleanup independently per instance',()=>{
  const {companionStartAllowed,companionSessionLabel}=companionHelpers;
  const source={id:'a',online:true,baselineReady:true,fault:null},route={instanceId:'a',publicationHash:'current',expectedPublicationHash:'current',preflight:{recordingReady:true,liveReady:false},session:{running:false,ready:false,mode:null,starting:false,busy:false,stopRequested:false,cleanupComplete:true,cleanup:'complete',recoveryRequired:false}};
  assert.equal(companionStartAllowed(route,'recording',false,source),true);assert.equal(companionStartAllowed(route,'live',false,source),false);
  for(const changed of [{...route,expectedPublicationHash:'old'},{...route,expectedPublicationHash:undefined},{...route,session:{...route.session,running:true}},{...route,session:{...route.session,busy:true}},{...route,session:{...route.session,starting:true}},{...route,session:{...route.session,cleanupComplete:false}},{...route,session:{...route.session,stopRequested:true,cleanupComplete:false}},{...route,session:{...route.session,recoveryRequired:true}}])assert.equal(companionStartAllowed(changed,'recording',false,source),false);
  assert.equal(companionStartAllowed(route,'recording',true,source),false);for(const changed of [undefined,{...source,id:'b'},{...source,online:false},{...source,baselineReady:false},{...source,fault:'lost'}])assert.equal(companionStartAllowed(route,'recording',false,changed),false);
  assert.match(companionSessionLabel(route.session),/cleanup complete/);assert.match(companionSessionLabel({...route.session,busy:true}),/pending/);assert.match(companionSessionLabel({...route.session,stopRequested:true,busy:true,cleanupComplete:false}),/waiting for native cleanup/);assert.match(companionSessionLabel({...route.session,recoveryRequired:true}),/Recovery required/);assert.match(companionSessionLabel({...route.session,starting:true}),/Starting/);assert.match(companionSessionLabel({...route.session,cleanupComplete:false}),/cleanup pending/);assert.equal(companionStartAllowed({...route,session:{...route.session,stopRequested:true}},'recording',false,source),true);assert.match(companionSessionLabel({...route.session,running:true,ready:true,mode:'recording'}),/Recording session.*Ready/);assert.match(companionSessionLabel({...route.session,running:true,ready:false,mode:'live'}),/Native live session.*Waiting/);
});

test('a stalled instance poll cannot block another instance or create duplicate requests for its own route',()=>{const guard=companionHelpers.createCompanionOperationGuard();guard.select('a');const first=guard.beginPoll('a');assert.ok(first);assert.equal(guard.beginPoll('a'),null);guard.select('b');const second=guard.beginPoll('b');assert.ok(second);assert.equal(guard.current(first),false);assert.equal(guard.current(second),true);guard.endPoll(first);assert.equal(guard.beginPoll('b'),null);guard.endPoll(second);assert.ok(guard.beginPoll('b'));});
