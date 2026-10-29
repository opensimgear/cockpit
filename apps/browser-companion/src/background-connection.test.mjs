import test from 'node:test';
import assert from 'node:assert/strict';
import { BackgroundConnection } from './background-connection.ts';

const identity={clientId:'browser',sessionId:'session',generation:'1',publicationHash:'sha256:'+'a'.repeat(64)};
function harness(){
 let clock=0,closed=false,pending;const sent=[],feedback=[];
 const socket={onclose(){},onreply(){},async connect(){},async request(frame){if(closed)throw Error('closed');sent.push(structuredClone(frame));if(pending)await pending;return{ok:true,...(frame.type==='open'?identity:{}),feedback:{sequence:String(sent.length)}};},close(){closed=true;}};
 const engine=new BackgroundConnection(socket,()=>clock,(reply,id)=>feedback.push({reply,id}));
 return{engine,sent,feedback,socket,setClock(value){clock=value;},hold(){let release;pending=new Promise(resolve=>release=resolve);return()=>{pending=undefined;release();};},get closed(){return closed;}};
}
async function armed(h){await h.engine.connect();await h.engine.request({type:'open',instanceId:'instance'});await h.engine.request({version:1,type:'baseline',...identity,sequence:'0',values:{button:false,axis:.5},operation:{phase:'ready'}});}

test('worker heartbeats continue while the page is frozen and held inputs are released once after one second',async()=>{
 const h=harness();await armed(h);await h.engine.request({type:'input',...identity,sequence:'999',changes:[{controlId:'button',value:true},{controlId:'axis',value:1}]});
 for(const time of [250,500,750,1000,1250,1500]){h.setClock(time);await h.engine.tick();}
 const inputs=h.sent.filter(frame=>frame.type==='input');assert.equal(inputs.length,2);assert.equal(inputs[0].sequence,'1');assert.equal(inputs[1].sequence,'2');
 assert.deepEqual(inputs[1].changes,[{controlId:'button',value:false},{controlId:'axis',value:.5}]);
 assert.equal(h.sent.filter(frame=>frame.type==='heartbeat').length,6);assert.equal(h.feedback.length,1);assert.equal(h.closed,false);
});

test('page and worker requests share one monotonically ordered heartbeat and input writer',async()=>{
 const h=harness();await armed(h);
 await h.engine.request({type:'heartbeat',...identity,sequence:'87'});h.setClock(250);
 await Promise.all([h.engine.tick(),h.engine.tick(),h.engine.request({type:'heartbeat',...identity,sequence:'1'})]);
 assert.deepEqual(h.sent.filter(frame=>frame.type==='heartbeat').map(frame=>frame.sequence),['1','2','3']);
 await h.engine.request({type:'input',...identity,sequence:'1',changes:[{controlId:'button',value:true}]});h.setClock(1300);await h.engine.tick();
 await h.engine.request({type:'input',...identity,sequence:'2',changes:[{controlId:'button',value:true}]});
 assert.deepEqual(h.sent.filter(frame=>frame.type==='input').map(frame=>frame.sequence),['1','2','3']);
});

test('disarmed preparation stays alive without creating a baseline or consuming held controls',async()=>{
 const h=harness();await h.engine.request({type:'open',instanceId:'instance'});
 await h.engine.request({type:'heartbeat',...identity,sequence:'1',operation:{phase:'checking-cache'}});
 h.setClock(5000);await h.engine.tick();
 assert.equal(h.sent.at(-1).operation.phase,'checking-cache');assert.equal(h.sent.some(frame=>frame.type==='baseline'||frame.type==='input'),false);
 await h.engine.request({type:'heartbeat',...identity,sequence:'2',panelError:true});await h.engine.tick();assert.equal(h.sent.at(-1).panelError,true);
});

test('failed background input is not replayed and close prevents all future worker traffic',async()=>{
 const h=harness();await armed(h);await h.engine.request({type:'input',...identity,sequence:'1',changes:[{controlId:'button',value:true}]});
 const original=h.socket.request;h.socket.request=async frame=>{if(frame.type==='input'){h.sent.push(frame);throw Error('Session is obsolete');}return original(frame);};
 h.setClock(1100);await assert.rejects(h.engine.tick(),/obsolete/);h.engine.close();const count=h.sent.length;
 await h.engine.tick();await assert.rejects(h.engine.request({type:'input',...identity,changes:[{controlId:'button',value:true}]}),/stopped/);
 assert.equal(h.sent.length,count);assert.equal(h.closed,true);assert.equal(h.sent.filter(frame=>frame.type==='input').length,2);
});

test('page input that expires behind another request is dropped before reaching the socket',async()=>{
 const h=harness();await armed(h);const release=h.hold();
 const first=h.engine.request({type:'heartbeat',...identity,sequence:'1'});await Promise.resolve();
 const queued=h.engine.request({type:'input',...identity,sequence:'1',changes:[{controlId:'button',value:true}]});
 h.setClock(600);release();await first;await assert.rejects(queued,/expired/);assert.equal(h.sent.some(frame=>frame.type==='input'),false);
});

test('a frozen page queues one feedback envelope and acknowledges into a complete current snapshot',async()=>{
 const h=harness();await armed(h);const state={first:false,second:0};
 h.socket.request=async frame=>{h.sent.push(structuredClone(frame));return{ok:true,type:frame.type==='feedback-resync'?'feedback-resync':'ack',feedback:{...identity,sequence:String(h.sent.length),mode:frame.type==='feedback-resync'?'snapshot':'delta',values:frame.type==='feedback-resync'?structuredClone(state):{first:state.first}}};};
 for(let tick=1;tick<=100;tick++){state.first=tick%2===0;state.second=tick;h.setClock(tick*250);await h.engine.tick();}
 assert.equal(h.feedback.length,1,'Frozen documents retain at most one queued feedback payload.');
 assert.equal(h.sent.filter(frame=>frame.type==='heartbeat').length,100,'Feedback backpressure must not pause the native lease.');
 const first=h.feedback[0];await h.engine.acknowledgeFeedback(first.id);
 assert.equal(h.feedback.length,2);assert.equal(h.sent.at(-1).type,'feedback-resync');assert.equal(h.sent.at(-1).sequence,'1');
 assert.equal(h.feedback[1].reply.type,'feedback-resync');assert.equal(h.feedback[1].reply.feedback.mode,'snapshot');assert.deepEqual(h.feedback[1].reply.feedback.values,{first:true,second:100});
 await h.engine.acknowledgeFeedback(first.id);assert.equal(h.feedback.length,2,'A stale ACK cannot release the new envelope.');
 await h.engine.acknowledgeFeedback(h.feedback[1].id);assert.equal(h.sent.filter(frame=>frame.type==='feedback-resync').length,1);
 await h.engine.request({type:'feedback-resync',...identity,sequence:'1'});assert.equal(h.sent.at(-1).sequence,'2','Page resyncs share the worker counter.');
});

test('replacing the session invalidates pending feedback and ignores acknowledgements for the retired session',async()=>{
 const h=harness();await armed(h);await h.engine.tick();const retired=h.feedback[0];
 await h.engine.request({type:'open',instanceId:'replacement'});await h.engine.tick();assert.equal(h.feedback.length,2);
 await h.engine.acknowledgeFeedback(retired.id);assert.equal(h.sent.some(frame=>frame.type==='feedback-resync'),false);
});
