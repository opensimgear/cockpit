import assert from 'node:assert/strict';
import test from 'node:test';
import {createBrowserFeedback} from './feedback.ts';
import {exampleUi} from '@opensimgear/companion-contracts';
import {feedbackPresentation} from '../../../packages/companion-ui/src/presentation.ts';

const identity={clientId:'browser',sessionId:'session',generation:'1',controllerInstanceId:'instance',publicationHash:'sha256:'+'a'.repeat(64)};
const ui={...exampleUi,feedbackWidgets:[...exampleUi.feedbackWidgets,{id:'altitude-lamp',channelId:'altitude',kind:'indicator',label:'Altitude threshold'}]};
const sample=(extra={})=>({channelId:'engine',outputId:'engine-status',status:'fresh',value:'Altitude 1000 ft',provenance:'requested',sourceKind:'host',sourceAgeMs:20,unit:'text',...extra});
const lamp=(value=true)=>sample({channelId:'altitude',outputId:'altitude-lamp',value,unit:'boolean'});
const frame=(extra={})=>({version:1,type:'feedback',...identity,sequence:'1',bindingGeneration:'1',mode:'snapshot',bindings:[{channelId:'engine',outputId:'engine-status'},{channelId:'altitude',outputId:'altitude-lamp'}],values:[sample(),lamp()],...extra});

test('full snapshots and deltas reach the shared renderer with exact requested provenance and independent sample ages',()=>{
 const view=createBrowserFeedback(),input=frame();assert.equal(view.accept(identity,input,ui,100),true);input.values[0].value='mutated';assert.equal(view.snapshot().engine.value,'Altitude 1000 ft');
 const widget={kind:'indicator',feedbackIds:['altitude-lamp'],config:{}};
 const fresh=feedbackPresentation(widget,ui,view.snapshot(),110);assert.equal(fresh.value,true);assert.equal(fresh.confirmed,false);assert.match(fresh.provenance,/requested.*host/);
 const delta=frame({mode:'delta',sequence:'2',values:[lamp(false)]});delete delta.bindings;assert.equal(view.accept(identity,delta,ui,300),true);assert.equal(view.snapshot().engine.receivedAtMs,100);assert.equal(view.snapshot().altitude.receivedAtMs,300);
 assert.equal(feedbackPresentation(widget,ui,view.snapshot(),310).value,false);const stale=feedbackPresentation(widget,ui,view.snapshot(),1400);assert.equal(stale.available,false);assert.equal(stale.value,null);assert.equal(stale.confirmed,false);
 const detached=view.snapshot();detached.altitude.value=true;assert.equal(view.snapshot().altitude.value,false);
});
test('every native identity field and exact unsigned64 counters fence old panels and out-of-order updates',()=>{
 const view=createBrowserFeedback();view.accept(identity,frame({sequence:'18446744073709551614'}),ui,100);
 for(const key of Object.keys(identity))assert.throws(()=>view.accept(identity,frame({[key]:key==='publicationHash'?'sha256:'+'b'.repeat(64):key==='generation'?'2':'other'}),ui,200),/another/);
 assert.throws(()=>view.accept(identity,frame({sequence:'18446744073709551613'}),ui,200),/Stale/);assert.throws(()=>view.accept(identity,frame({bindingGeneration:'0'}),ui,200),/Stale/);
 assert.equal(view.accept(identity,frame({sequence:'18446744073709551615'}),ui,200),true);assert.throws(()=>view.accept(identity,frame({sequence:'18446744073709551616'}),ui,300));
 assert.equal(view.accept(identity,frame({bindingGeneration:'2',sequence:'0'}),ui,300),true);
});
test('malformed, unbound or incomplete snapshots fail atomically and deltas require a matching current snapshot',()=>{
 const view=createBrowserFeedback(),delta=frame({mode:'delta',sequence:'2',values:[lamp(false)]});delete delta.bindings;assert.throws(()=>view.accept(identity,delta,ui,100),/snapshot/);view.accept(identity,frame(),ui,100);
 for(const input of[frame({values:[lamp()]}),frame({values:[lamp(),lamp()]}),frame({values:[sample({value:123}),lamp()]}),frame({values:[sample({channelId:'forged'}),lamp()]}),frame({unrecognized:true}),frame({values:[sample({provenance:'game-confirmed'}),lamp()]})])assert.throws(()=>view.accept(identity,input,ui,200));
 const unbound=frame({mode:'delta',sequence:'2',values:[lamp()]});delete unbound.bindings;const one=frame({bindings:[{channelId:'engine',outputId:'engine-status'}],values:[sample()]});view.accept(identity,one,ui,200,true);assert.throws(()=>view.accept(identity,unbound,ui,300),/binding/);
 assert.throws(()=>view.accept(identity,delta,ui,300),/binding/);assert.equal(view.snapshot().engine.value,'Altitude 1000 ft');
});
test('duplicates never renew freshness or undo local clear; explicit resync accepts only a complete current snapshot',()=>{
 const view=createBrowserFeedback();view.accept(identity,frame(),ui,100);assert.equal(view.accept(identity,frame(),ui,500),false);assert.equal(view.snapshot().engine.receivedAtMs,100);
 view.reset();assert.deepEqual(view.snapshot(),{});assert.equal(view.accept(identity,frame(),ui,600),false);assert.deepEqual(view.snapshot(),{});
 assert.equal(view.accept(identity,frame(),ui,700,true),true);assert.equal(view.snapshot().engine.receivedAtMs,700);
 const delta=frame({mode:'delta',values:[lamp(false)]});delete delta.bindings;assert.equal(view.accept(identity,delta,ui,800,true),false);assert.equal(view.snapshot().altitude.value,true);
 for(const now of[-1,NaN,600])assert.throws(()=>view.accept(identity,frame({sequence:'2'}),ui,now),/clock/);
});
test('disconnect strips all live values; a replacement panel requires a new full baseline and can never inherit feedback',()=>{
 const view=createBrowserFeedback();view.accept(identity,frame(),ui,100);view.disconnect();assert.equal(view.snapshot().altitude.status,'disconnected');assert.equal(Object.hasOwn(view.snapshot().altitude,'value'),false);assert.equal(feedbackPresentation({kind:'indicator',feedbackIds:['altitude-lamp'],config:{}},ui,view.snapshot(),200).available,false);
 view.clear();assert.deepEqual(view.snapshot(),{});const replacement={...identity,sessionId:'replacement',generation:'2'};assert.throws(()=>view.accept(replacement,frame(),ui,50),/another/);assert.equal(view.accept(replacement,frame({...replacement,sequence:'0'}),ui,50),true);
});
