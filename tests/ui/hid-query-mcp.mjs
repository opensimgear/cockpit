import assert from 'node:assert/strict';

// Checks the externally visible query contract against authored native observations.
export async function verifyReplayQueries(call,rejected,writer,replayId,archive,expected) {
  const identity={source:'replay',replayId};
  const tool='controller_hid_descriptor_query';
  const first=await call(writer,tool,{...identity,limit:17});
  assert.equal(first.nodes.entries[0].path,'interface');
  const nodes=[...first.nodes.entries];let offset=first.nodes.nextOffset;
  while(offset!==null) {
    await rejected(writer,tool,{...identity,limit:17,offset},'invalid_arguments');
    const page=await call(writer,tool,{...identity,limit:17,offset,queryFingerprint:first.queryFingerprint});
    assert.equal(page.queryFingerprint,first.queryFingerprint);
    assert.ok(page.nodes.entries.length<=17);nodes.push(...page.nodes.entries);
    if(page.nodes.nextOffset!==null)assert.ok(page.nodes.nextOffset>offset);
    offset=page.nodes.nextOffset;
  }
  assert.equal(nodes.length,first.nodes.total);
  assert.equal(new Set(nodes.map(node=>node.path)).size,nodes.length);
  const searched=await call(writer,tool,{...identity,search:'INTERFACEID'});
  assert.ok(searched.nodes.entries.some(node=>node.value===archive.interfaceId));
  const absent=await call(writer,tool,{...identity,search:'no-matching-native-field-185'});
  assert.equal(absent.nodes.total,0);

  const monitor='controller_hid_monitor_query';
  const elapsedUs=(BigInt(archive.records[0].receivedUs)-BigInt(archive.startedReceivedUs)).toString();
  const args={...identity,after:'0',elapsedUs,seek:true};
  const all=await call(writer,monitor,args);
  if(expected&&all.values.entries.length)assert.ok(all.values.entries.some(({presentation})=>
    [presentation.text,presentation.quality].some(value=>String(value).toLowerCase().includes(expected.toLowerCase()))),
  `${archive.interfaceId}: MCP presentation does not contain the UI's expected ${expected}`);
  assert.deepEqual(await call(writer,monitor,args),all,'Same archive cursor and clock must be deterministic');
  assert.deepEqual(all.reports.entries,[archive.records[0]]);
  assert.equal(all.sourceNext,'1');
  for(const invalid of [{changed:true},{after:'01'},{report:'256'},{interfaceId:'cross-source'},{limit:129}])
    await rejected(writer,monitor,{...args,...invalid},'invalid_arguments');
  const unmatched=await call(writer,monitor,{...args,usage:'no-matching-native-usage-185'});
  assert.equal(unmatched.values.total,0);
  assert.deepEqual(unmatched.reports.entries,all.reports.entries,'Decoded filtering must preserve raw context');
  const active=await call(writer,monitor,{...args,active:true});
  assert.deepEqual(active.values.entries,all.values.entries.filter(row=>row.presentation.active));
  const previous=all.values.entries.map(({value})=>({fieldId:value.fieldId,elementIndex:value.elementIndex,sample:value.sample}));
  const unchanged=await call(writer,monitor,{...args,changed:true,previous});
  assert.equal(unchanged.values.total,0,'A supplied identical baseline must suppress unchanged decoded samples');
  const initial=await call(writer,monitor,{...args,changed:true,previous:[]});
  assert.deepEqual(initial.values.entries,all.values.entries);
  const target=all.values.entries[0];
  if(target) {
    const selected=await call(writer,monitor,{...args,fieldId:target.value.fieldId,elementIndex:target.value.elementIndex});
    assert.ok(selected.values.entries.length>0);
    assert.ok(selected.values.entries.every(({value})=>value.fieldId===target.value.fieldId&&value.elementIndex===target.value.elementIndex));
    const usage=await call(writer,monitor,{...args,usage:target.value.fieldId});
    assert.ok(usage.values.entries.some(({value})=>value.fieldId===target.value.fieldId));
    if(all.values.total>1) {
      const firstValue=await call(writer,monitor,{...args,limit:1});
      const next=firstValue.values.nextOffset;
      const secondValue=await call(writer,monitor,{...args,limit:1,valueOffset:next,queryFingerprint:firstValue.queryFingerprint});
      assert.deepEqual(secondValue.values.entries,[all.values.entries[1]]);
      await rejected(writer,monitor,{...args,limit:1,valueOffset:next,queryFingerprint:'sha256:'+'0'.repeat(64)},'query_changed');
    }
  }
  const terminal=await call(writer,monitor,{...identity,after:'0'});
  assert.equal(terminal.state.terminal.kind,'stopped');
  const raw=await call(writer,monitor,{...identity,after:'0',report:'255'});
  assert.ok(raw.reports.entries.some(row=>row.kind==='terminal'),'Raw filtering must retain the native terminal marker');
}
