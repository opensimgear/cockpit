import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
import { BackgroundConnection } from './background-connection.ts';
const source=await readFile(new URL('./connection-worker.ts',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const identity={clientId:'browser',sessionId:'session',generation:'1',publicationHash:'hash'};
function harness(failConnect=0){
 let clock=0;const sockets=[],intervals=new Map(),timeouts=[];
 class Socket{
  constructor(){this.sent=[];this.connects=0;this.closed=false;sockets.push(this);}
  async connect(){if(++this.connects<=failConnect)throw Error('offline');}
  async request(frame){this.sent.push(frame);return{ok:true,...(frame.type==='open'?identity:{}),...(frame.type==='heartbeat'||frame.type==='feedback-resync'?{type:frame.type,feedback:{mode:frame.type==='feedback-resync'?'snapshot':'delta',sequence:String(this.sent.length)}}:{})};}
  close(){this.closed=true;this.onclose?.();}
 }
 const context={exports:{},performance:{now:()=>clock},setInterval(fn){const id=intervals.size+1;intervals.set(id,fn);return id;},clearInterval:id=>intervals.delete(id),setTimeout:fn=>timeouts.push(fn),require(name){if(name==='./socket-transport.ts')return{SocketTransport:Socket};if(name==='./background-connection.ts')return{BackgroundConnection};throw Error(name);}};
 vm.runInNewContext(code,context);
 const received=[],port={postMessage:message=>received.push(message),start(){},close(){this.closed=true;}};context.onconnect({ports:[port]});
 return{port,sockets,received,intervals,timeouts,setClock(value){clock=value;},async send(data){await port.onmessage({data});},async tick(){await Promise.all([...intervals.values()].map(fn=>fn()));}};
}
const flush=async()=>{for(let i=0;i<30;i++)await Promise.resolve();};

test('worker entry retries connection using its own timer and tears down timers and socket on explicit page close',async()=>{
 const h=harness(1),connecting=h.send({type:'connect'});await flush();assert.equal(h.timeouts.length,1);assert.equal(h.sockets[0].connects,1);
 h.timeouts.shift()();await connecting;assert.equal(h.sockets[0].connects,2);assert.equal(h.received.at(-1).type,'connected');assert.equal(h.intervals.size,1);
 await h.send({type:'close'});assert.equal(h.intervals.size,0);assert.equal(h.sockets[0].closed,true);assert.equal(h.port.closed,true);assert.equal(h.received.at(-1).type,'closed');
});

test('worker entry renews the socket without document messages and reports disconnect through the port',async()=>{
 const h=harness();await h.send({type:'connect'});
 await h.send({type:'request',id:1,frame:{type:'open',instanceId:'instance'}});
 await h.send({type:'request',id:2,frame:{type:'baseline',...identity,values:{button:false},sequence:'0'}});
 h.setClock(250);await h.tick();await flush();assert.equal(h.sockets[0].sent.at(-1).type,'heartbeat');assert.equal(h.received.at(-1).type,'feedback');const envelope=h.received.at(-1);
 h.setClock(500);await h.tick();await flush();assert.equal(h.received.filter(message=>message.type==='feedback').length,1);
 await h.send({type:'feedback-ack',id:envelope.id});assert.equal(h.received.at(-1).reply.feedback.mode,'snapshot');
 h.sockets[0].onclose();assert.equal(h.port.closed,true);assert.equal(h.intervals.size,0);assert.equal(h.received.at(-1).type,'closed');
});
