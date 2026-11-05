import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdir, writeFile} from 'node:fs/promises';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

test('a delayed controller preview receipt resumes at current absolute state', {
  skip: process.env.OSG_CONTROLLER_LATEST_NATIVE !== '1', timeout: 30000,
}, async () => {
  const page = await nativePage();
  try {
    const source = await page.evaluate(`(async()=>{
      const {gatewayRequest}=await import('/src/lib/gateway.ts');
      const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
      const {observeControllerInputs}=await import('/src/lib/controller-observations.ts');
      const controllers=(await gatewayRequest({command:'controller_list',offset:0})).controllers;
      const source=controllers.sources.filter(source=>source.online&&source.baselineReady)
        .sort((a,b)=>b.controlCount-a.controlCount)[0];
      if(!source)throw Error('An attached reporting physical controller is required.');
      const state={source,frames:[],errors:[],abort:new AbortController(),fastCount:0};
      window.__controllerLatestAcceptance=state;
      createNativeObservationTransport().open('controllers.input.'+source.id,{
        frame(envelope,ack){
          state.frames.push(envelope.frame);
          if(state.frames.length===1)state.release=ack;
          else void ack();
        },error(error){state.errors.push(error);}
      },state.abort.signal);
      state.stopFast=observeControllerInputs(source.id,value=>{
        if(value.status==='error')state.errors.push(value.error);
        if(value.status==='ready'){state.fast=value.detail;state.fastCount++;}
      });
      return source;
    })()`);
    const initial = await until(() => page.evaluate(`(()=>{const s=window.__controllerLatestAcceptance;
      return s.frames.length&&s.fast?{first:s.frames[0],fastCount:s.fastCount}:null;})()`));
    await new Promise(resolve => setTimeout(resolve, 750));
    const target = await page.evaluate(`(async()=>{
      const s=window.__controllerLatestAcceptance;
      const {gatewayRequest}=await import('/src/lib/gateway.ts');
      const inspected=(await gatewayRequest({command:'controller_inspect',sourceId:s.source.id,offset:0})).controllers;
      return {sequence:inspected.source.sequence,fastSequence:s.fast.source.sequence,fastCount:s.fastCount,heldFrames:s.frames.length};
    })()`);
    assert.equal(target.heldFrames, 1, 'The slow reader must retain exactly one in-flight frame');
    assert.ok(BigInt(target.sequence) > BigInt(initial.first.payload.source.sequence) + 2n,
      'The physical controller must report during the delayed receipt');
    assert.ok(target.fastCount > initial.fastCount + 2, 'An independent preview must keep receiving');
    await until(() => page.evaluate('window.__controllerLatestAcceptance.fast.source.sequence'),
      sequence => BigInt(sequence) >= BigInt(target.sequence));
    await page.evaluate('window.__controllerLatestAcceptance.release()');
    const result = await until(() => page.evaluate(`(()=>{const s=window.__controllerLatestAcceptance;
      return s.frames.length>1?{next:s.frames[1],errors:s.errors}:null;})()`));
    await mkdir('.research/reactive', {recursive:true});
    await writeFile('.research/reactive/controller-latest-state-native.json', JSON.stringify({source,initial,target,result,
      qualification:'Real passive HID/preview transport with an intentionally delayed receipt; no physical motion-to-photon timing claim.'}, null, 2));
    assert.deepEqual(result.errors, []);
    assert.equal(result.next.kind, 'snapshot');
    assert.ok(BigInt(result.next.payload.source.sequence) >= BigInt(target.sequence),
      `Preview replayed stale state ${result.next.payload.source.sequence}; expected at least ${target.sequence}`);
    assert.equal(result.next.generation, initial.first.generation);
    assert.equal(result.next.payload.controls.length, source.controlCount);
  } finally {
    await page.evaluate(`(()=>{const s=window.__controllerLatestAcceptance;
      if(s){s.abort.abort();s.stopFast?.();}delete window.__controllerLatestAcceptance;})()`).catch(()=>{});
    page.close();
  }
});
