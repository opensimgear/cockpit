import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createFeedbackView } from './feedback-state.ts';
import { exampleUi } from '../../../../packages/companion-contracts/src/index.ts';
const identity = { epoch:1,sessionId:'session',generation:'1',instanceId:'panel',publicationHash:'sha256:'+'a'.repeat(64) };
const value = (extra={}) => ({channelId:'engine',outputId:'engine-status',status:'fresh',value:'Running',provenance:'game-confirmed',sourceKind:'simulator',sourceAgeMs:20,unit:'state',...extra});
test('feedback snapshots are detached, typed, full replacements and independently fenced by every session field',()=>{
 const view=createFeedbackView(),values=[value()]; assert.equal(view.accept(identity,identity,1,values,exampleUi,100).accepted,true);values[0].value='mutated';assert.equal(view.snapshot().engine.value,'Running');const copy=view.snapshot();copy.engine.value='changed';assert.equal(view.snapshot().engine.value,'Running');
 for(const field of Object.keys(identity))assert.match(view.accept(identity,{...identity,[field]:String(identity[field])+'old'},2,[value()],exampleUi,200).error,/another/);
 assert.deepEqual(view.accept(identity,identity,1,[value()],exampleUi,200),{accepted:false});assert.equal(view.snapshot().engine.receivedAtMs,100);
 assert.equal(view.accept(identity,identity,2,[],exampleUi,200).accepted,true);assert.deepEqual(view.snapshot(),{});
});
test('reset and resync cannot replay a queued old snapshot, resurrect stale values, or create input effects',()=>{
 const view=createFeedbackView();view.accept(identity,identity,4,[value()],exampleUi,100);view.reset();assert.deepEqual(view.snapshot(),{});assert.equal(view.accept(identity,identity,4,[value()],exampleUi,200).accepted,false);assert.deepEqual(view.snapshot(),{});
 assert.equal(view.accept(identity,identity,5,[value({status:'unknown',value:undefined})],exampleUi,300).accepted,true);assert.equal(view.snapshot().engine.value,undefined);
 view.accept(identity,identity,6,[value()],exampleUi,400);view.disconnect();assert.equal(view.snapshot().engine.status,'disconnected');assert.equal(Object.hasOwn(view.snapshot().engine,'value'),false);
 view.clear();assert.equal(view.accept({...identity,sessionId:'new'},{...identity,sessionId:'new'},0,[value()],exampleUi,500).accepted,true);
});
test('malformed or over-budget feedback leaves the last good visual state intact',()=>{
 const view=createFeedbackView();view.accept(identity,identity,1,[value()],exampleUi,10);
 for(const [revision,values,now] of [[-1,[value()],20],[NaN,[value()],20],[2,[value()],-1],[2,[value({channelId:'wrong'})],20],[2,[value({value:123})],20],[2,[value(),value()],20]])assert.equal(!!view.accept(identity,identity,revision,values,exampleUi,now).error,true);
 assert.equal(view.snapshot().engine.value,'Running');assert.equal(view.accept(identity,identity,2,[value({value:''})],exampleUi,20).accepted,true);assert.equal(view.snapshot().engine.value,'');
});
