import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';

export async function controlParity({page, reader, writer, call, ok, mutation}) {
  const runtime = async () => (await page.native({command:'profile_status'})).profiles.profileSession;
  const learning = async (command, args) => (await page.native({command,...args})).profileLearning;
  const nativeResult = (command, args) => page.invoke('gateway_request', {
    request:{command,clientId:'mcp-control-parity-ui',...args},
  });
  assert.equal((await runtime()).owner,null,'The fixture requires an idle profile');
  const inventory = await ok(reader,'resources_list',{limit:32});
  const source = inventory.resources.find(item => item.resourceType==='host-controller'
    && item.availability==='online' && item.connections.length===1
    && item.connections[0].route.transport==='hid' && item.connections[0].route.baselineReady);
  assert.ok(source,'An attached HID controller with a complete baseline is required');
  const args = {resourceId:source.id,generation:source.generation,capabilityRevision:source.capabilityRevision};
  let learnId;
  try {
    const started = await learning('profile_input_learn_start',args);
    learnId = started.learnId;
    const first = {learnId};
    assert.equal(started.phase,'listening');
    const cancel = mutation(first);
    const cancelled = await ok(writer,'profile_input_learn_cancel',cancel);
    assert.equal(cancelled.phase,'stopped');
    assert.deepEqual(cancelled.candidates,[]);
    assert.deepEqual(await learning('profile_input_learn_status',first),cancelled);
    assert.deepEqual(await ok(writer,'profile_input_learn_cancel',cancel),cancelled);

    const next = await ok(writer,'profile_input_learn_start',mutation(args));
    learnId = next.learnId;
    assert.notEqual(next.learnId,first.learnId);
    const staleNative = await nativeResult('profile_input_learn_cancel',first);
    const staleMcp = await call(writer,'profile_input_learn_cancel',mutation(first));
    assert.equal(staleNative.ok,false);
    assert.equal(staleMcp.error.code,'expired');
    assert.equal(staleMcp.error.message,staleNative.error);
    const second = {learnId};
    assert.equal((await ok(reader,'profile_input_learn_status',second)).phase,'listening');
    const stopped = await learning('profile_input_learn_cancel',second);
    assert.equal(stopped.phase,'stopped');
    assert.deepEqual(stopped.candidates,[]);
    assert.deepEqual(await ok(reader,'profile_input_learn_status',second),stopped);
    learnId = undefined;
  } finally {
    if (learnId) await nativeResult('profile_input_learn_cancel',{learnId});
  }

  const manual = randomUUID(), remote = randomUUID();
  await page.native({command:'profile_recover',activationId:manual});
  await ok(writer,'profile_recover',{operationId:remote});
  for (const activationId of [manual,remote]) {
    const request = {activationId};
    const progress = await ok(reader,'activation_status',request);
    assert.deepEqual((await page.native({command:'profile_activation_status',...request})).profileActivation,progress);
    assert.equal(progress.phase,'succeeded');
    assert.equal(progress.origin,'recovery');
    assert.equal(progress.outcome,'inactive');
    assert.equal(progress.canCancel,false);
    assert.deepEqual(await ok(writer,'activation_cancel',mutation(request)),progress);
  }
  const status = await runtime();
  assert.equal(status.state,'stopped');
  assert.equal(status.owner,null);
  const missing = {activationId:randomUUID()};
  const nativeMissing = await nativeResult('profile_activation_cancel',missing);
  const mcpMissing = await call(writer,'activation_cancel',mutation(missing));
  assert.equal(nativeMissing.ok,false);
  assert.equal(mcpMissing.ok,false);
  assert.equal(mcpMissing.error.message,nativeMissing.error);

  const {document} = await ok(writer,'profile_create',mutation({name:`Output-free cancel fixture ${randomUUID()}`}));
  try {
    for (const field of ['resources','bindings','desktopOutputs','virtualOutputs','companionAssignments']) {
      assert.equal((document[field]??[]).length,0,field);
    }
    await page.native({command:'profile_apply',id:document.id,expectedRevision:document.revision,expectedActiveOwner:null});
    const active = await runtime();
    assert.equal(active.owner?.profileId,document.id);
    assert.equal(active.state,'running');
    const previous = {activationId:manual};
    await page.native({command:'profile_activation_cancel',...previous});
    await ok(writer,'activation_cancel',mutation(previous));
    assert.deepEqual((await runtime()).owner,active.owner);
    assert.deepEqual((await ok(reader,'profile_status')).profileSession.owner,active.owner);
  } finally {
    if ((await runtime()).owner?.profileId===document.id) await ok(writer,'profile_stop',mutation({}));
    await page.native({command:'profile_delete',id:document.id,expectedRevision:document.revision});
  }
  assert.equal((await runtime()).owner,null);
  return {crossSurfaceLearningCancel:true,staleCancelRejected:true,idleRecoveryParity:true,
    cleanupNonCancellable:true,staleActivationPreservesNewOwner:true};
}
